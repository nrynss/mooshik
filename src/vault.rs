//! Encrypted, local-only secret storage.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use argon2::Argon2;
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use fs2::FileExt;
use rand::{rngs::OsRng, TryRngCore};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{secure_path, text};

// Version 2 authenticates the complete on-disk header as AEAD associated
// data. Older files deliberately fail format/authentication checks rather
// than silently accepting unauthenticated key-provider metadata.
const MAGIC: &[u8] = b"MOOSHIK-VAULT\x02";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;
pub const MAX_VAULT_FILE_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_SECRET_VALUE_BYTES: usize = 1024 * 1024;

pub trait KeyProvider: Send + Sync {
    fn load_or_create(&self, salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, VaultError>;
}

pub trait KeyringBackend: Send + Sync {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, VaultError>;
    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), VaultError>;
}

pub struct SystemKeyring;

impl KeyringBackend for SystemKeyring {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, VaultError> {
        let entry = keyring::Entry::new(service, account).map_err(|_| VaultError::Keyring)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(VaultError::Keyring),
        }
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), VaultError> {
        let entry = keyring::Entry::new(service, account).map_err(|_| VaultError::Keyring)?;
        entry.set_password(value).map_err(|_| VaultError::Keyring)
    }
}

pub struct KeyringProvider {
    service: String,
    account: String,
    backend: Arc<dyn KeyringBackend>,
}

impl KeyringProvider {
    pub fn system() -> Self {
        Self::with_backend("mooshik", "vault-master", Arc::new(SystemKeyring))
    }

    pub fn with_backend(
        service: impl Into<String>,
        account: impl Into<String>,
        backend: Arc<dyn KeyringBackend>,
    ) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
            backend,
        }
    }
}

impl KeyProvider for KeyringProvider {
    fn load_or_create(&self, _salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, VaultError> {
        let encoded = self.backend.get(&self.service, &self.account)?;
        let encoded = match encoded {
            Some(value) => Zeroizing::new(value),
            None => {
                let mut key = Zeroizing::new([0; KEY_LEN]);
                OsRng
                    .try_fill_bytes(key.as_mut())
                    .map_err(|_| VaultError::Random)?;
                let value = Zeroizing::new(hex(key.as_ref()));
                self.backend.set(&self.service, &self.account, &value)?;
                return Ok(key);
            }
        };
        decode_hex_key(&encoded)
    }
}

pub struct PassphraseProvider {
    passphrase: Zeroizing<Vec<u8>>,
}

impl PassphraseProvider {
    pub fn new(passphrase: impl AsRef<[u8]>) -> Result<Self, VaultError> {
        let passphrase = passphrase.as_ref();
        if passphrase.is_empty() {
            return Err(VaultError::MissingPassphrase);
        }
        Ok(Self {
            passphrase: Zeroizing::new(passphrase.to_vec()),
        })
    }
}

impl KeyProvider for PassphraseProvider {
    fn load_or_create(&self, salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, VaultError> {
        let mut key = Zeroizing::new([0; KEY_LEN]);
        Argon2::default()
            .hash_password_into(&self.passphrase, salt, key.as_mut())
            .map_err(|_| VaultError::KeyDerivation)?;
        Ok(key)
    }
}

#[derive(Debug)]
pub enum VaultError {
    Io,
    InvalidFormat,
    Authentication,
    Keyring,
    Random,
    KeyDerivation,
    MissingPassphrase,
    InvalidName,
    NotFound,
    UnsafePath,
    LockFailed,
    MissingValue,
    NulByte,
    InputTooLarge,
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key = match self {
            Self::Io => "vault.io_failed",
            Self::InvalidFormat => "vault.invalid_format",
            Self::Authentication => "vault.authentication_failed",
            Self::Keyring => "vault.keyring_failed",
            Self::Random => "vault.random_failed",
            Self::KeyDerivation => "vault.key_derivation_failed",
            Self::MissingPassphrase => "vault.missing_passphrase",
            Self::InvalidName => "vault.invalid_name",
            Self::NotFound => "vault.not_found",
            Self::UnsafePath => "vault.unsafe_path",
            Self::LockFailed => "vault.lock_failed",
            Self::MissingValue => "vault.missing_value",
            Self::NulByte => "vault.nul_byte",
            Self::InputTooLarge => "vault.input_too_large",
        };
        f.write_str(text::get(key))
    }
}
impl std::error::Error for VaultError {}

#[derive(Default, Deserialize, Serialize)]
struct Secrets(BTreeMap<String, Zeroizing<String>>);

struct ParsedFile<'a> {
    header: &'a [u8],
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    ciphertext: &'a [u8],
}

