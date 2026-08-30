//! The `run_scratch_script` sandbox: isolated temp cwd, hard timeout, output cap.
//!
//! Sandbox intent (SPEC: "throwaway Python or bash in a sandbox, with a hard
//! timeout"): the code is written into a fresh, process-unique temp working
//! directory and exec'd by a whitelisted interpreter — never with `sh -c` — so
//! there is no shell to inject into and no user-supplied path enters the
//! command line. The executor rejects absolute/`..`-escaping paths and NUL bytes
//! at the door ([`validate_scratch`]), the child is **killed on expiry** and then
//! reaped (never left orphaned — this crate's rule is *bound the child*), and
//! captured output is capped so a talkative script cannot balloon the turn.
//!
//! The permission **prompt** is a seam here ([`ScratchConfig::confirm`]); since
//! M5 the grant gate lives at the tool-call boundary
//! ([`super::GatedTools`]), which decides *whether* the tool may run and asks
//! exactly once — this seam stays as the interactive ask behind it.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, io};

use super::schema::{
    ScratchLanguage, ScratchParams, SCRATCH_MAX_OUTPUT_BYTES, SCRATCH_MAX_SCRIPT_BYTES,
    SCRATCH_MAX_TIMEOUT_SECS,
};
use crate::text;
use crate::vault::{SecretToken, SharedVault, VaultError};

/// One resolved injection: the env-var name a script reads and the secret
/// value as an opaque token. The token keeps the plaintext out of logs,
/// errors, Debug output, and the transcript until the single [`Command::env`]
/// hand-off inside [`run_script`].
pub struct SecretEnv {
    pub var: String,
    pub token: SecretToken,
}

/// Resolve the configured `[tools.scratch.env]` table into fresh, per-run
/// secret tokens ([`crate::vault::SharedVault`]). Resolution happens at
/// execution time — after confirm, before spawn — so a rotated secret is
/// picked up by the very next run. All-or-nothing: any unresolvable entry
/// aborts before the script starts, so nothing ever runs half-injected. The
/// error names at most the secret *name*; never a value.
pub fn resolve_injection(
    table: &[(String, String)],
    vault: Option<&SharedVault>,
) -> Result<Vec<SecretEnv>, String> {
    if table.is_empty() {
        return Ok(Vec::new());
    }
    let Some(vault) = vault else {
        return Err(text::get("tools.scratch_env_unavailable").to_owned());
    };
    let vault = crate::vault::lock_shared(vault);
    let mut resolved = Vec::with_capacity(table.len());
    for (env_var, secret_name) in table {
        match vault.get(secret_name) {
            Ok(token) => resolved.push(SecretEnv {
                var: env_var.clone(),
                token,
            }),
            Err(VaultError::NotFound) => {
                return Err(
                    text::get("tools.scratch_secret_missing").replace("{name}", secret_name)
                );
            }
            Err(_) => return Err(text::get("tools.scratch_secret_failed").to_owned()),
        }
    }
    Ok(resolved)
}

/// Interactive permission prompt. Reads a single line from standard input and
/// accepts only an explicit `y`/`yes` (fail closed). This is the M4 prompt seam;
/// M5 replaces it with the real configured grant gate.
fn interactive_confirm(_params: &ScratchParams) -> bool {
    eprint!("{}", text::get("tools.scratch_prompt"));
    let _ = io::Write::flush(&mut io::stderr());
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    answer_yes(line.trim())
}

/// Whether a trimmed prompt answer grants permission (only explicit yes).
pub fn answer_yes(answer: &str) -> bool {
    let answer = answer.trim().to_ascii_lowercase();
    answer == "y" || answer == "yes"
}

/// The confirmed script run's configuration. `confirm` decides whether a request
/// is allowed to execute at all (the permission-prompt seam). `secret_env` maps
/// process-env names to vault secret *names* ([`resolve_injection`] turns them
/// into tokens at run time).
pub struct ScratchConfig {
    pub confirm: Box<dyn Fn(&ScratchParams) -> bool + Send + Sync>,
    /// Cap on captured stdout+stderr bytes. Deliberate limit: the cut is not
    /// secret-aware, so a cap landing inside a secret occurrence leaves the
    /// surviving prefix unmatchable and unredacted. Accepted because the
    /// script author already holds the plaintext — this is the accidental-
    /// echo defense, not a new grant.
    pub max_output_bytes: usize,
    /// Env-var name -> secret name, from `[tools.scratch.env]`.
    pub secret_env: Vec<(String, String)>,
}

