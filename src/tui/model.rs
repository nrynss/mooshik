//! What the screens render. One plain-data tree, no behaviour.
//!
//! The screens are a pure function of this type: given a [`Workspace`] and an
//! [`Area`](ratatui::layout::Rect) they draw the artboard, and they read nothing
//! else. That is what makes the layout testable against the design without a
//! database, a model endpoint or a clock behind it — see `screen::tests`.
//!
//! Two shapes here are deliberate and worth defending, because both look like
//! omissions:
//!
//! **Dates and times are display strings, not timestamps.** The design needs
//! "Thursday 27 August", "14:22", "Fri", "21-27 August" — six different
//! renderings of the same instant. Carrying a real date type would mean a
//! calendar dependency inside the render layer and a formatting decision at
//! every call site; carrying the strings means whoever fills the model formats
//! once, and the screens cannot accidentally show a different date on two
//! panels. It also keeps the model trivially constructible in a test.
//!
//! **Prose is unwrapped.** A day's entries render in a 15-column week gutter,
//! the 46-column Today panel and the 44-column week detail pane; a thread's
//! summary renders at roughly 30 columns on one screen and 40 on another.
//! Pre-wrapped lines would mean one `Day` per width, so the model carries
//! sentences and the screens wrap them through [`wrap`](super::wrap::wrap).
//!
//! **Strength is a list position, never a number in the data.** Artboard `1i`'s
//! "Never on screen" rule is explicit — no tier names, no scores, no
//! percentages — and the list is "always ordered by how often you return to
//! something, [so] there is no sort control, because there is nothing to sort
//! by". So [`Workspace::threads`] and [`Workspace::trickle`] arrive in order and
//! the renderer takes brightness from the index via
//! [`Strength::from_rank`](super::theme::Strength::from_rank). There is nowhere
//! to put a score, which is the point.
//!
//! Nothing from the engine underneath reaches this type either — no sessions,
//! no nodes, no relevance, no promotion or decay. A thread's strength is only
//! ever justified with the user's own history, which is why
//! [`Justification`] holds prose rather than a figure.
//!
//! **Every type here refuses unknown fields.** `demo*.toml` is the fidelity
//! spec — it is what the layout tests assert the artboards against — and with
//! `#[serde(default)]` alone a mistyped key in it was silently dropped and
//! `demo_toml_parses` still passed. A `weathr` on a week day would have left
//! that column with no weather and nothing to say so. So the fixture-facing
//! types are strict about names and permissive about absence, which is the pair
//! that makes a fixture checkable.

use serde::Deserialize;

/// How loud a piece of text is allowed to be. Three tones, because the design
/// spends colour on meaning rather than emphasis: a hard day and a caution
/// share one tone because they are the same statement about the day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    /// Ordinary. Body text.
    #[default]
    Plain,
    /// Worth noticing — the strongest thing on screen right now.
    Notable,
    /// A caution worth hearing, or a day that was hard. Twice a week at most.
    Hard,
}

/// Who is speaking. The distinction earns two different colours: the user's own
/// words are the brightest thing in the conversation, and Mooshik's are the
/// accent, so a glance separates "what I said" from "what it said back".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Speaker {
    /// The person using Mooshik.
    Person,
    /// Mooshik itself.
    Mooshik,
}

/// The clock in the title bar: "Mooshik · Thursday 27 August · 14:22".
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Stamp {
    /// "Thursday 27 August" on a wide terminal.
    pub long_date: String,
    /// "Thu 27 Aug" — what survives at 80 columns.
    pub short_date: String,
    /// "14:22".
    pub time: String,
}

/// One line of a day: a time, and what happened. The time is optional because
/// the week screen's day columns are too narrow for it and drop it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Entry {
    /// "09:04", or `None` in a narrow day column.
    pub time: Option<String>,
    /// The line itself, unwrapped — the panel it lands in decides the breaks.
    pub text: String,
    /// Ordinary, or the one hard thing that happened that day.
    pub tone: Tone,
}

impl Entry {
    /// A plain timed entry — the common case.
    pub fn at(time: &str, text: &str) -> Self {
        Self {
            time: Some(time.to_owned()),
            text: text.to_owned(),
            tone: Tone::Plain,
        }
    }

    /// An untimed line, for a week column.
    pub fn line(text: &str) -> Self {
        Self {
            time: None,
            text: text.to_owned(),
            tone: Tone::Plain,
        }
    }

    /// The same, marked as the hard thing.
    pub fn hard(mut self) -> Self {
        self.tone = Tone::Hard;
        self
    }
}

