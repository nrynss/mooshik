//! The configuration *write* path: what `mooshik config set` may change, how a
//! value is validated, and how one key is edited without reformatting a file
//! the operator owns.
//!
//! Three rules hold this module together.
//!
//! 1. **The settable surface is an allowlist.** An unknown key is an error that
//!    names the key and lists what is settable — never a silent no-op, and
//!    never a `Debug` dump.
//! 2. **A secret never lands in `config.toml`.** The two credential-bearing
//!    keys (`store.dsn`, `companion.api_key`) are refused by name and point at
//!    the vault reference that takes a secret *name* instead — the same shape
//!    `[mcp_servers.*.env]` already uses. A rejected value is never echoed,
//!    because a rejected value can itself be credential material.
//! 3. **Every edit is verified before it is written.** The editor is surgical
//!    (one line rewritten, every comment and blank line preserved), so the net
//!    under it is: re-parse the result, confirm the key reads back as the value
//!    that was asked for, and refuse otherwise.

use lambo::{EmbedderKind, StoreKind};
use zeroize::Zeroizing;

use super::{Config, ConfigError};
use crate::text;

/// How a raw CLI string becomes a TOML value for one key.
#[derive(Clone, Copy)]
enum Kind {
    /// Any non-empty string.
    Text,
    /// A non-empty `http://` or `https://` URL.
    Url,
    /// The NAME of a vault secret (never a value).
    SecretName,
    /// A filesystem path, as a non-empty string.
    Path,
    PositiveU32,
    PositiveU64,
    PositiveUsize,
    /// A finite number.
    Number,
    VaultProvider,
    Store,
    Embedder,
    CompanionAuth,
}

impl Kind {
    /// The "what is valid" sentence this kind contributes to a refusal.
    fn expected(self) -> &'static str {
        match self {
            Self::Text => text::get("config.expect_text"),
            Self::Url => text::get("config.expect_url"),
            Self::SecretName => text::get("config.expect_secret_name"),
            Self::Path => text::get("config.expect_path"),
            Self::PositiveU32 | Self::PositiveU64 | Self::PositiveUsize => {
                text::get("config.expect_positive")
            }
            Self::Number => text::get("config.expect_number"),
            Self::VaultProvider => text::get("config.expect_vault_provider"),
            Self::Store => text::get("config.invalid_store_kind"),
            Self::Embedder => text::get("config.invalid_embedder"),
            Self::CompanionAuth => text::get("config.expect_companion_auth"),
        }
    }

    fn parse(self, raw: &str) -> Option<toml::Value> {
        let trimmed = raw.trim();
        match self {
            Self::Text => (!trimmed.is_empty()).then(|| toml::Value::String(trimmed.to_owned())),
            Self::Url => (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
                .then(|| toml::Value::String(trimmed.to_owned())),
            Self::SecretName => crate::vault::is_valid_name(trimmed)
                .then(|| toml::Value::String(trimmed.to_owned())),
            // Not trimmed to non-empty only: a path is taken as given (minus
            // surrounding whitespace) because the filesystem, not this module,
            // decides what is reachable.
            Self::Path => (!trimmed.is_empty()).then(|| toml::Value::String(trimmed.to_owned())),
            Self::PositiveU32 => trimmed
                .parse::<u32>()
                .ok()
                .filter(|value| *value > 0)
                .map(|value| toml::Value::Integer(i64::from(value))),
            Self::PositiveU64 => trimmed
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0)
                .and_then(|value| i64::try_from(value).ok())
                .map(toml::Value::Integer),
            Self::PositiveUsize => trimmed
                .parse::<usize>()
                .ok()
                .filter(|value| *value > 0)
                .and_then(|value| i64::try_from(value).ok())
                .map(toml::Value::Integer),
            Self::Number => trimmed
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(toml::Value::Float),
            Self::VaultProvider => matches!(trimmed, "keyring" | "passphrase")
                .then(|| toml::Value::String(trimmed.to_owned())),
            Self::Store => trimmed
                .parse::<StoreKind>()
                .ok()
                .map(|_| toml::Value::String(trimmed.to_owned())),
            Self::Embedder => trimmed
                .parse::<EmbedderKind>()
                .ok()
                .map(|_| toml::Value::String(trimmed.to_owned())),
            Self::CompanionAuth => matches!(trimmed, "static" | "google")
                .then(|| toml::Value::String(trimmed.to_owned())),
        }
    }
}

