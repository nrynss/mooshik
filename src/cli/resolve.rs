//! Configuration that *references* a secret, resolved at the point of use.
//!
//! `config.toml` holds names, never values: `[store] dsn_secret` and
//! `[companion] api_key_secret` name entries in the vault, exactly as
//! `[mcp_servers.*.env]` already does. This module is the one place those
//! names become values, and the values go straight into the in-memory `Config`
//! the command is about to use — never back to disk, never into a message.
//!
//! Opening the vault takes an exclusive lock for the handle's lifetime, so a
//! command opens it **once**: [`load_with_secrets`] opens only when a
//! reference is actually configured and drops the handle before it returns,
//! while `chat` (which needs the same vault for egress redaction) opens it
//! itself and calls [`resolve_secrets`] with that handle.

use std::fs;

use crate::{
    config::{ApiKey, Config, ConfigError},
    home::HomeLayout,
    vault::{Vault, VaultError},
};

use super::secret::provider_for;

/// Whether this configuration references anything that lives in the vault.
pub(crate) fn needs_vault(config: &Config) -> bool {
    config.store.dsn_secret.is_some() || config.companion.api_key_secret.is_some()
}

/// Fill in every vault-referenced value on `config`.
///
/// Fails closed: a reference that cannot be resolved stops the command rather
/// than letting it run against the wrong database or talk to an endpoint
/// unauthenticated. The DSN is never echoed — only the secret *name*, which is
/// configuration and already printed by `config show`.
pub(crate) fn resolve_secrets(config: &mut Config, vault: &Vault) -> Result<(), ConfigError> {
    if let Some(name) = config.store.dsn_secret.clone() {
        let dsn = vault.get(&name).map_err(|error| match error {
            VaultError::NotFound => ConfigError::MissingStoreSecret(name.clone()),
            _ => ConfigError::VaultUnavailable,
        })?;
        config.store.dsn = Some(dsn.expose().to_owned());
    }
    if let Some(name) = config.companion.api_key_secret.clone() {
        let key = vault.get(&name).map_err(|error| match error {
            VaultError::NotFound => ConfigError::MissingApiKeySecret(name.clone()),
            _ => ConfigError::VaultUnavailable,
        })?;
        config.companion.api_key = Some(ApiKey::new(key.expose()));
    }
    Ok(())
}

/// Load configuration for a command that does not otherwise hold the vault.
pub(crate) fn load_with_secrets(layout: &HomeLayout, root: &fs::File) -> anyhow::Result<Config> {
    let mut config = Config::load_at(root).map_err(anyhow::Error::new)?;
    if !needs_vault(&config) {
        return Ok(config);
    }
    let vault = open_vault(layout, &config, root)?;
    resolve_secrets(&mut config, &vault).map_err(anyhow::Error::new)?;
    Ok(config)
}

/// Open the vault against an already-validated home descriptor.
pub(crate) fn open_vault(
    layout: &HomeLayout,
    config: &Config,
    root: &fs::File,
) -> anyhow::Result<Vault> {
    let provider = provider_for(config)?;
    let root = root
        .try_clone()
        .map_err(|_| anyhow::Error::new(ConfigError::VaultUnavailable))?;
    Vault::open_at(&layout.vault, root, provider).map_err(anyhow::Error::new)
}