/// How full a day was, as the height of one bar in the seven-day ribbon.
///
/// A bar, not a figure: the ribbon is read as a shape, and `1i` rules out
/// putting the number on screen. `level` indexes the eight block glyphs
/// `▁▂▃▄▅▆▇█`, so it is a drawing instruction rather than a measurement — which
/// is why it is clamped on the way in and cannot be out of range downstream.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(from = "LoadWire")]
pub struct Load {
    level: u8,
    /// Yellow on the day that was hard, bright on today, furniture otherwise.
    pub tone: Tone,
}

/// The shortest drawable bar.
///
/// Written out rather than derived: a derived `Default` would set `level` to
/// `0`, which is one below the first glyph and makes [`Load::glyph`] index out
/// of range. The clamp in [`Load::new`] is what keeps `glyph` total, and
/// `#[derive(Default)]` walks straight past it.
impl Default for Load {
    fn default() -> Self {
        Self::new(1, Tone::default())
    }
}

impl Load {
    /// The eight bar glyphs, shortest first.
    pub const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    /// A bar at `level` (1..=8), clamped. `0` would draw nothing at all and
    /// leave a hole in the ribbon, so an empty day still gets the shortest bar.
    pub fn new(level: u8, tone: Tone) -> Self {
        Self {
            level: level.clamp(1, Self::BARS.len() as u8),
            tone,
        }
    }

    /// The glyph for this bar.
    ///
    /// Total: `level` is clamped in [`Load::new`], the field is private, and
    /// [`Default`] is written out rather than derived — but the index is still
    /// saturated here, because a `Load` that somehow held `0` should draw the
    /// shortest bar rather than take down the whole frame.
    pub fn glyph(self) -> char {
        let index = usize::from(self.level).saturating_sub(1);
        Self::BARS[index.min(Self::BARS.len() - 1)]
    }
}

/// The deserialized form of [`Load`].
///
/// `Load::level` is private and clamped, which is what makes `Load::glyph`
/// total — so the fixture cannot set it directly. Deserializing through this
/// shim and converting runs the clamp on the way in, meaning a hand-edited
/// `level = 0` in `demo.toml` still yields a drawable bar rather than a hole in
/// the ribbon.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadWire {
    #[serde(default)]
    level: u8,
    #[serde(default)]
    tone: Tone,
}

impl From<LoadWire> for Load {
    fn from(wire: LoadWire) -> Self {
        Self::new(wire.level, wire.tone)
    }
}

/// How a day felt, in the day's own words: "A good day", "Mixed", "A rough
/// day". Never computed into a rating — the text is the whole content.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mood {
    /// "A rough day".
    pub text: String,
    /// Yellow for a hard day, bright for a notably good one, body otherwise.
    pub tone: Tone,
}

impl Mood {
    /// A mood in the ordinary tone.
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            tone: Tone::Plain,
        }
    }

    /// The mood of a day that was hard.
    pub fn hard(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            tone: Tone::Hard,
        }
    }

    /// The mood of a day worth noticing.
    pub fn notable(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            tone: Tone::Notable,
        }
    }
}

/// One day, at every size the design asks for it: a 17-column week gutter, the
/// 48-column Today panel, and the 46-column week detail pane.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Day {
    /// "Thu 27" — the week panel's title.
    pub short_label: String,
    /// "Wednesday 26 August" — the detail pane's title.
    pub long_label: String,
    /// "27" — the ribbon's column head.
    pub day_of_month: String,
    /// "Clear · 19°". `None` when nothing observed the weather; the line is
    /// then omitted rather than filled with a placeholder.
    pub weather: Option<String>,
    /// How the day felt, if it can be said yet.
    pub mood: Option<Mood>,
    /// This day's bar in the ribbon.
    pub load: Load,
    /// What happened, in order, with times — the day's log, as the week
    /// screen's detail pane and the Today panel show it.
    pub entries: Vec<Entry>,
    /// The same day in four words a line, for the week screen's 17-column
    /// gutter.
    ///
    /// Not a wrapped [`Day::entries`]: the artboard's Wednesday column reads
    /// "Incident / 09:42-11:40 / Drinks off / Mum called mid-incident / — not
    /// called back" while its detail pane carries the full timed log. A column
    /// that narrow needs a summary written for it, not a truncated log, so the
    /// two are separate fields. A day with no log falls back to these, so a day
    /// nobody has opened still shows something.
    ///
    /// Untimed [`Entry`] rather than plain strings, because the gutter still
    /// needs a tone: the artboard's "Incident 09:42-11:40" is the one yellow
    /// thing in the week.
    pub highlights: Vec<Entry>,
    /// Trailing observations on the week screen's detail pane — "You came back
    /// to the 512 cap four times on this day." Paragraphs, separated by a blank
    /// line; the gap is what makes them read as separate observations.
    pub notes: String,
}

