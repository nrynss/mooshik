//! Prose storage and generation.
//!
//! M12a left the four prose fields of [`crate::tui::model::Workspace`] empty:
//! a day's mood, its four-words-a-line gutter summary, the trailing notes on
//! its detail pane and why a thread sits where it does. Nothing in a graph
//! writes an English sentence, so this module is the source. reflect writes
//! these as **prose concepts** in the same graph, so the next tick sees them
//! and the screens stay a pure function of the view.
//!
//! ## Schema
//!
//! A prose concept is a regular `Observation` whose canonical key is
//! `mooshik-prose:<field>:<target>` and whose content is the prose itself.
//! `field` is one of `mood`, `gutter`, `notes`, `thread_reason`. `target` is
//! the day (`YYYY-MM-DD`) for `mood`/`gutter`/`notes`, or the thread's
//! strongest concept's node id for `thread_reason`. Prose is **first-write-
//! only**: a day or thread keeps the prose its first reflect run wrote, and a
//! re-run skips it (`plan_reflect` asks the index first). The canonical key is
//! what makes the stored concept readable by the view, and the `Observation`
//! type keeps the key out of the paraphrase-dedup logic in
//! [`crate::memory::view::one_thought`] — the prose key space is its own, not
//! a paraphrase radius.
//!
//! ## What this module writes and reads
//!
//! [`ProseConcept::key`] and [`ProseConcept::parse`] are the only thing that
//! touches the storage format; everything else ([`Reflector`], the
//! [`Reflector::fixture`] default) treats prose as a struct. The view calls
//! [`read_prose_concepts`] on the graph, parses each one and surfaces the
//! four fields by `(field, target)` lookup.
//!
//! ## The Reflector trait
//!
//! The prose itself is what makes a graph a *narrated* graph, and that is not
//! something the local operator types by hand — a sentence about a day needs
//! the day's log. The trait is the seam: [`FixtureReflector`] is the default
//! and only speaks deterministically off the day/thread contents, so the
//! offline suite never depends on a model. A `CompanionReflector` calls the
//! same `OpenAI-compatible /v1` client M3 built, gated behind `#[ignore =
//! "live Vertex"]` so it never runs by default and the offline gates stay
//! green.

use chrono::NaiveDate;
use lambo::{Concept, ConceptType, NodeId};
use std::collections::HashMap;

use super::{GraphSnapshot, Reflector};

/// The four prose fields a day or thread carries. Stored in the canonical key
/// and parsed back out; nothing else in the engine sees these strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    /// A day's mood: "A rough day", "Mixed", "A good day". One per day.
    Mood,
    /// The four-words-a-line gutter summary of a day.
    Gutter,
    /// The trailing notes on a day's detail pane.
    Notes,
    /// Why a thread sits where it does.
    ThreadReason,
}

impl Field {
    /// The string form stored in the canonical key. Stable, lowercase, and
    /// what [`Self::parse`] reads back.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mood => "mood",
            Self::Gutter => "gutter",
            Self::Notes => "notes",
            Self::ThreadReason => "thread_reason",
        }
    }

    /// Parse the string form. Anything else is rejected; tests assert every
    /// round-trip.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "mood" => Some(Self::Mood),
            "gutter" => Some(Self::Gutter),
            "notes" => Some(Self::Notes),
            "thread_reason" => Some(Self::ThreadReason),
            _ => None,
        }
    }
}

/// The storage envelope for a prose concept.
///
/// A prose concept is a regular `Observation` whose key is
/// `mooshik-prose:<field>:<target>`. Three things together identify one: the
/// field, the target (a date for mood/gutter/notes, a node id for
/// thread_reason), and the prose itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProseConcept {
    pub field: Field,
    pub target: Target,
    pub text: String,
}

/// What a prose concept is about. Day-keyed for mood/gutter/notes;
/// node-keyed for thread_reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    Day(NaiveDate),
    Thread(NodeId),
}

impl Target {
    /// The string form stored in the canonical key.
    pub fn as_str(self) -> String {
        match self {
            Self::Day(date) => date.format("%Y-%m-%d").to_string(),
            Self::Thread(id) => id.0.to_string(),
        }
    }

    /// Parse the string form. Anything that is not a date and not a UUID is
    /// rejected; tests assert every round-trip.
    pub fn parse(text: &str) -> Option<Self> {
        if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
            return Some(Self::Day(date));
        }
        if let Ok(uuid) = text.parse::<uuid::Uuid>() {
            return Some(Self::Thread(NodeId(uuid)));
        }
        None
    }
}

