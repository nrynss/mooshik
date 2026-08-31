use super::*;
use std::io::Cursor;

use crate::vault::PassphraseProvider;
use std::sync::Arc;

/// A verifier that never touches the network.
struct StubVerifier {
    fail_store: bool,
    fail_embedder: bool,
    fail_inference: bool,
}

impl Verifier for StubVerifier {
    fn verify_store(&self, _config: &Config) -> Result<(), String> {
        if self.fail_store {
            Err("connection refused".to_owned())
        } else {
            Ok(())
        }
    }
    fn verify_embedder(&self, _config: &Config) -> Result<(), String> {
        if self.fail_embedder {
            Err("probe failed".to_owned())
        } else {
            Ok(())
        }
    }
    fn verify_inference(&self, _config: &Config) -> Result<(), String> {
        if self.fail_inference {
            Err("unreachable".to_owned())
        } else {
            Ok(())
        }
    }
}

/// A scripted-answer home, passphrase-vaulted so no OS keyring is touched.
fn fixture_home(label: &str) -> HomeLayout {
    // The vault opens through `provider_for`, which reads MOOSHIK_VAULT_PASSPHRASE.
    std::env::set_var("MOOSHIK_VAULT_PASSPHRASE", "test-passphrase");
    let root = crate::secure_path::canonical_temp_dir()
        .join(format!("mooshik-init-flow-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = HomeLayout::new(&root);
    layout.init().unwrap();

    // Passphrase mode, so no keyring is involved.
    let file = std::fs::read_to_string(&layout.config).unwrap();
    let edited = crate::config::apply_setting(&file, "vault.provider", "passphrase", []).unwrap();
    std::fs::write(&layout.config, edited).unwrap();
    layout
}

/// Run the flow with scripted answers, returning output and written config.
fn drive(
    layout: &HomeLayout,
    answers: &str,
    verifier: &dyn Verifier,
    venv: Option<PathBuf>,
    adc: Option<PathBuf>,
) -> (String, String) {
    drive_env(layout, answers, verifier, venv, adc, Vec::new())
}

/// [`drive`] with a non-empty environment, for the env-overlay tests.
fn drive_env(
    layout: &HomeLayout,
    answers: &str,
    verifier: &dyn Verifier,
    venv: Option<PathBuf>,
    adc: Option<PathBuf>,
    environment: Vec<(String, String)>,
) -> (String, String) {
    let mut input = Cursor::new(answers.as_bytes().to_vec());
    let mut output: Vec<u8> = Vec::new();
    let outcome = run_with(
        layout,
        &mut input,
        &mut output,
        venv,
        adc,
        environment,
        false,
        verifier,
    );
    let text = String::from_utf8(output).unwrap();
    assert!(
        outcome.is_ok(),
        "flow failed: {outcome:?}\n--- output ---\n{text}"
    );
    let written = std::fs::read_to_string(&layout.config).unwrap();
    (text, written)
}

fn read_vault(layout: &HomeLayout, name: &str) -> Option<String> {
    let root = crate::secure_path::open_dir(&layout.root, false, 0o700).unwrap();
    let provider = Arc::new(PassphraseProvider::new("test-passphrase").unwrap());
    let vault = Vault::open_at(&layout.vault, root, provider).unwrap();
    vault.get(name).ok().map(|token| token.expose().to_owned())
}

fn seed_config(layout: &HomeLayout, text: &str) {
    std::fs::write(&layout.config, text).unwrap();
}

/// A venv with the named MCP console scripts, so the offer path runs.
fn fake_venv(layout: &HomeLayout, scripts: &[&str]) -> PathBuf {
    let bin = layout.root.join("venv/bin");
    std::fs::create_dir_all(&bin).unwrap();
    for script in scripts {
        std::fs::write(bin.join(script), "#!/bin/sh\n").unwrap();
    }
    layout.root.join("venv")
}

/// An ADC credentials file in the fixture home directory with a quota project.
fn fake_adc(layout: &HomeLayout) -> PathBuf {
    let gcloud = layout.root.join("gcloud");
    std::fs::create_dir_all(&gcloud).unwrap();
    let adc = gcloud.join("application_default_credentials.json");
    std::fs::write(&adc, "{\"quota_project_id\": \"test-quota-project\"}\n").unwrap();
    adc
}

/// An ADC credentials file with valid JSON but no quota project.
fn fake_adc_no_quota(layout: &HomeLayout) -> PathBuf {
    let gcloud = layout.root.join("gcloud");
    std::fs::create_dir_all(&gcloud).unwrap();
    let adc = gcloud.join("application_default_credentials.json");
    std::fs::write(&adc, "{\"client_id\": \"test-client\"}\n").unwrap();
    adc
}

/// An ADC credentials file that is not valid JSON.
fn fake_adc_malformed(layout: &HomeLayout) -> PathBuf {
    let gcloud = layout.root.join("gcloud");
    std::fs::create_dir_all(&gcloud).unwrap();
    let adc = gcloud.join("application_default_credentials.json");
    std::fs::write(&adc, "{ not-valid-json\n").unwrap();
    adc
}
#[test]
fn shared_posture_writes_a_working_config() {
    let layout = fixture_home("shared");
    // posture=1 (shared), store=1, DSN, project, differ default, credentials.
    let answers = "\n\npostgres://user@db.example/mooshik\nproj\n\n/key.json\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        None,
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(config.store.kind, StoreKind::Postgres);
    assert_eq!(config.store.dsn_secret.as_deref(), Some("store-dsn"));
    assert_eq!(config.embedder.gemini_project.as_deref(), Some("proj"));
    assert_eq!(
        config
            .embedder
            .gemini_credentials
            .as_deref()
            .map(Path::as_os_str),
        Some(std::ffi::OsStr::new("/key.json"))
    );
    assert_eq!(config.companion.auth, config::CompanionAuth::Google);
    assert_eq!(config.companion.google_project.as_deref(), Some("proj"));
    assert_eq!(config.companion.google_location.as_deref(), Some("global"));
    assert_eq!(config.companion.model, "google/gemini-3.7-flash");
    assert_eq!(
        config
            .companion
            .google_credentials
            .as_deref()
            .map(Path::as_os_str),
        Some(std::ffi::OsStr::new("/key.json"))
    );
    assert!(!written.contains("postgres://"), "{written}");
    assert!(!written.contains("db.example"), "{written}");
    // Secrets land in the vault.
    assert_eq!(
        read_vault(&layout, "store-dsn").as_deref(),
        Some("postgres://user@db.example/mooshik")
    );
    assert_eq!(
        read_vault(&layout, "gemini-project").as_deref(),
        Some("proj")
    );
    // The plan's two-locations trap is stated when the derivation fires.
    assert!(output.contains("Inference: Vertex Gemini"), "{output}");
    assert!(output.contains("Mooshik is set up."), "{output}");
}

#[test]
fn local_posture_writes_sqlite_and_a_local_companion() {
    let layout = fixture_home("local");
    // posture=2 (local), sqlite path, bge_m3 (default), dim, endpoint, model.
    let answers = "2\n/mnt/memory.db\n\n\nhttp://localhost:1234/v1\nmy-model\nn\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        None,
    );

    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(config.store.kind, StoreKind::Sqlite);
    assert_eq!(config.store.path.as_deref(), Some("/mnt/memory.db"));
    assert_eq!(config.embedder.kind, EmbedderKind::BgeM3);
    assert_eq!(config.companion.base_url, "http://localhost:1234/v1");
    assert_eq!(config.companion.model, "my-model");
    assert_eq!(config.companion.api_key_secret, None);
    assert_eq!(config.companion.auth, config::CompanionAuth::Static);
    assert!(output.contains("Mooshik is set up."), "{output}");
}

#[test]
fn cloud_postgres_choice_says_the_proxy_caveat() {
    let layout = fixture_home("cloud");
    // posture default (shared), store=2 (cloud postgres), DSN, project, credentials.
    let answers = "\n2\npostgres://user@db.example/mooshik\nproj\n\n/key.json\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        None,
    );
    assert!(output.contains("Auth Proxy"), "{output}");
    assert!(written.contains("dsn_secret = \"store-dsn\""), "{written}");
}

