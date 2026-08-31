//! Deterministic, net-free tests for the M10 MCP host against a fixture MCP
//! server. The fixture (`tests/fixture_server.py`) speaks newline-delimited MCP
//! JSON-RPC over stdio with only the Python standard library; tests spawn it via
//! rmcp's `transport-child-process`, the same path the live host uses. No
//! network, no external services.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;

use super::McpTools;
use crate::companion::ToolExecutor;
use crate::config::{Config, McpServerConfig, RawGrant};
use crate::vault::{PassphraseProvider, Vault as VaultT};

/// Absolute path to the fixture server script.
fn fixture_path() -> String {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/mcp_host/tests/fixture_server.py"
    )
    .to_owned()
}

/// Build a Config with one `[mcp_servers]` entry pointing at the fixture.
fn config_with_server(expose: &[&str], name: &str) -> Config {
    let mut config = Config::default();
    config.mcp_servers.insert(
        name.to_owned(),
        McpServerConfig {
            command: "python3".to_owned(),
            args: vec![fixture_path()],
            env: BTreeMap::new(),
            expose: expose.iter().map(|s| s.to_string()).collect(),
        },
    );
    config
}

// ---- tool listing + expose filtering ---------------------------------------

#[test]
fn specs_expose_only_the_allowlisted_tools() {
    let config = config_with_server(&["echo", "add"], "srv");
    let executor = Arc::new(McpTools::from_config(&config, None));
    let all_specs = executor.specs();
    let names: Vec<&str> = all_specs.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"mcp.srv.echo"), "got {names:?}");
    assert!(names.contains(&"mcp.srv.add"), "got {names:?}");
    assert!(!names.contains(&"mcp.srv.uuid"), "got {names:?}");
    let echo = all_specs.iter().find(|s| s.name == "mcp.srv.echo").unwrap();
    assert!(echo.description.contains("Echo"));
    assert_eq!(
        echo.parameters,
        json!({"type":"object","properties":{},"additionalProperties":true})
    );
}

#[test]
fn empty_expose_leaves_the_server_inert() {
    let config = config_with_server(&[], "srv");
    let executor = Arc::new(McpTools::from_config(&config, None));
    assert!(executor.specs().is_empty());
}

// ---- execute round-trip ------------------------------------------------------

#[test]
fn execute_echo_round_trips_arguments() {
    let config = config_with_server(&["echo"], "srv");
    let executor = Arc::new(McpTools::from_config(&config, None));
    let output = executor.execute("mcp.srv.echo", &json!({"a": 1, "b": "x"}));
    assert_eq!(output, "{\"a\": 1, \"b\": \"x\"}");
}

#[test]
fn execute_add_returns_the_sum() {
    let config = config_with_server(&["add"], "srv");
    let executor = Arc::new(McpTools::from_config(&config, None));
    let output = executor.execute("mcp.srv.add", &json!({"a": 20, "b": 22}));
    assert_eq!(output, "42");
}

// ---- unknown tool -----------------------------------------------------------

#[test]
fn unknown_tool_returns_a_contained_error() {
    let config = config_with_server(&["echo"], "srv");
    let executor = Arc::new(McpTools::from_config(&config, None));
    let output = executor.execute("mcp.srv.uuid", &json!({}));
    assert!(output.contains("no exposed tool"), "got {output:?}");
    let output = executor.execute("mcp.nope.echo", &json!({}));
    assert!(output.contains("unknown MCP server"), "got {output:?}");
    let output = executor.execute("run_scratch_script", &json!({}));
    assert!(output.contains("must start with 'mcp.'"), "got {output:?}");
}

// ---- isError -> contained string --------------------------------------------

#[test]
fn is_error_result_becomes_a_contained_error_string() {
    let config = config_with_server(&["fail"], "srv");
    let executor = Arc::new(McpTools::from_config(&config, None));
    let output = executor.execute("mcp.srv.fail", &json!({}));
    assert!(output.contains("simulated failure"), "got {output:?}");
}

// ---- reconnect after the fixture crashes ------------------------------------

