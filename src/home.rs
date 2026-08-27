//! Secure first-run layout for a Mooshik installation.

use std::{ffi::OsStr, fs, path::PathBuf};

use crate::{config::Config, secure_path, text};

const MARKER: &str = ".mooshik-home";
const MARKER_BYTES: &[u8] = b"mooshik home\n";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HomeLayout {
    pub root: PathBuf,
    pub config: PathBuf,
    pub database: PathBuf,
    pub vault: PathBuf,
    pub logs: PathBuf,
}

impl HomeLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            config: root.join("config.toml"),
            database: root.join("mooshik.db"),
            vault: root.join("vault"),
            logs: root.join("logs"),
            root,
        }
    }

    /// Create the home and its private support files.
    ///
    /// Initialize the home and return the already-open root descriptor.
    ///
    /// The descriptor must be carried through config and vault operations. A
    /// later reopen by `self.root` would permit an attacker to replace the
    /// validated directory with another ordinary directory between phases.
    pub fn init(&self) -> Result<fs::File, HomeError> {
        if is_filesystem_root(&self.root) {
            return Err(HomeError::UnsafePath);
        }
        let (root, created) = match secure_path::open_dir_with_status(&self.root, false, 0o700) {
            Ok((root, _)) => (root, false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                secure_path::open_dir_with_status(&self.root, true, 0o700).map_err(map_io)?
            }
            Err(error) => return Err(map_io(error)),
        };
        if !created {
            validate_existing_root(&root)?;
        }
        secure_path::set_dir_mode(&root, 0o700).map_err(map_io)?;
        if created {
            create_marker(&root)?;
        }
        ensure_layout(&root)?;
        let existing_vault = match secure_path::open_existing_at(&root, OsStr::new("vault")) {
            Ok(vault) => vault,
            Err(error) if error.kind() == std::io::ErrorKind::IsADirectory => {
                return Err(HomeError::LayoutConflict)
            }
            Err(error) => return Err(map_io(error)),
        };
        if let Some(vault) = existing_vault {
            if !vault.metadata().map_err(map_io)?.is_file() {
                return Err(HomeError::LayoutConflict);
            }
        }
        Ok(root)
    }

    pub fn open_existing_root(&self) -> Result<fs::File, HomeError> {
        if is_filesystem_root(&self.root) {
            return Err(HomeError::UnsafePath);
        }
        let root = secure_path::open_dir(&self.root, false, 0o700).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                HomeError::MissingHome
            } else {
                map_io(error)
            }
        })?;
        validate_existing_root(&root)?;
        Ok(root)
    }
}

#[derive(Debug)]
pub enum HomeError {
    Io,
    MissingHome,
    UnsafePath,
    MigrationRequired,
    LayoutConflict,
}

impl std::fmt::Display for HomeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key = match self {
            Self::Io => "home.init_failed",
            Self::MissingHome => "home.missing",
            Self::UnsafePath => "home.unsafe_path",
            Self::MigrationRequired => "home.migration_required",
            Self::LayoutConflict => "home.layout_conflict",
        };
        f.write_str(text::get(key))
    }
}
impl std::error::Error for HomeError {}

fn map_io(error: std::io::Error) -> HomeError {
    if matches!(
        error.kind(),
        std::io::ErrorKind::InvalidInput | std::io::ErrorKind::PermissionDenied
    ) || matches!(error.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR))
    {
        HomeError::UnsafePath
    } else {
        HomeError::Io
    }
}

fn is_filesystem_root(path: &std::path::Path) -> bool {
    path.is_absolute() && path.parent().is_none()
}

