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

/// One reflect pass over workspace memory: write this session's prose (and
/// consolidate paraphrase twins), or — with `--dry-run` — report what that
/// pass would write without touching the graph. Opens and closes its own
/// [`Memory`] handle like `stats`, because the pass is one-shot.
pub(crate) fn reflect(layout: &HomeLayout, matches: &clap::ArgMatches) -> anyhow::Result<()> {
    let root = layout.open_existing_root().map_err(anyhow::Error::new)?;
    let config = resolve::load_with_secrets(layout, &root)?;
    let dry_run = matches.get_flag("dry_run");
    let now = chrono::Utc::now();
    let outcome = block_on(async move {
        let memory = crate::memory::open(&config).await?;
        let result =
            crate::memory::run_reflect(&memory, &crate::memory::FixtureReflector, dry_run, now)
                .await;
        let outcome = result.map_err(crate::memory::MemoryError::from)?;
        memory.close().await?;
        Ok(outcome)
    })?;
    println!("{}", render::render_reflect(&outcome, dry_run));
    Ok(())
}
