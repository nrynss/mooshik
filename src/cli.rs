//! CLI definition.
//!
//! Built with clap's builder API rather than derive: help strings come from the
//! `text` module at runtime, and derive attributes only accept literals. Every
//! new subcommand registers here and adds its strings to `text/en.toml`.

use std::{
    env,
    io::{self, Read},
    sync::Arc,
};

use anyhow::anyhow;
use clap::{Arg, Command};
use zeroize::{Zeroize, Zeroizing};

use crate::{
    config::{self, Config, VaultProvider},
    home::HomeLayout,
    text,
    vault::{KeyProvider, KeyringProvider, PassphraseProvider, Vault},
};

pub fn command() -> Command {
    Command::new("mooshik")
        .version(env!("CARGO_PKG_VERSION"))
        .about(text::get("app.about"))
        .after_help(text::get("app.after_help"))
        .subcommand(Command::new("init").about(text::get("config.init_help")))
        .subcommand(
            Command::new("config")
                .about(text::get("config.show_help"))
                .subcommand_required(true)
                .subcommand(Command::new("show").about(text::get("config.show_help"))),
        )
        .subcommand(
            Command::new("secret")
                .about(text::get("vault.list_help"))
                .subcommand(secret_command("set", text::get("vault.set_help")))
                .subcommand(secret_command("get", text::get("vault.get_help")))
                .subcommand(Command::new("list").about(text::get("vault.list_help")))
                .subcommand_required(true),
        )
        .subcommand_required(false)
        .arg_required_else_help(true)
}

/// Parse argv and dispatch the commands implemented by the current milestones.
pub fn run() -> anyhow::Result<()> {
    let matches = command().get_matches();
    dispatch(&matches)?;
    Ok(())
}

fn secret_command(name: &'static str, help: &'static str) -> Command {
    let command = Command::new(name).about(help).arg(
        Arg::new("name")
            .help(text::get("vault.name_help"))
            .required(true),
    );
    if name == "set" {
        command.after_help(text::get("vault.set_after_help"))
    } else {
        command
    }
}

fn dispatch(matches: &clap::ArgMatches) -> anyhow::Result<()> {
    let home = config::resolve_home(env::vars()).map_err(anyhow::Error::new)?;
    let layout = HomeLayout::new(home);
    match matches.subcommand() {
        Some(("init", _)) => initialize(&layout),
        Some(("config", sub)) if sub.subcommand_name() == Some("show") => show_config(&layout),
        Some(("secret", sub)) => dispatch_secret(&layout, sub),
        _ => Ok(()),
    }
}

fn show_config(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    let output =
        toml::to_string_pretty(&config).map_err(|_| anyhow!(text::get("config.show_failed")))?;
    print!("{output}");
    Ok(())
}

fn dispatch_secret(layout: &HomeLayout, matches: &clap::ArgMatches) -> anyhow::Result<()> {
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

fn initialize(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.init().map_err(anyhow::Error::new)?;
    let config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    let provider = provider_for(&config)?;
    Vault::open_at(&layout.vault, root, provider).map_err(anyhow::Error::new)?;
    println!("{}", text::get("home.init_done"));
    Ok(())
}

fn provider_for(config: &Config) -> anyhow::Result<Arc<dyn KeyProvider>> {
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

fn read_secret_value() -> anyhow::Result<Zeroizing<String>> {
    const MAX_INPUT_BYTES: usize = crate::vault::MAX_SECRET_VALUE_BYTES;
    if let Ok(value) = env::var("MOOSHIK_SECRET_VALUE") {
        return normalize_environment_value(Zeroizing::new(value));
    }
    let mut bytes = Zeroizing::new(Vec::new());
    io::stdin()
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow!(text::get("vault.io_failed")))?;
    normalize_stdin_bytes(bytes)
}

fn normalize_environment_value(mut value: Zeroizing<String>) -> anyhow::Result<Zeroizing<String>> {
    const MAX_INPUT_BYTES: usize = crate::vault::MAX_SECRET_VALUE_BYTES;
    if value.len() > MAX_INPUT_BYTES {
        return Err(anyhow!(text::get("vault.input_too_large")));
    }
    while matches!(value.chars().last(), Some('\r' | '\n')) {
        value.pop();
    }
    if value.is_empty() {
        return Err(anyhow!(text::get("vault.missing_value")));
    }
    Ok(value)
}

fn normalize_stdin_bytes(mut bytes: Zeroizing<Vec<u8>>) -> anyhow::Result<Zeroizing<String>> {
    const MAX_INPUT_BYTES: usize = crate::vault::MAX_SECRET_VALUE_BYTES;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(anyhow!(text::get("vault.input_too_large")));
    }
    while matches!(bytes.last(), Some(b'\r' | b'\n')) {
        bytes.pop();
    }
    if bytes.is_empty() {
        return Err(anyhow!(text::get("vault.missing_value")));
    }
    let value = match std::str::from_utf8(bytes.as_slice()) {
        Ok(value) => value.to_owned(),
        Err(_) => {
            // Keep rejected credential bytes inside the zeroizing allocation;
            // do not move them into String::from_utf8's ordinary Vec error.
            bytes.as_mut_slice().zeroize();
            return Err(anyhow!(text::get("vault.io_failed")));
        }
    };
    Ok(Zeroizing::new(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_carries_strings_from_text_module() {
        let cmd = command();
        assert_eq!(cmd.get_name(), "mooshik");
        assert!(cmd
            .get_about()
            .unwrap()
            .to_string()
            .contains("cowork partner"));
        assert!(!cmd.get_after_help().unwrap().to_string().is_empty());
    }

    #[test]
    fn secret_set_does_not_accept_plaintext_command_line_values() {
        assert!(command()
            .try_get_matches_from(["mooshik", "secret", "set", "token", "--value", "secret"])
            .is_err());
        let help = command()
            .find_subcommand_mut("secret")
            .unwrap()
            .find_subcommand_mut("set")
            .unwrap()
            .render_help()
            .to_string();
        assert!(!help.contains("--value"));
        assert!(help.contains("MOOSHIK_SECRET_VALUE"));
        assert!(help.contains("stdin"));
        assert!(help.contains("cannot be supplied as command-line arguments"));
    }

    #[test]
    fn config_show_missing_home_explains_read_only_recovery() {
        let error = crate::home::HomeError::MissingHome;
        let message = error.to_string();
        assert!(message.contains("mooshik init"));
        assert!(message.contains("does not exist"));
    }

    #[test]
    fn environment_secret_strips_trailing_crlf() {
        let value = normalize_environment_value(Zeroizing::new("secret\r\n".to_owned())).unwrap();
        assert_eq!(&*value, "secret");
    }

    #[test]
    fn invalid_utf8_stdin_value_is_rejected_without_moving_bytes_out() {
        let error = normalize_stdin_bytes(Zeroizing::new(vec![b's', 0xff, b'\n'])).unwrap_err();
        assert_eq!(error.to_string(), text::get("vault.io_failed"));
    }
}
