use zeroize::Zeroizing;

use super::*;

/// A config file with the shapes the editor has to survive: a header comment,
/// an inline comment on a key it will rewrite, a blank line, and a later table
/// whose keys share a prefix with the edited one.
const SAMPLE: &str = "\
# Mooshik configuration.
[store]
kind = \"postgres\"   # the product backend

[companion]
base_url = \"http://127.0.0.1:8080/v1\"
model = \"local-model\"
temperature = 0.2
context_window = 32768
";

/// One database, spelled two ways: bare, and with the scheme alias, the host
/// in another casing, and the default port written out. Lambo's
/// `store_dsn_identity` must see one database here — that equivalence is the
/// whole reason a cosmetic edit passes without ceremony.
///
/// The password overlay is the other cosmetic case the identity rule covers.
/// It is exercised by `overlay::tests::password_overlay_of_one_database_is_accepted`
/// against the same function; this file cannot add a second fixture for it,
/// because a repository must not carry a string shaped like a credential.
const PLAIN_DSN: &str = "postgres://u@host/db";
const SAME_DATABASE_DSN: &str = "postgresql://u@HOST:5432/db";
const OTHER_DATABASE_DSN: &str = "postgres://u@other-host/db";

fn never_resolves(_: &str) -> Result<Zeroizing<String>, ConfigError> {
    panic!("the guard must not resolve a secret it was not given");
}

#[test]
fn a_written_setting_round_trips_and_keeps_the_rest_of_the_file_byte_for_byte() {
    let edited = apply_setting(SAMPLE, "companion.model", "test-placeholder-model", []).unwrap();
    let config = Config::from_toml_and_env(&edited, []).unwrap();
    assert_eq!(config.companion.model, "test-placeholder-model");
    // Everything the operator wrote is still there, including the comment on
    // an untouched line and the blank line between tables.
    assert!(edited.starts_with("# Mooshik configuration.\n"), "{edited}");
    assert!(edited.contains("kind = \"postgres\"   # the product backend"));
    assert!(edited.contains("\n\n[companion]"), "{edited}");
    assert!(
        edited.contains("model = \"test-placeholder-model\""),
        "{edited}"
    );
    assert!(!edited.contains("local-model"), "{edited}");
    // And nothing else moved: exactly one line differs.
    let changed: Vec<_> = SAMPLE
        .lines()
        .zip(edited.lines())
        .filter(|(before, after)| before != after)
        .collect();
    assert_eq!(changed.len(), 1, "{changed:?}");
}

#[test]
fn an_inline_comment_on_the_edited_line_survives_the_edit() {
    let edited = apply_setting(SAMPLE, "store.kind", "memory", []).unwrap();
    assert!(
        edited.contains("kind = \"memory\"   # the product backend"),
        "{edited}"
    );
    assert_eq!(
        Config::from_toml_and_env(&edited, []).unwrap().store.kind,
        StoreKind::Memory
    );
}

#[test]
fn a_missing_key_is_inserted_under_its_table_and_a_missing_table_is_appended() {
    let with_project = apply_setting(SAMPLE, "companion.google_project", "proj", []).unwrap();
    assert!(
        with_project.contains("[companion]\ngoogle_project = \"proj\"\n"),
        "{with_project}"
    );
    let edited = apply_setting(&with_project, "companion.auth", "google", []).unwrap();
    assert_eq!(
        Config::from_toml_and_env(&edited, [])
            .unwrap()
            .companion
            .auth,
        crate::config::CompanionAuth::Google
    );

    let appended = apply_setting(SAMPLE, "session.id", "workspace", []).unwrap();
    assert!(
        appended.contains("\n[session]\nid = \"workspace\"\n"),
        "{appended}"
    );
    assert_eq!(
        Config::from_toml_and_env(&appended, []).unwrap().session.id,
        "workspace"
    );
}

#[test]
fn a_google_posture_without_a_project_is_refused_rather_than_written() {
    // The whole file is validated, not only the key that was touched: an
    // endpoint derived from a project that does not exist is not a runtime
    // 404 to discover mid-chat.
    let error = apply_setting("", "companion.auth", "google", []).unwrap_err();
    assert!(
        matches!(error, ConfigError::MissingGoogleProject),
        "{error:?}"
    );
    assert!(error.to_string().contains("companion.google_project"));
}

#[test]
fn unknown_keys_fail_closed_naming_the_key_and_what_is_settable() {
    let error = apply_setting(SAMPLE, "companion.modle", "x", []).unwrap_err();
    let message = error.to_string();
    assert!(matches!(error, ConfigError::UnknownKey(_)));
    assert!(message.contains("companion.modle"), "{message}");
    assert!(message.contains("companion.model"), "{message}");
    // Never a Debug dump of a Rust type.
    assert!(!message.contains("ConfigError"), "{message}");
}

