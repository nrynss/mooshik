//! The frame every artboard imports, and the five things a frame can mean.
//!
//! Port of `scratch_design/Panel.dc.html`: a box-drawing border with the title
//! inset at **column 2** — so the padded title covers two cells of the top rule
//! and leaves the corner at column 0 and the rule at column 1 alone — punched
//! through that rule against the ground. `1i` states what the frames encode:
//! "A light frame is a panel. Accent frame means focused. Yellow frame is said
//! once and needs no answer" and "A double frame appears exactly once, on the
//! one action that cannot be undone by pressing esc."
//!
//! That last sentence is why [`Kind`] exists rather than separate `border`,
//! `title` and `border_type` fields. With free-form fields, a double rule is one
//! stray `BorderType::Double` away, and the app's most serious signal quietly
//! becomes decoration. Here the double rule is reachable only through
//! [`Kind::Danger`], so drawing one is a deliberate choice of meaning and
//! `only_danger_is_double_ruled` holds the line.
//!
//! The title's column-2 inset is also not ratatui's default — `Block::title`
//! renders at column 1 — so the border and the title are drawn separately: the
//! block for the rules, then the title written onto the top rule, and the badge
//! onto the bottom one.

use ratatui::widgets::{Block, BorderType, Borders};

use crate::tui::{
    grid::{Grid, Place},
    theme::Role,
};

/// What a frame means. Each variant fixes the border colour, the title colour
/// and whether the rule is single or double, so the meaning and the drawing
/// cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An ordinary panel. Furniture rule, body title.
    Idle,
    /// The panel holding focus. One accent, used as a line.
    Focused,
    /// Said once, needs no answer, nothing to dismiss.
    Caution,
    /// Something returning from another day. Only ever this.
    Returned,
    /// The one action `esc` cannot undo. The only double rule, and one of only
    /// two reds, in the whole app.
    ///
    /// Nothing draws it yet: both of `1i`'s two red uses live on artboard `1f`,
    /// which has not been ported. It is kept because it is the enforcement
    /// point for two of the design's rules at once — `only_danger_is_double_ruled`
    /// and `only_danger_is_red` are only meaningful because the double rule and
    /// the reserved red are reachable *through this variant and no other*, and
    /// the cross-screen tests then prove the ported screens spend neither.
    Danger,
}

impl Kind {
    /// The role the rules are drawn in.
    pub const fn border(self) -> Role {
        match self {
            Self::Idle => Role::Furniture,
            Self::Focused => Role::Accent,
            Self::Caution => Role::Caution,
            Self::Returned => Role::Returned,
            Self::Danger => Role::Danger,
        }
    }

    /// The role the title is drawn in.
    ///
    /// The two meaning-carrying frames colour their title the same as their
    /// rule — the frame and its name are one statement — while a plain panel
    /// keeps a body-coloured title against furniture rules, and a focused one
    /// brightens the title rather than accenting it, so focus reads as the rule.
    pub const fn title(self) -> Role {
        match self {
            Self::Idle => Role::Body,
            Self::Focused => Role::Strongest,
            Self::Caution => Role::Caution,
            Self::Returned => Role::Returned,
            Self::Danger => Role::Danger,
        }
    }

    /// The role a badge punched through the bottom rule is drawn in.
    ///
    /// The same as [`Kind::title`] everywhere except on a caution, where `1d`
    /// draws the badge div in `var(--d)` — furniture — while its title and rule
    /// are yellow. (`1c`'s recall badge *is* `var(--bl)`, the same as its frame,
    /// which is why this is a per-kind decision rather than one rule for badges.)
    ///
    /// It matters more than a shade: the caution's badge is the reassurance
    /// "Nothing's changed — say the word and I'll follow", and painting it yellow
    /// spent `1i`'s twice-a-week caution colour on the one line whose whole job
    /// is to be calm — three yellow runs on a card the artboard gives two.
    pub const fn badge(self) -> Role {
        match self {
            Self::Caution => Role::Furniture,
            other => other.title(),
        }
    }

