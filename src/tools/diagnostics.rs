//! Execute-time diagnostics: a sink, not a return value.
//!
//! Assembly-time notices come back as [`super::ChatStack::notices`]. Execute-time
//! messages — a tool panic, a failed MCP spawn — happen inside a call the
//! session is already driving, so they cannot be returned to anyone. The CLI
//! path prints them on stderr. Under the alternate screen a print corrupts the
//! frame, so the pane path installs a sink the redraw loop drains.

use std::sync::Arc;

/// Where an execute-time diagnostic goes.
///
/// Cloneable so a composite stack can hand the same sink to memory tools, the
/// permission gate, and the MCP host. The default writes stderr, which is what
/// `mooshik chat` wants; the pane installs a channel instead.
#[derive(Clone)]
pub struct Diagnostics {
    emit: Arc<dyn Fn(&str) + Send + Sync>,
}

impl Diagnostics {
    /// The CLI path: stderr is where a notice belongs when this process owns
    /// the terminal.
    pub fn stderr() -> Self {
        Self {
            emit: Arc::new(|msg| eprintln!("{msg}")),
        }
    }

    /// The pane path: the callback must not print. It typically sends on a
    /// channel the redraw loop drains.
    pub fn sink(emit: impl Fn(&str) + Send + Sync + 'static) -> Self {
        Self {
            emit: Arc::new(emit),
        }
    }

    /// Deliver one diagnostic. The message is already a rendered sentence;
    /// this does not format, classify, or translate.
    pub fn emit(&self, message: &str) {
        (self.emit)(message);
    }
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self::stderr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_sink_receives_what_emit_sends() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&seen);
        let diagnostics =
            Diagnostics::sink(move |msg| captured.lock().unwrap().push(msg.to_owned()));
        diagnostics.emit("one");
        diagnostics.emit("two");
        assert_eq!(*seen.lock().unwrap(), ["one", "two"]);
    }
}
