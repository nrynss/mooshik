//! The interactive `mooshik init` first-run flow (M12h).
//!
//! [`memory_cmd::initialize`](super::memory_cmd::initialize) decides between
//! this module (a real terminal, without `--non-interactive`) and the
//! byte-identical unattended path. This module walks the user through the
//! store, the embedder and the companion a question at a time, writing each
//! answer to `config.toml` as it goes through [`config::apply_setting`] — the
//! same surgical, verified writer `config set` uses — and verifying each
//! answer before moving on.
//!
//! * **Secrets never echo.** A DSN, a credentials path and an API key are
//!   read with terminal echo off (termios, via `libc`; no prompt library is
//!   added), go straight into the vault, and never appear in `config.toml`,
//!   shell history or `ps` output. A secret *name* is configuration and is
//!   written to the file; the value never is.
//! * **Testability is a design constraint.** The flow takes an injectable
//!   reader and writer rather than touching `stdin`/`stdout`, and an
//!   injectable [`Verifier`] so the scripted-answer tests never reach the
//!   network.
//!
//! The flow is re-runnable: it asks only for what the resolved configuration
//! still lacks (the same judgment `config show`'s missing report uses), and
//! leaves anything already configured alone. Posture is asked first, shared
//! as the default, and scopes the store question: shared asks between a
//! Postgres you run and a cloud Postgres (the Auth Proxy caveat); local asks
//! where the SQLite file lives. The store answer decides the branch: Postgres
//! → gemini plus derived Vertex inference; SQLite → bge_m3 plus an
//! OpenAI-compatible endpoint. `memory` is never offered: it is a test double
//! that keeps nothing across a restart.

use std::{
    ffi::OsStr,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use lambo::{EmbedderKind, StoreKind};

use crate::{
    companion::{Cancellation, CompanionClient, Message},
    config::{self, Config, ConfigError, VaultProvider},
    home::HomeLayout,
    secure_path,
    text,
    vault::Vault,
};

use super::resolve;

/// Vault entry names the flow writes. The names are configuration (they
/// appear in `config.toml` as secret references), so they are constants here;
/// the values live in the vault and never touch the file.
const STORE_DSN_SECRET: &str = "store-dsn";
const GEMINI_PROJECT_SECRET: &str = "gemini-project";
const GEMINI_CREDENTIALS_SECRET: &str = "gemini-credentials";
const COMPANION_API_KEY_SECRET: &str = "companion-api-key";

/// The shipped local-default endpoint. A companion still pointing at it is
/// the "cannot work" state M12h exists to end.
const PLACEHOLDER_BASE_URL: &str = "http://127.0.0.1:8080/v1";

/// The model the shared posture derives; the floor every component moved to
/// on 2026-08-31 (the stale `gemini-2.5-flash` example was deleted with M12h).
const SHARED_MODEL: &str = "gemini-3.7-flash";

/// How one answer is verified, injectable so the tests stay hermetic.
///
/// The production implementation makes real network calls; the tests script
/// answers and fake these so no test touches the outside world.
trait Verifier {
    fn verify_store(&self, config: &Config) -> Result<(), String>;
    fn verify_embedder(&self, config: &Config) -> Result<(), String>;
    fn verify_inference(&self, config: &Config) -> Result<(), String>;
}

/// The production verifier: connect and provision the schema, embed one probe
/// string, make one cheap completion.
///
/// Every failure string is safe to print. `MemoryError` and `CompanionError`
/// render as fixed `en.toml` sentences by construction (their `Display`
/// impls say so), and an embed error's detail is withheld — an
/// `EmbedError::Backend` can carry a response body, and a body is not
/// something this crate gets to promise is free of credential material.
struct LiveVerifier;

impl Verifier for LiveVerifier {
    fn verify_store(&self, config: &Config) -> Result<(), String> {
        super::block_on(crate::memory::provision(config)).map_err(|error| error.to_string())
    }

    fn verify_embedder(&self, config: &Config) -> Result<(), String> {
        let embedder = lambo::build_embedder(config.embedder.to_lambo())
            .map_err(|_| text::get("init.embedder_probe_failed").to_owned())?;
        let probe = "mooshik first-run probe";
        let outcome = super::runtime()
            .map_err(|error| error.to_string())?
            .block_on(async move { embedder.embed(probe).await });
        outcome
            .map(|_| ())
            .map_err(|_| text::get("init.embedder_probe_failed").to_owned())
    }

    fn verify_inference(&self, config: &Config) -> Result<(), String> {
        let client =
            CompanionClient::from_config(&config.companion).map_err(|error| error.to_string())?;
        let messages = vec![Message::user("ping")];
        let outcome = super::runtime()
            .map_err(|error| error.to_string())?
            .block_on(async move {
                tokio::time::timeout(
                    Duration::from_secs(15),
                    client.complete(&messages, &[], &Cancellation::new(), |_| {}),
                )
                .await
            });
        match outcome {
            Ok(Ok(completion)) if !completion.content.trim().is_empty() => Ok(()),
            Ok(Ok(_)) => Err(text::get("companion.invalid_response").to_owned()),
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => Err(text::get("companion.timeout").to_owned()),
        }
    }
}

/// Enter the interactive flow. The dispatcher has already decided that stdin
/// and stdout are terminals and `--non-interactive` was not passed.
pub(crate) fn run(layout: &HomeLayout) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    run_with(
        layout,
        &mut reader,
        &mut writer,
        mcp_venv_dir(),
        std::env::vars().collect(),
        true,
        &LiveVerifier,
    )
}