struct Setting {
    key: &'static str,
    section: &'static str,
    field: &'static str,
    kind: Kind,
}

/// Everything `mooshik config set` may change. Deliberately an allowlist: a
/// key that is not here cannot be written, however valid it looks.
const SETTABLE: &[Setting] = &[
    Setting {
        key: "vault.provider",
        section: "vault",
        field: "provider",
        kind: Kind::VaultProvider,
    },
    Setting {
        key: "session.id",
        section: "session",
        field: "id",
        kind: Kind::Text,
    },
    Setting {
        key: "session.agent",
        section: "session",
        field: "agent",
        kind: Kind::Text,
    },
    Setting {
        key: "store.kind",
        section: "store",
        field: "kind",
        kind: Kind::Store,
    },
    Setting {
        key: "store.dsn_secret",
        section: "store",
        field: "dsn_secret",
        kind: Kind::SecretName,
    },
    Setting {
        key: "embedder.kind",
        section: "embedder",
        field: "kind",
        kind: Kind::Embedder,
    },
    Setting {
        key: "embedder.dim",
        section: "embedder",
        field: "dim",
        kind: Kind::PositiveUsize,
    },
    Setting {
        key: "embedder.gemini_project",
        section: "embedder",
        field: "gemini_project",
        kind: Kind::Text,
    },
    Setting {
        key: "embedder.gemini_location",
        section: "embedder",
        field: "gemini_location",
        kind: Kind::Text,
    },
    Setting {
        key: "embedder.gemini_model",
        section: "embedder",
        field: "gemini_model",
        kind: Kind::Text,
    },
    Setting {
        key: "daemon.flush_interval_ms",
        section: "daemon",
        field: "flush_interval_ms",
        kind: Kind::PositiveU64,
    },
    Setting {
        key: "companion.base_url",
        section: "companion",
        field: "base_url",
        kind: Kind::Url,
    },
    Setting {
        key: "companion.model",
        section: "companion",
        field: "model",
        kind: Kind::Text,
    },
    Setting {
        key: "companion.api_key_secret",
        section: "companion",
        field: "api_key_secret",
        kind: Kind::SecretName,
    },
    Setting {
        key: "companion.auth",
        section: "companion",
        field: "auth",
        kind: Kind::CompanionAuth,
    },
    Setting {
        key: "companion.google_project",
        section: "companion",
        field: "google_project",
        kind: Kind::Text,
    },
    Setting {
        key: "companion.google_location",
        section: "companion",
        field: "google_location",
        kind: Kind::Text,
    },
    Setting {
        key: "companion.google_credentials",
        section: "companion",
        field: "google_credentials",
        kind: Kind::Path,
    },
    Setting {
        key: "companion.context_window",
        section: "companion",
        field: "context_window",
        kind: Kind::PositiveU32,
    },
    Setting {
        key: "companion.temperature",
        section: "companion",
        field: "temperature",
        kind: Kind::Number,
    },
];

/// Keys that hold a credential, and the reference key that takes a vault
/// secret *name* in their place. Refused by name so the write path can never
/// be the thing that undoes `config show`'s redaction and `ApiKey`'s refusal
/// to print itself.
const SECRET_KEYS: &[(&str, &str)] = &[
    ("store.dsn", "store.dsn_secret"),
    ("companion.api_key", "companion.api_key_secret"),
];

/// Every settable key, for `--help` and for the unknown-key message.
pub fn settable_keys() -> Vec<&'static str> {
    SETTABLE.iter().map(|setting| setting.key).collect()
}

