//! User-facing strings, loaded from TOML rather than written inline in Rust.
//!
//! Every help line, label and message lives in `en.toml` (see that file's header
//! for the schema rules). Keeping text out of source serves two goals at once:
//! localization becomes "add another file with the same keys", and Rust files
//! stay inside their line budget instead of bloating with prose.
//!
//! Today the default locale is embedded via `include_str!`. Once M1 lands the
//! home directory, the same loader can prefer `~/.mooshik/i18n/<locale>.toml`
//! selected by config, falling back to the embedded default — the seam is this
//! module and nothing else.

use std::sync::LazyLock;

use toml::{Table, Value};

const DEFAULT_LOCALE: &str = include_str!("en.toml");

static TABLE: LazyLock<Table> =
    LazyLock::new(|| toml::from_str(DEFAULT_LOCALE).expect("embedded en.toml must parse as TOML"));

/// Resolve a dotted key such as `app.after_help` to its string value.
///
/// Missing keys or non-string values are programmer errors against the embedded
/// schema, so they panic with the key named — this must never be reachable from
/// user input.
pub fn get(key: &str) -> &'static str {
    let segments: Vec<&str> = key.split('.').collect();
    let mut current: &Table = &TABLE;
    for (index, segment) in segments.iter().enumerate() {
        let last = index == segments.len() - 1;
        match current.get(*segment) {
            Some(Value::String(s)) if last => return s.as_str(),
            Some(Value::Table(t)) if !last => current = t,
            Some(_) => panic!("text key {key:?} exists but has the wrong shape"),
            None => panic!("text key {key:?} is missing from en.toml"),
        }
    }
    unreachable!("empty key never reaches get")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_nested_keys() {
        assert!(!get("app.about").is_empty());
        assert!(get("app.after_help").contains("peer"));
    }

    #[test]
    fn every_leaf_is_a_nonempty_string() {
        fn check(value: &Value, path: String) {
            match value {
                Value::String(s) => assert!(!s.is_empty(), "{path} is empty"),
                Value::Table(t) => {
                    for (k, v) in t {
                        check(v, format!("{path}.{k}"));
                    }
                }
                other => panic!("{path} is {other:?}, not a string"),
            }
        }
        for (k, v) in &*TABLE {
            check(v, k.clone());
        }
    }

    #[test]
    #[should_panic(expected = "missing")]
    fn missing_key_panics_with_the_key_named() {
        get("app.no_such_key");
    }

    #[test]
    #[should_panic(expected = "wrong shape")]
    fn non_leaf_key_panics() {
        get("app");
    }
}