impl Default for ScratchConfig {
    fn default() -> Self {
        Self {
            confirm: Box::new(interactive_confirm),
            max_output_bytes: SCRATCH_MAX_OUTPUT_BYTES,
            secret_env: Vec::new(),
        }
    }
}

impl ScratchConfig {
    /// A seam that never refuses. Chat composition uses this under the M5
    /// gate: [`super::GatedTools`] prompts once for prompt-mode grants, so the
    /// inner seam must not ask a second time.
    pub fn always_confirmed() -> Self {
        Self {
            confirm: Box::new(|_| true),
            max_output_bytes: SCRATCH_MAX_OUTPUT_BYTES,
            secret_env: Vec::new(),
        }
    }
}

/// Validate a scratch request at the door, mirroring the same fail-closed
/// discipline as the lifted lambo tools.
///
/// The sandbox is the **isolated temp working directory** plus **direct exec of
/// a whitelisted interpreter** (never `sh -c`), so neither user content nor any
/// path in it can reach our command line. Content-level path checks guard the
/// one real leak — a script that echoes its own surroundings — rather than
/// forbidding ordinary absolute paths inside a script (which would break
/// legitimate code while adding no isolation): we refuse a whole-script
/// absolute-path form and obvious `..` traversal out of the sandbox root.
pub fn validate_scratch(params: &ScratchParams) -> Result<(), String> {
    let code = params.code.trim();
    if code.is_empty() {
        return Err(text::get("tools.scratch_empty_code").to_owned());
    }
    if params.code.len() > SCRATCH_MAX_SCRIPT_BYTES {
        return Err(text::get("tools.scratch_code_too_large").to_owned());
    }
    if params.code.as_bytes().contains(&0) {
        return Err(text::get("tools.scratch_nul_byte").to_owned());
    }
    if starts_with_absolute_or_traversal(code) {
        return Err(text::get("tools.scratch_path_escape").to_owned());
    }
    if let Some(secs) = params.timeout_secs {
        if !(1..=SCRATCH_MAX_TIMEOUT_SECS).contains(&secs) {
            return Err(format!(
                "timeout_secs must be in 1..={SCRATCH_MAX_TIMEOUT_SECS}"
            ));
        }
    }
    Ok(())
}

/// Reject a first token that is an absolute path or a `..`-escape, which would
/// fly the sandbox root before the interpreter even starts.
fn starts_with_absolute_or_traversal(code: &str) -> bool {
    match code.split_whitespace().next() {
        Some(first) => first.starts_with('/') || first == ".." || first.starts_with("../"),
        None => false,
    }
}

/// The result of one completed (or killed) sandboxed run.
pub struct ScratchOutcome {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub timed_out: bool,
    pub duration: Duration,
}

/// Run `code` under `language` inside a fresh isolated temp directory with a
/// hard `timeout`. On expiry the child is killed and reaped before returning.
/// `env` carries the resolved secret injections; see [`interpreter_command`]
/// for the one place their plaintext exists.
///
/// Returns the captured output even for non-zero / timed-out exits so the caller
/// can hand the model useful partial output; the `timed_out` flag distinguishes
/// a kill from a normal exit.
pub fn run_script(
    code: &str,
    language: ScratchLanguage,
    timeout: Duration,
    max_output_bytes: usize,
    env: &[SecretEnv],
) -> Result<ScratchOutcome, String> {
    let sandbox = Sandbox::create()?;
    let script = sandbox.write_script(code, language)?;
    let start = Instant::now();

    #[cfg(unix)]
    let child = {
        let mut command = interpreter_command(language, &script, sandbox.path(), env);
        // Put the child in its own process group so a hard-timeout kill can
        // take down the whole tree (e.g. a bash script that launched a long
        // `sleep`), not just the direct interpreter. Without this, killing the
        // interpreter leaves a grandchild holding the stdout pipe and the
        // reader thread blocks until that grandchild exits on its own.
        use std::os::unix::process::CommandExt;
        // SAFETY: `setsid` is a plain libc call with no borrowed arguments.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        command.spawn()
    };
    #[cfg(not(unix))]
    let child = {
        let mut command = interpreter_command(language, &script, sandbox.path(), env);
        command.spawn()
    };
    let mut child = child.map_err(|error| fun("tools.scratch_spawn_failed", &error))?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let out_handle = std::thread::spawn(move || read_capped(stdout, max_output_bytes));
    let err_handle = std::thread::spawn(move || read_capped(stderr, max_output_bytes));

    let (exit_code, timed_out) = wait_child(&mut child, timeout);

    let duration = start.elapsed();
    let ((out_bytes, out_trunc), (err_bytes, err_trunc)) = join_readers(
        out_handle,
        err_handle,
        &mut child,
        timeout,
        text::get("tools.scratch_io_failed"),
    )?;

    Ok(ScratchOutcome {
        exit_code,
        stdout: String::from_utf8_lossy(&out_bytes).into_owned(),
        stderr: String::from_utf8_lossy(&err_bytes).into_owned(),
        truncated: out_trunc || err_trunc,
        timed_out,
        duration,
    })
}

