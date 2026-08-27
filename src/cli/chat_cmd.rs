//! `mooshik chat`.
//!
//! **This module never touches the memory subsystem** (M3/M4 pin, enforced
//! file-wide by `tools::recall`'s source pin and by `chat_command_never_opens_memory`
//! in `cli::tests`). Memory is opened by `tools::executor_for_chat`, which owns
//! the one `Memory` and the one single-writer lease; the chat entry point only
//! composes what that factory returns.

use std::sync::Arc;

use crate::{config::Config, home::HomeLayout, vault::Vault};

use super::resolve;

/// Test seam for the pin below: chat loads configuration without ever
/// touching the memory subsystem.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn load_chat_config(layout: &HomeLayout) -> anyhow::Result<Config> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    Config::load_at(&root).map_err(anyhow::Error::new)
}

pub(crate) fn chat(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let mut config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    // Open the vault ONCE for the whole session: it holds an exclusive lock
    // for its lifetime, so a second open in this process would block. Failure
    // is not fatal on its own (the tool boundary prints one notice and
    // continues unredacted) — but a configured secret *reference* that cannot
    // be resolved is, because running on unresolved credentials is worse than
    // not running.
    let vault = open_vault_for_chat(layout, &config, &root);
    match &vault {
        Some(vault) => resolve::resolve_secrets(&mut config, vault).map_err(anyhow::Error::new)?,
        None if resolve::needs_vault(&config) => {
            return Err(anyhow::Error::new(
                crate::config::ConfigError::VaultUnavailable,
            ))
        }
        None => {}
    }
    let executor = crate::tools::executor_for_chat(&config, vault.map(Vault::shared));
    // The injector shares the executor's Arc — one open Memory, one lease —
    // so turns dropped for context pressure come back as recalled memory.
    let recall = crate::tools::recall_for_chat(&config, Arc::clone(&executor));
    crate::companion::run_chat(&config, executor, recall).map_err(anyhow::Error::new)
}

/// Open the shared vault handle for chat. `None` on any failure — provider
/// selection, keyring/passphrase problems, a bad file — which
/// `executor_for_chat` turns into one stderr notice plus unredacted-only-
/// because-unopenable operation.
fn open_vault_for_chat(
    layout: &HomeLayout,
    config: &Config,
    root: &std::fs::File,
) -> Option<Vault> {
    resolve::open_vault(layout, config, root).ok()
}
