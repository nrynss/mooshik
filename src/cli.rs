//! CLI definition.
//!
//! Built with clap's builder API rather than derive: help strings come from the
//! `text` module at runtime, and derive attributes only accept literals. Every
//! new subcommand registers here and adds its strings to `text/en.toml`.
//!
//! M7 conventions pinned here:
//!
//! * Exit codes: `0` success · `2` user error · `1` internal failure, decided
//!   once in [`Failure`] and documented in `--help`.
//! * Errors reach the terminal through [`Failure::report`] and nowhere else —
//!   top-level message only, never a source chain (see its doc comment).
//! * Every example printed in `--help` parses as written (`cli::tests`).

use std::{
    env,
    future::Future,
    io::{self, Read},
    sync::Arc,
};

use anyhow::anyhow;
use clap::{Arg, Command};
use lambo::{ConceptType, MemoryStats, RecallResult};
use zeroize::{Zeroize, Zeroizing};

use crate::{
    companion::CompanionError,
    config::{self, Config, ConfigError, VaultProvider},
    home::{HomeError, HomeLayout},
    memory::MemoryError,
    text,
    vault::{KeyProvider, KeyringProvider, PassphraseProvider, Vault, VaultError},
};

/// Why one CLI invocation failed, and therefore how the process should exit.
///
/// Exit-code convention (also documented in `--help`'s afterword):
///
/// * `0` — success.
/// * `2` ([`Failure::User`]) — the operator asked for something the current
///   setup cannot do: bad usage, invalid configuration, a name that does not
///   exist. Scripts may branch on this; the message says what to fix.
/// * `1` ([`Failure::Internal`]) — unexpected internal failure: broken state,
///   IO, or a bug. Retrying or reporting is the next step, not reconfiguring.
pub enum Failure {
    User(anyhow::Error),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Self {
        if is_user_error(&error) {
            Self::User(error)
        } else {
            Self::Internal(error)
        }
    }
}

impl Failure {
    /// The process exit code for this failure class.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::User(_) => 2,
            Self::Internal(_) => 1,
        }
    }

    /// The message the terminal sees: exactly the top-level `Display`, nothing
    /// else. Every error type renders what failed, why, and what to do next
    /// through `text/en.toml`, so the top level is complete by construction —
    /// while wrapped sources are NOT covered by that guarantee
    /// (`MemoryError::Backend` can wrap a store error whose detail names DSN
    /// material), so the chain never prints.
    fn rendered(&self) -> String {
        match self {
            Self::User(error) | Self::Internal(error) => error.to_string(),
        }
    }

    /// THE one place an error reaches the terminal. Print here or fix the code
    /// that bypasses this; do not grow a second formatter.
    pub fn report(&self) -> i32 {
        eprintln!("{}", self.rendered());
        self.exit_code()
    }
}

/// Whether the deepest known cause is something the operator authored and can
/// fix. Unknown error classes fail internal (exit 1): an unclassified error
/// must never punish a script with a misleading "you did it wrong".
fn is_user_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<ConfigError>().is_some()
            || cause.downcast_ref::<HomeError>().is_some_and(|error| {
                matches!(
                    error,
                    HomeError::MissingHome
                        | HomeError::UnsafePath
                        | HomeError::MigrationRequired
                        | HomeError::LayoutConflict
                )
            })
            || cause.downcast_ref::<VaultError>().is_some_and(|error| {
                matches!(
                    error,
                    VaultError::NotFound
                        | VaultError::InvalidName
                        | VaultError::MissingValue
                        | VaultError::NulByte
                        | VaultError::InputTooLarge
                        | VaultError::MissingPassphrase
                        | VaultError::Authentication
                        // The rest of the vault surface prints operator
                        // fix-it instructions ("restore a valid vault",
                        // "select passphrase mode"), so it is a refusal,
                        // not an internal failure.
                        | VaultError::InvalidFormat
                        | VaultError::UnsafePath
                        | VaultError::LockFailed
                        | VaultError::Keyring
                )
            })
            || cause.downcast_ref::<MemoryError>().is_some_and(|error| {
                matches!(
                    error,
                    MemoryError::MissingDsn | MemoryError::SessionConflict(_)
                )
            })
            || cause.downcast_ref::<CompanionError>().is_some_and(|error| {
                matches!(
                    error,
                    CompanionError::Unreachable
                        | CompanionError::Timeout
                        | CompanionError::HttpStatus
                        | CompanionError::TurnTooLarge
                        // "Check the endpoint" / "try again" are
                        // reconfiguration-style advice.
                        | CompanionError::InvalidResponse
                        | CompanionError::ToolLoop
                )
            })
    })
}