/// Build the sandbox interpreter command: direct exec of the whitelisted
/// interpreter in the sandbox cwd, piped output, plus the resolved secret
/// environment. This is the ONLY place a secret's plaintext exists outside
/// the vault and the child process: [`SecretToken::expose`] feeds straight
/// into [`Command::env`], which is consumed by `spawn` — never logged,
/// rendered, or carried anywhere else.
fn interpreter_command(
    language: ScratchLanguage,
    script: &Path,
    cwd: &Path,
    env: &[SecretEnv],
) -> Command {
    let mut command = Command::new(language.interpreter());
    command
        .arg(script)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for item in env {
        command.env(&item.var, item.token.expose());
    }
    command
}

/// Poll the child until it exits, or kill + reap it at `timeout`. Always reaps.
fn wait_child(child: &mut Child, timeout: Duration) -> (Option<i32>, bool) {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return (status.code(), false),
            Ok(None) => {}
            Err(_) => return (None, false),
        }
        if Instant::now() >= deadline {
            // Hard timeout: kill the whole process group (the child ran with
            // `setsid`), then reap the direct child so it can never outlive the
            // caller as a zombie or orphan.
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            #[cfg(not(unix))]
            let _ = child.kill();
            let _ = child.wait();
            return (None, true);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The pair of reader results delivered by the sandbox output-threads: for
/// stdout then stderr, the captured bytes plus a truncated flag.
type ReaderResults = ((Vec<u8>, bool), (Vec<u8>, bool));

/// Bound the reader joins with the same wall-clock discipline as the timeout
/// path. The direct child may exit cleanly while a backgrounded grandchild of
/// its own still holds the stdout/stderr pipes, which would otherwise block
/// the caller until that grandchild exits. Give the readers one `timeout`
/// budget; if a reader is still blocked at expiry, kill the child's whole
/// process group (the child ran under `setsid`) so the pipes drain, then give
/// the readers one more budget before giving up — dropping the handles
/// detaches the reader threads rather than wedging the caller.
fn join_readers(
    out_handle: std::thread::JoinHandle<(Vec<u8>, bool)>,
    err_handle: std::thread::JoinHandle<(Vec<u8>, bool)>,
    child: &mut Child,
    timeout: Duration,
    io_failed: &str,
) -> Result<ReaderResults, String> {
    let mut out_handle = Some(out_handle);
    let mut err_handle = Some(err_handle);
    let mut out = None;
    let mut err = None;
    let mut deadline = Instant::now() + timeout;
    let mut killed = false;
    loop {
        if out.is_none() {
            if let Some(handle) = out_handle.take() {
                if handle.is_finished() {
                    out = Some(handle.join().map_err(|_| io_failed.to_owned())?);
                } else {
                    out_handle = Some(handle);
                }
            }
        }
        if err.is_none() {
            if let Some(handle) = err_handle.take() {
                if handle.is_finished() {
                    err = Some(handle.join().map_err(|_| io_failed.to_owned())?);
                } else {
                    err_handle = Some(handle);
                }
            }
        }
        if out.is_some() && err.is_some() {
            return Ok((
                out.take().expect("readers are guarded by is_some"),
                err.take().expect("readers are guarded by is_some"),
            ));
        }
        if Instant::now() >= deadline {
            if killed {
                // Still blocked after the group kill — a descendant escaped
                // the group (e.g. a double-forked `setsid`). Give up rather
                // than wedge the caller; dropping the handles detaches the
                // reader threads, which finish when their pipes drain.
                return Err(io_failed.to_owned());
            }
            // Same wall-clock discipline as the timeout path: kill the whole
            // group so a pipe-holding grandchild drains the readers.
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            #[cfg(not(unix))]
            let _ = child.kill();
            killed = true;
            deadline = Instant::now() + timeout;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Read `reader` until EOF, keeping at most `capsize` bytes; `truncated` is set
/// once anything beyond the cap was seen (and discarded — still read to EOF so
/// the child can finish writing).
fn read_capped(mut reader: impl Read, capsize: usize) -> (Vec<u8>, bool) {
    let mut captured: Vec<u8> = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let room = capsize.saturating_sub(captured.len());
                if n >= room {
                    let take = room.min(n);
                    if take > 0 {
                        captured.extend_from_slice(&chunk[..take]);
                    }
                    if n > room {
                        truncated = true;
                    }
                } else {
                    captured.extend_from_slice(&chunk[..n]);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    (captured, truncated)
}

/// A process-unique isolated scratch directory, removed on drop.
struct Sandbox {
    dir: PathBuf,
}

/// How many sandboxes this process has named, which is what makes two of them
/// created in the same instant different names.
static SANDBOXES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Sandbox {
    fn create() -> Result<Self, String> {
        // Three parts, and all three are load-bearing: the pid separates
        // processes, the counter separates two calls inside one, and the clock
        // separates this run from a directory a crashed one left behind (the
        // drop that removes it does not run on a kill). The clock alone did not:
        // macOS's realtime clock advances in microseconds, so two sandboxes
        // opened in the same microsecond drew the same name and the loser failed
        // with "File exists" — the same fault the vault fixtures hit on Darwin.
        let id = format!(
            "mooshik-scratch-{}-{:x}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0),
            SANDBOXES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(id);
        fs::create_dir(&dir).map_err(|error| fun("tools.scratch_sandbox_failed", &error))?;
        Ok(Self { dir })
    }

    fn path(&self) -> &PathBuf {
        &self.dir
    }

    fn write_script(&self, code: &str, language: ScratchLanguage) -> Result<PathBuf, String> {
        let name = format!("script.{}", language.extension());
        let path = self.dir.join(name);
        let mut file =
            fs::File::create(&path).map_err(|error| fun("tools.scratch_write_failed", &error))?;
        file.write_all(code.as_bytes())
            .map_err(|error| fun("tools.scratch_write_failed", &error))?;
        Ok(path)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn fun(key: &str, error: &dyn std::fmt::Display) -> String {
    format!("{}: {error}", text::get(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(code: &str) -> ScratchParams {
        ScratchParams {
            language: ScratchLanguage::Bash,
            code: code.to_owned(),
            timeout_secs: Some(5),
        }
    }

    #[test]
    fn validation_rejects_empty_code() {
        assert_eq!(
            validate_scratch(&params("   ")),
            Err(text::get("tools.scratch_empty_code").to_owned())
        );
    }

    #[test]
    fn validation_rejects_escaping_first_paths() {
        // A first token that flies the sandbox root is refused...
        assert!(validate_scratch(&params("/bin/sh")).is_err());
        assert!(validate_scratch(&params("/tmp/evil")).is_err());
        assert!(validate_scratch(&params("../escape")).is_err());
        // ...but an ordinary absolute path *inside* a multi-line script is the
        // script author's business: the sandbox is the isolated cwd + direct
        // exec, and forbidding `/` outright would break legitimate code.
        assert!(validate_scratch(&params("cat /etc/hostname")).is_ok());
        assert!(validate_scratch(&params("cat ../../etc/passwd")).is_ok());
    }

    #[test]
    fn validation_rejects_nul_bytes_and_huge_code() {
        assert!(validate_scratch(&params("echo \u{0} x")).is_err());
        let huge = "x".repeat(SCRATCH_MAX_SCRIPT_BYTES + 1);
        assert!(validate_scratch(&params(&huge)).is_err());
    }

    #[test]
    fn validation_rejects_out_of_range_timeout() {
        let mut p = params("echo hi");
        p.timeout_secs = Some(SCRATCH_MAX_TIMEOUT_SECS + 1);
        assert!(validate_scratch(&p).is_err());
    }

    #[test]
    fn permission_answer_accepts_only_explicit_yes() {
        assert!(answer_yes("y"));
        assert!(answer_yes("YES"));
        assert!(!answer_yes("n"));
        assert!(!answer_yes("maybe"));
        assert!(!answer_yes(""));
    }

    #[test]
    fn runs_successfully_in_the_sandbox_dir() {
        let p = params("pwd");
        let out = run_script(&p.code, p.language, Duration::from_secs(5), 4096, &[]).unwrap();
        assert_eq!(out.exit_code, Some(0));
        assert!(
            out.stdout.trim().contains("mooshik-scratch-"),
            "cwd must be the sandbox dir, got: {}",
            out.stdout.trim()
        );
        assert!(!out.timed_out);
        assert!(!out.truncated);
    }

    /// Two scripts started in the same instant get two sandboxes.
    ///
    /// They used to be named from the pid and the clock alone, and macOS's
    /// realtime clock advances in microseconds: two calls inside one microsecond
    /// drew the same name and the loser failed with "File exists" — reachable
    /// from two concurrent tool calls, and reached by this suite's own parallel
    /// tests. The clock is sampled once here so the test does not have to win a
    /// race to observe the fault it is guarding.
    #[test]
    fn two_sandboxes_opened_in_the_same_instant_are_two_directories() {
        let first = Sandbox::create().expect("a sandbox");
        let second = Sandbox::create().expect("a second sandbox");
        assert_ne!(first.path(), second.path());
        assert!(first.path().is_dir() && second.path().is_dir());
    }

    #[test]
    fn semicolons_are_script_content_not_injection() {
        // No shell is used by the runner, so `;` is bash itself within the file.
        let out = run_script(
            "echo a; echo b",
            ScratchLanguage::Bash,
            Duration::from_secs(5),
            4096,
            &[],
        )
        .unwrap();
        assert_eq!(out.exit_code, Some(0));
        assert_eq!(out.stdout.trim(), "a\nb");
    }

    #[test]
    fn hard_timeout_kills_the_child() {
        let start = Instant::now();
        let out = run_script(
            "sleep 60",
            ScratchLanguage::Bash,
            Duration::from_millis(300),
            4096,
            &[],
        )
        .unwrap();
        assert!(out.timed_out, "must report the timeout-kill");
        assert!(start.elapsed().as_secs() < 10, "kill must be prompt");
    }

    #[test]
    fn clean_exit_with_background_grandchild_bounds_the_reader_join() {
        // P2-M4-1 pin: the script itself exits 0, but it backgrounds a
        // grandchild that inherits the stdout/stderr pipes and keeps them open.
        // The reader join must still be bounded (same wall-clock discipline as
        // the timeout path) — the group is killed once the budget elapses, so
        // the caller never wedges waiting for the grandchild to exit.
        let start = Instant::now();
        let out = run_script(
            "sleep 100 &",
            ScratchLanguage::Bash,
            Duration::from_millis(300),
            4096,
            &[],
        )
        .unwrap();
        assert_eq!(out.exit_code, Some(0), "the script itself exits cleanly");
        assert!(!out.timed_out, "a clean exit is not a timeout-kill");
        assert!(
            start.elapsed().as_secs() < 10,
            "the reader join must be bounded like the timeout path"
        );
    }

    #[test]
    fn output_is_capped() {
        let out = run_script(
            "printf 'abcdefghij'",
            ScratchLanguage::Bash,
            Duration::from_secs(5),
            4,
            &[],
        )
        .unwrap();
        assert!(out.truncated, "more output than the cap must set truncated");
        assert!(out.stdout.len() <= 4, "captured output respects the cap");
    }

    #[test]
    fn non_zero_exit_is_reported_with_output() {
        let out = run_script(
            "echo boom && exit 7",
            ScratchLanguage::Bash,
            Duration::from_secs(5),
            4096,
            &[],
        )
        .unwrap();
        assert_eq!(out.exit_code, Some(7));
        assert_eq!(out.stdout.trim(), "boom");
    }
}