#[test]
fn a_crashed_child_is_reconnected_on_the_next_call() {
    let config = config_with_server(&["echo", "crash"], "srv");
    let executor = Arc::new(McpTools::from_config(&config, None));
    let first = executor.execute("mcp.srv.echo", &json!({"n": 1}));
    assert_eq!(first, "{\"n\": 1}");
    let _ = executor.execute("mcp.srv.crash", &json!({}));
    let second = executor.execute("mcp.srv.echo", &json!({"n": 2}));
    assert_eq!(second, "{\"n\": 2}");
}

// ---- a hung-but-alive child frees the worker (P2-M10-1) ---------------------

#[test]
fn a_hung_but_alive_child_does_not_pin_the_worker() {
    use std::time::Duration;
    let config = config_with_server(&["echo", "hang"], "srv");
    let executor =
        Arc::new(McpTools::from_config(&config, None).with_call_wait(Duration::from_millis(300)));
    // Warm the worker with the fixture up.
    let warm = executor.execute("mcp.srv.echo", &json!({"n": 1}));
    assert_eq!(warm, "{\"n\": 1}");
    // A child that never answers must surface a contained error within the
    // per-call bound — and the worker must still be usable afterwards.
    let hung = executor.execute("mcp.srv.hang", &json!({}));
    assert!(
        !hung.is_empty() && !hung.contains("panicked"),
        "hung call must be a contained error, got {hung:?}"
    );
    let after = executor.execute("mcp.srv.echo", &json!({"n": 2}));
    assert_eq!(after, "{\"n\": 2}");
}

#[test]
fn missing_secret_fails_the_server_closed() {
    let mut config = Config::default();
    config.mcp_servers.insert(
        "needy".to_owned(),
        McpServerConfig {
            command: "python3".to_owned(),
            args: vec![fixture_path()],
            env: BTreeMap::from([("MOOSHIK_NEEDY_TOKEN".into(), "no-such-secret".into())]),
            expose: vec!["echo".into()],
        },
    );
    let executor = Arc::new(McpTools::from_config(&config, None));
    assert!(executor.specs().is_empty());
    let output = executor.execute("mcp.needy.echo", &json!({"x": 1}));
    assert!(output.contains("could not complete tool"), "got {output:?}");
}

#[test]
fn a_present_secret_is_injected_into_the_child_environment() {
    let vault = fixture_vault(&[("gh-token", "super-secret-value")]);
    let mut config = config_with_server(&["echo"], "vim");
    config.mcp_servers.get_mut("vim").unwrap().env =
        BTreeMap::from([("MOOSHIK_TEST_TOKEN".into(), "gh-token".into())]);
    let executor = Arc::new(McpTools::from_config(&config, Some(vault)));
    let output = executor.execute("mcp.vim.echo", &json!({"k": "v"}));
    assert_eq!(output, "{\"k\": \"v\"}");
}

// ---- gate integration: mcp.* requires a grant --------------------------------

#[test]
fn mcp_tools_are_absent_without_a_grant_and_present_with_one() {
    use crate::tools::executor_for_chat;
    let config = config_with_server(&["echo"], "srv");
    // Default grants (all deny except the memory trio) => mcp.* hidden.
    let stack = executor_for_chat(&config, None);
    let names: Vec<String> = stack.specs().iter().map(|s| s.name.clone()).collect();
    assert!(
        !names.iter().any(|n| n.starts_with("mcp.")),
        "got {names:?}"
    );

    // Grant mcp.srv.* => the mcp tool flows through the same gate.
    let mut granted = config.clone();
    granted.permissions.entries =
        BTreeMap::from([("mcp.srv.*".to_owned(), RawGrant::Mode("allow".to_owned()))]);
    let stack = executor_for_chat(&granted, None);
    let names: Vec<String> = stack.specs().iter().map(|s| s.name.clone()).collect();
    assert!(
        names.iter().any(|n| n == "mcp.srv.echo"),
        "granted stack must expose mcp.srv.echo, got {names:?}"
    );
}

// ---- helpers ------------------------------------------------------------------