/// Why a thread is as strong as it is, said in the user's own history rather
/// than as a figure: "Every day this week · eight other notes lean on it".
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Justification {
    /// The reason, unwrapped.
    pub text: String,
    /// Set when this thread has *just* come back from another day, which is the
    /// only thing in the app allowed to be blue.
    pub returned: bool,
}

impl Justification {
    /// The ordinary case: strength explained by history.
    pub fn history(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            returned: false,
        }
    }

    /// The same, but this thread has just been brought back into the
    /// conversation — so it renders in the returning colour.
    pub fn came_back(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            returned: true,
        }
    }

    /// Whether there is a reason to draw at all.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// A thought the user keeps returning to.
///
/// Ordered by how often, and never labelled with how often — the position in
/// [`Workspace::threads`] is the encoding.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Thread {
    /// The thought itself, unwrapped.
    pub summary: String,
    /// Which of the week's seven days it came up on, Friday first. `false` is
    /// drawn as absence rather than left blank, so the shape stays readable.
    pub days: [bool; 7],
    /// Why it is where it is in the list.
    pub because: Justification,
    /// What else depends on it — shown when the user is about to contradict it
    /// (artboard `1d`), and empty the rest of the time.
    pub leaned_on: Vec<String>,
}

impl Thread {
    /// How many of the week's days this thread came up on. Used to order the
    /// list, never rendered.
    pub fn day_count(&self) -> usize {
        self.days.iter().filter(|d| **d).count()
    }
}

/// A line in "Just remembered": what Mooshik has picked up in the last little
/// while, freshest first, fading down the ramp to the oldest.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Trickle {
    /// The line itself.
    pub text: String,
    /// Set when this line is something returning from another day.
    pub returned: bool,
}

impl Trickle {
    /// Something newly noticed.
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            returned: false,
        }
    }

    /// Something brought back from another day.
    pub fn came_back(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            returned: true,
        }
    }
}

/// A thing the user said on another day, quoted back with its source and the
/// reason it surfaced. Artboard `1c` — "The model forgot; the memory didn't."
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Recall {
    /// The panel title: " From Monday 24 August ".
    pub source: String,
    /// The quote, in the user's own words, unwrapped.
    pub quote: String,
    /// The reason, punched through the panel's bottom rule: "You've come back to
    /// this every day this week".
    pub because: String,
}

/// Said once, in the conversation, in the place a reply would go — not a modal,
/// not an error, nothing to dismiss. Artboard `1d`.
///
/// **No title.** The card's name is fixed chrome — `1d`'s " One thing before you
/// do " — and lives in `en.toml` as `tui.panel_caution`, beside every other panel
/// title. It was a field here, set to that same sentence by every caution that
/// existed, which made a piece of the frame look like content and left the key
/// unread; a locale that translated `panel_caution` would have seen no change on
/// screen.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Caution {
    /// The opening, unwrapped. The quoted commitment inside it is emphasised by
    /// the renderer finding the quotation marks, not marked up here.
    pub lead: String,
    /// The things that lean on what is about to change.
    pub leaning: Vec<String>,
    /// The closing reassurance, punched through the bottom rule: "Nothing's
    /// changed — say the word and I'll follow".
    pub because: String,
}

/// One thing in the conversation.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Turn {
    /// Somebody spoke.
    Said {
        /// "09:04".
        time: String,
        /// Who.
        speaker: Speaker,
        /// What, unwrapped.
        text: String,
    },
    /// Memory produced something from another day, inline where it was needed.
    Recalled(Recall),
    /// Mooshik said one careful thing before the user changes their mind.
    Cautioned(Caution),
}

/// The input line, and the line under it that says nothing needs saving.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Composer {
    /// What has been typed so far. The cursor renders after it.
    pub draft: String,
}

/// The conversation panel's contents.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Conversation {
    /// The elision marker at the top — "... earlier today" — when there is more
    /// above than fits.
    pub earlier: Option<String>,
    /// The turns, oldest first.
    pub turns: Vec<Turn>,
    /// The input line.
    pub composer: Composer,
}

/// The one-word state on the status bar, and how much is behind it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Health {
    /// "Keeping up" — one word or two, never a sentence, and green only when
    /// `well` is set.
    pub state: String,
    /// "214 things remembered, back to 21 August".
    pub scope: String,
    /// "214 remembered" — what survives at 80 columns.
    ///
    /// A second field rather than a truncation of the first, for the same reason
    /// [`Stamp::short_date`] is: cutting "214 things remembered, back to 21
    /// August" to fit yields "214 things remembered, back t…", which reads as a
    /// bug. The short form is written, not derived.
    pub short_scope: String,
    /// Whether the state earns the affirming mark. When this is false the state
    /// drops to furniture rather than turning red — red is reserved.
    pub well: bool,
}