/// A value returned by the vault that is safe to put in diagnostics.
pub struct SecretToken(Zeroizing<String>);

impl SecretToken {
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }

    /// Length of the wrapped value in bytes, so egress redaction can order
    /// passes longest-first over overlapping prefixes. Reveals no plaintext.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the wrapped value is empty; redaction skips empty tokens.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
impl std::fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}
impl std::fmt::Display for SecretToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// Whether `name` is acceptable as a secret name. The same rules [`Vault::set`]
/// and [`Vault::get`] enforce; exposed so configuration can fail closed on an
/// impossible secret reference at load time instead of at first use.
pub fn is_valid_name(name: &str) -> bool {
    validate_name(name).is_ok()
}

pub struct Vault {
    path: PathBuf,
    parent: fs::File,
    leaf: OsString,
    _lock: fs::File,
    key: Zeroizing<[u8; KEY_LEN]>,
    salt: [u8; SALT_LEN],
    secrets: Secrets,
}

impl Vault {
    pub fn open(
        path: impl Into<PathBuf>,
        provider: Arc<dyn KeyProvider>,
    ) -> Result<Self, VaultError> {
        let path = path.into();
        let (parent, leaf) =
            secure_path::open_parent(&path, true).map_err(|_| VaultError::UnsafePath)?;
        Self::open_with_parent(path, parent, leaf, provider)
    }

    pub fn open_at(
        path: &Path,
        parent: fs::File,
        provider: Arc<dyn KeyProvider>,
    ) -> Result<Self, VaultError> {
        let leaf = path
            .file_name()
            .ok_or(VaultError::UnsafePath)?
            .to_os_string();
        if leaf == std::ffi::OsStr::new(".") || leaf == std::ffi::OsStr::new("..") {
            return Err(VaultError::UnsafePath);
        }
        Self::open_with_parent(path.to_path_buf(), parent, leaf, provider)
    }

    fn open_with_parent(
        path: PathBuf,
        parent: fs::File,
        leaf: OsString,
        provider: Arc<dyn KeyProvider>,
    ) -> Result<Self, VaultError> {
        let lock = acquire_lock(&parent)?;
        match secure_path::open_existing_at(&parent, &leaf).map_err(|_| VaultError::UnsafePath)? {
            Some(file) => {
                set_private_permissions(&file)?;
                if file.metadata().map_err(|_| VaultError::Io)?.len() > MAX_VAULT_FILE_BYTES {
                    return Err(VaultError::InputTooLarge);
                }
                let mut bytes = Vec::new();
                file.take(MAX_VAULT_FILE_BYTES.saturating_add(1))
                    .read_to_end(&mut bytes)
                    .map_err(|_| VaultError::Io)?;
                if bytes.len() as u64 > MAX_VAULT_FILE_BYTES {
                    return Err(VaultError::InputTooLarge);
                }
                let parsed = parse_file(&bytes)?;
                let key = provider.load_or_create(&parsed.salt)?;
                let secrets = decrypt(&key, parsed.header, &parsed.nonce, parsed.ciphertext)?;
                Ok(Self {
                    path,
                    parent,
                    leaf,
                    _lock: lock,
                    key,
                    salt: parsed.salt,
                    secrets,
                })
            }
            None => {
                let mut salt = [0; SALT_LEN];
                OsRng
                    .try_fill_bytes(&mut salt)
                    .map_err(|_| VaultError::Random)?;
                let key = provider.load_or_create(&salt)?;
                let vault = Self {
                    path,
                    parent,
                    leaf,
                    _lock: lock,
                    key,
                    salt,
                    secrets: Secrets::default(),
                };
                vault.persist()?;
                Ok(vault)
            }
        }
    }

    pub fn set(&mut self, name: &str, value: &str) -> Result<(), VaultError> {
        validate_name(name)?;
        if value.as_bytes().contains(&0) {
            // Interior NUL cannot survive `Command::env` (spawn fails with
            // `nul byte found in provided data`), so a stored NUL would
            // silently break every injected scratch run. Reject at the door,
            // like `validate_scratch` does for script code.
            return Err(VaultError::NulByte);
        }
        if value.len() > MAX_SECRET_VALUE_BYTES {
            return Err(VaultError::InputTooLarge);
        }
        self.secrets
            .0
            .insert(name.to_owned(), Zeroizing::new(value.to_owned()));
        self.persist()
    }

