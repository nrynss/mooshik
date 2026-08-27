//! The clap command tree.
//!
//! Built with clap's builder API rather than derive: help strings come from the
//! `text` module at runtime, and derive attributes only accept literals. Every
//! new subcommand registers here and adds its strings to `text/en.toml`.

use clap::{Arg, ArgAction, Command};

use crate::text;

pub fn command() -> Command {
    Command::new("mooshik")
        .version(env!("CARGO_PKG_VERSION"))
        .about(text::get("app.about"))
        .after_help(text::get("app.after_help"))
        .subcommand(Command::new("init").about(text::get("config.init_help")))
        .subcommand(
            Command::new("serve")
                .about(text::get("memory.serve_help"))
                .after_help(text::get("memory.serve_after_help")),
        )
        .subcommand(
            Command::new("chat")
                .about(text::get("companion.chat_help"))
                .after_help(text::get("companion.chat_after_help")),
        )
        .subcommand(
            Command::new("tui")
                .about(text::get("tui.help"))
                .after_help(text::get("tui.after_help"))
                .arg(
                    Arg::new("demo")
                        .long("demo")
                        .action(ArgAction::SetTrue)
                        .help(text::get("tui.demo_help")),
                ),
        )
        .subcommand(
            Command::new("recall")
                .about(text::get("memory.recall_help"))
                .after_help(text::get("memory.recall_after_help"))
                .arg(
                    Arg::new("query")
                        .help(text::get("memory.query_help"))
                        .required(true),
                ),
        )
        .subcommand(
            Command::new("stats")
                .about(text::get("memory.stats_help"))
                .after_help(text::get("memory.stats_after_help")),
        )
        .subcommand(
            Command::new("config")
                .about(text::get("config.show_help"))
                .subcommand_required(true)
                .subcommand(Command::new("show").about(text::get("config.show_help")))
                .subcommand(config_set_command()),
        )
        .subcommand(Command::new("permissions").about(text::get("permissions.help")))
        .subcommand(
            Command::new("secret")
                .about(text::get("vault.list_help"))
                .subcommand(secret_command("set", text::get("vault.set_help")))
                .subcommand(secret_command("get", text::get("vault.get_help")))
                .subcommand(Command::new("list").about(text::get("vault.list_help")))
                .subcommand_required(true),
        )
        .subcommand_required(false)
        .arg_required_else_help(true)
}

/// `mooshik config set <key> <value>`.
///
/// The settable keys are listed in `--help` from the same table the writer
/// enforces, so the two can never drift: adding a key to `config::write`
/// documents it here for free.
fn config_set_command() -> Command {
    let after = format!(
        "{}\n\n{} {}.",
        text::get("config.set_after_help"),
        text::get("config.set_keys_header"),
        crate::config::settable_keys().join(", ")
    );
    Command::new("set")
        .about(text::get("config.set_help"))
        .after_help(after)
        .arg(
            Arg::new("key")
                .help(text::get("config.set_key_help"))
                .required(true),
        )
        .arg(
            Arg::new("value")
                .help(text::get("config.set_value_help"))
                .required(true),
        )
        .arg(
            Arg::new("confirm-database-change")
                .long("confirm-database-change")
                .help(text::get("config.set_confirm_help"))
                .action(ArgAction::SetTrue),
        )
}

fn secret_command(name: &'static str, help: &'static str) -> Command {
    let command = Command::new(name).about(help).arg(
        Arg::new("name")
            .help(text::get("vault.name_help"))
            .required(true),
    );
    if name == "set" {
        command.after_help(text::get("vault.set_after_help"))
    } else {
        command
    }
}
