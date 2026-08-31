use std::{env, ffi::OsStr, path::Path};

use anyhow::anyhow;
use zeroize::Zeroizing;

use crate::{
    config::{self, Config, ConfigError},
    home::HomeLayout,
    secure_path, text,
    vault::{Vault, VaultError},
};

use super::{resolve, secret};

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

/// Configure the coding contractor MCP server (`mooshik configure coder --agent claude|omp|cursor|agy`).
pub(crate) fn configure_coder(
    layout: &HomeLayout,
    matches: &clap::ArgMatches,
) -> anyhow::Result<()> {
    let agent = matches
        .get_one::<String>("agent")
        .expect("clap marks the agent argument required");

    let (env_var, secret_name) = match agent.as_str() {
        "claude" => ("ANTHROPIC_API_KEY", "anthropic-api-key"),
        "omp" | "agy" => ("MOOSHIK_GEMINI_API_KEY", "gemini-api-key"),
        "cursor" => ("CURSOR_API_KEY", "cursor-api-key"),
        _ => return Err(anyhow!(text::get("config.invalid_coder_agent"))),
    };

    let root = layout.init().map_err(anyhow::Error::new)?;
    let before = read_config_text(&root)?;
    let current = Config::from_toml_and_env(&before, env::vars()).map_err(anyhow::Error::new)?;

    // Store secret if provided in environment
    if let Ok(provider) = secret::provider_for(&current) {
        if let Ok(root_clone) = root.try_clone() {
            if let Ok(mut vault) = Vault::open_at(&layout.vault, root_clone, provider) {
                if vault.get(secret_name).is_err() {
                    if let Ok(val) = env::var("MOOSHIK_SECRET_VALUE") {
                        if let Ok(val) = secret::normalize_environment_value(Zeroizing::new(val)) {
                            let _ = vault.set(secret_name, &val);
                        }
                    }
                }
            }
        }
    }

    let script_path = find_coder_script_path();
    let after = apply_coder_config(&before, agent, &script_path, env_var, secret_name);

    // Verify after parsing
    let _ = Config::from_toml_and_env(&after, env::vars()).map_err(anyhow::Error::new)?;

    secure_path::write_private_at(&root, OsStr::new("config.toml"), after.as_bytes())
        .map_err(|_| anyhow::Error::new(ConfigError::WriteFailed))?;

    println!(
        "{}",
        text::get("config.coder_done").replace("{agent}", agent)
    );
    Ok(())
}

fn apply_coder_config(
    before: &str,
    agent: &str,
    script_path: &str,
    env_var: &str,
    secret_name: &str,
) -> String {
    let mut in_coder_section = false;
    let mut cleaned_lines = Vec::new();
    for line in before.lines() {
        let trimmed = line.trim();
        if trimmed == "[mcp_servers.coder]" || trimmed == "[mcp_servers.coder.env]" {
            in_coder_section = true;
            continue;
        }
        if in_coder_section && trimmed.starts_with('[') {
            in_coder_section = false;
        }
        if !in_coder_section {
            cleaned_lines.push(line);
        }
    }

    let mut result = cleaned_lines.join("\n");
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }

    // Ensure permissions table contains "mcp.coder.*" = "prompt"
    if result.contains("[permissions]") {
        if !result.contains("\"mcp.coder.*\"") && !result.contains("mcp.coder") {
            if let Some(pos) = result.find("[permissions]") {
                let insert_at = pos + "[permissions]".len();
                let (head, tail) = result.split_at(insert_at);
                result = format!("{head}\n\"mcp.coder.*\" = \"prompt\"{tail}");
            }
        }
    } else {
        result.push_str("\n[permissions]\n\"mcp.coder.*\" = \"prompt\"\n");
    }

    // Append the coder mcp server block
    result.push_str(&format!(
        "\n[mcp_servers.coder]\ncommand = \"python3\"\nargs = [\"{}\"]\nexpose = [\"delegate\", \"check\"]\n\n[mcp_servers.coder.env]\nMOOSHIK_CODER_AGENT = \"{}\"\n{} = \"{}\"\n",
        script_path, agent, env_var, secret_name
    ));

    result
}

fn find_coder_script_path() -> String {
    let local = Path::new("mcp-servers/coder/server.py");
    if local.is_file() {
        if let Ok(abs) = local.canonicalize() {
            return abs.to_string_lossy().into_owned();
        }
        return local.to_string_lossy().into_owned();
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("mcp-servers/coder/server.py");
            if candidate.is_file() {
                if let Ok(abs) = candidate.canonicalize() {
                    return abs.to_string_lossy().into_owned();
                }
                return candidate.to_string_lossy().into_owned();
            }
            let repo_root = parent.join("../..");
            let candidate2 = repo_root.join("mcp-servers/coder/server.py");
            if candidate2.is_file() {
                if let Ok(abs) = candidate2.canonicalize() {
                    return abs.to_string_lossy().into_owned();
                }
                return candidate2.to_string_lossy().into_owned();
            }
        }
    }

    "/usr/local/share/mooshik/mcp-servers/coder/server.py".to_owned()
}
