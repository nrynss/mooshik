//! Behaviour and source pins for the CLI surface.
//!
//! The `include_str!` pin that used to read `cli.rs` now reads the file the
//! split moved its subject into: `chat_command_never_opens_memory` reads
//! `chat_cmd.rs`, and asserts the property over the WHOLE production half of
//! that file rather than over a hand-sliced pair of function bodies — which is
//! strictly stronger, because nothing in that module may reach the memory
//! subsystem, not merely nothing in those two functions.

use std::sync::Arc;

use lambo::{ConceptType, MemoryStats, RecallResult};
use zeroize::Zeroizing;

use crate::{
    companion::CompanionError,
    config::{Config, ConfigError},
    home::{HomeError, HomeLayout},
    memory::MemoryError,
    text,
    vault::{PassphraseProvider, Vault, VaultError},
};

use super::chat_cmd::load_chat_config;
use super::command::command;
use super::failure::Failure;
use super::render::{render_recall, render_stats};
use super::secret::{normalize_environment_value, normalize_stdin_bytes};

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

/// The M3/M4 pin, carried across the `cli.rs` split.
///
/// It used to slice two function bodies out of `cli.rs` by name. `chat` and
/// `load_chat_config` now have a module to themselves, so the same property is
/// asserted over that module's whole production half: NOTHING in the chat
/// entry point may reach the memory subsystem. `tools::executor_for_chat`
/// owns the one open `Memory` and the one single-writer lease; a second opener
/// here would contend with it.
///
/// The imports are checked too, since `use crate::memory::...` would let a
/// call site name `open(...)` without the `memory::` prefix.
#[test]
fn chat_command_never_opens_memory() {
    let src = include_str!("chat_cmd.rs");
    let production = src.split("#[cfg(test)]").next().unwrap();
    for forbidden in ["memory::", "crate::memory", "provision", "memory::serve"] {
        assert!(
            !production.contains(forbidden),
            "the chat command must not reach the memory subsystem ({forbidden}): {production}"
        );
    }
    assert!(
        production.contains("executor_for_chat"),
        "the chat command must build its tools through the factory: {production}"
    );
    assert!(
        production.contains("run_chat"),
        "the chat command must hand off to the chat loop: {production}"
    );
    // And the module the pin reads is the one dispatch actually routes to.
    let dispatch = include_str!("mod.rs");
    assert!(
        dispatch.contains("chat_cmd::chat(&layout)"),
        "dispatch must route `chat` to the pinned module: {dispatch}"
    );
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
        normalize_environment_value(Zeroizing::new("x".repeat(MAX_INPUT_BYTES + 1))).unwrap_err(),
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
    assert!(!include_str!("../main.rs").contains("{err:#}"));
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

/// "This process has no terminal" is attached to the handshake and to nothing
/// else.
///
/// It used to wrap every error the whole session could return, so a
/// `terminal.draw`, `event::poll` or `event::read` that failed an hour in was
/// reported as a missing terminal — a diagnosis true of neither the cause nor the
/// cure. Checked at the source, because reaching either failure needs a terminal
/// this test does not have: the property is which call each context is attached
/// to, and that is a structural fact about the file.
#[test]
fn the_missing_terminal_sentence_is_attached_only_to_the_handshake() {
    let src = include_str!("tui_cmd.rs");
    let production = src.split("#[cfg(test)]").next().unwrap();
    for (call, context) in [
        ("crate::tui::start()", "tui.needs_a_terminal"),
        ("crate::tui::run(terminal, workspace)", "tui.session_failed"),
    ] {
        let at = production
            .find(call)
            .unwrap_or_else(|| panic!("`{call}` is not called"));
        let tail = &production[at..];
        let next = tail
            .find("text::get(\"")
            .unwrap_or_else(|| panic!("`{call}` attaches no context"));
        assert!(
            tail[next..].starts_with(&format!("text::get(\"{context}\")")),
            "`{call}` is not followed by {context}"
        );
    }
    // And exactly one of each, so neither sentence is reused for the other half.
    assert_eq!(production.matches("tui.needs_a_terminal").count(), 1);
    assert_eq!(production.matches("tui.session_failed").count(), 1);
}

/// `--demo` takes an optional scene, defaults to the ordinary day, and refuses
/// anything it cannot draw.
///
/// It used to be a bare flag, which meant `1c` and `1d` — the two artboards that
/// carry the design's argument — were unreachable from the command line.
#[test]
fn the_demo_flag_takes_an_optional_scene() {
    let scene_of = |argv: [&str; 3]| -> Option<String> {
        let matches = command()
            .try_get_matches_from(argv)
            .unwrap_or_else(|error| panic!("`{}` does not parse: {error}", argv.join(" ")));
        matches
            .subcommand_matches("tui")
            .expect("the tui subcommand")
            .get_one::<String>("demo")
            .cloned()
    };
    // The bare flag is the ordinary day...
    assert_eq!(
        scene_of(["mooshik", "tui", "--demo"]).as_deref(),
        Some("today")
    );
    assert_eq!(
        crate::tui::Scene::named(scene_of(["mooshik", "tui", "--demo"]).as_deref()),
        crate::tui::Scene::Today
    );
    // ...and the two named scenes reach the two remaining artboards.
    for (value, expected) in [
        ("recall", crate::tui::Scene::Recall),
        ("caution", crate::tui::Scene::Caution),
        ("today", crate::tui::Scene::Today),
    ] {
        let parsed = scene_of(["mooshik", "tui", &format!("--demo={value}")]);
        assert_eq!(parsed.as_deref(), Some(value));
        assert_eq!(crate::tui::Scene::named(parsed.as_deref()), expected);
    }
    // Without the flag there is no scene, which is the live workspace.
    let matches = command()
        .try_get_matches_from(["mooshik", "tui"])
        .expect("`mooshik tui` must parse");
    assert!(matches
        .subcommand_matches("tui")
        .unwrap()
        .get_one::<String>("demo")
        .is_none());
    // A value nothing can draw is a usage error, not a silently wrong artboard.
    assert!(command()
        .try_get_matches_from(["mooshik", "tui", "--demo=nonsense"])
        .is_err());
}

#[test]
fn every_documented_example_parses_as_written() {
    let src = include_str!("../text/en.toml");
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

// ---------------------------------------------------------------------------
// The configuration write path (`mooshik config set`).
// ---------------------------------------------------------------------------

/// Drive `config set` exactly as argv would: through the real clap tree, so a
/// flag or positional that stops parsing is caught here rather than in a demo.
fn run_config_set(layout: &HomeLayout, args: &[&str]) -> Result<(), Failure> {
    let mut argv = vec!["mooshik", "config", "set"];
    argv.extend_from_slice(args);
    let matches = command()
        .try_get_matches_from(argv)
        .expect("`config set` must parse");
    let sub = matches
        .subcommand_matches("config")
        .unwrap()
        .subcommand_matches("set")
        .unwrap()
        .clone();
    super::configure::set_config(layout, &sub).map_err(Failure::from)
}

/// `config set` must succeed, and a failure must say why in the operator's
/// own words rather than as an opaque `Debug` (which `Failure` deliberately
/// does not implement).
fn config_set_ok(layout: &HomeLayout, args: &[&str]) {
    if let Err(failure) = run_config_set(layout, args) {
        panic!(
            "`config set {}` failed: {}",
            args.join(" "),
            failure.rendered()
        );
    }
}

fn fixture_home(label: &str) -> HomeLayout {
    let root = crate::secure_path::canonical_temp_dir()
        .join(format!("mooshik-set-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = HomeLayout::new(&root);
    layout.init().unwrap();
    layout
}

#[cfg(unix)]
#[test]
fn a_written_setting_round_trips_and_the_file_stays_private() {
    use std::os::unix::fs::PermissionsExt;
    let layout = fixture_home("roundtrip");
    // Widen the mode first: the write must not merely preserve 0600, it must
    // land at 0600 whatever it found.
    std::fs::set_permissions(&layout.config, std::fs::Permissions::from_mode(0o644)).unwrap();

    config_set_ok(&layout, &["companion.model", "gemini-2.5-flash"]);

    let written = std::fs::read_to_string(&layout.config).unwrap();
    assert!(
        written.contains("model = \"gemini-2.5-flash\""),
        "{written}"
    );
    assert_eq!(
        std::fs::metadata(&layout.config)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    // The value survives a real load, and the shipped comments survive with it.
    let reloaded = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(reloaded.companion.model, "gemini-2.5-flash");
    assert!(
        written.contains("# Autonomy is granted, not configured"),
        "{written}"
    );
    assert!(written.contains("[permissions]"), "{written}");
    let _ = std::fs::remove_dir_all(&layout.root);
}

/// The Google posture is reachable without hand-editing a file, which is the
/// point: `init` writes the local default, and two commands turn it into a
/// working Vertex endpoint that was never pasted.
#[test]
fn the_google_posture_is_reachable_from_the_cli_alone() {
    let layout = fixture_home("google");
    config_set_ok(&layout, &["companion.google_project", "mooshik"]);
    config_set_ok(&layout, &["companion.auth", "google"]);
    config_set_ok(&layout, &["companion.model", "gemini-2.5-flash"]);

    let written = std::fs::read_to_string(&layout.config).unwrap();
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(config.companion.auth, crate::config::CompanionAuth::Google);
    assert_eq!(
        config.companion.resolved_base_url(),
        crate::config::vertex_base_url("mooshik", "us-central1")
    );
    // No URL was ever typed, and the local default is still the file's
    // `base_url` — the derivation is what wins.
    assert_eq!(config.companion.base_url, "http://127.0.0.1:8080/v1");
    assert!(!written.contains("aiplatform"), "{written}");
    let _ = std::fs::remove_dir_all(&layout.root);
}

#[test]
fn setting_a_credential_key_is_refused_and_leaves_the_file_untouched() {
    let layout = fixture_home("credential");
    let before = std::fs::read_to_string(&layout.config).unwrap();
    for key in ["store.dsn", "companion.api_key"] {
        let failure = run_config_set(&layout, &[key, "a-value-that-must-not-land"])
            .err()
            .unwrap_or_else(|| panic!("{key} must be refused"));
        assert_eq!(failure.exit_code(), 2);
        let message = failure.rendered();
        assert!(message.contains(key), "{message}");
        assert!(message.contains("mooshik secret set"), "{message}");
        assert!(!message.contains("a-value-that-must-not-land"), "{message}");
    }
    assert_eq!(std::fs::read_to_string(&layout.config).unwrap(), before);
    let _ = std::fs::remove_dir_all(&layout.root);
}

#[test]
fn an_unknown_key_is_a_named_user_error_not_a_silent_no_op() {
    let layout = fixture_home("unknown");
    let before = std::fs::read_to_string(&layout.config).unwrap();
    let failure = run_config_set(&layout, &["companion.modle", "x"])
        .expect_err("an unknown key must be refused");
    assert_eq!(failure.exit_code(), 2);
    let message = failure.rendered();
    assert!(message.contains("companion.modle"), "{message}");
    assert!(message.contains("companion.model"), "{message}");
    assert!(!message.contains("ConfigError"), "{message}");
    assert_eq!(std::fs::read_to_string(&layout.config).unwrap(), before);
    let _ = std::fs::remove_dir_all(&layout.root);
}

/// The database guard, end to end through the CLI.
///
/// A store that names a database cannot be pointed somewhere else without
/// `--confirm-database-change`, because what Mooshik remembers stays where it
/// was written. Cosmetic edits — and edits that touch anything but the store —
/// pass without ceremony.
#[test]
fn moving_the_store_is_refused_until_confirmed_and_never_echoes_the_dsn() {
    let layout = fixture_home("move");
    // A home that already names a database. No password: a repository must not
    // carry a credential-shaped string, and the identity rule that makes a
    // password cosmetic is pinned in `config::write::tests`.
    let dsn = "postgres://mooshik@db.internal/mooshik";
    std::fs::write(
        &layout.config,
        format!("[store]\nkind = \"postgres\"\ndsn = \"{dsn}\"\n"),
    )
    .unwrap();

    // Not a store change at all: no ceremony, no confirmation flag.
    config_set_ok(&layout, &["companion.model", "local-model-2"]);

    let before = std::fs::read_to_string(&layout.config).unwrap();
    let failure = run_config_set(&layout, &["store.kind", "memory"])
        .expect_err("a genuine move must be refused");
    assert_eq!(failure.exit_code(), 2);
    let message = failure.rendered();
    assert!(message.contains("different database"), "{message}");
    assert!(message.contains("stays"), "{message}");
    assert!(message.contains("--confirm-database-change"), "{message}");
    // Not one byte of connection material in the warning.
    assert!(!message.contains(dsn), "{message}");
    assert!(!message.contains("postgres://"), "{message}");
    assert!(!message.contains("db.internal"), "{message}");
    assert!(!message.contains("mooshik@"), "{message}");
    // Refused means unwritten.
    assert_eq!(std::fs::read_to_string(&layout.config).unwrap(), before);

    // Confirmed, it goes through.
    config_set_ok(
        &layout,
        &["store.kind", "memory", "--confirm-database-change"],
    );
    let after = std::fs::read_to_string(&layout.config).unwrap();
    assert!(after.contains("kind = \"memory\""), "{after}");
    let _ = std::fs::remove_dir_all(&layout.root);
}

/// Naming a first database strands nothing, so it must not ask for
/// confirmation. A guard that fires on a no-op is a guard operators learn to
/// ignore.
#[test]
fn naming_a_first_database_needs_no_confirmation() {
    let layout = fixture_home("first");
    // `init` writes `kind = "postgres"` with no DSN: there is nothing behind.
    config_set_ok(&layout, &["store.dsn_secret", "prod-dsn"]);
    let written = std::fs::read_to_string(&layout.config).unwrap();
    assert!(written.contains("dsn_secret = \"prod-dsn\""), "{written}");
    // A NAME landed, never a connection string.
    assert!(!written.contains("postgres://"), "{written}");
    let _ = std::fs::remove_dir_all(&layout.root);
}

#[test]
fn config_set_help_lists_every_settable_key_from_the_writer_itself() {
    let mut cmd = command();
    let help = cmd
        .find_subcommand_mut("config")
        .unwrap()
        .find_subcommand_mut("set")
        .unwrap()
        .render_help()
        .to_string();
    for key in crate::config::settable_keys() {
        assert!(
            help.contains(key),
            "`config set --help` omits {key}: {help}"
        );
    }
    assert!(help.contains("--confirm-database-change"), "{help}");
    // And the two credential keys are NOT advertised as settable.
    assert!(!help.contains("store.dsn "), "{help}");
}
