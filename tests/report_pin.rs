//! P2-b: `Failure::report` is THE one place an error reaches the terminal.
//!
//! The unit pins cover `rendered()` (data level) and the main.rs source text;
//! neither observes `report`'s actual print. This test drives the real binary
//! in a subprocess and asserts its stderr byte-for-byte: exactly the
//! top-level message, never a cause chain — even when the chain carries
//! planted connection material.

use std::fs;
use std::process::Command;

use mooshik::home::HomeLayout;

#[test]
fn report_prints_only_the_top_level_message_even_when_the_chain_carries_material() {
    // Canonicalized because the home walk refuses symlinked components and
    // macOS's temp dir sits under `/var -> private/var`.
    let root = std::env::temp_dir()
        .canonicalize()
        .expect("platform temp dir resolves")
        .join(format!("mooshik-report-pin-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let layout = HomeLayout::new(&root);
    layout.init().expect("fixture home initializes");

    // Postgres DSN against a refused local port: `recall` fails fast with a
    // backend error whose wrapped chain names this DSN material.
    const DSN_MATERIAL: &str = "postgres://m7user:m7p4ssw0rd@127.0.0.1:9/mooshik";
    fs::write(&layout.config, format!("[store]\ndsn = '{DSN_MATERIAL}'\n"))
        .expect("write fixture config");

    let output = Command::new(env!("CARGO_BIN_EXE_mooshik"))
        .env("MOOSHIK_HOME", &root)
        // Keep ambient DSN authorities from overriding the fixture config.
        .env_remove("MOOSHIK_POSTGRES_DSN")
        .env_remove("LAMBO_POSTGRES_DSN")
        .env_remove("DATABASE_URL")
        .args(["recall", "deploy checklist"])
        .output()
        .expect("run the mooshik binary");

    assert_eq!(output.status.code(), Some(1), "backend failure is internal");
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf-8");
    let expected = format!("{}\n", mooshik::text::get("memory.backend_failed"));
    assert_eq!(
        stderr, expected,
        "the terminal must see exactly the top-level message, never a chain"
    );
    assert!(!stderr.contains("m7p4ssw0rd"), "{stderr}");
    assert!(!stderr.contains("postgres://"), "{stderr}");
    assert!(output.stdout.is_empty(), "errors go to stderr only");

    let _ = fs::remove_dir_all(root);
}