impl ProseConcept {
    /// The prefix every prose canonical key starts with.
    pub const PREFIX: &'static str = "mooshik-prose";

    /// The canonical key Lambo writes to disk for this prose concept. Round-
    /// trips through [`Self::from_key`].
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}",
            Self::PREFIX,
            self.field.as_str(),
            self.target.as_str()
        )
    }

    /// Build a [`ProseConcept`] from a canonical key and content, or `None` if
    /// the key is not a prose key, or the field/target string is unknown.
    pub fn from_key(canonical_key: &str, content: &str) -> Option<Self> {
        let rest = canonical_key
            .strip_prefix(Self::PREFIX)?
            .strip_prefix(':')?;
        let (field_str, target_str) = rest.split_once(':')?;
        Some(Self {
            field: Field::parse(field_str)?,
            target: Target::parse(target_str)?,
            text: content.to_owned(),
        })
    }
    /// [`Self::PREFIX`]. The view uses this to walk only the prose concepts,
    /// not the full concept set.
    pub fn is_prose(concept: &Concept) -> bool {
        concept.canonical_key.starts_with(Self::PREFIX)
    }
    /// What a derived concept looks like on the way to disk. The content is
    /// the prose text and the canonical key is the structured one.
    pub fn as_concept(
        &self,
        origin: NodeId,
        session_id: lambo::SessionId,
        agent: lambo::AgentId,
    ) -> Concept {
        Concept {
            id: NodeId::new(),
            session_id,
            content: self.text.clone(),
            canonical_key: self.key(),
            concept_type: ConceptType::Observation,
            origin_interaction: origin,
            origin_agent: agent,
            created_at: chrono::Utc::now(),
            access_count: 0,
            last_accessed: None,
            gc_survived: 0,
            human_confirmed: 0,
            canonization_status: lambo::CanonizationStatus::None,
            blast_radius: None,
            last_demotion_time: None,
            embedding: None,
            chunk_group_id: None,
        }
    }
}

/// Every prose concept in a graph, indexed by `(field, target)` so the view
/// can look up the prose for one day or one thread in one step.
#[derive(Debug, Default, Clone)]
pub struct ProseIndex {
    by_key: HashMap<(Field, Target), String>,
}
impl ProseIndex {
    /// Read every prose concept out of a slice of concepts — the read side
    /// the view's tick uses, which holds the concepts itself rather than a
    /// full [`GraphSnapshot`]. `from_snapshot` delegates here so the two
    /// share exactly one parse rule.
    pub fn from_concepts(concepts: &[Concept]) -> Self {
        let mut by_key = HashMap::new();
        for concept in concepts {
            if !ProseConcept::is_prose(concept) {
                continue;
            }
            if let Some(prose) = ProseConcept::from_key(&concept.canonical_key, &concept.content) {
                by_key.insert((prose.field, prose.target), prose.text);
            }
        }
        Self { by_key }
    }

    /// Read every prose concept out of a graph snapshot.
    pub fn from_snapshot(snapshot: &GraphSnapshot) -> Self {
        Self::from_concepts(&snapshot.concepts)
    }

    /// Look up the prose for one `(field, target)`, if any was written.
    pub fn get(&self, field: Field, target: Target) -> Option<&str> {
        self.by_key.get(&(field, target)).map(String::as_str)
    }

    /// The number of prose concepts in this index.
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// Whether the index is empty — a graph that has never been reflected on.
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Iterate every entry — for tests and for the dry-run report.
    pub fn iter(&self) -> impl Iterator<Item = ((Field, Target), &str)> {
        self.by_key.iter().map(|(k, v)| (*k, v.as_str()))
    }
}

/// The fixture `Reflector`: deterministic prose derived only from the inputs
/// the snapshot hands it, so the offline suite never depends on a model.
///
/// The tone of the output is the seam where a `CompanionReflector` would write
/// differently; this one uses rules of thumb: a day's mood is set by how many
/// turns the day carries (none = quiet, many = busy, a hard day is one with
/// a `ConceptType::Constraint` thought); the gutter summary is the four
/// strongest concepts in four-word windows; the trailing notes are short
/// observations that quote the day's counts; a thread's reason names what it
/// came back to and how often.
#[derive(Debug, Default, Clone, Copy)]
pub struct FixtureReflector;