    pub fn get(&self, name: &str) -> Result<SecretToken, VaultError> {
        validate_name(name)?;
        self.secrets
            .0
            .get(name)
            .cloned()
            .map(SecretToken)
            .ok_or(VaultError::NotFound)
    }

    pub fn list(&self) -> Vec<String> {
        self.secrets.0.keys().cloned().collect()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Wrap this vault in a [`SharedVault`] handle for the tool boundary.
    pub fn shared(self) -> SharedVault {
        Arc::new(std::sync::Mutex::new(self))
    }

    fn persist(&self) -> Result<(), VaultError> {
        let mut nonce = [0; NONCE_LEN];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| VaultError::Random)?;
        let plaintext = Zeroizing::new(
            serde_json::to_vec(&self.secrets).map_err(|_| VaultError::InvalidFormat)?,
        );
        let cipher = XChaCha20Poly1305::new((&*self.key).into());
        let mut header = Vec::with_capacity(MAGIC.len() + SALT_LEN + NONCE_LEN);
        header.extend_from_slice(MAGIC);
        header.extend_from_slice(&self.salt);
        header.extend_from_slice(&nonce);
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext.as_ref(),
                    aad: &header,
                },
            )
            .map_err(|_| VaultError::Authentication)?;
        if header.len() + ciphertext.len() > MAX_VAULT_FILE_BYTES as usize {
            return Err(VaultError::InputTooLarge);
        }
        let mut file = Vec::with_capacity(header.len() + ciphertext.len());
        file.extend_from_slice(&header);
        file.extend_from_slice(&ciphertext);
        atomic_private_write(&self.parent, &self.leaf, &file)
    }
}

fn decrypt(
    key: &[u8; KEY_LEN],
    header: &[u8],
    nonce: &[u8; NONCE_LEN],
    ciphertext: &[u8],
) -> Result<Secrets, VaultError> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: header,
                },
            )
            .map_err(|_| VaultError::Authentication)?,
    );
    serde_json::from_slice(&plaintext).map_err(|_| VaultError::Authentication)
}

fn parse_file(bytes: &[u8]) -> Result<ParsedFile<'_>, VaultError> {
    let min = MAGIC.len() + SALT_LEN + NONCE_LEN + 16;
    if bytes.len() < min || &bytes[..MAGIC.len()] != MAGIC {
        return Err(VaultError::InvalidFormat);
    }
    let mut salt = [0; SALT_LEN];
    let mut nonce = [0; NONCE_LEN];
    let salt_end = MAGIC.len() + SALT_LEN;
    salt.copy_from_slice(&bytes[MAGIC.len()..salt_end]);
    let nonce_end = salt_end + NONCE_LEN;
    nonce.copy_from_slice(&bytes[salt_end..nonce_end]);
    Ok(ParsedFile {
        header: &bytes[..nonce_end],
        salt,
        nonce,
        ciphertext: &bytes[nonce_end..],
    })
}

fn acquire_lock(parent: &fs::File) -> Result<fs::File, VaultError> {
    let file = secure_path::open_lock_at(parent, std::ffi::OsStr::new(".vault.lock"))
        .map_err(|_| VaultError::LockFailed)?;
    set_private_permissions(&file)?;
    file.lock_exclusive().map_err(|_| VaultError::LockFailed)?;
    Ok(file)
}

fn atomic_private_write(
    parent: &fs::File,
    leaf: &std::ffi::OsStr,
    bytes: &[u8],
) -> Result<(), VaultError> {
    let mut random = [0u8; 12];
    OsRng
        .try_fill_bytes(&mut random)
        .map_err(|_| VaultError::Random)?;
    let temp = OsString::from(format!(
        ".vault-{}-{}.tmp",
        std::process::id(),
        hex(&random)
    ));
    let result = (|| {
        let mut file = secure_path::create_new_at(parent, &temp).map_err(|_| VaultError::Io)?;
        file.write_all(bytes).map_err(|_| VaultError::Io)?;
        file.sync_all().map_err(|_| VaultError::Io)?;
        secure_path::rename_at(parent, &temp, leaf).map_err(|_| VaultError::Io)?;
        parent.sync_all().map_err(|_| VaultError::Io)
    })();
    if result.is_err() {
        let _ = secure_path::unlink_at(parent, &temp);
    }
    result
}

