//! The `[permissions]` grant model: *autonomy is granted, not configured*
//! (docs/SPEC.md). This module owns the grammar, validation, and resolution of
//! the grant table; enforcement lives at the tool-call boundary
//! ([`crate::tools::GatedTools`]).
//!
//! Grammar:
//!
//! ```toml
//! [permissions]
//! memory             = ["recall", "derive"]  # allow-list of family members
//! scratch            = "prompt"              # family-wide mode
//! run_scratch_script = "allow"               # per-tool entry, beats the family
//! "mcp.github.*"     = "allow"               # prefix rule, ready for M10 servers
//! web                = "deny"                # unknown scope: parses, enforces deny
//! ```
//!
//! Resolution for a tool name, most specific first: an explicit per-tool
//! entry, then an explicit name entry, then the longest `*` prefix rule, then
//! the family table, then deny. Defaults (Decision 6): the three lambo memory
//! tools are granted, `run_scratch_script` prompts, everything else is denied.
//!
//! **The graph is never a permission authority.** Nothing here reads
//! configuration other than this table — no concept, however canonical, can
//! widen a grant (pinned by a test below).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::ConfigError;
use crate::text;

/// Full member tool names of the `memory` family, as the executor dispatches
/// them. Pinned against [`crate::tools`]' constants by a test there.
const MEMORY_TOOLS: &[&str] = &["lambo_recall", "lambo_derive", "lambo_stats"];

/// Full member tool names of the `scratch` family.
const SCRATCH_TOOLS: &[&str] = &["run_scratch_script"];

fn family_members(family: &str) -> Option<&'static [&'static str]> {
    match family {
        "memory" => Some(MEMORY_TOOLS),
        "scratch" => Some(SCRATCH_TOOLS),
        _ => None,
    }
}

fn is_known_tool(name: &str) -> bool {
    MEMORY_TOOLS.contains(&name) || SCRATCH_TOOLS.contains(&name)
}

/// Resolve one allow-list entry under `family` to a member's full tool name.
/// Both the spec's short spelling (`recall`, `derive`) and the full tool name
/// are accepted; anything else is a config error (fail closed).
fn family_alias(family: &str, entry: &str) -> Option<&'static str> {
    let members = family_members(family)?;
    if let Some(full) = members.iter().copied().find(|member| *member == entry) {
        return Some(full);
    }
    let prefixed = format!("lambo_{entry}");
    if let Some(full) = members.iter().copied().find(|member| *member == prefixed) {
        return Some(full);
    }
    match (family, entry) {
        ("scratch", "script") => Some("run_scratch_script"),
        _ => None,
    }
}

/// What a grant permits. `Prompt` defers to the user at call time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrantMode {
    Allow,
    Prompt,
    Deny,
}

impl GrantMode {
    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "allow" => Some(Self::Allow),
            "prompt" => Some(Self::Prompt),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }

    /// The mode as printed by `mooshik permissions`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => text::get("permissions.mode_allow"),
            Self::Prompt => text::get("permissions.mode_prompt"),
            Self::Deny => text::get("permissions.mode_deny"),
        }
    }
}

/// Where an effective decision came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrantSource {
    /// The built-in Decision 6 defaults.
    Default,
    /// The `[permissions]` table in the loaded configuration.
    Config,
}

impl GrantSource {
    /// The source as printed by `mooshik permissions`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => text::get("permissions.source_default"),
            Self::Config => text::get("permissions.source_config"),
        }
    }
}

/// One effective decision: what is permitted and where it came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GrantDecision {
    pub mode: GrantMode,
    pub source: GrantSource,
}

const DENIED_BY_DEFAULT: GrantDecision = GrantDecision {
    mode: GrantMode::Deny,
    source: GrantSource::Default,
};

/// A configured value before it is bound to tools: a mode for the keyed scope,
/// or an allow-list of names.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RawGrant {
    Mode(String),
    Tools(Vec<String>),
}