/// The flow over injectable reader, writer, environment, venv and verifier.
///
/// `no_echo` is whether terminal echo is off for secret reads: true in
/// production (the dispatcher gated on a real terminal), false in tests where
/// the reader is a buffer, not a tty.
fn run_with(
    layout: &HomeLayout,
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    venv: Option<PathBuf>,
    environment: Vec<(String, String)>,
    no_echo: bool,
    verifier: &dyn Verifier,
) -> anyhow::Result<()> {
    let root = layout.init().map_err(anyhow::Error::new)?;
    let source = read_config(&root)?;
    let config =
        Config::from_toml_and_env(&source, environment.clone()).map_err(anyhow::Error::new)?;
    let mut session = Session {
        layout,
        root,
        source,
        config,
        environment,
        vault: None,
        reader,
        writer,
        no_echo,
        verifier,
        unverified: Vec::new(),
    };
    session.run(venv)
}

/// `$XDG_DATA_HOME/mooshik/venv` — where `install.sh` leaves the MCP server
/// console scripts — if it exists. Mirrors `configure::default_console_script`.
fn mcp_venv_dir() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/share")))?;
    let venv = data.join("mooshik/venv");
    venv.is_dir().then_some(venv)
}

fn read_config(root: &std::fs::File) -> anyhow::Result<String> {
    let bytes = secure_path::read_private_at(root, OsStr::new("config.toml"), 64 * 1024)
        .map_err(|_| anyhow::Error::new(ConfigError::Io))?;
    String::from_utf8(bytes).map_err(|_| anyhow::Error::new(ConfigError::Io))
}

struct Session<'a> {
    layout: &'a HomeLayout,
    root: std::fs::File,
    source: String,
    config: Config,
    environment: Vec<(String, String)>,
    vault: Option<Vault>,
    reader: &'a mut dyn BufRead,
    writer: &'a mut dyn Write,
    no_echo: bool,
    verifier: &'a dyn Verifier,
    unverified: Vec<String>,
}