#[test]
fn verification_failure_offers_retry_then_continues() {
    let layout = fixture_home("retry");
    // Store verification fails until retry=no: DSN, retry=yes -> DSN again,
    // retry=no; then the embedder and inference answers.
    let answers = "\n\nfirst-dsn\ny\nsecond-dsn\nn\nproj\n\n/key.json\n";
    let verifier = StubVerifier {
        fail_store: true,
        fail_embedder: false,
        fail_inference: false,
    };
    let (output, written) = drive(&layout, answers, &verifier, None, None);
    assert_eq!(
        read_vault(&layout, "store-dsn").as_deref(),
        Some("second-dsn")
    );
    assert!(output.contains("Unverified"), "{output}");
    assert!(
        output.contains("store (connect and provision the schema)"),
        "{output}"
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(config.embedder.gemini_project.as_deref(), Some("proj"));
}

#[test]
fn mcp_servers_are_wired_when_the_venv_is_there() {
    let layout = fixture_home("mcp");
    // posture default, store default, DSN, project, differ default,
    // credentials, news=yes, artifacts=no, coder=yes, agent=claude, key.
    let answers =
        "\n\npostgres://user@db.example/mooshik\nproj\n\n/key.json\n\nn\ny\n\nagent-key\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        Some(fake_venv(
            &layout,
            &[
                "mooshik-news-mcp",
                "mooshik-artifacts-mcp",
                "mooshik-coder-mcp",
            ],
        )),
        None,
    );
    assert!(written.contains("[mcp_servers.news]"), "{written}");
    assert!(written.contains("search_news"), "{written}");
    assert!(!written.contains("[mcp_servers.artifacts]"), "{written}");
    assert!(written.contains("[mcp_servers.coder]"), "{written}");
    assert!(written.contains("\"mcp.news.*\" = \"allow\""), "{written}");
    assert!(
        written.contains("\"mcp.coder.*\" = \"prompt\""),
        "{written}"
    );
    assert!(
        written.contains("MOOSHIK_GEMINI_PROJECT = \"gemini-project\""),
        "{written}"
    );
    assert_eq!(
        read_vault(&layout, "anthropic-api-key").as_deref(),
        Some("agent-key")
    );
    assert!(!written.contains("agent-key"), "{written}");
    // The heading discloses where the installer left the servers.
    assert!(
        output.contains("MCP servers: the installer left them at"),
        "{output}"
    );
    assert!(output.contains("news, coder wired"), "{output}");
}

