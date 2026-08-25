use std::sync::Arc;

use tokio::sync::watch;

/// Abort handle for an in-flight companion request.
#[derive(Clone)]
pub struct Cancellation {
    tx: Arc<watch::Sender<bool>>,
    rx: watch::Receiver<bool>,
}

impl Cancellation {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self {
            tx: Arc::new(tx),
            rx,
        }
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.tx.borrow()
    }

    pub async fn cancelled(&self) {
        let mut rx = self.rx.clone();
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                return;
            }
        }
    }
}

impl Default for Cancellation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelled_resolves_after_cancel() {
        let cancel = Cancellation::new();
        assert!(!cancel.is_cancelled());
        cancel.cancel();
        cancel.cancelled().await;
        assert!(cancel.is_cancelled());
    }
}
