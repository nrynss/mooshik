//! Configuration loading and the environment overlay.

use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{secure_path, text};

const MAX_CONFIG_BYTES: u64 = 64 * 1024;

pub const HOME_ENV: &str = "MOOSHIK_HOME";
pub const PROVIDER_ENV: &str = "MOOSHIK_VAULT_PROVIDER";
pub const PASSPHRASE_ENV: &str = "MOOSHIK_VAULT_PASSPHRASE";

#[derive(Debug)]
pub enum ConfigError {
    Io,
    HomeUnavailable,
    InvalidToml,
    InvalidValue,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key = match self {
            Self::Io => "config.read_failed",
            Self::HomeUnavailable => "config.home_unavailable",
            Self::InvalidToml => "config.invalid_toml",
            Self::InvalidValue => "config.invalid_value",
        };
        f.write_str(text::get(key))
    }
}

impl std::error::Error for ConfigError {}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultProvider {
    #[default]
    Keyring,
    Passphrase,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VaultConfig {
    #[serde(default)]
    pub provider: VaultProvider,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            provider: VaultProvider::Keyring,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub vault: VaultConfig,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let source = match secure_path::open_parent(path, false) {
            Ok((parent, leaf)) => {
                match secure_path::read_private_at(&parent, &leaf, MAX_CONFIG_BYTES) {
                    Ok(bytes) => String::from_utf8(bytes).map_err(|_| ConfigError::Io)?,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
                    Err(_) => return Err(ConfigError::Io),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_) => return Err(ConfigError::Io),
        };
        Self::from_toml_and_env(&source, env::vars())
    }

    pub fn load_at(parent: &fs::File) -> Result<Self, ConfigError> {
        let source =
            match secure_path::read_private_at(parent, OsStr::new("config.toml"), MAX_CONFIG_BYTES)
            {
                Ok(bytes) => String::from_utf8(bytes).map_err(|_| ConfigError::Io)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(_) => return Err(ConfigError::Io),
            };
        Self::from_toml_and_env(&source, env::vars())
    }

    pub fn from_toml_and_env<I>(source: &str, environment: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut config = if source.trim().is_empty() {
            Self::default()
        } else {
            toml::from_str(source).map_err(|_| ConfigError::InvalidToml)?
        };
        let values: std::collections::HashMap<String, String> = environment.into_iter().collect();
        if let Some(value) = non_empty(&values, PROVIDER_ENV) {
            config.vault.provider = match value.to_ascii_lowercase().as_str() {
                "keyring" => VaultProvider::Keyring,
                "passphrase" => VaultProvider::Passphrase,
                _ => return Err(ConfigError::InvalidValue),
            };
        }
        Ok(config)
    }

    pub fn default_toml() -> &'static str {
        "[vault]\nprovider = \"keyring\"\n"
    }
}

pub fn resolve_home(
    environment: impl IntoIterator<Item = (String, String)>,
) -> Result<PathBuf, ConfigError> {
    let values: std::collections::HashMap<String, String> = environment.into_iter().collect();
    if let Some(path) = non_empty(&values, HOME_ENV) {
        return Ok(PathBuf::from(path));
    }
    values
        .get("HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or(ConfigError::HomeUnavailable)
        .map(|path| path.join(".mooshik"))
}

fn non_empty(values: &std::collections::HashMap<String, String>, key: &str) -> Option<String> {
    values.get(key).filter(|value| !value.is_empty()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(provider: &str) -> Vec<(String, String)> {
        vec![(PROVIDER_ENV.to_owned(), provider.to_owned())]
    }

    #[test]
    fn non_empty_environment_value_wins() {
        let config =
            Config::from_toml_and_env("[vault]\nprovider = 'passphrase'", env("keyring")).unwrap();
        assert_eq!(config.vault.provider, VaultProvider::Keyring);
    }

    #[test]
    fn empty_environment_value_preserves_file() {
        let config =
            Config::from_toml_and_env("[vault]\nprovider = 'passphrase'", env("")).unwrap();
        assert_eq!(config.vault.provider, VaultProvider::Passphrase);
    }

    #[test]
    fn unknown_and_malformed_values_are_rejected() {
        assert!(matches!(
            Config::from_toml_and_env("[other]\nx = 1", []),
            Err(ConfigError::InvalidToml)
        ));
        assert!(matches!(
            Config::from_toml_and_env("[vault]\nprovider = 'other'", []),
            Err(ConfigError::InvalidToml)
        ));
    }

    #[test]
    fn missing_home_is_an_error_instead_of_current_directory() {
        assert!(matches!(
            resolve_home([]),
            Err(ConfigError::HomeUnavailable)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn config_load_rejects_symlink_and_repairs_mode() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = std::env::temp_dir().join(format!("mooshik-config-{}", std::process::id()));
        let outside = root.with_extension("outside");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&outside);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&outside, Config::default_toml()).unwrap();
        let link = root.join("config.toml");
        symlink(&outside, &link).unwrap();
        assert!(matches!(Config::load(&link), Err(ConfigError::Io)));
        std::fs::remove_file(&link).unwrap();
        std::fs::write(&link, Config::default_toml()).unwrap();
        std::fs::set_permissions(&link, std::fs::Permissions::from_mode(0o644)).unwrap();
        Config::load(&link).unwrap();
        assert_eq!(
            std::fs::metadata(&link).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(outside);
    }

    #[cfg(unix)]
    #[test]
    fn config_load_rejects_symlinked_parent() {
        use std::os::unix::fs::symlink;
        let root =
            std::env::temp_dir().join(format!("mooshik-config-parent-{}", std::process::id()));
        let outside = root.with_extension("outside");
        let _ = std::fs::remove_file(&root);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("config.toml"), Config::default_toml()).unwrap();
        symlink(&outside, &root).unwrap();
        assert!(matches!(
            Config::load(&root.join("config.toml")),
            Err(ConfigError::Io)
        ));
        let _ = std::fs::remove_file(root);
        let _ = std::fs::remove_dir_all(outside);
    }
}