#[test]
fn rerun_asks_only_for_what_is_still_missing() {
    let layout = fixture_home("rerun");
    let answers = "\n\npostgres://user@db.example/mooshik\nproj\n\n/key.json\n";
    drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        None,
    );
    let before = std::fs::read_to_string(&layout.config).unwrap();

    // Second run: nothing left to ask; the flow must finish without reading.
    let mut input = Cursor::new(Vec::new());
    let mut output: Vec<u8> = Vec::new();
    let outcome = run_with(
        &layout,
        &mut input,
        &mut output,
        None,
        None,
        Vec::new(),
        false,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
    );
    assert!(outcome.is_ok(), "re-run failed: {outcome:?}");
    // Nothing left to ask means the flow never reads: EOF is not reached.
    let after = std::fs::read_to_string(&layout.config).unwrap();
    assert_eq!(before, after, "a re-run must not rewrite the file");
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("Mooshik is set up."), "{output}");
}

#[test]
fn secrets_never_appear_in_the_written_file_or_output() {
    let layout = fixture_home("secrets");
    let adc = fake_adc(&layout);
    let adc_secret = "secret-refresh-token-xyz-987";
    std::fs::write(
        &adc,
        format!(
            "{{\"quota_project_id\": \"test-quota-project\", \"refresh_token\": \"{adc_secret}\"}}\n"
        ),
    )
    .unwrap();

    let secret = "s3cret-dsn-value-with-password";
    let answers = format!("\n\n{secret}\nproj\n\n\n");
    let (output, written) = drive(
        &layout,
        &answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        Some(adc.clone()),
    );
    assert!(!written.contains(secret), "{written}");
    assert!(!output.contains(secret), "{output}");
    assert_eq!(read_vault(&layout, "store-dsn").as_deref(), Some(secret));
    // ADC path is printed in the prompt, but no internal ADC fields ever appear.
    assert!(output.contains(&adc.display().to_string()), "{output}");
    assert!(!output.contains(adc_secret), "{output}");
    assert!(!written.contains(adc_secret), "{written}");
}