/// Apply one `key = value` to `source`, returning the edited TOML.
///
/// The result is verified before it is returned: it must parse, the key must
/// read back as exactly the value asked for, and the whole file must still
/// satisfy the schema under the current environment. Anything else is
/// [`ConfigError::WriteVerifyFailed`] and nothing is written.
pub fn apply_setting(
    source: &str,
    key: &str,
    value: &str,
    environment: impl IntoIterator<Item = (String, String)>,
) -> Result<String, ConfigError> {
    if let Some((secret, reference)) = SECRET_KEYS
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(name, reference)| (*name, *reference))
    {
        return Err(ConfigError::SecretKey {
            key: secret,
            reference,
        });
    }
    let Some(setting) = SETTABLE.iter().find(|setting| setting.key == key) else {
        return Err(ConfigError::UnknownKey(key.to_owned()));
    };
    let parsed = setting
        .kind
        .parse(value)
        .ok_or(ConfigError::InvalidSetting {
            key: setting.key,
            expected: setting.kind.expected(),
        })?;
    let edited = set_in_toml(source, setting.section, setting.field, &parsed.to_string());
    let table: toml::Table = toml::from_str(&edited).map_err(|_| ConfigError::WriteVerifyFailed)?;
    let landed = table
        .get(setting.section)
        .and_then(toml::Value::as_table)
        .and_then(|section| section.get(setting.field));
    if landed != Some(&parsed) {
        return Err(ConfigError::WriteVerifyFailed);
    }
    // The schema check runs against the real environment, exactly as loading
    // does, so a file that is only complete with an env overlay is not refused
    // for an edit that had nothing to do with it.
    Config::from_toml_and_env(&edited, environment)?;
    Ok(edited)
}

/// Which database a `[store]` table names, read straight from TOML rather than
/// from a loaded `Config`: the guard is about what this *file* says, so the
/// environment must not mask a move the operator is making to the file.
#[derive(Eq, PartialEq)]
struct StoreAuthority {
    kind: String,
    dsn: Option<String>,
    dsn_secret: Option<String>,
    path: Option<String>,
}

impl StoreAuthority {
    /// Whether this table names a database at all. A `[store]` that names none
    /// has nothing behind it, so moving away from it strands nothing.
    fn names_a_database(&self) -> bool {
        self.dsn.is_some() || self.dsn_secret.is_some() || self.path.is_some()
    }
}