pub fn command() -> Command {
    Command::new("mooshik")
        .version(env!("CARGO_PKG_VERSION"))
        .about(text::get("app.about"))
        .after_help(text::get("app.after_help"))
        .subcommand(Command::new("init").about(text::get("config.init_help")))
        .subcommand(
            Command::new("serve")
                .about(text::get("memory.serve_help"))
                .after_help(text::get("memory.serve_after_help")),
        )
        .subcommand(
            Command::new("chat")
                .about(text::get("companion.chat_help"))
                .after_help(text::get("companion.chat_after_help")),
        )
        .subcommand(
            Command::new("recall")
                .about(text::get("memory.recall_help"))
                .after_help(text::get("memory.recall_after_help"))
                .arg(
                    Arg::new("query")
                        .help(text::get("memory.query_help"))
                        .required(true),
                ),
        )
        .subcommand(
            Command::new("stats")
                .about(text::get("memory.stats_help"))
                .after_help(text::get("memory.stats_after_help")),
        )
        .subcommand(
            Command::new("config")
                .about(text::get("config.show_help"))
                .subcommand_required(true)
                .subcommand(Command::new("show").about(text::get("config.show_help"))),
        )
        .subcommand(Command::new("permissions").about(text::get("permissions.help")))
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
///
/// Clap answers usage errors itself (its own help/usage text on stderr, exit
/// code 2), which is the same number [`Failure`] uses for runtime user errors —
/// one convention end to end.
pub fn run() -> Result<(), Failure> {
    let matches = command().get_matches();
    dispatch(&matches).map_err(Failure::from)
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
        Some(("serve", _)) => serve(&layout),
        Some(("chat", _)) => chat(&layout),
        Some(("recall", args)) => recall(&layout, args),
        Some(("stats", _)) => stats(&layout),
        Some(("config", sub)) if sub.subcommand_name() == Some("show") => show_config(&layout),
        Some(("permissions", _)) => show_permissions(&layout),
        Some(("secret", sub)) => dispatch_secret(&layout, sub),
        _ => Ok(()),
    }
}

fn show_config(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    print!("{}", config.redacted_toml());
    Ok(())
}

fn show_permissions(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    print!("{}", config.permissions.grants().render());
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
    block_on(crate::memory::provision(&config))?;
    println!("{}", text::get("home.init_done"));
    Ok(())
}

fn serve(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    block_on(crate::memory::serve(&config))
}

/// One-shot search over workspace memory (`crate::memory::recall` opens and
/// closes its own handle), then render the hits for the local operator.
fn recall(layout: &HomeLayout, matches: &clap::ArgMatches) -> anyhow::Result<()> {
    let query = matches
        .get_one::<String>("query")
        .expect("clap marks the query argument required")
        .clone();
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    let recalled = block_on(crate::memory::recall(&config, query.clone()))?;
    println!("{}", render_recall(&query, &recalled));
    Ok(())
}

/// Session health over workspace memory, rendered for the local operator.
fn stats(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    let health = block_on(crate::memory::stats(&config))?;
    println!("{}", render_stats(&health));
    Ok(())
}

