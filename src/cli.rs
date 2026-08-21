//! CLI definition.
//!
//! Built with clap's builder API rather than derive: help strings come from the
//! `text` module at runtime, and derive attributes only accept literals. Every
//! new subcommand registers here and adds its strings to `text/en.toml`.

use clap::Command;

use crate::text;

pub fn command() -> Command {
    Command::new("mooshik")
        .version(env!("CARGO_PKG_VERSION"))
        .about(text::get("app.about"))
        .after_help(text::get("app.after_help"))
        .subcommand_required(false)
        .arg_required_else_help(true)
}

/// Parse argv and dispatch. Subcommands arrive with their milestones; until then
/// the surface is help and version only.
pub fn run() -> anyhow::Result<()> {
    let _matches = command().get_matches();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