#[test]
fn rerun_keeps_a_real_static_endpoint() {
    // A real endpoint still on the shipped local-model default: the
    // re-run must keep the static auth and not derive a Gemini model.
    let layout = fixture_home("static-rerun");
    seed_config(
        &layout,
        r#"vault = { provider = "passphrase" }
store = { kind = "postgres", dsn = "postgres://user@db.example/mooshik" }
embedder = { kind = "gemini", gemini_project = "proj", gemini_credentials = "/key.json" }
companion = { base_url = "https://my-llm.example/v1", model = "local-model", google_location = "global", google_project = "proj", google_credentials = "/key.json" }
"#,
    );
    let (output, written) = drive(
        &layout,
        "",
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        None,
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(config.companion.auth, config::CompanionAuth::Static);
    assert_eq!(config.companion.base_url, "https://my-llm.example/v1");
    assert_eq!(config.companion.model, "local-model");
    assert!(!written.contains("gemini-3.7-flash"), "{written}");
    assert!(!written.contains("auth = \"google\""), "{written}");
    // The two-locations trap belongs to the derivation, not to any
    // static endpoint, so a real one must not hear it.
    assert!(!output.contains("Inference: Vertex Gemini"), "{output}");
    assert!(output.contains("Mooshik is set up."), "{output}");
}

#[test]
fn differ_offer_writes_a_separate_companion_project() {
    let layout = fixture_home("differ");
    // posture default, store default, DSN, project=proj, differ=no -> inference-proj.
    let answers = "\n\npostgres://user@db.example/mooshik\nproj\nn\ninference-proj\n/key.json\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        None,
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(config.embedder.gemini_project.as_deref(), Some("proj"));
    assert_eq!(
        config.companion.google_project.as_deref(),
        Some("inference-proj")
    );
    assert_eq!(config.companion.auth, config::CompanionAuth::Google);
    assert_eq!(
        read_vault(&layout, "gemini-project").as_deref(),
        Some("proj")
    );
    assert!(output.contains("Mooshik is set up."), "{output}");
}

#[test]
fn mcp_offer_gates_on_the_vault_and_restores_missing_gemini_secrets() {
    // Values in the config, nothing in the vault: the offer must hold
    // and the vault be re-stored, so the env-map names resolve at spawn.
    let layout = fixture_home("mcp-gate");
    seed_config(
        &layout,
        r#"vault = { provider = "passphrase" }
store = { kind = "postgres", dsn = "postgres://user@db.example/mooshik" }
embedder = { kind = "gemini", gemini_project = "proj", gemini_credentials = "/key.json" }
companion = { auth = "google", model = "google/gemini-3.7-flash", google_location = "global", google_project = "proj", google_credentials = "/key.json" }
"#,
    );
    let (output, written) = drive(
        &layout,
        "\n\n",
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        Some(fake_venv(
            &layout,
            &["mooshik-news-mcp", "mooshik-artifacts-mcp"],
        )),
        None,
    );
    assert!(written.contains("[mcp_servers.news]"), "{written}");
    assert!(written.contains("[mcp_servers.artifacts]"), "{written}");
    assert_eq!(
        read_vault(&layout, "gemini-project").as_deref(),
        Some("proj")
    );
    assert_eq!(
        read_vault(&layout, "gemini-credentials").as_deref(),
        Some("/key.json")
    );
    assert!(output.contains("news, artifacts wired"), "{output}");
}

#[test]
fn local_rerun_keeps_a_chosen_gemini_embedder() {
    // Interrupted local run: gemini chosen, project entered, credentials missing.
    let layout = fixture_home("gemini-rerun");
    seed_config(
        &layout,
        r#"vault = { provider = "passphrase" }
store = { kind = "sqlite", path = "/mnt/memory.db" }
[embedder]
kind = "gemini"
gemini_project = "proj"
[companion]
base_url = "http://localhost:1234/v1"
model = "my-model"
google_project = "proj"
"#,
    );
    let (output, written) = drive(
        &layout,
        "/key2.json\n",
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        None,
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(config.embedder.kind, EmbedderKind::Gemini);
    assert_eq!(config.embedder.gemini_project.as_deref(), Some("proj"));
    assert_eq!(
        config
            .embedder
            .gemini_credentials
            .as_deref()
            .map(Path::as_os_str),
        Some(std::ffi::OsStr::new("/key2.json"))
    );
    assert_eq!(
        config
            .companion
            .google_credentials
            .as_deref()
            .map(Path::as_os_str),
        Some(std::ffi::OsStr::new("/key2.json"))
    );
    assert_eq!(
        read_vault(&layout, "gemini-credentials").as_deref(),
        Some("/key2.json")
    );
    // The kind question was never asked.
    assert!(!output.contains("bge_m3"), "{output}");
    assert!(output.contains("Mooshik is set up."), "{output}");
}

#[test]
fn local_rerun_kind_default_keeps_an_interrupted_gemini_choice() {
    // Kind set, project never answered (the interruption window): the
    // re-run re-asks the kind but defaults it to gemini, so a plain
    // Enter keeps the choice.
    let layout = fixture_home("gemini-window");
    seed_config(
        &layout,
        r#"vault = { provider = "passphrase" }
store = { kind = "sqlite", path = "/mnt/memory.db" }
[embedder]
kind = "gemini"
[companion]
base_url = "http://localhost:1234/v1"
model = "my-model"
"#,
    );
    let (output, written) = drive(
        &layout,
        "\nproj\n/key.json\n",
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        None,
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(config.embedder.kind, EmbedderKind::Gemini);
    assert_eq!(config.embedder.gemini_project.as_deref(), Some("proj"));
    assert_eq!(
        config
            .embedder
            .gemini_credentials
            .as_deref()
            .map(Path::as_os_str),
        Some(std::ffi::OsStr::new("/key.json"))
    );
    assert!(!written.contains("bge_m3"), "{written}");
    assert!(output.contains("Choice [1]:"), "{output}");
    assert!(output.contains("Mooshik is set up."), "{output}");
}

#[test]
fn env_forced_sqlite_still_defaults_a_fresh_embedder_kind_to_bge_m3() {
    // MOOSHIK_STORE_KIND is a documented overlay channel. On a FRESH
    // file (store.kind still the shipped postgres default) an env-forced
    // sqlite store must not look like a sqlite re-run: the local kind
    // question keeps the bge_m3 default, so a plain Enter picks bge_m3.
    let layout = fixture_home("env-sqlite");
    // posture=2 (local), sqlite path, kind default, dim default,
    // endpoint default, model default, key=no.
    let answers = "2\n/mnt/memory.db\n\n\n\n\nn\n";
    let environment = vec![("MOOSHIK_STORE_KIND".to_owned(), "sqlite".to_owned())];
    let (output, written) = drive_env(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        None,
        environment,
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(config.store.kind, StoreKind::Sqlite);
    assert_eq!(config.embedder.kind, EmbedderKind::BgeM3);
    assert!(!written.contains("kind = \"gemini\""), "{written}");
    assert!(output.contains("Choice [2]:"), "{output}");
    assert!(output.contains("Mooshik is set up."), "{output}");
}

/// [`install_echo_handler`] and [`NoEchoRestore::drop`] need no tty —
/// they are pure `sigaction` installs and restores, so the round trip
/// is testable headless, mirroring the tui session's disposition test.
#[cfg(unix)]
#[test]
fn echo_read_dispositions_are_restored_when_the_guard_drops() {
    let disposition = |signal: libc::c_int| -> libc::sigaction {
        // SAFETY: `out` is a zeroed but valid out-parameter; reading
        // with a null new-disposition pointer installs nothing.
        let mut out: libc::sigaction = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::sigaction(signal, std::ptr::null(), &mut out) };
        assert_eq!(result, 0, "reading the disposition of {signal} failed");
        out
    };
    // SAFETY: `zeroed` is a valid `tcgetattr` out-parameter; on a
    // non-tty stdin (the test runner) the restore is a harmless no-op.
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };
    let _ = unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut termios) };
    for (index, &signal) in NO_ECHO_SIGNALS.iter().enumerate() {
        let before = disposition(signal);
        let mut guard = NoEchoRestore {
            termios,
            previous: [None; 5],
        };
        guard.previous[index] = Some(install_echo_handler(signal).unwrap());
        drop(guard);
        let after = disposition(signal);
        assert_eq!(
            after.sa_sigaction, before.sa_sigaction,
            "signal {signal} was left with the read's handler installed",
        );
    }
}