/// Render recall results. Local-operator output only — see
/// `memory::ops::recall` for why this path deliberately skips chat's egress
/// redaction: nothing recalled here reaches a model or history.
fn render_recall(query: &str, recalled: &RecallResult) -> String {
    if recalled.hits.is_empty() {
        return text::get("memory.recall_empty").replace("{query}", query);
    }
    let mut out = text::get("memory.recall_header").replace("{query}", query);
    out.push('\n');
    for (index, hit) in recalled.hits.iter().enumerate() {
        out.push_str(&format!("\n  {}. {}\n     ", index + 1, hit.content));
        let mut detail: Vec<String> = Vec::new();
        if let Some(kind) = hit.concept_type {
            detail.push(concept_kind(kind).to_owned());
        }
        if hit.is_canonical {
            detail.push(text::get("memory.recall_canonical").to_owned());
        }
        detail.push(
            text::get("memory.recall_relevance").replace("{score}", &format!("{:.2}", hit.score)),
        );
        if let Some(radius) = hit.blast_radius {
            detail.push(
                text::get("memory.recall_blast_radius").replace("{count}", &radius.to_string()),
            );
        }
        out.push_str(&detail.join(" · "));
        out.push('\n');
    }
    if !recalled.warnings.is_empty() {
        out.push('\n');
        out.push_str(text::get("memory.recall_warnings"));
        out.push('\n');
        for warning in &recalled.warnings {
            out.push_str(&format!("  - {warning}\n"));
        }
    }
    out
}

fn concept_kind(kind: ConceptType) -> &'static str {
    match kind {
        ConceptType::Entity => text::get("memory.kind_entity"),
        ConceptType::Logic => text::get("memory.kind_logic"),
        ConceptType::Constraint => text::get("memory.kind_constraint"),
        ConceptType::Resource => text::get("memory.kind_resource"),
        ConceptType::Observation => text::get("memory.kind_observation"),
    }
}

fn render_stats(health: &MemoryStats) -> String {
    let degraded = if health.degraded {
        text::get("memory.degraded_yes")
    } else {
        text::get("memory.degraded_no")
    };
    [
        text::get("memory.stats_header")
            .replace("{session}", health.session.as_str())
            .replace("{agent}", health.agent.as_str()),
        format!(
            "  {}",
            text::get("memory.stats_concepts")
                .replace("{total}", &health.concept_count.to_string())
                .replace("{canonical}", &health.canonical_count.to_string())
                .replace("{embedded}", &health.embedded_concepts.to_string())
        ),
        format!(
            "  {}",
            text::get("memory.stats_graph")
                .replace("{nodes}", &health.node_count.to_string())
                .replace("{edges}", &health.edge_count.to_string())
        ),
        format!(
            "  {}",
            text::get("memory.stats_log_depth").replace("{depth}", &health.log_depth.to_string())
        ),
        format!(
            "  {}",
            text::get("memory.stats_flush_lag")
                .replace("{lag}", &format!("{:.1}s", health.flush_lag.as_secs_f64()),)
        ),
        format!(
            "  {}",
            text::get("memory.stats_dead_letters")
                .replace("{count}", &health.dead_lettered.to_string())
        ),
        format!(
            "  {}",
            text::get("memory.stats_degraded").replace("{degraded}", degraded)
        ),
        format!(
            "  {}",
            text::get("memory.stats_cycles")
                .replace("{daemon}", &health.daemon_cycles.to_string())
                .replace("{canonization}", &health.canonization_cycles.to_string())
                .replace("{failures}", &health.canonization_failures.to_string())
        ),
    ]
    .join("\n")
}

/// Test seam for the M3 pin below: chat loads configuration without ever
/// touching the memory subsystem.
#[cfg_attr(not(test), allow(dead_code))]
fn load_chat_config(layout: &HomeLayout) -> anyhow::Result<Config> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    Config::load_at(&root).map_err(anyhow::Error::new)
}

fn chat(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    // Open the vault once for the whole session; failure is not fatal (the
    // tool boundary prints one notice and continues unredacted).
    let executor =
        crate::tools::executor_for_chat(&config, open_vault_for_chat(layout, &config, &root));
    crate::companion::run_chat(&config, executor).map_err(anyhow::Error::new)
}