    /// Whether this frame is double-ruled. True for [`Kind::Danger`] and
    /// nothing else — see this module's header.
    pub const fn is_double(self) -> bool {
        matches!(self, Self::Danger)
    }

    /// The frame for a panel that may or may not hold focus.
    pub const fn focused_if(focused: bool) -> Self {
        if focused {
            Self::Focused
        } else {
            Self::Idle
        }
    }
}

/// A framed region: rules, an inset title, and optionally a badge punched
/// through the bottom rule.
#[derive(Debug, Clone)]
pub struct Panel<'a> {
    title: &'a str,
    kind: Kind,
    badge: Option<&'a str>,
    title_role: Option<Role>,
}

impl<'a> Panel<'a> {
    /// The column the title and badge sit at, inside the frame's own left edge.
    /// The design's `--cw * 2` relative to every panel.
    pub const INSET: u16 = 2;

    /// A panel titled `title`, framed as `kind`.
    ///
    /// `title` is given unpadded — "The conversation", not " The conversation "
    /// — because the single space either side is invariant across every artboard
    /// and belongs to the frame rather than to each call site.
    pub fn new(title: &'a str, kind: Kind) -> Self {
        Self {
            title,
            kind,
            badge: None,
            title_role: None,
        }
    }

    /// Draw the title in `role` instead of the one [`Kind`] would choose.
    ///
    /// For the week screen's day columns, whose titles are dates — and `1i`
    /// gives dates their own colour: "the spine everything hangs from". The
    /// *border* still comes from [`Kind`], so focus and the one-double-frame
    /// rule are untouched; only the name in the rule changes colour.
    pub fn titled_as(mut self, role: Role) -> Self {
        self.title_role = Some(role);
        self
    }

    /// Punch `badge` through the bottom rule, at the same inset as the title.
    ///
    /// This is where the design puts a thing's reason — "You've come back to
    /// this every day this week", "Nothing's changed — say the word and I'll
    /// follow" — so the justification is part of the frame rather than another
    /// line of body text competing with the content.
    pub fn badge(mut self, badge: &'a str) -> Self {
        self.badge = Some(badge);
        self
    }

    /// Draw the panel at `at` of `grid`, and return a grid over its interior,
    /// addressed from that interior's own origin.
    ///
    /// Chaining the interior out of the draw call is what keeps the screens
    /// short: a panel is placed once, in the design's coordinates, and its
    /// contents are then written relative to it.
    pub fn draw<'g>(self, grid: &'g mut Grid<'_>, at: Place) -> Grid<'g> {
        let Place { col, row, w, h } = at;
        let border_type = if self.kind.is_double() {
            BorderType::Double
        } else {
            BorderType::Plain
        };
        // The rules themselves are the one thing ratatui already draws exactly
        // right, corner joints and both weights included, so `Block` draws them
        // and this module only places the title and badge over them.
        grid.widget(
            col,
            row,
            w,
            h,
            Block::default()
                .borders(Borders::ALL)
                .border_type(border_type)
                .border_style(self.kind.border().style()),
        );

        // A frame with no interior has nothing to name, so it writes neither its
        // title nor its badge and yields an empty grid — contents then simply do
        // not draw rather than spilling over the frame.
        //
        // The condition is the interior's own emptiness, which the rules inset by
        // one on all four sides, and every part of it is reachable.
        // `today::Split` gives the thread panel `h = 0` whenever the band is 16
        // rows or shorter (a terminal 18 rows tall at 100 columns or wider), and
        // `grid.widget` is a no-op at zero height while the title write was not: a
        // bare ` What keeps coming back ` floated on the blank row above the bottom
        // rule with no frame around it. At `h = 1` the title and the badge shared
        // one row and the badge won. And at `w = 2` the title was written from the
        // inset, past the frame's own right rule and into whatever panel sat
        // beside it — the general form of which is [`Panel::punch`]'s job now, and
        // this guard is only the "no interior at all" case it leaves behind.
        if w < 3 || h < 3 {
            return grid.sub(col, row, 0, 0);
        }

        // The title and badge overwrite the rule they sit on, against the
        // ground, which is how the design punches them through it.
        let title_style = self
            .title_role
            .unwrap_or_else(|| self.kind.title())
            .on_ground();
        // An untitled panel keeps its rule intact. The padding is the frame's
        // rather than the call site's, so an empty title would still write
        // `"  "` on the ground — two black cells punched into an accent rule,
        // which reads as a rendering fault. The week screen's detail pane draws
        // exactly this when no day is selected.
        if !self.title.is_empty() {
            self.punch(grid, col, row, w, &format!(" {} ", self.title), title_style);
        }
        if let Some(badge) = self.badge {
            self.punch(
                grid,
                col,
                row.saturating_add(h.saturating_sub(1)),
                w,
                &format!(" {badge} "),
                self.kind.badge().on_ground(),
            );
        }

        // The interior is the frame inset by its own rules on all four sides.
        grid.sub(
            col.saturating_add(1),
            row.saturating_add(1),
            w.saturating_sub(2),
            h.saturating_sub(2),
        )
    }