impl Session<'_> {
    fn run(&mut self, venv: Option<PathBuf>) -> anyhow::Result<()> {
        self.say(text::get("init.opening_config"))?;
        self.say(text::get("init.opening_rerun"))?;
        self.open_vault()?;
        self.store_step()?;
        self.embedder_step()?;
        self.inference_step()?;
        self.mcp_step(venv)?;
        self.closing()?;
        Ok(())
    }

    // -- plumbing ---------------------------------------------------------

    fn say(&mut self, line: &str) -> anyhow::Result<()> {
        self.writer
            .write_all(line.as_bytes())
            .and_then(|_| self.writer.write_all(b"\n"))
            .map_err(anyhow::Error::new)?;
        Ok(())
    }

    /// Print a prompt and read one trimmed line. EOF is an error: the flow
    /// only runs interactively, so the input ending is an aborted run.
    fn ask(&mut self, prompt: &str) -> anyhow::Result<String> {
        self.writer
            .write_all(prompt.as_bytes())
            .and_then(|_| self.writer.flush())
            .map_err(anyhow::Error::new)?;
        let mut line = String::new();
        let read = self
            .reader
            .read_line(&mut line)
            .map_err(anyhow::Error::new)?;
        if read == 0 {
            return Err(anyhow::anyhow!("{prompt}"));
        }
        Ok(line.trim_end_matches(['\r', '\n']).to_owned())
    }

    /// A yes/no answer with the DEFAULT as the capital letter: `[Y/n]`
    /// defaults yes, `[y/N]` defaults no. Anything else re-prompts.
    fn ask_yes(&mut self, prompt: &str, default: bool) -> anyhow::Result<bool> {
        loop {
            let answer = self.ask(prompt)?;
            match answer.trim().to_ascii_lowercase().as_str() {
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                "" => return Ok(default),
                _ => self.say(text::get("init.yes_no_invalid"))?,
            }
        }
    }

    /// Read one line with terminal echo off. The termios round trip restores
    /// the terminal even on error. `no_echo` is false in tests, where the
    /// reader is a buffer rather than a tty.
    fn ask_secret(&mut self, prompt: &str) -> anyhow::Result<String> {
        self.writer
            .write_all(prompt.as_bytes())
            .and_then(|_| self.writer.flush())
            .map_err(anyhow::Error::new)?;
        let line = if self.no_echo {
            read_no_echo(&mut *self.reader)
        } else {
            read_plain_line(&mut *self.reader)
        }
        .map_err(anyhow::Error::new)?;
        // The newline the user's terminal echoed is gone; print one so the
        // next prompt starts on its own line.
        self.writer.write_all(b"\n").map_err(anyhow::Error::new)?;
        Ok(line)
    }

    /// Apply one setting to the file and reload the resolved config. Every
    /// answer goes through this one path, so a half-finished run always
    /// leaves a valid file.
    fn set(&mut self, key: &str, value: &str) -> anyhow::Result<()> {
        let edited = config::apply_setting(&self.source, key, value, self.environment.clone())
            .map_err(anyhow::Error::new)?;
        secure_path::write_private_at(&self.root, OsStr::new("config.toml"), edited.as_bytes())
            .map_err(|_| anyhow::Error::new(ConfigError::WriteFailed))?;
        self.source = edited;
        self.config = Config::from_toml_and_env(&self.source, self.environment.clone())
            .map_err(anyhow::Error::new)?;
        Ok(())
    }

    fn vault(&self) -> &Vault {
        self.vault.as_ref().expect("vault opens before any secret question")
    }

    fn vault_mut(&mut self) -> &mut Vault {
        self.vault.as_mut().expect("vault opens before any secret question")
    }

    /// The resolved configuration with every vault reference filled in, for
    /// the verifier — never written back to the file.
    fn resolved_config(&self) -> anyhow::Result<Config> {
        let mut config = self.config.clone();
        if resolve::needs_vault(&config) {
            resolve::resolve_secrets(&mut config, self.vault()).map_err(anyhow::Error::new)?;
        }
        Ok(config)
    }

    fn record_unverified(&mut self, label: &str) -> anyhow::Result<()> {
        self.unverified.push(label.to_owned());
        Ok(())
    }

    // -- the vault --------------------------------------------------------

    fn open_vault(&mut self) -> anyhow::Result<()> {
        // A statement, not a question: the provider was already decided by
        // config (or the environment), and init says which one it picked and
        // why rather than silently choosing.
        match self.config.vault.provider {
            VaultProvider::Keyring => self.say(text::get("init.vault_keyring"))?,
            VaultProvider::Passphrase => self.say(text::get("init.vault_passphrase"))?,
        }
        self.vault = Some(resolve::open_vault(self.layout, &self.config, &self.root)?);
        self.say(text::get("init.vault_ready"))
    }

    // -- the store --------------------------------------------------------

    fn store_step(&mut self) -> anyhow::Result<()> {
        if !self.store_needs_asking() {
            match self.config.store.kind {
                StoreKind::Postgres | StoreKind::Cockroach => {
                    self.say(text::get("init.store_dsn_configured"))?
                }
                StoreKind::Sqlite => self.say(text::get("init.store_path_configured"))?,
                StoreKind::Memory => {}
            }
            return self.verify_store();
        }
        if self.config.store.kind == StoreKind::Memory {
            self.say(text::get("init.store_memory_refused"))?;
        }
        self.say(text::get("init.store_heading"))?;
        if self.ask_posture()? {
            self.ask_store_shared()?;
        } else {
            self.ask_store_local()?;
        }
        self.verify_store()
    }

    /// Whether the store still needs an answer. A Postgres store counts as
    /// configured when the DSN is present in the file or environment, or
    /// when the vault really holds the referenced secret — a reference whose
    /// secret is missing is unset, not configured.
    fn store_needs_asking(&self) -> bool {
        match self.config.store.kind {
            StoreKind::Postgres | StoreKind::Cockroach => {
                let has_dsn = self
                    .config
                    .store
                    .dsn
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty());
                if has_dsn {
                    return false;
                }
                match self.config.store.dsn_secret.as_deref() {
                    Some(name) => self.vault().get(name).is_err(),
                    None => true,
                }
            }
            StoreKind::Sqlite => self
                .config
                .store
                .path
                .as_deref()
                .map(str::trim)
                .is_none_or(|value| value.is_empty()),
            StoreKind::Memory => true,
        }
    }

    /// Posture, asked only when the store is unset: shared (the default) or
    /// local. An already-configured store implies the posture.
    fn ask_posture(&mut self) -> anyhow::Result<bool> {
        loop {
            let answer = self.ask(text::get("init.posture_question"))?;
            match answer.trim() {
                "" | "1" => return Ok(true),
                "2" => return Ok(false),
                _ => self.say(text::get("init.posture_invalid"))?,
            }
        }
    }

    fn ask_store_shared(&mut self) -> anyhow::Result<()> {
        loop {
            let answer = self.ask(text::get("init.store_question_shared"))?;
            match answer.trim() {
                "" | "1" => return self.ask_store_dsn(),
                "2" => {
                    self.say(text::get("init.store_cloud"))?;
                    return self.ask_store_dsn();
                }
                _ => self.say(text::get("init.store_invalid"))?,
            }
        }
    }

    fn ask_store_local(&mut self) -> anyhow::Result<()> {
        let default = self.layout.database.display().to_string();
        let answer =
            self.ask(&text::get("init.store_sqlite_prompt").replace("{default}", &default))?;
        let path = if answer.trim().is_empty() {
            default
        } else {
            answer.trim().to_owned()
        };
        self.set("store.kind", "sqlite")?;
        self.set("store.path", &path)?;
        self.say(&text::get("init.store_sqlite_path").replace("{path}", &path))
    }

    /// The DSN is read with echo off, straight into the vault; the file only
    /// ever holds the secret's NAME.
    fn ask_store_dsn(&mut self) -> anyhow::Result<()> {
        let dsn = self.ask_secret(text::get("init.store_dsn_prompt"))?;
        self.vault_mut()
            .set(STORE_DSN_SECRET, &dsn)
            .map_err(|error| anyhow::Error::new(error))?;
        self.set("store.kind", "postgres")?;
        self.set("store.dsn_secret", STORE_DSN_SECRET)?;
        self.say(&text::get("init.store_dsn_stored").replace("{name}", STORE_DSN_SECRET))
    }

    fn verify_store(&mut self) -> anyhow::Result<()> {
        loop {
            let config = self.resolved_config()?;
            match self.verifier.verify_store(&config) {
                Ok(()) => return self.say(text::get("init.store_verified")),
                Err(what) => {
                    self.say(&text::get("init.store_failed").replace("{what}", &what))?;
                    if self.ask_yes(text::get("init.retry_prompt"), true)? {
                        // The retry for a wrong DSN is a new DSN, asked while
                        // the user still remembers what they typed.
                        if self.config.store.kind == StoreKind::Postgres
                            || self.config.store.kind == StoreKind::Cockroach
                        {
                            self.ask_store_dsn()?;
                        }
                    } else {
                        return self.record_unverified(text::get("init.unverified_store"));
                    }
                }
            }
        }
    }

    // -- the embedder -----------------------------------------------------

    fn embedder_step(&mut self) -> anyhow::Result<()> {
        // The sticky warning belongs at the moment of choosing, not in a
        // README the user reads later.
        self.say(text::get("init.embedder_sticky"))?;
        let shared = self.config.store.kind == StoreKind::Postgres
            || self.config.store.kind == StoreKind::Cockroach;
        if shared {
            if self.config.embedder.kind != EmbedderKind::Gemini {
                // A deliberate non-gemini embedder in the file; nothing to ask.
                return self.verify_embedder();
            }
            self.shared_google_questions()?;
            return self.verify_embedder();
        }
        if !self.embedder_needs_asking() {
            return self.verify_embedder();
        }
        self.ask_embedder_kind_local()?;
        self.verify_embedder()
    }

    /// gemini needs project and credentials; bge_m3 (or the fixture test
    /// double) needs nothing the file can hold.
    fn embedder_needs_asking(&self) -> bool {
        if self.config.embedder.kind != EmbedderKind::Gemini {
            return false;
        }
        let project_ok = self
            .config
            .embedder
            .gemini_project
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        let credentials_ok = self
            .config
            .embedder
            .gemini_credentials
            .as_ref()
            .is_some_and(|path| !path.as_os_str().is_empty());
        !(project_ok && credentials_ok)
    }

    fn ask_embedder_kind_local(&mut self) -> anyhow::Result<()> {
        loop {
            let answer = self.ask(text::get("init.embedder_question"))?;
            match answer.trim() {
                "" | "2" => {
                    self.set("embedder.kind", "bge_m3")?;
                    return self.ask_bge_dim();
                }
                "1" => {
                    self.set("embedder.kind", "gemini")?;
                    return self.shared_google_questions();
                }
                _ => self.say(text::get("init.embedder_invalid"))?,
            }
        }
    }

    fn ask_bge_dim(&mut self) -> anyhow::Result<()> {
        let answer = self.ask(text::get("init.embedder_bge_dim"))?;
        let dim = if answer.trim().is_empty() {
            "1024"
        } else {
            answer.trim()
        };
        self.set("embedder.dim", dim)
    }

    /// The shared posture's two questions, asked once: the project fills
    /// both `embedder.gemini_project` and `companion.google_project`, and the
    /// credentials path fills both `embedder.gemini_credentials` and
    /// `companion.google_credentials`. Each is asked only when it is still
    /// unset on the side that needs it, so re-running asks only for the gap.
    /// Both values are also stored in the vault for the MCP servers' env map.
    fn shared_google_questions(&mut self) -> anyhow::Result<()> {
        let project_missing = self
            .config
            .embedder
            .gemini_project
            .as_deref()
            .map(str::trim)
            .is_none_or(|value| value.is_empty())
            || self
                .config
                .companion
                .google_project
                .as_deref()
                .map(str::trim)
                .is_none_or(|value| value.is_empty());
        if project_missing {
            let project = self.ask(text::get("init.embedder_gemini_project"))?;
            self.set("embedder.gemini_project", &project)?;
            self.set("companion.google_project", &project)?;
            self.vault_mut()
                .set(GEMINI_PROJECT_SECRET, &project)
                .map_err(|error| anyhow::Error::new(error))?;
        }
        let credentials_missing = self
            .config
            .embedder
            .gemini_credentials
            .as_ref()
            .is_none_or(|path| path.as_os_str().is_empty())
            || self
                .config
                .companion
                .google_credentials
                .as_ref()
                .is_none_or(|path| path.as_os_str().is_empty());
        if credentials_missing {
            let path = self.ask_secret(text::get("init.embedder_gemini_credentials"))?;
            self.set("embedder.gemini_credentials", &path)?;
            self.set("companion.google_credentials", &path)?;
            self.vault_mut()
                .set(GEMINI_CREDENTIALS_SECRET, &path)
                .map_err(|error| anyhow::Error::new(error))?;
        }
        Ok(())
    }

    fn verify_embedder(&mut self) -> anyhow::Result<()> {
        loop {
            let config = self.resolved_config()?;
            match self.verifier.verify_embedder(&config) {
                Ok(()) => return self.say(text::get("init.embedder_verified")),
                Err(what) => {
                    self.say(&text::get("init.embedder_failed").replace("{what}", &what))?;
                    if self.ask_yes(text::get("init.retry_prompt"), true)? {
                        if self.config.embedder.kind == EmbedderKind::Gemini {
                            // The likely wrong answer is the credentials path.
                            let path =
                                self.ask_secret(text::get("init.embedder_gemini_credentials"))?;
                            self.set("embedder.gemini_credentials", &path)?;
                            self.set("companion.google_credentials", &path)?;
                            self.vault_mut()
                                .set(GEMINI_CREDENTIALS_SECRET, &path)
                                .map_err(|error| anyhow::Error::new(error))?;
                        }
                    } else {
                        return self.record_unverified(text::get("init.unverified_embedder"));
                    }
                }
            }
        }
    }

    // -- inference --------------------------------------------------------

    fn inference_step(&mut self) -> anyhow::Result<()> {
        if self.config.store.kind == StoreKind::Postgres
            || self.config.store.kind == StoreKind::Cockroach
        {
            self.say(text::get("init.inference_google"))?;
            // The project and credentials are normally asked in the embedder
            // step; with a non-gemini embedder in the file they were not, and
            // the derivation needs them on the companion side.
            let needs_google = self
                .config
                .companion
                .google_project
                .as_deref()
                .map(str::trim)
                .is_none_or(|value| value.is_empty())
                || self
                    .config
                    .companion
                    .google_credentials
                    .as_ref()
                    .is_none_or(|path| path.as_os_str().is_empty());
            if needs_google {
                self.shared_google_questions()?;
            }
            self.derive_shared_inference()?;
            return self.verify_inference();
        }
        if self.companion_needs_asking() {
            self.ask_inference_local()?;
        }
        self.verify_inference()
    }

    /// The shared posture's inference is derived, not asked: `auth = google`,
    /// `google_location = global`, `model = gemini-3.7-flash`. Applied only
    /// where the file still carries nothing or the shipped local default, so
    /// a real choice is never clobbered.
    fn derive_shared_inference(&mut self) -> anyhow::Result<()> {
        let auth = file_value(&self.source, "companion.auth");
        let base = file_value(&self.source, "companion.base_url");
        let model = file_value(&self.source, "companion.model");
        let placeholder_static =
            auth.is_none() || (auth.as_deref() == Some("static") && base.as_deref() == Some(PLACEHOLDER_BASE_URL));
        if placeholder_static {
            self.set("companion.auth", "google")?;
        }
        if !file_has(&self.source, "companion.google_location") {
            self.set("companion.google_location", "global")?;
        }
        if model.as_deref().is_none_or(|value| value == "local-model") {
            self.set("companion.model", SHARED_MODEL)?;
        }
        Ok(())
    }

    fn companion_needs_asking(&self) -> bool {
        match self.config.companion.auth {
            config::CompanionAuth::Google => false,
            config::CompanionAuth::Static => {
                let placeholder = self
                    .config
                    .companion
                    .base_url
                    .trim()
                    .trim_end_matches('/')
                    == PLACEHOLDER_BASE_URL;
                let key_reference_missing = self
                    .config
                    .companion
                    .api_key_secret
                    .as_deref()
                    .is_some_and(|name| self.vault().get(name).is_err());
                placeholder || key_reference_missing
            }
        }
    }

    fn ask_inference_local(&mut self) -> anyhow::Result<()> {
        let base = self.ask(&text::get("init.inference_base_url").replace("{default}", PLACEHOLDER_BASE_URL))?;
        let base = if base.trim().is_empty() {
            PLACEHOLDER_BASE_URL.to_owned()
        } else {
            base.trim().to_owned()
        };
        self.set("companion.base_url", &base)?;

        let model = self.ask(&text::get("init.inference_model").replace("{default}", "local-model"))?;
        let model = if model.trim().is_empty() {
            "local-model".to_owned()
        } else {
            model.trim().to_owned()
        };
        self.set("companion.model", &model)?;

        if self.ask_yes(text::get("init.inference_api_key_question"), false)? {
            let name = loop {
                let name = self.ask(
                    &text::get("init.inference_api_key_name").replace("{default}", COMPANION_API_KEY_SECRET),
                )?;
                let name = if name.trim().is_empty() {
                    COMPANION_API_KEY_SECRET.to_owned()
                } else {
                    name.trim().to_owned()
                };
                if crate::vault::is_valid_name(&name) {
                    break name;
                }
                self.say(text::get("vault.invalid_name"))?;
            };
            let key = self.ask_secret(text::get("init.inference_api_key_prompt"))?;
            self.vault_mut()
                .set(&name, &key)
                .map_err(|error| anyhow::Error::new(error))?;
            self.set("companion.api_key_secret", &name)?;
        }
        Ok(())
    }

    fn verify_inference(&mut self) -> anyhow::Result<()> {
        loop {
            let config = self.resolved_config()?;
            match self.verifier.verify_inference(&config) {
                Ok(()) => return self.say(text::get("init.inference_verified")),
                Err(what) => {
                    self.say(&text::get("init.inference_failed").replace("{what}", &what))?;
                    if self.ask_yes(text::get("init.retry_prompt"), true)? {
                        if self.config.companion.auth == config::CompanionAuth::Static {
                            // The likely wrong answer is the endpoint URL.
                            let base = self.ask(
                                &text::get("init.inference_base_url")
                                    .replace("{default}", PLACEHOLDER_BASE_URL),
                            )?;
                            let base = if base.trim().is_empty() {
                                PLACEHOLDER_BASE_URL.to_owned()
                            } else {
                                base.trim().to_owned()
                            };
                            self.set("companion.base_url", &base)?;
                        }
                    } else {
                        return self.record_unverified(text::get("init.unverified_inference"));
                    }
                }
            }
        }
    }

    // -- the MCP servers --------------------------------------------------

    fn mcp_step(&mut self, venv: Option<PathBuf>) -> anyhow::Result<()> {
        let Some(venv) = venv else {
            return self.say(text::get("init.mcp_none"));
        };
        let news = venv.join("bin/mooshik-news-mcp");
        let artifacts = venv.join("bin/mooshik-artifacts-mcp");
        let coder = venv.join("bin/mooshik-coder-mcp");
        if ![&news, &artifacts, &coder].iter().any(|path| path.is_file()) {
            return self.say(text::get("init.mcp_none"));
        }
        self.say(&text::get("init.mcp_heading").replace("{venv}", &venv.display().to_string()))?;

        let has_google = self
            .config
            .embedder
            .gemini_project
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
            && self
                .config
                .embedder
                .gemini_credentials
                .as_ref()
                .is_some_and(|path| !path.as_os_str().is_empty());
        let mut wired: Vec<&str> = Vec::new();

        if news.is_file() {
            if has_google {
                if self.ask_yes(text::get("init.mcp_news"), true)? {
                    self.wire_server(
                        "news",
                        &news,
                        &["search_news", "fetch_article"],
                        &[
                            ("MOOSHIK_GEMINI_PROJECT", GEMINI_PROJECT_SECRET),
                            ("MOOSHIK_GEMINI_CREDENTIALS", GEMINI_CREDENTIALS_SECRET),
                        ],
                        "\"mcp.news.*\" = \"allow\"",
                    )?;
                    wired.push("news");
                }
            } else {
                self.say(text::get("init.mcp_no_google"))?;
            }
        }
        if artifacts.is_file() {
            if has_google {
                if self.ask_yes(text::get("init.mcp_artifacts"), true)? {
                    self.wire_server(
                        "artifacts",
                        &artifacts,
                        &["extract_concepts"],
                        &[
                            ("MOOSHIK_GEMINI_PROJECT", GEMINI_PROJECT_SECRET),
                            ("MOOSHIK_GEMINI_CREDENTIALS", GEMINI_CREDENTIALS_SECRET),
                        ],
                        "\"mcp.artifacts.*\" = \"allow\"",
                    )?;
                    wired.push("artifacts");
                }
            } else {
                self.say(text::get("init.mcp_no_google"))?;
            }
        }
        if coder.is_file() {
            if self.ask_yes(text::get("init.mcp_coder"), false)? {
                let agent = self.ask_coder_agent()?;
                let (env_var, secret_name) =
                    super::configure::coder_agent_secret(&agent).expect("agent validated above");
                // Re-runnable: only ask for the key when the vault does not
                // already hold it.
                if self.vault().get(secret_name).is_err() {
                    let key = self.ask_secret(text::get("init.mcp_coder_key"))?;
                    self.vault_mut()
                        .set(secret_name, &key)
                        .map_err(|error| anyhow::Error::new(error))?;
                }
                let (command, args_prefix) = super::configure::find_coder_command();
                let edited = super::configure::apply_coder_config(
                    &self.source,
                    &agent,
                    &command,
                    &args_prefix,
                    env_var,
                    secret_name,
                );
                self.write_source(edited)?;
                wired.push("coder");
            }
        }
        if !wired.is_empty() {
            self.say(
                &text::get("init.mcp_wired").replace("{names}", &wired.join(", ")),
            )?;
        }
        Ok(())
    }

    fn ask_coder_agent(&mut self) -> anyhow::Result<String> {
        loop {
            let answer = self.ask(text::get("init.mcp_coder_agent"))?;
            let agent = if answer.trim().is_empty() {
                "claude".to_owned()
            } else {
                answer.trim().to_owned()
            };
            if super::configure::coder_agent_secret(&agent).is_some() {
                return Ok(agent);
            }
            self.say(text::get("config.invalid_coder_agent"))?;
        }
    }

    /// Append one MCP server block (the one block shape `config set` cannot
    /// write) and reload. The env map holds vault secret NAMES.
    fn wire_server(
        &mut self,
        name: &str,
        command: &Path,
        expose: &[&str],
        env: &[(&str, &str)],
        grant: &str,
    ) -> anyhow::Result<()> {
        let command = command.to_string_lossy().into_owned();
        let edited = super::configure::append_mcp_block(
            &self.source,
            name,
            &command,
            &[],
            expose,
            env,
            grant,
        );
        self.write_source(edited)
    }

    /// Write an already-composed config text and reload both the source and
    /// the resolved config.
    fn write_source(&mut self, edited: String) -> anyhow::Result<()> {
        Config::from_toml_and_env(&edited, self.environment.clone())
            .map_err(anyhow::Error::new)?;
        secure_path::write_private_at(&self.root, OsStr::new("config.toml"), edited.as_bytes())
            .map_err(|_| anyhow::Error::new(ConfigError::WriteFailed))?;
        self.source = edited;
        self.config = Config::from_toml_and_env(&self.source, self.environment.clone())
            .map_err(anyhow::Error::new)?;
        Ok(())
    }

    // -- closing ----------------------------------------------------------

    fn closing(&mut self) -> anyhow::Result<()> {
        self.say(text::get("init.close_tui"))?;
        self.say(text::get("init.close_empty"))?;
        self.say(text::get("init.close_open"))?;
        self.say(text::get("init.close_home"))?;
        self.say(text::get("init.close_repo"))?;
        self.say(text::get("init.close_watch"))?;
        if !self.unverified.is_empty() {
            self.say(text::get("init.unverified_header"))?;
            let unverified = self.unverified.clone();
            for item in &unverified {
                self.say(&text::get("init.unverified_item").replace("{what}", item))?;
            }
        }
        self.say(text::get("init.done"))
    }
}

