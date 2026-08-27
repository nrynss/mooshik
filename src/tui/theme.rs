//! The palette, and the one rule it exists to enforce.
//!
//! Authority: artboard `1i` of the design — "16-colour ANSI plus the dim
//! attribute (SGR 2). Nothing depends on the background — every mark is a glyph
//! in a foreground colour."
//!
//! Two consequences are load-bearing, and both are easy to break by accident:
//!
//! 1. **Every colour here is an ANSI slot, never a hex.** The design lists a
//!    hex beside each slot, but that is the *reference rendering* of colour 5,
//!    not the definition of it. Writing `Color::Rgb(0x91, 0x84, 0xd9)` would
//!    pin Mooshik's accent to one designer's terminal theme and make it clash
//!    with every other pane in the user's tmux session. `Color::Magenta` asks
//!    the terminal for its own 5, which is the whole point of a 16-colour app.
//! 2. **Nothing sets a background.** The ground (colour 0) is "just what is
//!    behind" — so a panel is a frame drawn in a foreground colour, not a
//!    filled rectangle, and the app inherits whatever the terminal is painted.
//!    The single exception the design allows is a title or badge punched
//!    through a rule, which reads as the ground colour rather than as a fill;
//!    that is [`Role::Ground`] and it appears three times in the whole app.
//!
//! Five bright slots are deliberately unspent — bright red 9, bright green 10,
//! bright yellow 11, bright blue 12, bright cyan 14. `1i`: "An app that runs
//! all day cannot spend every colour it owns." [`Role`] has no variant for
//! them, so spending one is a code change with a comment to argue against,
//! rather than a `Color::LightRed` someone slips into a match arm.

use ratatui::style::{Color, Modifier, Style};

/// A semantic colour role. The variants are the design's own vocabulary, so a
/// call site says *what it is drawing* and the palette decides how that looks.
///
/// Deliberately not `Copy`-into-`Color`: several roles are a colour *plus* the
/// dim attribute, and a bare `Color` cannot carry that. Go through
/// [`Role::style`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Colour 0. The ground — the only role that is ever a *background*, and
    /// only to punch a title or badge through a rule it sits on.
    Ground,
    /// Colour 8. Frames, timestamps, key hints, and anything already dealt
    /// with. Most of the chrome is this.
    Furniture,
    /// Colour 8 + dim. Absence: a day a thought did not come up on, and the
    /// oldest end of the trickle. Drawn rather than written.
    Absence,
    /// Colour 7. What Mooshik says, and ordinary body text.
    Body,
    /// Colour 7 + dim. Further down the list — fading, not gone. It can climb
    /// back up, which is why this is not [`Role::Absence`].
    Fading,
    /// Colour 15. The user's own words, and whatever is strongest right now.
    Strongest,
    /// Colour 5. Mooshik speaking, the focused panel, and the cursor. One
    /// accent, used as a line.
    Accent,
    /// Colour 4. Something came back. Only ever on a thing returning from
    /// another day — never decoration, never a second accent.
    Returned,
    /// Colour 6. Days and dates: the spine everything hangs from.
    Date,
    /// Colour 3. A caution worth hearing, and a day that was hard. Twice a
    /// week at most.
    Caution,
    /// Colour 2. One mark, one word: reachable, saved, keeping up. Never a
    /// sentence.
    Affirm,
    /// Colour 1. Reserved. Two uses in the whole app: a refused credential,
    /// and leaving a database behind.
    ///
    /// Nothing draws it yet — both uses live on artboard `1f`, which has not
    /// been ported. It is kept because it is the enforcement point: this
    /// variant plus [`Kind::Danger`](crate::tui::widget::Kind::Danger) is what
    /// makes `1i`'s reserved-red rule checkable, and
    /// `the_reserved_red_is_unspent_on_these_screens` proves the ported
    /// screens do not spend it.
    Danger,
    /// Colour 13. A key being pressed. Nothing else.
    ///
    /// Nothing draws it yet either — no screen animates a keypress. It is kept
    /// to reserve ANSI 13 so nothing else spends it; `no_colour_escapes_the_sixteen`
    /// pins the held-back brights, and this variant keeps 13 from quietly
    /// becoming a second accent.
    Keypress,
}