#[test]
fn invalid_values_name_the_key_and_what_is_valid_without_echoing_the_value() {
    for (key, value, expect) in [
        ("companion.context_window", "0", "greater than zero"),
        ("companion.temperature", "warm", "number"),
        ("companion.base_url", "ftp://elsewhere/v1", "http://"),
        ("store.dsn_secret", "not a name!", "secret set"),
        ("companion.auth", "oauth", "\"google\""),
        ("vault.provider", "yubikey", "keyring"),
        ("embedder.kind", "word2vec", "gemini"),
        ("embedder.dim", "-4", "greater than zero"),
    ] {
        let error = apply_setting(SAMPLE, key, value, []).unwrap_err();
        let message = error.to_string();
        assert!(
            matches!(error, ConfigError::InvalidSetting { .. }),
            "{key}: {error:?}"
        );
        assert!(message.contains(key), "{key}: {message}");
        assert!(message.contains(expect), "{key}: {message}");
    }
    // A rejected value can itself be credential material, so it is never
    // echoed back to the terminal.
    let error = apply_setting(SAMPLE, "store.dsn_secret", PLAIN_DSN, []).unwrap_err();
    let message = error.to_string();
    assert!(!message.contains(PLAIN_DSN), "{message}");
    assert!(!message.contains("postgres://"), "{message}");
}

#[test]
fn credential_keys_are_refused_and_point_at_the_vault_reference() {
    for (key, reference) in [
        ("store.dsn", "store.dsn_secret"),
        ("companion.api_key", "companion.api_key_secret"),
    ] {
        let error = apply_setting(SAMPLE, key, "a-value-that-must-not-land", []).unwrap_err();
        let message = error.to_string();
        assert!(matches!(error, ConfigError::SecretKey { .. }), "{error:?}");
        assert!(message.contains(key), "{message}");
        assert!(message.contains(reference), "{message}");
        assert!(message.contains("mooshik secret set"), "{message}");
        assert!(
            !message.contains("a-value-that-must-not-land"),
            "the refused credential must not be echoed: {message}"
        );
    }
    // The reference keys themselves ARE settable, and hold only a name.
    for (key, field) in [
        ("store.dsn_secret", "dsn_secret"),
        ("companion.api_key_secret", "api_key_secret"),
    ] {
        let edited = apply_setting(SAMPLE, key, "prod-credential", []).unwrap();
        assert!(
            edited.contains(&format!("{field} = \"prod-credential\"")),
            "{edited}"
        );
    }
}

/// The pin the requirement names directly: whatever `config set` writes, a
/// secret is not in it.
#[test]
fn a_secret_can_never_land_in_config_toml_through_the_write_path() {
    // The two keys that hold credentials are refused outright...
    for key in ["store.dsn", "companion.api_key"] {
        assert!(apply_setting(SAMPLE, key, PLAIN_DSN, []).is_err(), "{key}");
    }
    // ...and their reference keys take only a vault NAME, which by
    // construction cannot be a connection string: the name validator rejects
    // ':', '/' and '@'.
    for key in ["store.dsn_secret", "companion.api_key_secret"] {
        assert!(apply_setting(SAMPLE, key, PLAIN_DSN, []).is_err(), "{key}");
    }
    // Whatever else is set, the file never gains an assignment to either
    // credential field.
    for key in settable_keys() {
        let source = "[companion]\ngoogle_project = \"proj\"\n";
        let value = match key {
            "vault.provider" => "passphrase",
            "store.kind" => "memory",
            "embedder.kind" => "fixture",
            "companion.auth" => "google",
            "companion.base_url" => "https://example.test/v1",
            "embedder.dim" | "companion.context_window" | "daemon.flush_interval_ms" => "1536",
            "companion.temperature" => "0.4",
            key if key.ends_with("_secret") => "some-secret-name",
            _ => "a-value",
        };
        let edited = apply_setting(source, key, value, []).unwrap();
        assert!(!edited.contains("\ndsn = "), "{key}: {edited}");
        assert!(!edited.contains("\napi_key = "), "{key}: {edited}");
    }
}

#[test]
fn a_cosmetic_dsn_edit_is_not_a_move_but_a_different_database_is() {
    let mut resolve = |name: &str| -> Result<Zeroizing<String>, ConfigError> {
        Ok(Zeroizing::new(
            match name {
                "plain" => PLAIN_DSN,
                "same-database" => SAME_DATABASE_DSN,
                "elsewhere" => OTHER_DATABASE_DSN,
                other => panic!("unexpected secret {other}"),
            }
            .to_owned(),
        ))
    };
    let before = "[store]\nkind = \"postgres\"\ndsn_secret = \"plain\"\n";
    let cosmetic = "[store]\nkind = \"postgres\"\ndsn_secret = \"same-database\"\n";
    let moved = "[store]\nkind = \"postgres\"\ndsn_secret = \"elsewhere\"\n";
    assert!(
        !store_move_requires_confirmation(before, cosmetic, &mut resolve).unwrap(),
        "the scheme alias, host casing and an explicit :5432 are one database"
    );
    assert!(
        store_move_requires_confirmation(before, moved, &mut resolve).unwrap(),
        "a different host is a different database"
    );
}