impl Reflector for FixtureReflector {
    fn day_mood(&self, snapshot: &GraphSnapshot, day: NaiveDate) -> String {
        let turns = snapshot.turns_on(day);
        let mut constraints = 0usize;
        let mut entities = 0usize;
        for concept in &snapshot.concepts_on(day) {
            match concept.concept_type {
                ConceptType::Constraint => constraints += 1,
                ConceptType::Entity => entities += 1,
                _ => {}
            }
        }
        match (turns, constraints) {
            (0, _) => "A quiet day".to_owned(),
            (_, c) if c >= 1 => "A hard day".to_owned(),
            (t, _) if t >= 6 => "A busy day".to_owned(),
            _ if entities == 0 => "Mixed".to_owned(),
            _ => "An ordinary day".to_owned(),
        }
    }

    fn day_gutter(&self, snapshot: &GraphSnapshot, day: NaiveDate) -> Vec<String> {
        // The four strongest entities of the day (by length), each shrunk to
        // a four-word window — the 17-column pane reads the day's own words,
        // never a truncation of the log, and a day with nothing on record
        // keeps an honest empty gutter.
        let mut entities = snapshot.concept_contents_on(day, ConceptType::Entity);
        entities.sort_by_key(|content| std::cmp::Reverse(content.len()));
        entities.truncate(4);
        entities
            .into_iter()
            .map(|content| four_word_summary(&content))
            .collect()
    }

    fn day_notes(&self, snapshot: &GraphSnapshot, day: NaiveDate) -> String {
        let turns = snapshot.turns_on(day);
        let concepts = snapshot.concepts_on(day);
        if turns == 0 && concepts.is_empty() {
            return String::new();
        }
        let concept_count = concepts.len();
        let mut lines = Vec::new();
        lines.push(format!(
            "You wrote {turns} turn{turns_pl} and noted {concept_count} thing{concept_count_pl} on this day.",
            turns = turns,
            turns_pl = if turns == 1 { "" } else { "s" },
            concept_count = concept_count,
            concept_count_pl = if concept_count == 1 { "" } else { "s" },
        ));
        let mut constraints = 0usize;
        for c in &concepts {
            if c.concept_type == ConceptType::Constraint {
                constraints += 1;
            }
        }
        if constraints > 0 {
            lines.push(format!(
                "{n} constraint{n_pl} held on this day.",
                n = constraints,
                n_pl = if constraints == 1 { "" } else { "s" },
            ));
        }
        lines.join("\n\n")
    }

    fn thread_reason(&self, snapshot: &GraphSnapshot, anchor: NodeId) -> String {
        let derived = snapshot.derived_interactions(anchor);
        let count = derived.len();
        let days: std::collections::BTreeSet<NaiveDate> = derived
            .iter()
            .filter_map(|turn| snapshot.day_of_interaction(turn.id))
            .collect();
        let day_count = days.len();
        let content = snapshot.content_of(anchor).unwrap_or("a thought");
        if count == 0 {
            format!("Reached once — {content}.")
        } else {
            format!(
                "You came back to {content} {count} time{count_pl}, across {day_count} day{day_count_pl}.",
                content = content,
                count = count,
                count_pl = if count == 1 { "" } else { "s" },
                day_count = day_count,
                day_count_pl = if day_count == 1 { "" } else { "s" },
            )
        }
    }
}