/// Open the shared vault handle for chat. `None` on any failure — provider
/// selection, keyring/passphrase problems, a bad file — which
/// `executor_for_chat` turns into one stderr notice plus unredacted-only-
/// because-unopenable operation.
fn open_vault_for_chat(
    layout: &HomeLayout,
    config: &Config,
    root: &std::fs::File,
) -> Option<crate::vault::SharedVault> {
    let provider = provider_for(config).ok()?;
    Vault::open_at(&layout.vault, root.try_clone().ok()?, provider)
        .ok()
        .map(crate::vault::Vault::shared)
}

fn block_on<T>(fut: impl Future<Output = Result<T, MemoryError>>) -> anyhow::Result<T> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| anyhow!(text::get("memory.runtime_failed")))?
        .block_on(fut)
        .map_err(anyhow::Error::new)
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

fn read_secret_value() -> anyhow::Result<Zeroizing<String>> {
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

fn normalize_environment_value(mut value: Zeroizing<String>) -> anyhow::Result<Zeroizing<String>> {
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

fn normalize_stdin_bytes(mut bytes: Zeroizing<Vec<u8>>) -> anyhow::Result<Zeroizing<String>> {
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
    fn permissions_help_comes_from_text() {
        let cmd = command();
        let permissions = cmd
            .find_subcommand("permissions")
            .unwrap()
            .get_about()
            .unwrap()
            .to_string();
        assert_eq!(permissions, text::get("permissions.help"));
    }

    #[test]
    fn recall_and_stats_help_come_from_text() {
        let cmd = command();
        let recall = cmd.find_subcommand("recall").unwrap();
        assert_eq!(
            recall.get_about().unwrap().to_string(),
            text::get("memory.recall_help")
        );
        let stats = cmd.find_subcommand("stats").unwrap();
        assert_eq!(
            stats.get_about().unwrap().to_string(),
            text::get("memory.stats_help")
        );
        // The query argument is positional and required: `mooshik recall` with
        // no query is a usage error answered by clap, not a panic later.
        assert!(command()
            .try_get_matches_from(["mooshik", "recall"])
            .is_err());
    }

    #[test]
    fn chat_dispatch_does_not_open_memory() {
        let src = include_str!("cli.rs");
        let load = src
            .split("fn load_chat_config")
            .nth(1)
            .unwrap()
            .split("fn chat(")
            .next()
            .unwrap();
        assert!(!load.contains("memory::"), "{load}");
        assert!(!load.contains("provision"), "{load}");
        let body = src
            .split("fn chat(")
            .nth(1)
            .unwrap()
            .split("fn block_on")
            .next()
            .unwrap();
        assert!(!body.contains("memory::"), "{body}");
        assert!(!body.contains("provision"), "{body}");
        assert!(!body.contains("serve("), "{body}");
        assert!(body.contains("run_chat"), "{body}");
    }

    #[test]
    fn chat_prepare_succeeds_on_default_home_without_dsn() {
        let root = crate::secure_path::canonical_temp_dir()
            .join(format!("mooshik-chat-prep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = HomeLayout::new(&root);
        layout.init().unwrap();
        let config = load_chat_config(&layout).unwrap();
        assert_eq!(config.companion.model, "local-model");
        let file = std::fs::read_to_string(&layout.config).unwrap();
        assert!(!file.contains("dsn"), "{file}");
        let isolated = Config::from_toml_and_env(&file, []).unwrap();
        assert!(isolated.store.dsn.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn chat_help_comes_from_text() {
        let mut cmd = command();
        let chat = cmd
            .find_subcommand("chat")
            .unwrap()
            .get_about()
            .unwrap()
            .to_string();
        assert_eq!(chat, text::get("companion.chat_help"));
        let help = cmd
            .find_subcommand_mut("chat")
            .unwrap()
            .render_help()
            .to_string();
        assert!(help.contains(text::get("companion.chat_help")));
        assert!(help.contains("mooshik init"));
        assert!(help.contains("[companion]"));
    }

    #[test]
    fn serve_and_init_help_come_from_text() {
        let mut cmd = command();
        let init = cmd
            .find_subcommand("init")
            .unwrap()
            .get_about()
            .unwrap()
            .to_string();
        assert_eq!(init, text::get("config.init_help"));
        assert!(init.contains("MOOSHIK_POSTGRES_DSN"));
        let serve = cmd
            .find_subcommand("serve")
            .unwrap()
            .get_about()
            .unwrap()
            .to_string();
        assert_eq!(serve, text::get("memory.serve_help"));
        let serve_after = cmd
            .find_subcommand_mut("serve")
            .unwrap()
            .render_help()
            .to_string();
        assert!(serve_after.contains("stdio"));
        assert!(serve_after.contains("MOOSHIK_POSTGRES_DSN"));
        assert!(serve_after.contains(text::get("memory.serve_help")));
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

    #[test]
    fn exit_codes_distinguish_user_error_from_internal_failure() {
        let user_errors = [
            anyhow::Error::new(ConfigError::InvalidToml),
            anyhow::Error::new(HomeError::MissingHome),
            anyhow::Error::new(HomeError::UnsafePath),
            anyhow::Error::new(HomeError::MigrationRequired),
            anyhow::Error::new(HomeError::LayoutConflict),
            anyhow::Error::new(VaultError::NotFound),
            anyhow::Error::new(VaultError::InvalidName),
            anyhow::Error::new(VaultError::MissingPassphrase),
            anyhow::Error::new(VaultError::InvalidFormat),
            anyhow::Error::new(VaultError::UnsafePath),
            anyhow::Error::new(VaultError::LockFailed),
            anyhow::Error::new(VaultError::Keyring),
            anyhow::Error::new(MemoryError::MissingDsn),
            anyhow::Error::new(MemoryError::SessionConflict(
                "session mooshik is already held by another writer".to_owned(),
            )),
            anyhow::Error::new(CompanionError::Unreachable),
            anyhow::Error::new(CompanionError::HttpStatus),
            anyhow::Error::new(CompanionError::InvalidResponse),
            anyhow::Error::new(CompanionError::ToolLoop),
        ];
        for error in user_errors {
            assert_eq!(Failure::from(error).exit_code(), 2);
        }
        let internal_errors = [
            anyhow::Error::new(HomeError::Io),
            anyhow::Error::new(VaultError::Io),
            anyhow::Error::new(CompanionError::Runtime),
        ];
        for error in internal_errors {
            assert_eq!(Failure::from(error).exit_code(), 1);
        }
    }

    #[test]
    fn empty_secret_values_classify_user_with_the_missing_value_message() {
        // P1-a: both input paths used to raise a bare `anyhow!` whose chain
        // carried no VaultError, so the canonical operator mistake exited 1.
        let errors = [
            normalize_environment_value(Zeroizing::new(String::new())).unwrap_err(),
            normalize_stdin_bytes(Zeroizing::new(Vec::new())).unwrap_err(),
        ];
        for error in errors {
            assert_eq!(error.to_string(), text::get("vault.missing_value"));
            assert!(
                error.chain().any(|cause| cause
                    .downcast_ref::<VaultError>()
                    .is_some_and(|e| matches!(e, VaultError::MissingValue))),
                "the chain must carry the typed variant: {error:?}"
            );
            let failure = Failure::from(error);
            assert_eq!(failure.exit_code(), 2);
            assert_eq!(failure.rendered(), text::get("vault.missing_value"));
        }
    }

    #[test]
    fn oversized_unreadable_and_non_utf8_secret_input_is_typed_too() {
        const MAX_INPUT_BYTES: usize = crate::vault::MAX_SECRET_VALUE_BYTES;
        let too_large = vec![b'x'; MAX_INPUT_BYTES + 1];
        let errors = [
            normalize_environment_value(Zeroizing::new("x".repeat(MAX_INPUT_BYTES + 1)))
                .unwrap_err(),
            normalize_stdin_bytes(Zeroizing::new(too_large)).unwrap_err(),
            normalize_stdin_bytes(Zeroizing::new(vec![b's', 0xff])).unwrap_err(),
        ];
        for error in errors {
            assert!(
                error
                    .chain()
                    .any(|cause| cause.downcast_ref::<VaultError>().is_some()),
                "every secret-input rejection must be typed: {error:?}"
            );
        }
    }

    #[test]
    fn lease_conflicts_classify_user_and_render_holder_remediation() {
        // P2-c: a held single-writer lease is an operator situation, not an
        // internal failure — and its safe detail (holder + age + takeover
        // hint) must surface instead of the wrong "check credentials" advice.
        let detail = "session mooshik is already held by another writer (a@h#1) — it \
                      acquired the single-writer lease 12s ago. If that holder is wedged, \
                      an operator can force a takeover"
            .to_owned();
        let failure = Failure::from(anyhow::Error::new(MemoryError::SessionConflict(
            detail.clone(),
        )));
        assert_eq!(failure.exit_code(), 2);
        let rendered = failure.rendered();
        assert!(rendered.contains("another writer"), "{rendered}");
        assert!(rendered.contains("force a takeover"), "{rendered}");
        assert!(rendered.contains(&detail), "{rendered}");
        assert!(!rendered.contains("postgres://"));
    }

    #[test]
    fn lambo_conflicts_map_to_the_session_conflict_variant_not_the_generic_backend() {
        let mapped = MemoryError::from(lambo::LamboError::Conflict("held".to_owned()));
        assert!(
            matches!(mapped, MemoryError::SessionConflict(_)),
            "{mapped:?}"
        );
        let mapped = MemoryError::from(lambo::LamboError::Other(anyhow::anyhow!("boom")));
        assert!(matches!(mapped, MemoryError::Backend(_)), "{mapped:?}");
        // The generic backend path still renders the fixed message and stays
        // internal — only Conflict is promoted.
        let error = anyhow::Error::new(mapped);
        assert_eq!(Failure::from(error).exit_code(), 1);
    }

    #[test]
    fn backend_failures_classify_internal_but_render_the_mapped_message() {
        let error = anyhow::Error::new(MemoryError::Backend(lambo::LamboError::Config(
            "session lease refused".to_owned(),
        )));
        let failure = Failure::from(error);
        assert_eq!(failure.exit_code(), 1);
        assert_eq!(failure.rendered(), text::get("memory.backend_failed"));
    }

    #[test]
    fn report_renders_the_top_level_message_never_the_wrapped_chain() {
        let dsn_material = "postgres://m7user:m7p4ssw0rd@db.internal/mooshik";
        let error = anyhow::Error::new(MemoryError::Backend(lambo::LamboError::Config(format!(
            "connect failed for {dsn_material}"
        ))));
        // The chain DOES carry the wrapped detail — which is exactly why the
        // terminal renderer must not walk it.
        assert!(error
            .chain()
            .any(|cause| cause.to_string().contains(dsn_material)));
        let failure = Failure::from(error);
        assert_eq!(failure.exit_code(), 1);
        let rendered = failure.rendered();
        assert_eq!(rendered, text::get("memory.backend_failed"));
        assert!(!rendered.contains(dsn_material));
        // And the entry point itself never grew a chain printer back.
        assert!(!include_str!("main.rs").contains("{err:#}"));
    }

    #[test]
    fn a_vault_value_never_reaches_a_rendered_error() {
        let value_in_play = "s3cret-alpha-value-m7";
        let root = crate::secure_path::canonical_temp_dir()
            .join(format!("mooshik-m7-vault-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = HomeLayout::new(&root);
        let file = layout.init().unwrap();
        let provider = Arc::new(PassphraseProvider::new("m7-pin-passphrase").unwrap());
        let mut vault = Vault::open_at(&layout.vault, file, provider).unwrap();
        vault.set("alpha", value_in_play).unwrap();
        let missing = vault.get("beta").unwrap_err();
        drop(vault);
        let failure = Failure::from(anyhow::Error::new(missing));
        assert_eq!(failure.exit_code(), 2, "an unknown name is a user error");
        let rendered = failure.rendered();
        assert_eq!(rendered, text::get("vault.not_found"));
        assert!(
            !rendered.contains(value_in_play),
            "a stored value leaked into an error about a different name"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn every_documented_example_parses_as_written() {
        let src = include_str!("text/en.toml");
        let examples = documented_mooshik_examples(src);
        assert!(
            examples.contains(&"mooshik init".to_owned()),
            "en.toml should document `mooshik init`; extraction broke"
        );
        for example in &examples {
            let tokens = tokenize_example(example);
            assert!(
                command().try_get_matches_from(tokens).is_ok(),
                "`{example}` is documented but does not parse as written"
            );
        }
    }

    /// Backticked spans in en.toml that start with `mooshik ` — the runnable
    /// examples help promises. `split('`')` alternates outside/inside, so odd
    /// slices are the spans.
    fn documented_mooshik_examples(src: &str) -> Vec<String> {
        src.split('`')
            .skip(1)
            .step_by(2)
            .map(str::trim)
            .filter(|span| span.starts_with("mooshik ") || *span == "mooshik")
            .map(str::to_owned)
            .collect()
    }

    fn tokenize_example(example: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let mut current = String::new();
        let mut quoted = false;
        for ch in example.chars() {
            match ch {
                '"' => quoted = !quoted,
                c if c.is_whitespace() && !quoted => {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                }
                c => current.push(c),
            }
        }
        if !current.is_empty() {
            tokens.push(current);
        }
        tokens
    }

    #[test]
    fn recall_render_names_hits_and_warns_without_leaking_types() {
        let recalled = RecallResult {
            hits: vec![lambo::RecallHit {
                node_id: lambo::NodeId::nil(),
                content: "live m7 cli sweep marker".to_owned(),
                concept_type: Some(ConceptType::Entity),
                score: 0.83,
                is_canonical: true,
                blast_radius: Some(2),
            }],
            context: String::new(),
            warnings: vec!["embedding leg skipped".to_owned()],
        };
        let rendered = render_recall("m7 marker", &recalled);
        assert!(rendered.contains("Matches for 'm7 marker':"), "{rendered}");
        assert!(rendered.contains("live m7 cli sweep marker"), "{rendered}");
        assert!(rendered.contains("entity"), "{rendered}");
        assert!(rendered.contains("canonical"), "{rendered}");
        assert!(rendered.contains("relevance 0.83"), "{rendered}");
        assert!(rendered.contains("blast radius 2"), "{rendered}");
        assert!(rendered.contains("Warnings:"), "{rendered}");
        assert!(
            !rendered.contains("Entity"),
            "Rust variant names must not reach the terminal"
        );

        let empty = RecallResult {
            hits: Vec::new(),
            context: String::new(),
            warnings: Vec::new(),
        };
        let rendered = render_recall("nothing-here", &empty);
        assert!(rendered.contains("nothing-here"), "{rendered}");
        assert!(rendered.contains("mooshik chat"), "{rendered}");
    }

    #[test]
    fn stats_render_reports_health_in_one_voice() {
        let health = MemoryStats {
            session: lambo::SessionId::new("mooshik"),
            agent: lambo::AgentId::new("mooshik"),
            flush_lag: std::time::Duration::from_millis(1500),
            log_depth: 3,
            flush_depth: 5,
            dead_lettered: 1,
            degraded: true,
            node_count: 12,
            edge_count: 20,
            concept_count: 8,
            canonical_count: 3,
            embedded_concepts: 7,
            epoch: 4,
            daemon_cycles: 9,
            canonization_cycles: 2,
            canonization_failures: 1,
        };
        let rendered = render_stats(&health);
        assert!(rendered.contains("session 'mooshik'"), "{rendered}");
        assert!(rendered.contains("concepts: 8 total, 3 canonical, 7 embedded"));
        assert!(rendered.contains("graph: 12 nodes, 20 edges"));
        assert!(rendered.contains("write-behind log depth: 3"));
        assert!(rendered.contains("flush lag: 1.5s"));
        assert!(rendered.contains("dead-lettered batches: 1"));
        assert!(rendered.contains("durability degraded: yes"));
        assert!(rendered.contains("2 canonization (1 failed)"));
    }
}