/// Whether a default-disposition SIGTSTP actually stops a process in
/// this environment. Some sandboxes suppress catchable stops (the
/// process stays in state `S`), which makes the stop/resume path
/// unobservable; the terminate-class round trip below then verifies
/// the raise instead.
#[cfg(unix)]
fn sigtstp_stop_is_observable() -> bool {
    // SAFETY: fork, set the default disposition, raise and waitpid on
    // a short-lived grandchild; the grandchild calls only `_exit`.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed");
    if pid == 0 {
        unsafe { libc::signal(libc::SIGTSTP, libc::SIG_DFL) };
        unsafe { libc::raise(libc::SIGTSTP) };
        unsafe { libc::_exit(42) };
    }
    let mut status: libc::c_int = 0;
    let mut stopped = false;
    loop {
        let waited = unsafe { libc::waitpid(pid, &mut status, libc::WUNTRACED) };
        assert_eq!(waited, pid, "waitpid failed");
        if libc::WIFSTOPPED(status) {
            stopped = true;
            let resumed = unsafe { libc::kill(pid, libc::SIGCONT) };
            assert_eq!(resumed, 0, "SIGCONT failed");
            continue;
        }
        break;
    }
    stopped
}

/// The handler body's raise path, exercised headless. A child installs
/// the SIGTSTP handler, raises it and stops (the default disposition
/// after `restore_echo_and_raise` re-raises); the parent observes the
/// stop, resumes the child, and the child asserts the no-echo state and
/// the handler are back in place, then that dropping the guard restores
/// the dispositions. Without the `raise` the child never stops and the
/// parent's `WIFSTOPPED` assertion fails. Where the sandbox suppresses
/// catchable stops, the resume path is not observable and the test says
/// so; `echo_read_handler_raise_terminates_with_the_default_disposition`
/// still gates the raise there.
#[cfg(unix)]
#[test]
fn echo_read_handler_raises_and_rearms_after_a_stop_resume() {
    if !sigtstp_stop_is_observable() {
        // Live PTY runs on this machine confirmed the default SIGTSTP
        // action leaves a process in state `S`, never `T`, so the
        // stop/resume path cannot be exercised in-suite here.
        eprintln!(
            "note: SIGTSTP stops are suppressed in this environment; \
                 the stop/resume path is not observable in-suite"
        );
        return;
    }
    let disposition = |signal: libc::c_int| -> libc::sigaction {
        // SAFETY: `out` is a zeroed but valid out-parameter; reading
        // with a null new-disposition pointer installs nothing.
        let mut out: libc::sigaction = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::sigaction(signal, std::ptr::null(), &mut out) };
        assert_eq!(result, 0, "reading the disposition of {signal} failed");
        out
    };
    // SAFETY: `zeroed` is a valid `tcgetattr` out-parameter; on a
    // non-tty stdin (the test runner) the restores are harmless no-ops.
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };
    let _ = unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut termios) };
    let handler = restore_echo_and_raise as *const () as libc::sighandler_t;
    // SAFETY: `fork` from the test process; the child touches only
    // async-signal-safe libc calls and `_exit`, so the forked
    // multi-threaded state is never used for allocation or I/O.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed");
    if pid == 0 {
        // The child is the no-echo read: termios saved, handler armed.
        let before = disposition(libc::SIGTSTP);
        let mut guard = NoEchoRestore {
            termios,
            previous: [None; 5],
        };
        guard.previous[1] = Some(install_echo_handler(libc::SIGTSTP).unwrap());
        unsafe { ECHO_TERMIOS = Some(termios) };
        // Ctrl-Z: the handler restores echo, re-raises with the default
        // disposition, and the process stops until the parent resumes.
        unsafe { libc::raise(libc::SIGTSTP) };
        // Resumed: the re-arm block ran, so the no-echo state and the
        // handler are back in place.
        let mut ok =
            unsafe { ECHO_TERMIOS }.is_some() && disposition(libc::SIGTSTP).sa_sigaction == handler;
        drop(guard);
        // The guard's drop restored the termios, the dispositions and
        // cleared `ECHO_TERMIOS`, closing the read.
        ok &= unsafe { ECHO_TERMIOS }.is_none()
            && disposition(libc::SIGTSTP).sa_sigaction == before.sa_sigaction;
        unsafe { libc::_exit(if ok { 0 } else { 1 }) };
    }
    let mut status: libc::c_int = 0;
    let mut stopped = false;
    loop {
        let waited = unsafe { libc::waitpid(pid, &mut status, libc::WUNTRACED) };
        assert_eq!(waited, pid, "waitpid failed");
        if libc::WIFSTOPPED(status) {
            stopped = true;
            assert_eq!(
                libc::WSTOPSIG(status),
                libc::SIGTSTP,
                "child stopped by the wrong signal",
            );
            assert_eq!(
                unsafe { libc::kill(pid, libc::SIGCONT) },
                0,
                "SIGCONT failed",
            );
            continue;
        }
        break;
    }
    // The probe above proved stops work here, so a child that never
    // stopped means the handler's `raise` is missing.
    assert!(stopped, "the child never stopped: the raise is missing");
    assert!(libc::WIFEXITED(status), "child did not exit normally");
    assert_eq!(
        libc::WEXITSTATUS(status),
        0,
        "child's post-resume assertions failed",
    );
}