/// Take a piece of prose and shrink it to four words, hyphenated or not. The
/// gutter is read in a 17-column panel, and the artboard's own example reads
/// "Incident / 09:42-11:40 / Drinks off / Mum called mid-incident" — four
/// words, in the day's own voice, never a truncation of the log. A phrase
/// with more than four words drops the smallest ones from the middle.
// Called by [`FixtureReflector::day_gutter`], which `plan_reflect` reaches
// through the `Reflector` trait object — one production caller, so the
// offline suite exercises the same path the pane renders.
fn four_word_summary(text: &str) -> String {
    let trimmed = text.trim();
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() <= 4 {
        return trimmed.to_owned();
    }
    // Keep the first and last two words — the lead and the punch.
    let mut out = Vec::with_capacity(4);
    out.push(words[0]);
    if words.len() >= 2 {
        out.push(words[1]);
    }
    let n = words.len();
    out.push(words[n - 2]);
    out.push(words[n - 1]);
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_round_trips() {
        for field in [
            Field::Mood,
            Field::Gutter,
            Field::Notes,
            Field::ThreadReason,
        ] {
            assert_eq!(Field::parse(field.as_str()), Some(field));
        }
        assert_eq!(Field::parse("not-a-field"), None);
    }

    #[test]
    fn target_round_trips_for_both_shapes() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 27).unwrap();
        let day = Target::Day(date);
        assert_eq!(Target::parse(&day.as_str()), Some(day));
        let id = NodeId::new();
        let thread = Target::Thread(id);
        assert_eq!(Target::parse(&thread.as_str()), Some(thread));
        assert_eq!(Target::parse("not-a-target"), None);
    }

    #[test]
    fn prose_concept_key_round_trips() {
        let prose = ProseConcept {
            field: Field::Mood,
            target: Target::Day(NaiveDate::from_ymd_opt(2026, 8, 27).unwrap()),
            text: "A hard day".to_owned(),
        };
        let key = prose.key();
        assert!(key.starts_with("mooshik-prose:"));
        let parsed = ProseConcept::from_key(&key, &prose.text).unwrap();
        assert_eq!(parsed, prose);
    }

    #[test]
    fn prose_concept_rejects_a_non_prose_key() {
        assert!(ProseConcept::from_key("entity:windpipe", "anything").is_none());
    }

    #[test]
    fn four_word_summary_short_text_is_unchanged() {
        assert_eq!(four_word_summary("Incident"), "Incident");
        assert_eq!(four_word_summary("Drinks off"), "Drinks off");
    }

    #[test]
    fn four_word_summary_drops_the_middle() {
        let out = four_word_summary("a hard day with the windpipe ring blocking");
        assert_eq!(out, "a hard ring blocking");
    }
    /// A snapshot whose one interaction (`now`) carries `contents` as
    /// entities — the day those all land on is today.
    fn snapshot_with_entities(contents: &[&str]) -> GraphSnapshot {
        let mut graph = lambo::Graph::new(lambo::SessionId::new("mooshik"));
        let origin = NodeId::new();
        let at = chrono::Utc::now();
        graph
            .insert_interaction(lambo::Interaction {
                id: origin,
                session_id: lambo::SessionId::new("mooshik"),
                agent_id: lambo::AgentId::new("mooshik"),
                prompt_text: None,
                previous_id: None,
                created_at: at,
                event_time: None,
            })
            .unwrap();
        for (index, content) in contents.iter().enumerate() {
            graph
                .insert_concept(
                    Concept {
                        id: NodeId::new(),
                        session_id: lambo::SessionId::new("mooshik"),
                        content: (*content).to_owned(),
                        canonical_key: format!("entity:{index}"),
                        concept_type: ConceptType::Entity,
                        origin_interaction: origin,
                        origin_agent: lambo::AgentId::new("mooshik"),
                        created_at: at,
                        access_count: 0,
                        last_accessed: None,
                        gc_survived: 0,
                        human_confirmed: 0,
                        canonization_status: lambo::CanonizationStatus::None,
                        blast_radius: None,
                        last_demotion_time: None,
                        embedding: None,
                        chunk_group_id: None,
                    },
                    origin,
                )
                .unwrap();
        }
        GraphSnapshot::from_graph(&graph)
    }

    #[test]
    fn day_gutter_is_four_word_lines_from_the_top_four_entities() {
        let today = chrono::Utc::now().date_naive();
        let snapshot = snapshot_with_entities(&[
            "a very long entity that goes on and on about the artboard layout",
            "the ring holds five hundred and twelve copies",
            "mum called mid-incident",
            "drinks off",
            "windpipe",
        ]);
        let gutter = FixtureReflector.day_gutter(&snapshot, today);
        // The stub this replaces printed "Nothing on record" on days with
        // turns — a false statement on the pane.
        assert_ne!(
            gutter,
            vec!["Nothing on record".to_owned()],
            "a day with entities must not be reported as having nothing on record"
        );
        // The four longest entities, each shrunk to a four-word window;
        // the fifth-longest ("windpipe") is beyond the top four.
        assert_eq!(
            gutter,
            vec![
                "a very artboard layout",
                "the ring twelve copies",
                "mum called mid-incident",
                "drinks off",
            ]
        );
    }

    #[test]
    fn day_gutter_stays_empty_on_a_day_with_no_entities() {
        let today = chrono::Utc::now().date_naive();
        // A day with a turn but no entity: nothing to summarize, so the
        // gutter is honestly empty rather than a "Nothing on record" claim.
        let snapshot = snapshot_with_entities(&[]);
        assert_eq!(
            FixtureReflector.day_gutter(&snapshot, today),
            Vec::<String>::new()
        );
    }
}