fn store_authority(source: &str) -> Result<StoreAuthority, ConfigError> {
    let table: toml::Table = toml::from_str(source).map_err(|_| ConfigError::InvalidToml)?;
    let store = table.get("store").and_then(toml::Value::as_table);
    let field = |name: &str| {
        store
            .and_then(|store| store.get(name))
            .and_then(toml::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    Ok(StoreAuthority {
        kind: field("kind").unwrap_or_else(|| "postgres".to_owned()),
        dsn: field("dsn"),
        dsn_secret: field("dsn_secret"),
        path: field("path"),
    })
}

impl StoreAuthority {
    /// The identity of the database this table names: the store kind plus
    /// Lambo's own DSN identity, so a password overlay or an explicit `:5432`
    /// on the same host is the same database and passes without ceremony.
    ///
    /// The DSN itself never leaves this function.
    fn identity(
        &self,
        resolve: &mut dyn FnMut(&str) -> Result<Zeroizing<String>, ConfigError>,
    ) -> Result<String, ConfigError> {
        let dsn = match (&self.dsn, &self.dsn_secret) {
            (Some(dsn), _) => Some(Zeroizing::new(dsn.clone())),
            (None, Some(name)) => Some(resolve(name)?),
            (None, None) => None,
        };
        let identity = dsn
            .map(|dsn| lambo::store_dsn_identity(&dsn))
            .unwrap_or_default();
        Ok(format!(
            "{}\u{1}{identity}\u{1}{}",
            self.kind,
            self.path.as_deref().unwrap_or_default()
        ))
    }
}

/// Whether going from `before` to `after` moves Mooshik to a different
/// database — the only case worth stopping an operator for.
///
/// Nothing is copied when the store changes and nothing is deleted: the
/// memory simply stays in the database it was written to, which looks exactly
/// like data loss from the new one. So a genuine move is refused until it is
/// confirmed, while a cosmetic edit (a password added, a default port spelled
/// out) is not a move and passes silently.
///
/// Arriving at a first database from none is not a move: there is nothing
/// behind to leave.
pub fn store_move_requires_confirmation(
    before: &str,
    after: &str,
    resolve: &mut dyn FnMut(&str) -> Result<Zeroizing<String>, ConfigError>,
) -> Result<bool, ConfigError> {
    let before = store_authority(before)?;
    let after = store_authority(after)?;
    // An untouched `[store]` table is not a move, and must not open the vault
    // to prove it: `config set companion.model ...` has to keep working on a
    // machine whose keyring is unavailable, even when a DSN secret is
    // configured.
    if before == after {
        return Ok(false);
    }
    // Arriving at a first database strands nothing, so it passes without
    // ceremony — otherwise the very first `config set` teaches operators that
    // this warning is noise.
    if !before.names_a_database() {
        return Ok(false);
    }
    Ok(before.identity(resolve)? != after.identity(resolve)?)
}

/// Set `field = rendered` inside `[section]`, preserving every other byte.
///
/// Comments, blank lines, key order and the layout of untouched tables all
/// survive, because only the one matching line is rewritten — and its own
/// trailing comment is carried across. A key that is absent is inserted
/// directly under its table header; a table that is absent is appended.
///
/// Known limits, both caught by the verification in [`apply_setting`] rather
/// than silently corrupting a file: a quoted key spelling (`"model" = ...`) is
/// not recognised as the same key, and a value spread over several lines is
/// not rewritten in place.
fn set_in_toml(source: &str, section: &str, field: &str, rendered: &str) -> String {
    let mut out = String::with_capacity(source.len() + rendered.len() + 32);
    let mut in_section = false;
    let mut replaced = false;
    let mut insert_at: Option<usize> = None;
    for line in source.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim_start();
        if trimmed.starts_with('[') {
            in_section = table_name(trimmed) == Some(section);
            out.push_str(line);
            if in_section {
                insert_at = Some(out.len());
            }
            continue;
        }
        if in_section && !replaced {
            if let Some(comment) = trailing_comment_of(body, field) {
                let indent = &body[..body.len() - trimmed.len()];
                out.push_str(indent);
                out.push_str(field);
                out.push_str(" = ");
                out.push_str(rendered);
                out.push_str(comment);
                out.push_str(&line[body.len()..]);
                replaced = true;
                continue;
            }
        }
        out.push_str(line);
    }
    if replaced {
        return out;
    }
    let entry = format!("{field} = {rendered}\n");
    match insert_at {
        Some(at) if out[..at].ends_with('\n') => out.insert_str(at, &entry),
        Some(at) => {
            out.insert_str(at, &format!("\n{entry}"));
        }
        None => {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!("\n[{section}]\n{entry}"));
        }
    }
    out
}

/// The table name a header line declares, or `None` for an array-of-tables
/// header (which never holds a settable key).
fn table_name(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('[')?;
    if rest.starts_with('[') {
        return None;
    }
    let end = rest.find(']')?;
    Some(rest[..end].trim())
}

/// If `body` assigns `field`, the trailing comment to carry across (including
/// the whitespace before it), otherwise `None`.
fn trailing_comment_of<'a>(body: &'a str, field: &str) -> Option<&'a str> {
    let trimmed = body.trim_start();
    let rest = trimmed.strip_prefix(field)?;
    let value = rest.trim_start().strip_prefix('=')?;
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut basic = false;
    let mut literal = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if basic => index += 1,
            b'"' if !literal => basic = !basic,
            b'\'' if !basic => literal = !literal,
            b'#' if !basic && !literal => {
                let mut start = index;
                while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
                    start -= 1;
                }
                return Some(&value[start..]);
            }
            _ => {}
        }
        index += 1;
    }
    Some("")
}

#[cfg(test)]
mod tests;