impl Role {
    /// The ANSI slot this role asks the terminal for.
    pub const fn color(self) -> Color {
        match self {
            Self::Ground => Color::Black,
            Self::Furniture | Self::Absence => Color::DarkGray,
            Self::Body | Self::Fading => Color::Gray,
            Self::Strongest => Color::White,
            Self::Accent => Color::Magenta,
            Self::Returned => Color::Blue,
            Self::Date => Color::Cyan,
            Self::Caution => Color::Yellow,
            Self::Affirm => Color::Green,
            Self::Danger => Color::Red,
            Self::Keypress => Color::LightMagenta,
        }
    }

    /// Whether this role carries the dim attribute (SGR 2) as well as a colour.
    ///
    /// The design uses dim to make one slot do two jobs — colour 8 is furniture
    /// and, dimmed, absence; colour 7 is body and, dimmed, fading — which is
    /// how a four-step brightness ramp fits in a 16-colour palette.
    pub const fn is_dim(self) -> bool {
        matches!(self, Self::Absence | Self::Fading)
    }

    /// This role as a foreground style. The common case, and the only way to
    /// get the dim attribute along with the colour.
    pub fn style(self) -> Style {
        let style = Style::default().fg(self.color());
        if self.is_dim() {
            style.add_modifier(Modifier::DIM)
        } else {
            style
        }
    }

    /// This role as a *background*, for the one thing the design paints: a
    /// panel title or an inline badge punched through the rule it overlaps.
    ///
    /// What is enforced is the *background*, not the caller: the only colour
    /// this API can put behind anything is colour 0, because the background is
    /// hard-coded and no argument reaches it. Any role may be the foreground —
    /// the accent for a focused panel's title, the returning blue for a recall
    /// card's badge — but a filled rectangle in some other colour, the flood
    /// the design spends its whole argument avoiding, is unreachable from here.
    /// `nothing_paints_a_background_but_the_ground` holds the other half: no
    /// screen reaches a background by any other route.
    pub fn on_ground(self) -> Style {
        self.style().bg(Self::Ground.color())
    }
}

/// How strongly something is being returned to, as the design's four-step
/// brightness ramp: "strongest first, fading down. Four steps, no labels, no
/// numbers, no tiers."
///
/// A rank, not a score. Nothing renders the number — `1i`'s "Never on screen"
/// rules out tier names, scores and percentages — so this exists only to pick a
/// [`Role`], and [`Strength::from_rank`] saturates rather than wrapping so a
/// long list simply bottoms out at the faintest step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Strength {
    /// Step 1 of 4 — colour 15. Whatever is strongest right now.
    Strongest,
    /// Step 2 of 4 — colour 7.
    Strong,
    /// Step 3 of 4 — colour 7 + dim.
    Fading,
    /// Step 4 of 4 — colour 8. Still on screen; still able to climb back up.
    Faintest,
}

impl Strength {
    /// The step for a zero-based list position, saturating at the faintest.
    ///
    /// **The third step is doubled**, which is the artboards' own arithmetic
    /// rather than an off-by-one: `1a` draws five threads over four steps, and
    /// the one it spends twice is the fading step — ranks 2 and 3 are both
    /// `var(--t2)` there, and only the fifth thread drops to furniture. Four
    /// steps for five rows has to double one of them somewhere, and the design
    /// doubles the one where a reader is least able to tell two rows apart.
    ///
    /// Saturating past that is the other half: the list is "always ordered by
    /// how often you return to something", so position *is* the encoding, and a
    /// sixth item should look like the fifth rather than wrap back to brightest.
    pub const fn from_rank(rank: usize) -> Self {
        match rank {
            0 => Self::Strongest,
            1 => Self::Strong,
            2 | 3 => Self::Fading,
            _ => Self::Faintest,
        }
    }

    /// The role this step of the ramp draws in.
    pub const fn role(self) -> Role {
        match self {
            Self::Strongest => Role::Strongest,
            Self::Strong => Role::Body,
            Self::Fading => Role::Fading,
            Self::Faintest => Role::Furniture,
        }
    }

    /// This step as a foreground style.
    pub fn style(self) -> Style {
        self.role().style()
    }