    /// Punch `text` through the rule on `row`, at the inset, **clipped to the
    /// frame** rather than to the grid.
    ///
    /// The clip is the whole point, and it is structural: the text is written
    /// into a sub-grid that is exactly the cells between the inset and the
    /// frame's own right rule, so there is no arithmetic at the call site that
    /// could be got wrong and no way for a long title to reach past it.
    ///
    /// It used to be a bare `grid.put`, which clips to the *grid* and not to the
    /// frame — so a title or badge longer than `w - 4` overwrote the panel's own
    /// right rule and carried on into whatever sat beside it. That is reachable
    /// on the demo path: `--demo caution` at 100 to 103 columns lost the card's
    /// bottom-right corner to a 48-character badge, and the week screen — which
    /// has no narrow variant — lost the detail pane's `┐` at 60 columns and had
    /// the thread panel's title spill out of it at 40. `1i` makes the frame
    /// load-bearing ("A light frame is a panel. Accent frame means focused"), so
    /// a missing corner is a broken signal rather than a cosmetic loss, and a
    /// clipped name is the cheaper of the two.
    ///
    /// The room is `w - (INSET + 1)`: the inset takes two cells off the left and
    /// the right rule keeps the last one. At `w = 3` that is zero cells and
    /// nothing is written, which is right — a three-wide frame has one interior
    /// column and no room to name itself.
    fn punch(
        &self,
        grid: &mut Grid<'_>,
        col: u16,
        row: u16,
        w: u16,
        text: &str,
        style: ratatui::style::Style,
    ) {
        let room = w.saturating_sub(Self::INSET.saturating_add(1));
        // A name needs its space either side and at least one character between
        // them. Below that the padded string contributes only ground-coloured
        // blanks — at `w = 4`, a single black cell punched into the rule with no
        // letter to justify it, which is the fault the empty-title guard exists
        // to prevent, one cell narrower.
        if room < Self::INSET.saturating_add(1) {
            return;
        }
        grid.sub(col.saturating_add(Self::INSET), row, room, 1)
            .put(0, 0, text, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, layout::Rect, style::Style};

    use crate::tui::theme::Role;

    /// How many variants [`Kind`] has, pinned so the walk below can be checked
    /// against it. Bumping this is the second half of adding a variant; the
    /// first half is the compile error in [`after`].
    const KINDS: usize = 5;

    /// Every [`Kind`], walked through an exhaustive `match`.
    ///
    /// This used to be a hand-written `[Kind; 5]`, and three of `1i`'s frame
    /// rules iterate it — `only_danger_is_double_ruled`, `only_danger_is_red`
    /// and the badge test. A sixth variant simply not added to that array would
    /// have escaped all three without failing anything, which is a rule the
    /// tests claim to hold and quietly would not.
    ///
    /// [`after`] is exhaustive, so `Kind::Whatever` added tomorrow does not
    /// compile until somebody says where it sits in the walk, and sitting
    /// anywhere in the walk is what puts it in this list. What the compiler
    /// still cannot force is that the new arm is *reachable* — an arm returning
    /// `None` beside `Danger`'s leaves the variant off the end — so
    /// `the_walk_reaches_every_kind` pins the count as well, and the two
    /// together mean a variant can only escape by two deliberate edits against
    /// a comment that says not to.
    fn all_kinds() -> Vec<Kind> {
        /// The next kind after this one, or `None` at the end of the walk.
        const fn after(kind: Kind) -> Option<Kind> {
            match kind {
                Kind::Idle => Some(Kind::Focused),
                Kind::Focused => Some(Kind::Caution),
                Kind::Caution => Some(Kind::Returned),
                Kind::Returned => Some(Kind::Danger),
                Kind::Danger => None,
            }
        }
        let mut kinds = vec![Kind::Idle];
        // Stopping on a repeat rather than trusting the arms: a walk written
        // into a cycle would otherwise spin here, and the test below is what
        // reports it as a short list.
        while let Some(next) = after(*kinds.last().expect("the walk starts at Idle")) {
            if kinds.contains(&next) {
                break;
            }
            kinds.push(next);
        }
        kinds
    }

    /// The walk visits every kind exactly once, so the three rules that iterate
    /// it are iterating all of them.
    #[test]
    fn the_walk_reaches_every_kind() {
        let kinds = all_kinds();
        assert_eq!(
            kinds.len(),
            KINDS,
            "a variant is off the end of the walk, or `KINDS` was not bumped"
        );
        for (index, kind) in kinds.iter().enumerate() {
            assert!(!kinds[..index].contains(kind), "{kind:?} is walked twice");
        }
    }

    fn row_text(buf: &Buffer, row: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, row)].symbol().chars().next().unwrap_or(' '))
            .collect()
    }

    /// The pin from this module's header: the double rule belongs to the one
    /// irreversible action and nothing else can draw it.
    #[test]
    fn only_danger_is_double_ruled() {
        for kind in all_kinds() {
            assert_eq!(
                kind.is_double(),
                kind == Kind::Danger,
                "{kind:?} disagrees with the one-double-frame rule"
            );
        }
    }

    /// Red is reserved to two uses in the app, so only the danger frame reaches
    /// it — a caution is yellow, and a focused panel is the accent.
    #[test]
    fn only_danger_is_red() {
        for kind in all_kinds() {
            let is_red = kind.border() == Role::Danger;
            assert_eq!(
                is_red,
                kind == Kind::Danger,
                "{kind:?} spends the reserved red"
            );
        }
    }

    /// The title sits at column 2 with one space either side — over the corner
    /// and one cell of rule, as `Panel.dc.html` places it.
    #[test]
    fn the_title_is_inset_two_cells_over_the_rule() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 4));
        let area = buf.area;
        let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
        Panel::new("You", Kind::Idle).draw(&mut grid, Place::new(0, 0, 14, 3));
        assert_eq!(row_text(&buf, 0), "┌─ You ──────┐          ");
    }

    /// An untitled panel leaves its top rule whole. A padded empty title would
    /// punch ground-coloured cells through the rule for no name at all — the
    /// week screen's detail pane, with no day selected.
    #[test]
    fn an_untitled_panel_leaves_its_rule_intact() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 3));
        let area = buf.area;
        let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
        Panel::new("", Kind::Focused).draw(&mut grid, Place::new(0, 0, 14, 3));
        assert_eq!(row_text(&buf, 0), "┌────────────┐");
        for col in 0..14u16 {
            assert_eq!(
                buf[(col, 0)].bg,
                ratatui::style::Color::Reset,
                "column {col} of the rule is painted"
            );
        }
    }

    /// The badge sits on the bottom rule at the same inset, which is where the
    /// design puts a thing's reason.
    #[test]
    fn the_badge_is_punched_through_the_bottom_rule() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 4));
        let area = buf.area;
        let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
        Panel::new("From Monday", Kind::Returned)
            .badge("three times")
            .draw(&mut grid, Place::new(0, 0, 20, 3));
        assert_eq!(row_text(&buf, 2), "└─ three times ────┘    ");
    }

    /// A title or a badge longer than the frame is clipped at the frame, not at
    /// the grid — so the panel keeps its right rule and its corners, and nothing
    /// reaches the panel beside it.
    ///
    /// This is the fault [`Panel::punch`] exists for. `--demo caution` at 100 to
    /// 103 columns lost the recall card's bottom-right corner to a 48-character
    /// badge, and the week screen lost the detail pane's `┐` at 60 columns. `1i`
    /// makes the frame the signal — "A light frame is a panel. Accent frame means
    /// focused" — so a missing corner says something false about focus.
    #[test]
    fn a_long_title_or_badge_is_clipped_at_the_frame() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 4));
        let area = buf.area;
        let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
        Panel::new("A title far longer than this frame", Kind::Idle)
            .badge("and a badge that is longer still")
            .draw(&mut grid, Place::new(0, 0, 14, 3));
        // The frame is whole at both ends of both rules, and the cells past it
        // are untouched.
        assert_eq!(row_text(&buf, 0), "┌─ A title fa┐                ");
        assert_eq!(row_text(&buf, 2), "└─ and a badg┘                ");
        // The right rule of the interior row is intact too, which is what the
        // panel beside this one would otherwise have lost.
        assert_eq!(buf[(13, 1)].symbol(), "│");
    }

    /// A caution's badge is furniture, not yellow — `1d` draws its badge div in
    /// `var(--d)` while its title and rule are yellow. Every other frame colours
    /// the badge the same as its title, `1c`'s blue recall badge included.
    #[test]
    fn only_a_cautions_badge_leaves_its_frames_colour() {
        for kind in all_kinds() {
            let expected = if kind == Kind::Caution {
                Role::Furniture
            } else {
                kind.title()
            };
            assert_eq!(kind.badge(), expected, "{kind:?}'s badge");
        }
        assert_eq!(Kind::Returned.badge(), Role::Returned);
        assert_ne!(Kind::Caution.badge(), Role::Caution);
    }

    /// And it is drawn that way, not merely decided that way: the reassurance on a
    /// caution's bottom rule is the one line whose job is to be calm, and three
    /// yellow runs on a card the artboard gives two spent `1i`'s twice-a-week
    /// colour on it.
    #[test]
    fn a_cautions_badge_is_drawn_in_furniture() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 4));
        let area = buf.area;
        let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
        Panel::new("One thing before you do", Kind::Caution)
            .badge("Nothing's changed")
            .draw(&mut grid, Place::new(0, 0, 30, 3));
        // Column 3 of the top rule is the title; column 3 of the bottom rule is
        // the badge.
        assert_eq!(buf[(3, 0)].fg, Role::Caution.color(), "the title faded");
        assert_eq!(buf[(0, 2)].fg, Role::Caution.color(), "the rule faded");
        assert_eq!(
            buf[(3, 2)].fg,
            Role::Furniture.color(),
            "the badge is yellow"
        );
    }

    /// A frame with no interior writes neither its title nor its badge.
    ///
    /// `grid.widget` is already a no-op at zero height, but the title write was
    /// not, and it clipped only to the enclosing grid — so `today::Split`'s
    /// zero-row thread panel left a bare ` What keeps coming back ` floating on the
    /// blank row above the bottom rule with no frame around it. At one row the
    /// title and the badge shared a row and the badge won.
    #[test]
    fn a_frame_with_no_interior_writes_no_name() {
        for (w, h) in [(20u16, 0u16), (20, 1), (20, 2), (2, 4), (1, 4), (0, 4)] {
            let mut buf = Buffer::empty(Rect::new(0, 0, 24, 6));
            let area = buf.area;
            let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
            let inner = Panel::new("Named", Kind::Idle)
                .badge("Because")
                .draw(&mut grid, Place::new(0, 1, w, h));
            assert_eq!(inner.width(), 0, "{w}x{h} has an interior");
            assert_eq!(inner.height(), 0, "{w}x{h} has an interior");
            let all: String = (0..6).map(|r| row_text(&buf, r)).collect();
            assert!(!all.contains("Named"), "{w}x{h} wrote its title: {all:?}");
            assert!(!all.contains("Because"), "{w}x{h} wrote its badge: {all:?}");
        }
        // Two by two draws a frame with no interior, and its name must not be
        // written past its own right rule into the panel beside it.
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        let area = buf.area;
        let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
        Panel::new("Named", Kind::Idle).draw(&mut grid, Place::new(0, 0, 2, 2));
        assert_eq!(row_text(&buf, 0), "┌┐  ");
    }

    /// The title and badge are drawn against the ground so they read as punched
    /// through the rule rather than sitting on top of it.
    #[test]
    fn the_title_is_drawn_against_the_ground() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
        let area = buf.area;
        let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
        Panel::new("You", Kind::Idle).draw(&mut grid, Place::new(0, 0, 14, 3));
        assert_eq!(buf[(3, 0)].bg, Role::Ground.color());
        assert_eq!(buf[(0, 0)].bg, ratatui::style::Color::Reset);
    }

    /// The interior is the frame inset by one on every side, and is addressed
    /// from its own origin so contents need no knowledge of where the panel is.
    #[test]
    fn the_interior_excludes_the_rules_and_rebases() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 6));
        let area = buf.area;
        let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
        let mut inner = Panel::new("t", Kind::Idle).draw(&mut grid, Place::new(2, 1, 10, 4));
        assert_eq!(inner.width(), 8);
        assert_eq!(inner.height(), 2);
        inner.put(0, 0, "x", Style::default());
        assert_eq!(row_text(&buf, 2).trim_end(), "  │x       │");
    }

    /// A panel with no room for an interior yields an empty grid, so its
    /// contents do not spill over the frame in a small terminal.
    #[test]
    fn a_frame_too_small_to_hold_anything_yields_nothing() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        let area = buf.area;
        let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
        let inner = Panel::new("t", Kind::Idle).draw(&mut grid, Place::new(0, 0, 2, 2));
        assert_eq!(inner.width(), 0);
        assert_eq!(inner.height(), 0);
    }

    /// A title role override changes the name in the rule and nothing else — the
    /// border, and so the one-double-frame rule, still come from the kind.
    #[test]
    fn a_title_override_leaves_the_border_alone() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
        let area = buf.area;
        let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
        Panel::new("Fri 21", Kind::Idle)
            .titled_as(Role::Date)
            .draw(&mut grid, Place::new(0, 0, 17, 3));
        assert_eq!(buf[(3, 0)].fg, Role::Date.color());
        assert_eq!(buf[(0, 0)].fg, Role::Furniture.color());
        assert_eq!(buf[(0, 0)].symbol(), "┌");
    }

    /// A frame too narrow to hold a name and its two spaces writes neither, so
    /// the rule stays whole. At four cells the padded title contributed one
    /// ground-painted blank and no letter — the same black notch the empty-title
    /// guard exists to prevent, one cell narrower.
    #[test]
    fn a_frame_too_narrow_for_a_name_writes_none() {
        for w in 3..=5u16 {
            let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
            let area = buf.area;
            let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
            Panel::new("Fri 21", Kind::Idle).draw(&mut grid, Place::new(0, 0, w, 3));
            for col in 0..w {
                assert_eq!(
                    buf[(col, 0)].bg,
                    ratatui::style::Color::Reset,
                    "a {w}-cell frame punched ground into its rule at column {col}"
                );
            }
        }
        // Six cells is the narrowest that can carry a letter between its spaces.
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        let area = buf.area;
        let mut grid = crate::tui::grid::Grid::new(&mut buf, area);
        Panel::new("Fri 21", Kind::Idle).draw(&mut grid, Place::new(0, 0, 6, 3));
        assert_eq!(buf[(3, 0)].symbol(), "F");
    }

    /// Focus is carried by the rule turning accent; the title brightens rather
    /// than also turning accent, so there is one accent line per screen.
    #[test]
    fn focus_moves_the_rule_to_the_accent() {
        assert_eq!(Kind::focused_if(true), Kind::Focused);
        assert_eq!(Kind::focused_if(false), Kind::Idle);
        assert_eq!(Kind::Focused.border(), Role::Accent);
        assert_eq!(Kind::Focused.title(), Role::Strongest);
        assert_eq!(Kind::Idle.border(), Role::Furniture);
    }
}