/// The same raise line through the terminate class, which every
/// environment supports. A child installs the SIGTERM handler, raises
/// it and is killed by the re-raised default action; without the
/// `raise` the handler returns, the child exits and this fails.
#[cfg(unix)]
#[test]
fn echo_read_handler_raise_terminates_with_the_default_disposition() {
    // SAFETY: `zeroed` is a valid `tcgetattr` out-parameter; on a
    // non-tty stdin (the test runner) the restore is a harmless no-op.
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };
    let _ = unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut termios) };
    // SAFETY: `fork` from the test process; the child touches only
    // async-signal-safe libc calls and `_exit`.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed");
    if pid == 0 {
        let mut guard = NoEchoRestore {
            termios,
            previous: [None; 5],
        };
        guard.previous[4] = Some(install_echo_handler(libc::SIGTERM).unwrap());
        unsafe { ECHO_TERMIOS = Some(termios) };
        // `kill <pid>`: the handler restores echo and re-raises SIGTERM
        // with the default disposition, which terminates the child.
        unsafe { libc::raise(libc::SIGTERM) };
        // Reached only when the handler swallowed the signal (the bug).
        unsafe { libc::_exit(2) };
    }
    let mut status: libc::c_int = 0;
    let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
    assert_eq!(waited, pid, "waitpid failed");
    assert!(
        libc::WIFSIGNALED(status),
        "child was not terminated by the re-raised signal: the raise is missing",
    );
    assert_eq!(
        libc::WTERMSIG(status),
        libc::SIGTERM,
        "child died by the wrong signal",
    );
}

#[test]
fn google_inference_retry_re_asks_the_credentials() {
    let layout = fixture_home("inf-retry");
    // posture default, store default, DSN, project, differ default,
    // credentials; inference fails, retry=yes -> re-ask, retry=no -> continue.
    let answers = "\n\npostgres://user@db.example/mooshik\nproj\n\n/key.json\ny\n/key2.json\nn\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: true,
        },
        None,
        None,
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(
        config
            .companion
            .google_credentials
            .as_deref()
            .map(Path::as_os_str),
        Some(std::ffi::OsStr::new("/key2.json"))
    );
    assert_eq!(
        config
            .embedder
            .gemini_credentials
            .as_deref()
            .map(Path::as_os_str),
        Some(std::ffi::OsStr::new("/key2.json"))
    );
    assert_eq!(
        read_vault(&layout, "gemini-credentials").as_deref(),
        Some("/key2.json")
    );
    assert!(output.contains("Unverified"), "{output}");
    assert!(
        output.contains("inference (one cheap completion)"),
        "{output}"
    );
}

#[test]
fn adc_present_enter_accepts_the_detected_path() {
    let layout = fixture_home("adc-enter");
    let adc = fake_adc(&layout);
    // posture default, store default, DSN, project, differ default, Enter for ADC.
    let answers = "\n\npostgres://user@db.example/mooshik\nproj\n\n\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        Some(adc.clone()),
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(
        config.embedder.gemini_credentials.as_deref(),
        Some(adc.as_path())
    );
    assert_eq!(
        config.companion.google_credentials.as_deref(),
        Some(adc.as_path())
    );
    assert_eq!(
        read_vault(&layout, "gemini-credentials").as_deref(),
        Some(adc.to_str().unwrap())
    );
    // Output prints the detected path in the prompt.
    assert!(output.contains(&adc.display().to_string()), "{output}");
    assert!(output.contains("Mooshik is set up."), "{output}");
}