fn fixture_vault(secrets: &[(&str, &str)]) -> crate::vault::SharedVault {
    // A counter, not the clock: macOS's realtime clock ticks in microseconds,
    // so nanosecond-named dirs collide across parallel tests and one test's
    // cleanup races another's open.
    static FIXTURE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = crate::secure_path::canonical_temp_dir().join(format!(
        "mooshik-mcp-vault-{}-{}",
        std::process::id(),
        FIXTURE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut vault = VaultT::open(
        dir.join("vault"),
        Arc::new(PassphraseProvider::new("pw").unwrap()),
    )
    .unwrap();
    for (name, value) in secrets {
        vault.set(name, value).unwrap();
    }

    vault.shared()
}

#[test]
fn a_later_call_after_a_dead_initial_spawn_recovers() {
    // P3-M10-2: attempt_spawn leaves `spawned=false` on a failed first pass,
    // so a server that came up late is NOT hidden from specs() forever. Crash
    // on first expose'd call, then the next echo must reconnect and succeed.
    let config = config_with_server(&["echo", "crash"], "srv");
    let executor = Arc::new(McpTools::from_config(&config, None));
    let _ = executor.execute("mcp.srv.crash", &json!({}));
    let out = executor.execute("mcp.srv.echo", &json!({"k": "v"}));
    assert_eq!(out, "{\"k\": \"v\"}");
}

#[test]
fn execute_time_diagnostics_do_not_print() {
    // M12e: under the alternate screen a print corrupts the frame. These
    // sites used to be eprintln!; they must go through Diagnostics so the
    // pane can drain them. The CLI path still prints because the default
    // sink is stderr.
    // `with_call_wait` is `#[cfg(test)]` mid-file, so splitting on that
    // marker would drop the execute path. The tests live in this sibling.
    let production = include_str!("mod.rs")
        .split("#[cfg(test)]\nmod tests;")
        .next()
        .unwrap();
    for forbidden in ["eprintln!", "print!", "eprint!"] {
        assert!(
            !production.contains(forbidden),
            "mcp_host execute-time diagnostics must not {forbidden}"
        );
    }
    assert!(
        production.contains(".emit("),
        "execute-time diagnostics must go through the sink"
    );
}

#[test]
fn child_stderr_reaches_the_sink_without_corrupting_tool_output() {
    use std::sync::{Arc, Mutex};

    let vault = fixture_vault(&[("marker", "fixture-noise-77")]);
    let mut config = config_with_server(&["echo"], "srv");
    config.mcp_servers.get_mut("srv").unwrap().env =
        BTreeMap::from([("MOOSHIK_STDERR_MARKER".into(), "marker".into())]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&seen);
    let diagnostics = crate::tools::Diagnostics::sink(move |message: &str| {
        captured.lock().unwrap().push(message.to_owned());
    });
    let executor =
        Arc::new(McpTools::from_config(&config, Some(vault)).with_diagnostics(diagnostics));
    let output = executor.execute("mcp.srv.echo", &json!({"ok": 1}));
    assert_eq!(output, "{\"ok\": 1}");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if seen
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("fixture-noise-77"))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "child stderr never reached the sink: {seen:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_child_that_floods_stderr_past_the_pipe_buffer_still_handshakes() {
    // The deadlock guard: a piped stderr nobody drains fills the 64 KiB pipe
    // buffer and blocks the child before it answers initialize. Four MiB of
    // startup noise must not stall the handshake or the first tool call.
    let vault = fixture_vault(&[("noise", "4194304")]);
    let mut config = config_with_server(&["echo"], "srv");
    config.mcp_servers.get_mut("srv").unwrap().env =
        BTreeMap::from([("MOOSHIK_STDERR_BYTES".into(), "noise".into())]);
    let executor = Arc::new(McpTools::from_config(&config, Some(vault)));
    let names: Vec<String> = executor.specs().iter().map(|s| s.name.clone()).collect();
    assert!(names.iter().any(|n| n == "mcp.srv.echo"), "got {names:?}");
    let output = executor.execute("mcp.srv.echo", &json!({"n": 1}));
    assert_eq!(output, "{\"n\": 1}");
}
