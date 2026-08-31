//! `mooshik secret set/get/list`, and the vault key provider every command
//! that touches a secret goes through.

use std::{
    env,
    io::{self, Read},
    sync::Arc,
};

use anyhow::anyhow;
use zeroize::{Zeroize, Zeroizing};

use crate::{
    config::{self, Config, VaultProvider},
    home::HomeLayout,
    text,
    vault::{KeyProvider, KeyringProvider, PassphraseProvider, Vault, VaultError},
};

pub(crate) fn dispatch_secret(
    layout: &HomeLayout,
    matches: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let root = layout.init().map_err(anyhow::Error::new)?;
    let config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    let provider = provider_for(&config)?;
    let mut vault = Vault::open_at(&layout.vault, root, provider).map_err(anyhow::Error::new)?;
    match matches.subcommand() {
        Some(("set", args)) => {
            let Some(name) = args.get_one::<String>("name") else {
                return Err(anyhow!(text::get("vault.set_failed")));
            };
            let value = read_secret_value()?;
            vault.set(name, &value).map_err(anyhow::Error::new)
        }
        Some(("get", args)) => {
            let Some(name) = args.get_one::<String>("name") else {
                return Err(anyhow!(text::get("vault.get_failed")));
            };
            let value = vault.get(name).map_err(anyhow::Error::new)?;
            println!("{}", value.expose());
            Ok(())
        }
        Some(("list", _)) => {
            for name in vault.list() {
                println!("{name}");
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub(crate) fn provider_for(config: &Config) -> anyhow::Result<Arc<dyn KeyProvider>> {
    match config.vault.provider {
        VaultProvider::Keyring => Ok(Arc::new(KeyringProvider::system())),
        VaultProvider::Passphrase => {
            let passphrase = Zeroizing::new(env::var(config::PASSPHRASE_ENV).unwrap_or_default());
            Ok(Arc::new(
                PassphraseProvider::new(&*passphrase).map_err(anyhow::Error::new)?,
            ))
        }
    }
}

pub(crate) fn read_secret_value() -> anyhow::Result<Zeroizing<String>> {
    const MAX_INPUT_BYTES: usize = crate::vault::MAX_SECRET_VALUE_BYTES;
    if let Ok(value) = env::var("MOOSHIK_SECRET_VALUE") {
        return normalize_environment_value(Zeroizing::new(value));
    }
    let mut bytes = Zeroizing::new(Vec::new());
    io::stdin()
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        // Typed, not bare anyhow!: the classifier — not this call site —
        // decides the exit class, so the chain must carry a known variant.
        .map_err(|_| anyhow::Error::new(VaultError::Io))?;
    normalize_stdin_bytes(bytes)
}

pub(crate) fn normalize_environment_value(
    mut value: Zeroizing<String>,
) -> anyhow::Result<Zeroizing<String>> {
    const MAX_INPUT_BYTES: usize = crate::vault::MAX_SECRET_VALUE_BYTES;
    if value.len() > MAX_INPUT_BYTES {
        return Err(anyhow::Error::new(VaultError::InputTooLarge));
    }
    while matches!(value.chars().last(), Some('\r' | '\n')) {
        value.pop();
    }
    if value.is_empty() {
        return Err(anyhow::Error::new(VaultError::MissingValue));
    }
    Ok(value)
}

pub(crate) fn normalize_stdin_bytes(
    mut bytes: Zeroizing<Vec<u8>>,
) -> anyhow::Result<Zeroizing<String>> {
    const MAX_INPUT_BYTES: usize = crate::vault::MAX_SECRET_VALUE_BYTES;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(anyhow::Error::new(VaultError::InputTooLarge));
    }
    while matches!(bytes.last(), Some(b'\r' | b'\n')) {
        bytes.pop();
    }
    if bytes.is_empty() {
        return Err(anyhow::Error::new(VaultError::MissingValue));
    }
    let value = match std::str::from_utf8(bytes.as_slice()) {
        Ok(value) => value.to_owned(),
        Err(_) => {
            // Keep rejected credential bytes inside the zeroizing allocation;
            // do not move them into String::from_utf8's ordinary Vec error.
            bytes.as_mut_slice().zeroize();
            return Err(anyhow::Error::new(VaultError::Io));
        }
    };
    Ok(Zeroizing::new(value))
}