#[test]
fn adc_present_typed_path_overrides_the_default() {
    let layout = fixture_home("adc-override");
    let adc = fake_adc(&layout);
    // posture default, store default, DSN, project, differ default, typed path.
    let answers = "\n\npostgres://user@db.example/mooshik\nproj\n\n/custom-key.json\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        Some(adc.clone()),
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(
        config
            .embedder
            .gemini_credentials
            .as_deref()
            .map(Path::as_os_str),
        Some(std::ffi::OsStr::new("/custom-key.json"))
    );
    assert_eq!(
        config
            .companion
            .google_credentials
            .as_deref()
            .map(Path::as_os_str),
        Some(std::ffi::OsStr::new("/custom-key.json"))
    );
    assert_eq!(
        read_vault(&layout, "gemini-credentials").as_deref(),
        Some("/custom-key.json")
    );
    assert!(output.contains(&adc.display().to_string()), "{output}");
    assert!(output.contains("Mooshik is set up."), "{output}");
}

#[test]
fn adc_absent_credentials_question_is_unchanged() {
    let layout = fixture_home("adc-absent");
    // posture default, store default, DSN, project, differ default, key.
    let answers = "\n\npostgres://user@db.example/mooshik\nproj\n\n/key.json\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        None,
    );
    let prompt = text::get("init.embedder_gemini_credentials");
    assert!(output.contains(prompt), "{output}");
    let adc_prompt_prefix = text::get("init.embedder_gemini_credentials_adc")
        .split('{')
        .next()
        .unwrap()
        .to_owned();
    assert!(!output.contains(&adc_prompt_prefix), "{output}");
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(
        config
            .embedder
            .gemini_credentials
            .as_deref()
            .map(Path::as_os_str),
        Some(std::ffi::OsStr::new("/key.json"))
    );
}

#[test]
fn embedder_retry_with_adc_offers_the_same_default() {
    let layout = fixture_home("adc-retry");
    let adc = fake_adc(&layout);
    // Embedder verification fails: retry=yes -> Enter (accept ADC), retry=no -> continue.
    let answers = "\n\npostgres://user@db.example/mooshik\nproj\n\n/bad-key.json\ny\n\nn\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: true,
            fail_inference: false,
        },
        None,
        Some(adc.clone()),
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(
        config.embedder.gemini_credentials.as_deref(),
        Some(adc.as_path())
    );
    assert_eq!(
        config.companion.google_credentials.as_deref(),
        Some(adc.as_path())
    );
    assert_eq!(
        read_vault(&layout, "gemini-credentials").as_deref(),
        Some(adc.to_str().unwrap())
    );
    assert!(output.contains(&adc.display().to_string()), "{output}");
}

#[test]
fn adc_path_is_printed_but_file_contents_never_appear() {
    let layout = fixture_home("adc-contents");
    let adc = fake_adc(&layout);
    let sentinel = "super-secret-adc-token-content-12345";
    std::fs::write(
        &adc,
        format!(
            "{{\"quota_project_id\": \"test-quota-project\", \"token\": \"{sentinel}\", \"client_secret\": \"super-secret-key-67890\"}}\n"
        ),
    )
    .unwrap();

    let answers = "\n\npostgres://user@db.example/mooshik\nproj\n\n\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        Some(adc.clone()),
    );
    assert!(output.contains(&adc.display().to_string()), "{output}");
    assert!(!output.contains(sentinel), "{output}");
    assert!(!output.contains("super-secret-key-67890"), "{output}");
    assert!(!written.contains(sentinel), "{written}");
    assert!(!written.contains("super-secret-key-67890"), "{written}");
}

#[test]
fn adc_with_quota_project_runs_silently_without_guidance() {
    let layout = fixture_home("adc-with-quota");
    let adc = fake_adc(&layout);
    // posture default, store default, DSN, project, differ default, Enter for ADC.
    let answers = "\n\npostgres://user@db.example/mooshik\nproj\n\n\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        Some(adc.clone()),
    );
    assert!(output.contains("Mooshik is set up."), "{output}");
    assert!(
        !output.contains("set-quota-project"),
        "A correct ADC setup must not be nagged: {output}"
    );
    assert!(
        !output.contains("application-default login"),
        "A correct ADC setup must not show login guidance: {output}"
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(
        config.embedder.gemini_credentials.as_deref(),
        Some(adc.as_path())
    );
}