fn validate_existing_root(root: &fs::File) -> Result<(), HomeError> {
    let metadata = root.metadata().map_err(map_io)?;
    if !metadata.is_dir() {
        return Err(HomeError::UnsafePath);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = metadata.permissions().mode() & 0o7777;
        // Existing directories are only accepted when they already have the
        // private app-home contract. This prevents MOOSHIK_HOME from turning
        // an unrelated directory (or a broad system directory) into a home.
        if mode != 0o700 || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(HomeError::UnsafePath);
        }
    }
    match secure_path::open_existing_at(root, OsStr::new(MARKER)).map_err(map_io)? {
        Some(marker) => {
            if !marker.metadata().map_err(map_io)?.is_file() {
                return Err(HomeError::UnsafePath);
            }
            let marker_bytes =
                secure_path::read_private_at(root, OsStr::new(MARKER), 128).map_err(map_io)?;
            if marker_bytes != MARKER_BYTES {
                return Err(HomeError::UnsafePath);
            }
        }
        None => {
            // A private, genuinely empty directory is a safe first-run
            // override. A non-empty unmarked directory may be a legacy home,
            // but it may also be unrelated user data; never adopt or mutate
            // either case implicitly. Tell the operator how to migrate it.
            if !secure_path::is_empty_dir(root).map_err(map_io)? {
                return Err(HomeError::MigrationRequired);
            }
            create_marker(root)?;
        }
    }
    Ok(())
}

fn create_marker(root: &fs::File) -> Result<(), HomeError> {
    match secure_path::create_new_at(root, OsStr::new(MARKER)) {
        Ok(mut marker) => {
            use std::io::Write;
            marker.write_all(MARKER_BYTES).map_err(map_io)?;
            marker.sync_all().map_err(map_io)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let bytes =
                secure_path::read_private_at(root, OsStr::new(MARKER), 128).map_err(map_io)?;
            if bytes == MARKER_BYTES {
                Ok(())
            } else {
                Err(HomeError::UnsafePath)
            }
        }
        Err(error) => Err(map_io(error)),
    }
}