/// The seven-day view.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Week {
    /// "21-27 August".
    pub label: String,
    /// Friday first, today last.
    pub days: Vec<Day>,
    /// Which day the detail pane is showing, as an index into `days`.
    pub selected: usize,
}

impl Day {
    /// The lines a detail pane should show: the timed log if there is one, and
    /// the gutter summary if there is not.
    pub fn detail_entries(&self) -> Vec<Entry> {
        if !self.entries.is_empty() {
            return self.entries.clone();
        }
        self.highlights.clone()
    }
}

// `Week` deliberately has no `selected_day`. It had one, and it was a trap: the
// index it read is not clamped to the seven days the week screen draws — the
// model may hold more, and `H`/`L` clamp to `days.len()` — so it returned days
// no column was showing and the detail pane described one of them. Which day is
// on screen is a question about the *window*, and only the screen knows the
// width that window comes from, so `week::selected_in` answers it there.

/// Everything on screen.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Workspace {
    /// Whose words are the brightest thing in the conversation.
    pub person: String,
    /// The clock in the title bar.
    pub now: Stamp,
    /// Today, for the Today panel.
    pub today: Day,
    /// The week, for the week screen and the ribbon.
    pub week: Week,
    /// What keeps coming back, strongest first.
    pub threads: Vec<Thread>,
    /// Just remembered, freshest first.
    pub trickle: Vec<Trickle>,
    /// The conversation.
    pub conversation: Conversation,
    /// The status bar.
    pub health: Health,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `level` is a drawing instruction, so it is clamped into the glyph range
    /// on the way in and `glyph` is total. A `0` from a caller that measured an
    /// empty day must still draw a bar, not a hole in the ribbon.
    #[test]
    fn a_bar_is_always_drawable() {
        // The default must be drawable too: a derived Default would set level 0
        // and index one below the first glyph.
        assert_eq!(Load::default().glyph(), '▁');
        assert_eq!(Day::default().load.glyph(), '▁');
        assert_eq!(Load::new(0, Tone::Plain).glyph(), '▁');
        assert_eq!(Load::new(1, Tone::Plain).glyph(), '▁');
        assert_eq!(Load::new(8, Tone::Plain).glyph(), '█');
        assert_eq!(Load::new(200, Tone::Plain).glyph(), '█');
        for level in 1..=8u8 {
            assert_eq!(
                Load::new(level, Tone::Plain).glyph(),
                Load::BARS[usize::from(level) - 1]
            );
        }
    }

    /// The ribbon has one bar a day, so the glyph table has to have exactly the
    /// eight steps the design draws from.
    #[test]
    fn there_are_eight_bar_steps() {
        assert_eq!(Load::BARS.len(), 8);
        assert_eq!(Load::BARS.first(), Some(&'▁'));
        assert_eq!(Load::BARS.last(), Some(&'█'));
    }

    /// A thread's day marks span exactly the week, so the count can never
    /// exceed seven — this is the array's job, and the test pins that the
    /// counter agrees with it.
    #[test]
    fn a_thread_counts_only_the_weeks_days() {
        let mut thread = Thread::default();
        assert_eq!(thread.day_count(), 0);
        thread.days = [true; 7];
        assert_eq!(thread.day_count(), 7);
        thread.days = [true, false, true, false, true, false, true];
        assert_eq!(thread.day_count(), 4);
    }

    /// Blue is only ever a thing returning. The two constructors are the only
    /// way to set the flag, so the tone cannot be applied by accident.
    #[test]
    fn only_returning_things_are_marked_as_returning() {
        assert!(!Justification::history("Every day this week").returned);
        assert!(Justification::came_back("Came back just now").returned);
        assert!(!Trickle::new("12km before standup").returned);
        assert!(Trickle::came_back("Brought back Monday's decision").returned);
    }

    /// A day with a timed log shows it; a day with only a gutter summary shows
    /// that instead, so an unopened day is never a blank pane.
    #[test]
    fn a_detail_pane_falls_back_to_the_gutter_summary() {
        let mut day = Day::default();
        assert!(day.detail_entries().is_empty());

        day.highlights = vec![Entry::line("Rode in"), Entry::line("Cooked")];
        let fallback = day.detail_entries();
        assert_eq!(fallback.len(), 2);
        assert!(fallback.iter().all(|entry| entry.time.is_none()));
        assert_eq!(fallback[0].text, "Rode in");

        day.entries = vec![Entry::at("09:42", "The ring overflowed")];
        let log = day.detail_entries();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].time.as_deref(), Some("09:42"));
    }
}