    /// The role for position `rank` of the trickle, which has its own ramp.
    ///
    /// "Just remembered" is not the thread list. A thread can climb back up, so
    /// its faintest step is furniture — still furniture, still there. The
    /// trickle is on its way out: `1i` gives its oldest end to
    /// [`Role::Absence`], the same colour as a day a thought did not come up on.
    /// So the trickle starts one step quieter than the thread list and bottoms
    /// out one step further down.
    pub const fn trickle_role(rank: usize) -> Role {
        match rank {
            0 => Role::Body,
            1 => Role::Fading,
            2 => Role::Furniture,
            _ => Role::Absence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pin from this module's header, as a test: no role may resolve to an
    /// RGB triple. `Color::Rgb`/`Color::Indexed` would both defeat the point —
    /// the first hard-codes a theme, the second reaches past the 16 slots the
    /// design commits to.
    #[test]
    fn every_role_is_a_named_ansi_slot() {
        for role in ALL_ROLES {
            match role.color() {
                Color::Rgb(..) | Color::Indexed(_) => {
                    panic!("{role:?} escapes the 16-colour palette")
                }
                _ => {}
            }
        }
    }

    /// `1i` holds five bright slots back on purpose. Nothing in the palette may
    /// reach them, so spending one has to be a deliberate edit here.
    #[test]
    fn the_held_back_brights_are_unspent() {
        const HELD_BACK: [Color; 5] = [
            Color::LightRed,
            Color::LightGreen,
            Color::LightYellow,
            Color::LightBlue,
            Color::LightCyan,
        ];
        for role in ALL_ROLES {
            assert!(
                !HELD_BACK.contains(&role.color()),
                "{role:?} spends a colour the design holds back"
            );
        }
    }

    /// Dim is what makes one slot do two jobs. If these pairs ever stopped
    /// sharing a colour, the four-step ramp would need more than 16 colours.
    #[test]
    fn dim_doubles_two_slots_rather_than_adding_colours() {
        assert_eq!(Role::Furniture.color(), Role::Absence.color());
        assert_eq!(Role::Body.color(), Role::Fading.color());
        assert!(Role::Absence.is_dim() && Role::Fading.is_dim());
        assert!(!Role::Furniture.is_dim() && !Role::Body.is_dim());
    }

    /// No role paints a background of its own; only `on_ground` does, and only
    /// with colour 0.
    #[test]
    fn styles_never_carry_a_background() {
        for role in ALL_ROLES {
            assert_eq!(role.style().bg, None, "{role:?} paints a background");
        }
        assert_eq!(Role::Accent.on_ground().bg, Some(Color::Black));
    }

    /// The ramp is four distinct steps; the ranks it maps them onto are the
    /// artboards', which spend the *third* step twice — five threads over four
    /// steps — and saturate on the last rather than wrapping to brightest.
    #[test]
    fn the_ramp_is_four_steps_and_saturates() {
        let steps = [
            Strength::Strongest,
            Strength::Strong,
            Strength::Fading,
            Strength::Faintest,
        ];
        let styles: Vec<_> = steps.iter().map(|s| s.style()).collect();
        for (i, a) in styles.iter().enumerate() {
            for b in &styles[i + 1..] {
                assert_ne!(a, b, "two steps of the ramp look the same");
            }
        }
        // `1a`'s thread panel, rank by rank.
        assert_eq!(Strength::from_rank(0), Strength::Strongest);
        assert_eq!(Strength::from_rank(1), Strength::Strong);
        assert_eq!(Strength::from_rank(2), Strength::Fading);
        assert_eq!(
            Strength::from_rank(3),
            Strength::Fading,
            "the design spends the third step twice, not the fourth"
        );
        assert_eq!(Strength::from_rank(4), Strength::Faintest);
        assert_eq!(Strength::from_rank(99), Strength::Faintest);
    }

    /// The trickle fades further than the thread list: it ends in absence,
    /// where a thread's faintest step is furniture, because a thread can climb
    /// back up and a trickle line is on its way out.
    #[test]
    fn the_trickle_fades_further_than_the_thread_list() {
        assert_eq!(Strength::trickle_role(0), Role::Body);
        assert_eq!(Strength::trickle_role(1), Role::Fading);
        assert_eq!(Strength::trickle_role(2), Role::Furniture);
        assert_eq!(Strength::trickle_role(3), Role::Absence);
        assert_eq!(Strength::trickle_role(99), Role::Absence);
        assert_eq!(Strength::from_rank(0).role(), Role::Strongest);
        assert_ne!(Strength::from_rank(99).role(), Role::Absence);
    }

    const ALL_ROLES: [Role; 13] = [
        Role::Ground,
        Role::Furniture,
        Role::Absence,
        Role::Body,
        Role::Fading,
        Role::Strongest,
        Role::Accent,
        Role::Returned,
        Role::Date,
        Role::Caution,
        Role::Affirm,
        Role::Danger,
        Role::Keypress,
    ];
}