#[test]
fn arriving_at_a_first_database_is_not_a_move() {
    // Nothing is stranded when there was nothing behind, so this must pass
    // without ceremony — or the very first `config set` teaches operators that
    // the warning is noise.
    let mut resolve = |_: &str| -> Result<Zeroizing<String>, ConfigError> {
        Ok(Zeroizing::new(PLAIN_DSN.to_owned()))
    };
    let before = "[store]\nkind = \"postgres\"\n";
    let after = "[store]\nkind = \"postgres\"\ndsn_secret = \"first\"\n";
    assert!(!store_move_requires_confirmation(before, after, &mut resolve).unwrap());
}

#[test]
fn changing_the_store_kind_is_a_move() {
    let before = "[store]\nkind = \"postgres\"\ndsn_secret = \"prod\"\n";
    let after = "[store]\nkind = \"memory\"\ndsn_secret = \"prod\"\n";
    let mut resolve = |_: &str| -> Result<Zeroizing<String>, ConfigError> {
        Ok(Zeroizing::new(PLAIN_DSN.to_owned()))
    };
    assert!(store_move_requires_confirmation(before, after, &mut resolve).unwrap());
    // An unchanged store is never a move, and never even asks the vault.
    assert!(!store_move_requires_confirmation(before, before, &mut never_resolves).unwrap());
}

#[test]
fn a_referenced_secret_that_is_not_in_the_vault_fails_closed_naming_the_name() {
    let mut resolve = |name: &str| -> Result<Zeroizing<String>, ConfigError> {
        match name {
            "present" => Ok(Zeroizing::new(PLAIN_DSN.to_owned())),
            other => Err(ConfigError::MissingStoreSecret(other.to_owned())),
        }
    };
    let before = "[store]\ndsn_secret = \"present\"\n";
    let after = "[store]\ndsn_secret = \"absent\"\n";
    let error = store_move_requires_confirmation(before, after, &mut resolve).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("absent"), "{message}");
    assert!(message.contains("mooshik secret set"), "{message}");
    assert!(!message.contains("postgres://"), "{message}");
}

#[test]
fn no_dsn_material_reaches_the_move_refusal() {
    // The refusal an operator actually sees carries no connection string, no
    // host and no credential — only what it means and what to do about it.
    let message = ConfigError::StoreMoveUnconfirmed.to_string();
    assert!(message.contains("different database"), "{message}");
    assert!(message.contains("stays"), "{message}");
    assert!(message.contains("--confirm-database-change"), "{message}");
    assert!(!message.contains("postgres://"), "{message}");
    assert!(!message.contains('@'), "{message}");
}

#[test]
fn a_two_authority_store_table_fails_the_load() {
    let error = Config::from_toml_and_env(
        &format!("[store]\ndsn = \"{PLAIN_DSN}\"\ndsn_secret = \"prod\"\n"),
        [],
    )
    .unwrap_err();
    assert!(matches!(error, ConfigError::DsnAndSecret), "{error:?}");
    assert!(!error.to_string().contains("postgres://"));
}

#[test]
fn the_editor_refuses_rather_than_corrupts_when_it_cannot_place_a_key() {
    // A quoted key spelling is not recognised as the same key, so the naive
    // edit would append a duplicate. The verification catches it and nothing
    // is written — the documented fail-closed limit, proven rather than
    // promised.
    let quoted = "[companion]\n\"model\" = \"local-model\"\n";
    let error = apply_setting(quoted, "companion.model", "other", []).unwrap_err();
    assert!(matches!(error, ConfigError::WriteVerifyFailed), "{error:?}");
    assert!(error.to_string().contains("config.toml"));
}

#[test]
fn every_settable_key_is_reachable_and_actually_lands() {
    // `companion.auth = "google"` only validates alongside a project, so the
    // base file carries one.
    let source = "[companion]\ngoogle_project = \"proj\"\n";
    for key in settable_keys() {
        let value = match key {
            "vault.provider" => "passphrase",
            "store.kind" => "memory",
            "embedder.kind" => "fixture",
            "companion.auth" => "google",
            "embedder.dim" => "1536",
            "daemon.flush_interval_ms" => "500",
            "companion.context_window" => "8192",
            "companion.temperature" => "0.4",
            "companion.base_url" => "https://example.test/v1",
            key if key.ends_with("_secret") => "some-secret-name",
            _ => "a-value",
        };
        let edited = apply_setting(source, key, value, [])
            .unwrap_or_else(|error| panic!("settable key {key} could not be set: {error}"));
        let (section, field) = key.split_once('.').unwrap();
        let table: toml::Table = toml::from_str(&edited).unwrap();
        assert!(
            table[section].as_table().unwrap().contains_key(field),
            "{key} did not land: {edited}"
        );
    }
}