fn set_private_permissions(file: &fs::File) -> Result<(), VaultError> {
    #[cfg(unix)]
    {
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| VaultError::Io)
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<(), VaultError> {
    // A leading `-` would make `secret list | xargs mooshik secret get`
    // feed the name to the argument parser as a flag — rejected at set time.
    if name.is_empty()
        || name.starts_with('-')
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        Err(VaultError::InvalidName)
    } else {
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex_key(value: &str) -> Result<Zeroizing<[u8; KEY_LEN]>, VaultError> {
    if value.len() != KEY_LEN * 2 {
        return Err(VaultError::Keyring);
    }
    let mut key = Zeroizing::new([0; KEY_LEN]);
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        key[index] = (hex_digit(chunk[0])? << 4) | hex_digit(chunk[1])?;
    }
    Ok(key)
}
fn hex_digit(value: u8) -> Result<u8, VaultError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(VaultError::Keyring),
    }
}

/// A vault handle shared across the tool boundary: egress redaction resolves
/// values per tool call, scratch injection resolves them per script run, and
/// `set` stays reachable for rotation. Locks are held only for the duration of
/// a `get`/`list`/`set`, never across output scanning or process spawns.
pub type SharedVault = Arc<std::sync::Mutex<Vault>>;

/// Lock a shared vault, recovering a poisoned guard rather than panicking:
/// nothing in this crate holds the lock across a fallible operation, so a
/// poisoned mutex must never take chat down with it.
pub fn lock_shared(vault: &SharedVault) -> std::sync::MutexGuard<'_, Vault> {
    vault
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn redact_output(output: &str, values: impl IntoIterator<Item = SecretToken>) -> String {
    let mut safe = output.to_owned();
    for token in values {
        if !token.0.is_empty() {
            safe = safe.replace(token.0.as_str(), "[REDACTED]");
        }
    }
    safe
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeKeyring {
        value: Mutex<Option<String>>,
    }
    impl KeyringBackend for FakeKeyring {
        fn get(&self, _: &str, _: &str) -> Result<Option<String>, VaultError> {
            Ok(self.value.lock().unwrap().clone())
        }
        fn set(&self, _: &str, _: &str, value: &str) -> Result<(), VaultError> {
            *self.value.lock().unwrap() = Some(value.to_owned());
            Ok(())
        }
    }
    fn path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mooshik-vault-{label}-{}", std::process::id()))
    }
    fn clean(path: &Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn keyring_round_trip_and_fake_provider() {
        let path = path("keyring");
        clean(&path);
        let backend = Arc::new(FakeKeyring::default());
        let provider = Arc::new(KeyringProvider::with_backend("test", "vault", backend));
        let mut vault = Vault::open(&path, provider.clone()).unwrap();
        vault.set("token", "secret-value").unwrap();
        assert_eq!(vault.get("token").unwrap().expose(), "secret-value");
        drop(vault);
        let reopened = Vault::open(&path, provider).unwrap();
        assert_eq!(reopened.list(), vec!["token"]);
        assert!(!String::from_utf8_lossy(&fs::read(&path).unwrap()).contains("secret-value"));
        clean(&path);
    }

    #[test]
    fn keyring_header_salt_mutation_fails_authentication() {
        let path = path("keyring-salt-mutation");
        clean(&path);
        let backend = Arc::new(FakeKeyring::default());
        let provider = Arc::new(KeyringProvider::with_backend("test-salt", "vault", backend));
        let vault = Vault::open(&path, provider.clone()).unwrap();
        drop(vault);
        let mut bytes = fs::read(&path).unwrap();
        bytes[MAGIC.len()] ^= 0x80;
        fs::write(&path, bytes).unwrap();
        assert!(matches!(
            Vault::open(&path, provider),
            Err(VaultError::Authentication)
        ));
        clean(&path);
    }

    #[test]
    fn passphrase_round_trip_and_wrong_key_rejection() {
        let path = path("passphrase");
        clean(&path);
        let mut vault =
            Vault::open(&path, Arc::new(PassphraseProvider::new("correct").unwrap())).unwrap();
        vault.set("token", "secret-value").unwrap();
        drop(vault);
        assert!(matches!(
            Vault::open(&path, Arc::new(PassphraseProvider::new("wrong").unwrap())),
            Err(VaultError::Authentication)
        ));
        clean(&path);
    }

    #[test]
    fn token_debug_and_output_are_redacted() {
        let token = SecretToken(Zeroizing::new("secret-value".to_owned()));
        assert_eq!(format!("{token:?}"), "[REDACTED]");
        assert_eq!(format!("{token}"), "[REDACTED]");
        assert_eq!(
            redact_output("before secret-value after", [token]),
            "before [REDACTED] after"
        );
    }

    #[test]
    fn names_and_missing_values_are_distinct_and_safe() {
        assert!(validate_name("bad\nname").is_err());
        assert!(
            validate_name("-flaglike").is_err(),
            "a leading hyphen would become a flag for naive xargs consumers"
        );
        assert!(validate_name("ok_name-1").is_ok());
        assert!(validate_name("-").is_err());
        let path = path("not-found");
        clean(&path);
        let vault =
            Vault::open(&path, Arc::new(PassphraseProvider::new("correct").unwrap())).unwrap();
        assert!(matches!(vault.get("absent"), Err(VaultError::NotFound)));
        clean(&path);
    }

    #[test]
    fn concurrent_writers_keep_both_updates() {
        let path = path("concurrent");
        clean(&path);
        let path_a = path.clone();
        let path_b = path.clone();
        let first = std::thread::spawn(move || {
            let mut vault = Vault::open(
                &path_a,
                Arc::new(PassphraseProvider::new("correct").unwrap()),
            )
            .unwrap();
            vault.set("alpha", "one").unwrap();
        });
        let second = std::thread::spawn(move || {
            let mut vault = Vault::open(
                &path_b,
                Arc::new(PassphraseProvider::new("correct").unwrap()),
            )
            .unwrap();
            vault.set("beta", "two").unwrap();
        });
        first.join().unwrap();
        second.join().unwrap();
        let vault =
            Vault::open(&path, Arc::new(PassphraseProvider::new("correct").unwrap())).unwrap();
        assert_eq!(vault.list(), vec!["alpha", "beta"]);
        clean(&path);
    }

    #[cfg(unix)]
    #[test]
    fn existing_vault_permissions_are_repaired_and_symlinks_rejected() {
        let vault_path = path("security");
        let outside = path("outside-file");
        clean(&vault_path);
        clean(&outside);
        let mut vault = Vault::open(
            &vault_path,
            Arc::new(PassphraseProvider::new("correct").unwrap()),
        )
        .unwrap();
        vault.set("token", "value").unwrap();
        drop(vault);
        fs::set_permissions(&vault_path, fs::Permissions::from_mode(0o644)).unwrap();
        let vault = Vault::open(
            &vault_path,
            Arc::new(PassphraseProvider::new("correct").unwrap()),
        )
        .unwrap();
        assert_eq!(
            fs::metadata(&vault_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(vault);
        fs::rename(&vault_path, &outside).unwrap();
        symlink(&outside, &vault_path).unwrap();
        assert!(matches!(
            Vault::open(
                &vault_path,
                Arc::new(PassphraseProvider::new("correct").unwrap())
            ),
            Err(VaultError::UnsafePath)
        ));
        clean(&vault_path);
        clean(&outside);
    }

    #[test]
    fn oversized_secret_is_rejected_before_storage() {
        let path = path("oversized");
        clean(&path);
        let mut vault =
            Vault::open(&path, Arc::new(PassphraseProvider::new("correct").unwrap())).unwrap();
        let value = "x".repeat(MAX_SECRET_VALUE_BYTES + 1);
        assert!(matches!(
            vault.set("large", &value),
            Err(VaultError::InputTooLarge)
        ));
        clean(&path);
    }

    #[test]
    fn nul_byte_value_is_rejected_at_set() {
        // P3-M6-4: an interior NUL cannot survive `Command::env`, so storing
        // one would silently break every injected scratch run. Rejected at
        // the door as a contained VaultError.
        let path = path("nul-byte");
        clean(&path);
        let mut vault =
            Vault::open(&path, Arc::new(PassphraseProvider::new("correct").unwrap())).unwrap();
        assert!(matches!(
            vault.set("nul", "before\u{0}after"),
            Err(VaultError::NulByte)
        ));
        assert!(!vault.list().contains(&"nul".to_owned()));
        clean(&path);
    }
}