/// What a scoped (non-family, non-tool) key means after validation. A
/// `*`-suffixed key is a prefix rule; a plain unknown name may match a future
/// tool verbatim.
#[derive(Clone, Debug, PartialEq)]
pub enum ScopedGrant {
    Mode(GrantMode),
    Tools(Vec<String>),
}

/// The `[permissions]` table as written: scope key → grant. Every key is data
/// (families, tool names, `*` prefixes, unknown future scopes), so the table is
/// a map rather than a fixed struct; unknown keys parse and enforce as deny
/// until a tool matches them.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct PermissionsConfig {
    pub entries: BTreeMap<String, RawGrant>,
}

impl PermissionsConfig {
    /// Fail-closed validation. Anything this cannot interpret — a bad mode
    /// string, an empty list entry, an allow-list naming nothing in the family,
    /// a list where only a mode makes sense — fails config load.
    pub fn validate(&self) -> Result<(), ConfigError> {
        for (key, grant) in &self.entries {
            match grant {
                RawGrant::Mode(mode) => {
                    if GrantMode::parse(mode).is_none() {
                        return Err(ConfigError::InvalidPermissions);
                    }
                }
                RawGrant::Tools(list) => {
                    if key.ends_with('*') || is_known_tool(key) {
                        return Err(ConfigError::InvalidPermissions);
                    }
                    for entry in list {
                        let entry = entry.trim();
                        if entry.is_empty() {
                            return Err(ConfigError::InvalidPermissions);
                        }
                        if family_members(key).is_some() && family_alias(key, entry).is_none() {
                            return Err(ConfigError::InvalidPermissions);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Resolve the table against the Decision 6 defaults. Assumes
    /// [`Self::validate`] ran at load; anything unrecognized resolves as deny,
    /// never as a wider grant.
    pub fn grants(&self) -> Grants {
        let mut grants = Grants::default();
        for tool in MEMORY_TOOLS {
            grants.family.insert(
                (*tool).to_owned(),
                GrantDecision {
                    mode: GrantMode::Allow,
                    source: GrantSource::Default,
                },
            );
        }
        for tool in SCRATCH_TOOLS {
            grants.family.insert(
                (*tool).to_owned(),
                GrantDecision {
                    mode: GrantMode::Prompt,
                    source: GrantSource::Default,
                },
            );
        }
        for (key, grant) in &self.entries {
            if key.ends_with('*') {
                if let RawGrant::Mode(mode) = grant {
                    if let Some(mode) = GrantMode::parse(mode) {
                        grants.scoped.insert(key.clone(), ScopedGrant::Mode(mode));
                    }
                }
                continue;
            }
            if let Some(members) = family_members(key) {
                match grant {
                    RawGrant::Mode(mode) => {
                        if let Some(mode) = GrantMode::parse(mode) {
                            for member in members {
                                grants.family.insert(
                                    (*member).to_owned(),
                                    GrantDecision {
                                        mode,
                                        source: GrantSource::Config,
                                    },
                                );
                            }
                        }
                    }
                    RawGrant::Tools(list) => {
                        // An explicit allow-list defines the whole family:
                        // listed members are granted, the rest are denied.
                        let allowed: BTreeSet<&str> = list
                            .iter()
                            .filter_map(|entry| family_alias(key, entry.trim()))
                            .collect();
                        for member in members {
                            let mode = if allowed.contains(member) {
                                GrantMode::Allow
                            } else {
                                GrantMode::Deny
                            };
                            grants.family.insert(
                                (*member).to_owned(),
                                GrantDecision {
                                    mode,
                                    source: GrantSource::Config,
                                },
                            );
                        }
                    }
                }
                continue;
            }
            if is_known_tool(key) {
                if let RawGrant::Mode(mode) = grant {
                    if let Some(mode) = GrantMode::parse(mode) {
                        grants.exact.insert(
                            key.clone(),
                            GrantDecision {
                                mode,
                                source: GrantSource::Config,
                            },
                        );
                    }
                }
                continue;
            }
            // Unknown scope: parsed, shown by `mooshik permissions`, and inert
            // until a tool matches it — which today means it enforces as deny.
            match grant {
                RawGrant::Mode(mode) => {
                    if let Some(mode) = GrantMode::parse(mode) {
                        grants.scoped.insert(key.clone(), ScopedGrant::Mode(mode));
                    }
                }
                RawGrant::Tools(list) => {
                    grants.scoped.insert(
                        key.clone(),
                        ScopedGrant::Tools(list.iter().map(|e| e.trim().to_owned()).collect()),
                    );
                }
            }
        }
        grants
    }
}

/// The resolved grant set the gate enforces. Built once at composition time;
/// lookups are pure functions of this table and nothing else.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grants {
    /// Explicit per-tool entries; beat everything below.
    exact: BTreeMap<String, GrantDecision>,
    /// Effective decision for every known tool after its family's entry (if
    /// any) was applied. Seeded with the Decision 6 defaults.
    family: BTreeMap<String, GrantDecision>,
    /// Prefix rules (`key*`) and unknown scopes, in deterministic order.
    scoped: BTreeMap<String, ScopedGrant>,
}

impl Grants {
    /// The effective decision for `tool`. Order, most specific first: explicit
    /// tool entry → explicit name entry → longest `*` prefix → family → deny.
    /// The graph is never consulted.
    pub fn decision_for(&self, tool: &str) -> GrantDecision {
        if let Some(decision) = self.exact.get(tool) {
            return *decision;
        }
        if let Some(ScopedGrant::Mode(mode)) = self.scoped.get(tool) {
            return GrantDecision {
                mode: *mode,
                source: GrantSource::Config,
            };
        }
        let mut best: Option<(&str, GrantDecision)> = None;
        for (pattern, grant) in &self.scoped {
            let Some(prefix) = pattern.strip_suffix('*') else {
                continue;
            };
            let ScopedGrant::Mode(mode) = grant else {
                continue;
            };
            if tool.starts_with(prefix) && best.is_none_or(|(seen, _)| prefix.len() > seen.len()) {
                best = Some((
                    prefix,
                    GrantDecision {
                        mode: *mode,
                        source: GrantSource::Config,
                    },
                ));
            }
        }
        if let Some((_, decision)) = best {
            return decision;
        }
        self.family.get(tool).copied().unwrap_or(DENIED_BY_DEFAULT)
    }

    /// Whether the model should see the tool at all: granted outright, or
    /// granted pending a prompt. Ungranted tools are neither advertised nor
    /// callable.
    pub fn advertised(&self, tool: &str) -> bool {
        matches!(
            self.decision_for(tool).mode,
            GrantMode::Allow | GrantMode::Prompt
        )
    }

    /// Deterministic rendering for `mooshik permissions`: each known family
    /// with its members' effective mode and source, then any configured scopes
    /// that match no current tool.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(text::get("permissions.resolved_header"));
        out.push('\n');
        for family in ["memory", "scratch"] {
            out.push_str(family);
            out.push('\n');
            for member in family_members(family).unwrap_or_default() {
                let decision = self
                    .family
                    .get(*member)
                    .copied()
                    .unwrap_or(DENIED_BY_DEFAULT);
                out.push_str(&format!(
                    "  {} {} ({})\n",
                    member,
                    decision.mode.label(),
                    decision.source.label()
                ));
            }
        }
        if !self.scoped.is_empty() {
            out.push_str(text::get("permissions.unmatched_header"));
            out.push('\n');
            for (pattern, grant) in &self.scoped {
                match grant {
                    ScopedGrant::Mode(mode) => out.push_str(&format!(
                        "  {} {} ({})\n",
                        pattern,
                        mode.label(),
                        GrantSource::Config.label()
                    )),
                    ScopedGrant::Tools(names) => out.push_str(&format!(
                        "  {} {} [{}] ({})\n",
                        pattern,
                        text::get("permissions.mode_allow"),
                        names.join(", "),
                        GrantSource::Config.label()
                    )),
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    const RECALL: &str = "lambo_recall";
    const DERIVE: &str = "lambo_derive";
    const STATS: &str = "lambo_stats";
    const SCRATCH: &str = "run_scratch_script";

    fn grants_from(table: &str) -> Grants {
        Config::from_toml_and_env(&format!("[permissions]\n{table}\n"), [])
            .unwrap()
            .permissions
            .grants()
    }

    fn modes(grants: &Grants) -> [(String, GrantMode, GrantSource); 4] {
        [RECALL, DERIVE, STATS, SCRATCH].map(|tool| {
            let decision = grants.decision_for(tool);
            (tool.to_owned(), decision.mode, decision.source)
        })
    }

    #[test]
    fn decision6_defaults_are_exactly_the_settled_grant_set() {
        let grants = Config::default().permissions.grants();
        assert_eq!(
            modes(&grants),
            [
                (RECALL.to_owned(), GrantMode::Allow, GrantSource::Default),
                (DERIVE.to_owned(), GrantMode::Allow, GrantSource::Default),
                (STATS.to_owned(), GrantMode::Allow, GrantSource::Default),
                (SCRATCH.to_owned(), GrantMode::Prompt, GrantSource::Default),
            ]
        );
        // Everything else is denied by default, from nowhere.
        assert_eq!(grants.decision_for("web_fetch"), DENIED_BY_DEFAULT);
        assert_eq!(grants.decision_for("fs_read"), DENIED_BY_DEFAULT);
    }

    #[test]
    fn family_mode_covers_every_member() {
        let grants = grants_from("memory = 'deny'");
        assert_eq!(
            modes(&grants),
            [
                (RECALL.to_owned(), GrantMode::Deny, GrantSource::Config),
                (DERIVE.to_owned(), GrantMode::Deny, GrantSource::Config),
                (STATS.to_owned(), GrantMode::Deny, GrantSource::Config),
                (SCRATCH.to_owned(), GrantMode::Prompt, GrantSource::Default),
            ]
        );
    }

    #[test]
    fn per_tool_entry_beats_the_family_mode() {
        let grants = grants_from("memory = 'deny'\nlambo_recall = 'allow'");
        assert_eq!(grants.decision_for(RECALL).mode, GrantMode::Allow);
        assert_eq!(grants.decision_for(RECALL).source, GrantSource::Config);
        assert_eq!(grants.decision_for(DERIVE).mode, GrantMode::Deny);
        assert_eq!(grants.decision_for(STATS).mode, GrantMode::Deny);
    }

    #[test]
    fn allow_list_narrows_to_the_listed_members() {
        // An explicit allow-list defines the whole family: unlisted members
        // fall to deny, not back to their defaults.
        let grants = grants_from("memory = ['recall']");
        assert_eq!(grants.decision_for(RECALL).mode, GrantMode::Allow);
        assert_eq!(grants.decision_for(DERIVE).mode, GrantMode::Deny);
        assert_eq!(grants.decision_for(STATS).mode, GrantMode::Deny);

        let spelled = grants_from("memory = ['lambo_derive', 'stats']");
        assert_eq!(spelled.decision_for(DERIVE).mode, GrantMode::Allow);
        assert_eq!(spelled.decision_for(STATS).mode, GrantMode::Allow);
        assert_eq!(spelled.decision_for(RECALL).mode, GrantMode::Deny);
    }

    #[test]
    fn unknown_scopes_parse_round_trip_and_enforce_deny() {
        let source = "[permissions]\nweb = 'deny'\nfilesystem_read = ['~/work']\n";
        let config = Config::from_toml_and_env(source, []).unwrap();
        // Round-trips through `config show`.
        let shown = config.redacted_toml();
        assert!(shown.contains("web"), "{shown}");
        assert!(shown.contains("filesystem_read"), "{shown}");
        assert!(shown.contains("~/work"), "{shown}");
        // And enforces as deny, because no tool matches them.
        let grants = config.permissions.grants();
        assert_eq!(grants.decision_for("web_search"), DENIED_BY_DEFAULT);
        assert_eq!(grants.decision_for("fs_read"), DENIED_BY_DEFAULT);
        assert!(!grants.advertised("anything_at_all"));
    }

    #[test]
    fn prefix_rule_matches_future_tool_names_by_longest_prefix() {
        let grants = grants_from("'mcp.github.*' = 'allow'\n'mcp.*' = 'deny'");
        assert_eq!(
            grants.decision_for("mcp.github.create_issue").mode,
            GrantMode::Allow
        );
        assert_eq!(
            grants.decision_for("mcp.other.search").mode,
            GrantMode::Deny
        );
        // A plain unknown name may also name a future tool verbatim. (TOML
        // treats bare dots as nested-table syntax, so namespaced tool names
        // are quoted — the same spelling M10's servers will use.)
        let exact = grants_from("'mcp.github.create_issue' = 'prompt'");
        assert_eq!(
            exact.decision_for("mcp.github.create_issue").mode,
            GrantMode::Prompt
        );
    }

    #[test]
    fn malformed_permissions_tables_fail_closed() {
        use crate::config::ConfigError;
        // Wrong value type: not interpretable at all.
        assert!(matches!(
            Config::from_toml_and_env("[permissions]\nmemory = 5\n", []),
            Err(ConfigError::InvalidToml)
        ));
        // Bad mode string.
        for table in [
            "memory = 'banana'",
            "scratch = ''",
            "'mcp.github.*' = 'ALLOW ALL'",
        ] {
            assert!(matches!(
                Config::from_toml_and_env(&format!("[permissions]\n{table}\n"), []),
                Err(ConfigError::InvalidPermissions)
            ));
        }
        // Allow-lists that name nothing real, or sit where only a mode fits.
        for table in [
            "memory = ['bogus']",
            "scratch = ['recall']",
            "memory = ['']",
            "lambo_stats = ['x']",
            "'mcp.github.*' = ['create_issue']",
        ] {
            assert!(matches!(
                Config::from_toml_and_env(&format!("[permissions]\n{table}\n"), []),
                Err(ConfigError::InvalidPermissions)
            ));
        }
        // The failure is a clean config error, not a panic, and names no values.
        let error = Config::from_toml_and_env("[permissions]\nmemory = 'banana'\n", [])
            .unwrap_err()
            .to_string();
        assert!(error.contains("[permissions]"), "{error}");
    }

    #[test]
    fn render_prints_each_known_family_then_unmatched_scopes_deterministically() {
        let grants = grants_from("web = 'deny'\n'mcp.github.*' = 'allow'\n");
        let rendered = grants.render();
        let resolved = rendered
            .find(text::get("permissions.resolved_header"))
            .unwrap();
        let memory = rendered.find("memory\n").unwrap();
        let recall = rendered.find("  lambo_recall allow (default)\n").unwrap();
        let derive = rendered.find("  lambo_derive allow (default)\n").unwrap();
        let stats = rendered.find("  lambo_stats allow (default)\n").unwrap();
        let scratch = rendered.find("scratch\n").unwrap();
        let prompt = rendered
            .find("  run_scratch_script prompt (default)\n")
            .unwrap();
        let unmatched = rendered
            .find(text::get("permissions.unmatched_header"))
            .unwrap();
        let web = rendered.find("  web deny (config)\n").unwrap();
        let prefix = rendered.find("  mcp.github.* allow (config)\n").unwrap();
        // Deterministic order: header, families in fixed order, then scopes sorted.
        let ordered = [
            resolved, memory, recall, derive, stats, scratch, prompt, unmatched, prefix, web,
        ];
        assert!(ordered.windows(2).all(|w| w[0] < w[1]), "{rendered}");

        let quiet = Config::default().permissions.grants().render();
        assert!(
            !quiet.contains(text::get("permissions.unmatched_header")),
            "{quiet}"
        );
    }

    #[test]
    fn the_grant_model_is_graph_independent() {
        // Same pin technique as the M3 seams: the permission model reads
        // configuration only. The graph is never a permission authority.
        let production = include_str!("permissions.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            !production.contains("crate::memory"),
            "the grant model must never reference the graph"
        );
    }
}
