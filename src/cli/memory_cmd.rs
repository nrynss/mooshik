//! The commands that open workspace memory: `init`, `serve`, `recall`, `stats`.

use crate::{config::Config, home::HomeLayout, text};

use super::{block_on, render, resolve};

pub(crate) fn initialize(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.init().map_err(anyhow::Error::new)?;
    let mut config = Config::load_at(&root).map_err(anyhow::Error::new)?;
    // First run creates the vault, so it is opened here regardless — and the
    // same handle resolves any secret reference the file already carries.
    let vault = resolve::open_vault(layout, &config, &root)?;
    resolve::resolve_secrets(&mut config, &vault).map_err(anyhow::Error::new)?;
    drop(vault);
    block_on(crate::memory::provision(&config))?;
    println!("{}", text::get("home.init_done"));
    Ok(())
}

pub(crate) fn serve(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = resolve::load_with_secrets(layout, &root)?;
    block_on(crate::memory::serve(&config))
}

/// One-shot search over workspace memory (`crate::memory::recall` opens and
/// closes its own handle), then render the hits for the local operator.
pub(crate) fn recall(layout: &HomeLayout, matches: &clap::ArgMatches) -> anyhow::Result<()> {
    let query = matches
        .get_one::<String>("query")
        .expect("clap marks the query argument required")
        .clone();
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = resolve::load_with_secrets(layout, &root)?;
    let recalled = block_on(crate::memory::recall(&config, query.clone()))?;
    println!("{}", render::render_recall(&query, &recalled));
    Ok(())
}

/// Session health over workspace memory, rendered for the local operator.
pub(crate) fn stats(layout: &HomeLayout) -> anyhow::Result<()> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = resolve::load_with_secrets(layout, &root)?;
    let health = block_on(crate::memory::stats(&config))?;
    println!("{}", render::render_stats(&health));
    Ok(())
}