#[test]
fn adc_without_quota_project_stops_early_and_names_set_quota_project() {
    let layout = fixture_home("adc-without-quota");
    let adc = fake_adc_no_quota(&layout);
    // posture default, store default, DSN. The flow stops before Google questions.
    let answers = "\n\npostgres://user@db.example/mooshik\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        Some(adc),
    );
    assert!(output.contains("set-quota-project"), "{output}");
    assert!(output.contains("mooshik init"), "{output}");
    assert!(
        !output.contains("Mooshik is set up."),
        "Flow must stop before completing: {output}"
    );
    assert!(
        !output.contains("Embedder: one probe string embedded."),
        "Flow must not attempt verification: {output}"
    );
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(config.embedder.gemini_project, None);
    assert_eq!(config.embedder.gemini_credentials, None);
}

#[test]
fn adc_absent_prints_guidance_and_asks_for_key_path() {
    let layout = fixture_home("adc-absent-guidance");
    // posture default, store default, DSN, project, differ default, key path.
    let answers = "\n\npostgres://user@db.example/mooshik\nproj\n\n/key.json\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        None,
    );
    let guidance = text::get("init.adc_not_found_guidance");
    let guidance_snippet = guidance.lines().next().unwrap();
    assert!(output.contains(guidance_snippet), "{output}");
    assert!(output.contains("application-default login"), "{output}");
    assert!(output.contains("set-quota-project"), "{output}");
    assert!(
        output.contains(text::get("init.embedder_gemini_credentials")),
        "{output}"
    );
    assert!(output.contains("Mooshik is set up."), "{output}");
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(
        config
            .embedder
            .gemini_credentials
            .as_deref()
            .map(Path::as_os_str),
        Some(std::ffi::OsStr::new("/key.json"))
    );
}

#[test]
fn malformed_or_unreadable_adc_file_is_treated_as_absent() {
    let layout = fixture_home("adc-malformed");
    let adc = fake_adc_malformed(&layout);
    // posture default, store default, DSN, project, differ default, key path.
    let answers = "\n\npostgres://user@db.example/mooshik\nproj\n\n/key.json\n";
    let (output, written) = drive(
        &layout,
        answers,
        &StubVerifier {
            fail_store: false,
            fail_embedder: false,
            fail_inference: false,
        },
        None,
        Some(adc),
    );
    assert!(output.contains("application-default login"), "{output}");
    assert!(
        output.contains(text::get("init.embedder_gemini_credentials")),
        "{output}"
    );
    assert!(output.contains("Mooshik is set up."), "{output}");
    let config = Config::from_toml_and_env(&written, []).unwrap();
    assert_eq!(
        config
            .embedder
            .gemini_credentials
            .as_deref()
            .map(Path::as_os_str),
        Some(std::ffi::OsStr::new("/key.json"))
    );
}

#[test]
fn non_tty_unattended_init_prints_no_guidance_and_never_stops() {
    let layout = fixture_home("non-tty");
    let db_path = layout.root.join("test.db");
    seed_config(
        &layout,
        &format!(
            "vault = {{ provider = \"passphrase\" }}\nstore = {{ kind = \"sqlite\", path = \"{}\" }}\n",
            db_path.display()
        ),
    );
    let matches = clap::Command::new("test")
        .arg(
            clap::Arg::new("non_interactive")
                .long("non-interactive")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches_from(["test", "--non-interactive"]);
    let result = crate::cli::memory_cmd::initialize(&layout, &matches);
    assert!(result.is_ok(), "unattended init must succeed: {result:?}");
    let root = std::fs::File::open(&layout.root).unwrap();
    let config = Config::load_at(&root).unwrap();
    assert_eq!(config.store.kind, StoreKind::Sqlite);
}

#[test]
fn en_toml_contains_no_hardcoded_model_ids() {
    let en = include_str!("../text/en.toml");
    // Model IDs in user-facing strings and comments must be substituted from
    // constants, never hardcoded. This guard catches drift after a model bump.
    for line in en.lines() {
        assert!(
            !line.contains("gemini-3."),
            "en.toml contains a hardcoded inference model: {line}"
        );
        assert!(
            !line.contains("gemini-embedding"),
            "en.toml contains a hardcoded embedding model: {line}"
        );
    }
}

#[test]
fn rust_shared_model_is_the_prefixed_python_default() {
    // The Rust constant carries `google/` because Vertex's OpenAI-compatible
    // endpoint needs the publisher prefix. The Python DEFAULT_MODEL carries
    // the bare name because google-genai rejects the prefix. The invariant:
    // SHARED_MODEL == "google/" + DEFAULT_MODEL. A cargo test is cheaper
    // than a CI grep and runs on every commit.
    let python = include_str!("../../mooshik-common/mooshik_common/models.py");
    let default_model = python
        .lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("DEFAULT_MODEL = \"")
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .expect("models.py must define DEFAULT_MODEL");
    let expected = format!("google/{default_model}");
    assert_eq!(
        crate::config::SHARED_MODEL,
        expected,
        "SHARED_MODEL ({}) must equal google/ + Python DEFAULT_MODEL ({default_model})",
        crate::config::SHARED_MODEL,
    );
}
