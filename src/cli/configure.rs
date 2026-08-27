//! `mooshik config show`, `mooshik config set`, and `mooshik permissions`.
//!
//! The write path inherits `init`'s obligations exactly — private (0600),
//! atomic, and never through a symbolic link — because it goes through the
//! same `secure_path` primitives `init` and the vault already use, rather than
//! opening `config.toml` by path.

use std::ffi::OsStr;

use zeroize::Zeroizing;

use crate::{
    config::{self, Config, ConfigError},
    home::HomeLayout,
    secure_path, text,
    vault::{Vault, VaultError},
};

use super::resolve;

/// The same bound `Config::load` applies, so `config set` cannot be used to
/// grow a file past what the loader will read back.
const MAX_CONFIG_BYTES: u64 = 64 * 1024;

pub(crate) fn show_config(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    print!("{}", config.redacted_toml());
    Ok(())
}

pub(crate) fn show_permissions(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    print!("{}", config.permissions.grants().render());
    Ok(())
}

/// Change one setting in `config.toml`.
///
/// The order is deliberate. Validate the key and value first (so a typo never
/// reaches the filesystem), edit in memory, verify the edit reads back, ask
/// whether this moves the database, and only then write — atomically, over the
/// retained home descriptor.
pub(crate) fn set_config(layout: &HomeLayout, matches: &clap::ArgMatches) -> anyhow::Result<()> {
    let key = matches
        .get_one::<String>("key")
        .expect("clap marks the key argument required");
    let value = matches
        .get_one::<String>("value")
        .expect("clap marks the value argument required");
    let confirmed = matches.get_flag("confirm-database-change");

    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let before = read_config_text(&root)?;
    // Parse the current file before editing it, so a file that is already
    // broken is reported as such rather than as a failed write.
    let current =
        Config::from_toml_and_env(&before, std::env::vars()).map_err(anyhow::Error::new)?;
    let after =
        config::apply_setting(&before, key, value, std::env::vars()).map_err(anyhow::Error::new)?;

    guard_store_move(layout, &current, &root, &before, &after, confirmed)?;

    secure_path::write_private_at(&root, OsStr::new("config.toml"), after.as_bytes())
        .map_err(|_| anyhow::Error::new(ConfigError::WriteFailed))?;
    println!("{}", text::get("config.set_done").replace("{key}", key));
    Ok(())
}

/// Refuse a change that points Mooshik at a different database unless the
/// operator said so.
///
/// Changing the store does not move what Mooshik remembers — it leaves it
/// behind, in a database this process will no longer open. Nothing is deleted,
/// but from the new database the assistant looks empty, which reads as data
/// loss. So a *genuine* move is stopped; a cosmetic edit (the same database
/// spelled with a password, or with `:5432` written out) is not a move and is
/// not mentioned, because ceremony over a no-op teaches operators to ignore
/// the warning that matters.
///
/// The vault is opened lazily and only when a DSN secret is actually
/// referenced, so `config set companion.model ...` still works on a machine
/// whose keyring is unavailable.
fn guard_store_move(
    layout: &HomeLayout,
    config: &Config,
    root: &std::fs::File,
    before: &str,
    after: &str,
    confirmed: bool,
) -> anyhow::Result<()> {
    let mut vault: Option<Vault> = None;
    let mut resolver = |name: &str| -> Result<Zeroizing<String>, ConfigError> {
        if vault.is_none() {
            vault = Some(
                resolve::open_vault(layout, config, root)
                    .map_err(|_| ConfigError::VaultUnavailable)?,
            );
        }
        let vault = vault.as_ref().expect("just opened");
        vault
            .get(name)
            .map(|token| Zeroizing::new(token.expose().to_owned()))
            .map_err(|error| match error {
                VaultError::NotFound => ConfigError::MissingStoreSecret(name.to_owned()),
                _ => ConfigError::VaultUnavailable,
            })
    };
    let moved = config::store_move_requires_confirmation(before, after, &mut resolver)
        .map_err(anyhow::Error::new)?;
    if moved && !confirmed {
        return Err(anyhow::Error::new(ConfigError::StoreMoveUnconfirmed));
    }
    Ok(())
}

/// Read `config.toml` through the retained home descriptor: no symlink, no
/// second path lookup, and the same size bound the loader applies.
fn read_config_text(root: &std::fs::File) -> anyhow::Result<String> {
    match secure_path::read_private_at(root, OsStr::new("config.toml"), MAX_CONFIG_BYTES) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| anyhow::Error::new(ConfigError::Io)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(_) => Err(anyhow::Error::new(ConfigError::Io)),
    }
}