fn ensure_layout(root: &fs::File) -> Result<(), HomeError> {
    secure_path::ensure_dir_at(root, OsStr::new("logs"), 0o700).map_err(map_io)?;
    secure_path::ensure_private_file_at(
        root,
        OsStr::new("config.toml"),
        Config::default_toml().as_bytes(),
    )
    .map_err(map_io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::{KeyringBackend, KeyringProvider, PassphraseProvider, Vault, VaultError};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestKeyring {
        value: Mutex<Option<String>>,
    }

    impl KeyringBackend for TestKeyring {
        fn get(&self, _: &str, _: &str) -> Result<Option<String>, VaultError> {
            Ok(self.value.lock().unwrap().clone())
        }

        fn set(&self, _: &str, _: &str, value: &str) -> Result<(), VaultError> {
            *self.value.lock().unwrap() = Some(value.to_owned());
            Ok(())
        }
    }

    fn path(label: &str) -> PathBuf {
        crate::secure_path::canonical_temp_dir()
            .join(format!("mooshik-home-{label}-{}", std::process::id()))
    }

    #[cfg(unix)]
    #[test]
    fn init_creates_private_usable_layout_and_repairs_modes() {
        let root = path("private");
        let _ = fs::remove_dir_all(&root);
        let layout = HomeLayout::new(&root);
        let root_handle = layout.init().unwrap();
        assert!(layout.config.is_file());
        assert!(!layout.database.exists());
        assert!(layout.logs.is_dir());
        let provider = Arc::new(PassphraseProvider::new("lifecycle-passphrase").unwrap());
        let vault = Vault::open_at(&layout.vault, root_handle, provider.clone()).unwrap();
        assert!(layout.vault.is_file());
        assert_eq!(
            fs::metadata(&layout.vault).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(vault);
        let reopened = Vault::open(&layout.vault, provider).unwrap();
        assert!(reopened.list().is_empty());
        drop(reopened);
        fs::set_permissions(&layout.config, fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(&layout.logs, fs::Permissions::from_mode(0o755)).unwrap();
        layout.init().unwrap();
        assert_eq!(
            fs::metadata(&layout.config).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&layout.logs).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn first_run_vault_lifecycle_works_with_persistent_keyring_provider() {
        let root = path("keyring-lifecycle");
        let _ = fs::remove_dir_all(&root);
        let layout = HomeLayout::new(&root);
        let backend = Arc::new(TestKeyring::default());
        let provider = Arc::new(KeyringProvider::with_backend("home-test", "vault", backend));
        let root_handle = layout.init().unwrap();
        let mut vault = Vault::open_at(&layout.vault, root_handle, provider.clone()).unwrap();
        vault.set("first-run", "value").unwrap();
        drop(vault);
        let reopened = Vault::open(&layout.vault, provider).unwrap();
        assert_eq!(reopened.get("first-run").unwrap().expose(), "value");
        assert_eq!(
            fs::metadata(&layout.vault).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn existing_empty_and_marked_partial_homes_are_recovered_without_data_loss() {
        let root = path("recovery");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let layout = HomeLayout::new(&root);
        layout.init().unwrap();
        assert_eq!(fs::read(layout.root.join(MARKER)).unwrap(), MARKER_BYTES);

        fs::remove_file(&layout.config).unwrap();
        fs::remove_dir(&layout.logs).unwrap();
        fs::write(root.join("user-data"), b"untouched").unwrap();
        layout.init().unwrap();
        assert_eq!(
            fs::read(layout.root.join("user-data")).unwrap(),
            b"untouched"
        );
        assert!(layout.config.is_file());
        assert!(layout.logs.is_dir());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn unmarked_nonempty_home_requires_explicit_migration() {
        let root = path("legacy");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.join("config.toml"), Config::default_toml()).unwrap();
        assert!(matches!(
            HomeLayout::new(&root).init(),
            Err(HomeError::MigrationRequired)
        ));
        assert!(!root.join(MARKER).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_vault_symlink_and_directory_migration() {
        let root = path("symlink");
        let outside = path("outside");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        // Mark the home explicitly (mode included): under a restrictive umask
        // the raw directory would already pass validation, and an unmarked,
        // non-empty root would stop at MigrationRequired before the vault
        // check. With a valid marked home, the Err(UnsafePath) below can only
        // come from rejecting the vault symlink itself.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(root.join(MARKER), MARKER_BYTES).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.join(MARKER), fs::Permissions::from_mode(0o600)).unwrap();
        }
        std::os::unix::fs::symlink(&outside, root.join("vault")).unwrap();
        assert!(matches!(
            HomeLayout::new(&root).init(),
            Err(HomeError::UnsafePath)
        ));
        let _ = fs::remove_dir_all(&root);
        HomeLayout::new(&root).init().unwrap();
        fs::create_dir_all(root.join("vault")).unwrap();
        assert!(matches!(
            HomeLayout::new(&root).init(),
            Err(HomeError::LayoutConflict)
        ));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_home_parent() {
        let root = path("parent-link");
        let outside = path("parent-link-outside");
        let _ = fs::remove_file(&root);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, &root).unwrap();
        assert!(matches!(
            HomeLayout::new(&root).open_existing_root(),
            Err(HomeError::UnsafePath)
        ));
        let _ = fs::remove_file(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_root_and_unrelated_existing_directory_without_mutating_it() {
        use std::os::unix::fs::PermissionsExt;
        assert!(matches!(
            HomeLayout::new("/").init(),
            Err(HomeError::UnsafePath)
        ));

        let root = path("unrelated-existing");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            HomeLayout::new(&root).init(),
            Err(HomeError::UnsafePath)
        ));
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(!root.join("config.toml").exists());
        assert!(!root.join("logs").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn retained_root_descriptor_survives_path_swap() {
        use std::os::unix::fs::PermissionsExt;
        let root = path("swap");
        let moved = path("swap-moved");
        let replacement = path("swap-replacement");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&moved);
        let _ = fs::remove_dir_all(&replacement);
        let layout = HomeLayout::new(&root);
        let root_handle = layout.init().unwrap();
        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            root.join("config.toml"),
            b"[vault]\nprovider = \"passphrase\"\n",
        )
        .unwrap();

        // All later operations use the descriptor returned by init, so they
        // remain in the moved original home rather than the replacement.
        let config = Config::load_at(&root_handle).unwrap();
        assert_eq!(config.vault.provider, crate::config::VaultProvider::Keyring);
        let provider = Arc::new(PassphraseProvider::new("swap-passphrase").unwrap());
        let vault = Vault::open_at(&layout.vault, root_handle, provider).unwrap();
        drop(vault);
        assert!(moved.join("vault").is_file());
        assert!(!root.join("vault").exists());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(moved);
        let _ = fs::remove_dir_all(replacement);
    }
}