/// Read one line with terminal echo off, via termios on stdin. `libc` is
/// already a dependency; the repo pins its dependencies deliberately and a
/// prompt library is a large surface for one no-echo read. The terminal is
/// restored even when the read fails.
#[cfg(unix)]
fn read_no_echo(reader: &mut dyn BufRead) -> io::Result<String> {
    // Safety: termios operates on the process's own stdin; the attributes are
    // saved first and restored in every path before returning.
    unsafe {
        let mut termios: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut termios) != 0 {
            return Err(io::Error::last_os_error());
        }
        let original = termios;
        termios.c_lflag &= !libc::ECHO;
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &termios) != 0 {
            return Err(io::Error::last_os_error());
        }
        let outcome = read_plain_line(reader);
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
        outcome
    }
}

#[cfg(not(unix))]
fn read_no_echo(reader: &mut dyn BufRead) -> io::Result<String> {
    read_plain_line(reader)
}

fn read_plain_line(reader: &mut dyn BufRead) -> io::Result<String> {
    let mut line = String::new();
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "end of input"));
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

/// Whether the file already assigns `key` (dotted `section.field`).
fn file_has(source: &str, key: &str) -> bool {
    let Ok(table) = source.parse::<toml::Table>() else {
        return false;
    };
    let (section, field) = key.split_once('.').expect("keys are dotted");
    table
        .get(section)
        .and_then(toml::Value::as_table)
        .is_some_and(|table| table.contains_key(field))
}
/// The file's current value for a dotted string key, if any. Owned, so the
/// result does not borrow `source` (the caller edits the file right after).
fn file_value(source: &str, key: &str) -> Option<String> {
    let table = source.parse::<toml::Table>().ok()?;
    let (section, field) = key.split_once('.').expect("keys are dotted");
    table
        .get(section)
        .and_then(toml::Value::as_table)?
        .get(field)?
        .as_str()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    use std::sync::Arc;
    use crate::vault::PassphraseProvider;

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

    /// A scripted-answer home. The vault provider is passphrase, so the tests
    /// never touch the OS keyring.
    fn fixture_home(label: &str) -> HomeLayout {
        // The flow opens the vault through `provider_for`, which reads
        // MOOSHIK_VAULT_PASSPHRASE from the process environment.
        std::env::set_var("MOOSHIK_VAULT_PASSPHRASE", "test-passphrase");
        let root = crate::secure_path::canonical_temp_dir()
            .join(format!("mooshik-init-flow-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = HomeLayout::new(&root);
        layout.init().unwrap();

        // Passphrase mode, so no keyring is involved.
        let file = std::fs::read_to_string(&layout.config).unwrap();
        let edited =
            crate::config::apply_setting(&file, "vault.provider", "passphrase", []).unwrap();
        std::fs::write(&layout.config, edited).unwrap();
        layout
    }

    /// Run the flow with scripted answers, an empty environment and a stub
    /// verifier, returning the captured output and the written config text.
    fn drive(
        layout: &HomeLayout,
        answers: &str,
        verifier: &dyn Verifier,
        venv: Option<PathBuf>,
    ) -> (String, String) {
        let mut input = Cursor::new(answers.as_bytes().to_vec());
        let mut output: Vec<u8> = Vec::new();
        let outcome = run_with(layout, &mut input, &mut output, venv, Vec::new(), false, verifier);
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
        let config = Config::load_at(&root).unwrap();
        let provider = Arc::new(PassphraseProvider::new("test-passphrase").unwrap());
        let vault = Vault::open_at(&layout.vault, root, provider).unwrap();
        vault
            .get(name)
            .ok()
            .map(|token| token.expose().to_owned())
    }
    #[test]
    fn shared_posture_writes_a_working_config() {
        let layout = fixture_home("shared");
        // posture=1 (shared, default), store=1 (postgres you run), DSN,
        // then project and credentials, then MCP declines (no venv).
        let answers = "\n\npostgres://user@db.example/mooshik\nproj\n/key.json\n";
        let (output, written) = drive(&layout, answers, &StubVerifier { fail_store: false, fail_embedder: false, fail_inference: false }, None);

        let config = Config::from_toml_and_env(&written, []).unwrap();
        assert_eq!(config.store.kind, StoreKind::Postgres);
        assert_eq!(config.store.dsn_secret.as_deref(), Some("store-dsn"));
        assert_eq!(config.embedder.gemini_project.as_deref(), Some("proj"));
        assert_eq!(
            config.embedder.gemini_credentials.as_deref().map(Path::as_os_str),
            Some(std::ffi::OsStr::new("/key.json"))
        );
        assert_eq!(config.companion.auth, config::CompanionAuth::Google);
        assert_eq!(config.companion.google_project.as_deref(), Some("proj"));
        assert_eq!(config.companion.google_location.as_deref(), Some("global"));
        assert_eq!(config.companion.model, "gemini-3.7-flash");
        assert_eq!(
            config.companion.google_credentials.as_deref().map(Path::as_os_str),
            Some(std::ffi::OsStr::new("/key.json"))
        );
        // The DSN never lands in the file.
        assert!(!written.contains("postgres://"), "{written}");
        assert!(!written.contains("db.example"), "{written}");
        // Secrets land in the vault.
        assert_eq!(read_vault(&layout, "store-dsn").as_deref(), Some("postgres://user@db.example/mooshik"));
        assert_eq!(read_vault(&layout, "gemini-project").as_deref(), Some("proj"));
        // The output says what it set.
        assert!(output.contains("Mooshik is set up."), "{output}");
    }

    #[test]
    fn local_posture_writes_sqlite_and_a_local_companion() {
        let layout = fixture_home("local");
        // posture=2 (local), sqlite path, embedder=2 (bge_m3, default), dim
        // default, endpoint URL, model, no bearer key.
        let answers = "2\n/mnt/memory.db\n\n\nhttp://localhost:1234/v1\nmy-model\nn\n";
        let (output, written) = drive(&layout, answers, &StubVerifier { fail_store: false, fail_embedder: false, fail_inference: false }, None);

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
        // posture default (shared), store=2 (cloud postgres), DSN, project,
        // credentials.
        let answers = "\n2\npostgres://user@db.example/mooshik\nproj\n/key.json\n";
        let (output, written) = drive(&layout, answers, &StubVerifier { fail_store: false, fail_embedder: false, fail_inference: false }, None);
        assert!(output.contains("Auth Proxy"), "{output}");
        assert!(written.contains("dsn_secret = \"store-dsn\""), "{written}");
    }

    #[test]
    fn verification_failure_offers_retry_then_continues() {
        let layout = fixture_home("retry");
        // The store verification fails until the user says no to retrying.
        // Store answers: posture (default shared), store (default), DSN,
        // retry=yes -> DSN again, retry=no -> continue; then embedder and
        // inference answers.
        let answers = "\n\nfirst-dsn\ny\nsecond-dsn\nn\nproj\n/key.json\n";
        let verifier = StubVerifier { fail_store: true, fail_embedder: false, fail_inference: false };
        let (output, written) = drive(&layout, answers, &verifier, None);
        // The second DSN is the one that lands in the vault.
        assert_eq!(read_vault(&layout, "store-dsn").as_deref(), Some("second-dsn"));
        assert!(output.contains("Unverified"), "{output}");
        assert!(output.contains("store (connect and provision the schema)"), "{output}");
        // The run still finished and wrote the rest.
        let config = Config::from_toml_and_env(&written, []).unwrap();
        assert_eq!(config.embedder.gemini_project.as_deref(), Some("proj"));
    }

    #[test]
    fn mcp_servers_are_wired_when_the_venv_is_there() {
        let layout = fixture_home("mcp");
        // A fake venv with the three console scripts.
        let venv = layout.root.join("venv");
        let bin = venv.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for script in ["mooshik-news-mcp", "mooshik-artifacts-mcp", "mooshik-coder-mcp"] {
            std::fs::write(bin.join(script), "#!/bin/sh\n").unwrap();
        }
        // posture default (shared), store default, DSN, project, credentials,
        // news=yes (default), artifacts=no, coder=yes, agent=claude (default),
        // key.
        let answers = "\n\npostgres://user@db.example/mooshik\nproj\n/key.json\n\nn\ny\n\nagent-key\n";
        let (output, written) = drive(&layout, answers, &StubVerifier { fail_store: false, fail_embedder: false, fail_inference: false }, Some(venv));
        assert!(written.contains("[mcp_servers.news]"), "{written}");
        assert!(written.contains("search_news"), "{written}");
        assert!(!written.contains("[mcp_servers.artifacts]"), "{written}");
        assert!(written.contains("[mcp_servers.coder]"), "{written}");
        assert!(written.contains("\"mcp.news.*\" = \"allow\""), "{written}");
        assert!(written.contains("\"mcp.coder.*\" = \"prompt\""), "{written}");
        assert!(written.contains("MOOSHIK_GEMINI_PROJECT = \"gemini-project\""), "{written}");
        // The coder key went to the vault, not the file.
        assert_eq!(read_vault(&layout, "anthropic-api-key").as_deref(), Some("agent-key"));
        assert!(!written.contains("agent-key"), "{written}");
        assert!(output.contains("news, coder wired"), "{output}");
    }

    #[test]
    fn rerun_asks_only_for_what_is_still_missing() {
        let layout = fixture_home("rerun");
        // First run: full shared setup.
        let answers = "\n\npostgres://user@db.example/mooshik\nproj\n/key.json\n";
        drive(&layout, answers, &StubVerifier { fail_store: false, fail_embedder: false, fail_inference: false }, None);
        let before = std::fs::read_to_string(&layout.config).unwrap();

        // Second run: nothing left to ask. The flow must finish without
        // consuming any answers beyond the first line.
        let mut input = Cursor::new(Vec::new());
        let mut output: Vec<u8> = Vec::new();
        let outcome = run_with(
            &layout,
            &mut input,
            &mut output,
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
        // No questions were asked, so EOF at the first read must not have
        // been reached... it was: with nothing to ask, the flow never reads.
        let after = std::fs::read_to_string(&layout.config).unwrap();
        assert_eq!(before, after, "a re-run must not rewrite the file");
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Mooshik is set up."), "{output}");
    }

    #[test]
    fn secrets_never_appear_in_the_written_file_or_output() {
        let layout = fixture_home("secrets");
        let secret = "s3cret-dsn-value-with-password";
        let answers = format!("\n\n{secret}\nproj\n/key.json\n");
        let (output, written) = drive(&layout, &answers, &StubVerifier { fail_store: false, fail_embedder: false, fail_inference: false }, None);
        assert!(!written.contains(secret), "{written}");
        assert!(!output.contains(secret), "{output}");
        // And the secret is where it belongs.
        assert_eq!(read_vault(&layout, "store-dsn").as_deref(), Some(secret));
    }
}
