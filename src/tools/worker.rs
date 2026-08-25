//! A Tokio runtime pinned to a dedicated OS thread.
//!
//! The companion `ToolExecutor` seam ([`crate::companion::ToolExecutor::execute`])
//! is synchronous, but the tools it dispatches are not all synchronous:
//! `lambo::Memory::recall` and `derive` are `async`, and `run_scratch_script`
//! must enforce a hard wall-clock timeout on a child process. The chat loop
//! (`crate::companion::chat`, `Session::turn`) already runs on *its own* Tokio
//! runtime, so we cannot simply build a second runtime and call `block_on` from
//! inside `execute` — Tokio forbids creating **and** dropping a `Runtime` from
//! within a runtime's async context.
//!
//! The design decision for M4 (the async-tool seam): pin one runtime to a
//! dedicated background thread that is created **and** destroyed on that plain,
//! non-async thread. `execute` submits a closure (which drives the async work
//! via [`tokio::runtime::Runtime::block_on`] on the worker) and blocks the
//! calling thread on a **bounded** wait for the result. Tool execution is a
//! turn boundary; the bounded blocking mirrors the session's existing
//! `MAX_TOOL_ROUNDS` / `companion.tool_loop` cap rather than being a new
//! unbounded stall vector.
//!
//! A panic inside a submitted closure is contained on the worker ([`catch_unwind`]),
//! reported as an error result, and does **not** kill the worker loop or the
//! process — the same panic-containment discipline lambo exercises at its MCP
//! boundary.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::runtime::Runtime;

/// Work item: a boxed closure that runs on the pinned runtime's thread.
type Job = Box<dyn FnOnce(&Runtime) + Send + 'static>;

/// Why a job submitted to [`ToolRuntime::run`] produced no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRunError {
    /// The worker thread is gone (it only exits at shutdown, or the loop died).
    Unavailable,
    /// The calling thread's bounded wait elapsed before the job answered.
    /// The job keeps running on the worker; the caller has already moved on.
    TimedOut,
    /// The job panicked on the worker. Contained here, not a dead process.
    Panicked,
}

/// A Tokio runtime that lives on its own OS thread.
///
/// Constructing this spawns the worker thread, which builds the runtime and
/// then services request closures until the channel closes (on drop).
pub struct ToolRuntime {
    tx: Option<Sender<Job>>,
    handle: Option<JoinHandle<()>>,
}

impl ToolRuntime {
    /// Spawn the worker thread and its runtime.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let handle = std::thread::Builder::new()
            .name("mooshik-tools".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(_) => return, // nothing can run without a runtime
                };
                // Each job's panic is contained here so one bad tool cannot kill
                // the loop: the worker stays alive for the next call.
                while let Ok(job) = rx.recv() {
                    let _ = catch_unwind(AssertUnwindSafe(|| job(&rt)));
                }
            })
            .expect("spawn mooshik-tools worker thread");
        Self {
            tx: Some(tx),
            handle: Some(handle),
        }
    }

    /// Run `f` on the pinned runtime, blocking for at most `timeout`.
    ///
    /// `f` drives any async work itself via the [`Runtime`] it is handed (for
    /// example with [`Runtime::block_on`]); the result is delivered back to the
    /// calling thread.
    pub fn run<R>(
        &self,
        f: impl FnOnce(&Runtime) -> R + Send + 'static,
        timeout: Duration,
    ) -> Result<R, ToolRunError>
    where
        R: Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<R>();
        let job: Job = Box::new(move |rt| {
            let _ = tx.send(f(rt));
        });
        self.tx
            .as_ref()
            .ok_or(ToolRunError::Unavailable)?
            .send(job)
            .map_err(|_| ToolRunError::Unavailable)?;
        match rx.recv_timeout(timeout) {
            Ok(value) => Ok(value),
            Err(RecvTimeoutError::Timeout) => Err(ToolRunError::TimedOut),
            Err(RecvTimeoutError::Disconnected) => Err(ToolRunError::Panicked),
        }
    }
}

impl Default for ToolRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ToolRuntime {
    fn drop(&mut self) {
        // Disconnecting every sender ends the worker's `recv` loop. We drop the
        // runtime on the worker's own (non-async) thread, never inside an async
        // context.
        self.tx.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panicking_job_is_contained_and_the_worker_survives() {
        let worker = ToolRuntime::new();
        // A job that panics must come back as an error, not kill the process.
        let panicked = worker.run(|_rt| -> i32 { panic!("boom") }, Duration::from_secs(2));
        assert_eq!(panicked, Err(ToolRunError::Panicked));
        // The worker is still alive and usable afterwards.
        let ok = worker.run(|rt| rt.block_on(async { 41 + 1 }), Duration::from_secs(2));
        assert_eq!(ok, Ok(42));
    }

    #[test]
    fn timed_out_job_does_not_kill_the_worker() {
        let worker = ToolRuntime::new();
        let late = worker.run(
            |rt| -> String {
                rt.block_on(async {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    "late".to_owned()
                })
            },
            Duration::from_millis(50),
        );
        assert_eq!(late, Err(ToolRunError::TimedOut));
        // The bounded wait protects the caller; the worker stays functional.
        let ok = worker.run(|_rt| 7, Duration::from_secs(5));
        assert_eq!(ok, Ok(7));
    }
}
