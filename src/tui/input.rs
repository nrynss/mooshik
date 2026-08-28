//! Keys to [`Action`]s — the whole keymap, and nothing the screens do not print.
//!
//! Everything bound, in full: `Tab`/`Shift-Tab` cycle panels, `^1` and `^2`
//! choose the view, `Enter` sends, `Backspace` edits the draft, the arrows and
//! `H`/`J`/`K`/`L` move the two cursors, and `Esc`, `q` and `^C` leave. That list
//! and the hints in `src/text/en.toml` are the same list, deliberately: the
//! design's rules also printed `Alt-H/L resize`, `^K a day`, `? keys`, `/ find`,
//! `^, settings` and `Enter open the day`, none of which is bound to anything,
//! and a hint that does nothing is worse than no hint. They come back here and
//! there together, or not at all.
//!
//! **`Alt-Enter` used to be that hint, from the inside.** The composer's rule
//! advertised it as `newline`; it pushed `'\n'` onto the draft, and
//! `Buffer::set_stringn` filters control characters, so nothing appeared — and
//! the composer has one interior text row, so nothing could have. Typing `abc`,
//! `Alt-Enter`, `Backspace` looked like two keys doing nothing and then a third
//! eating a letter. So it is unbound, and the rule no longer names it. Both come
//! back with the chat loop, which is also what gives `Enter` something to do:
//! it is bound and deliberately inert (see [`Action::Send`]), which is why the
//! rule does not promise `Enter send` either.
//!
//! Modifiers Mooshik does not use are refused rather than ignored. `Alt-h` used
//! to fall through the `Char('h')` arm and move the week's day cursor, so a key
//! the app never claimed did something the user did not ask for. With
//! `Alt-Enter` gone, Alt is claimed for nothing at all and every Alt chord is
//! refused here.
//!
//! **Why the keymap is modal but the app is not.** When the conversation has
//! focus, `j` is the letter `j`; when the thread list has focus, it moves the
//! cursor. That is unavoidable in a keyboard-only app with a text field in it,
//! and it is why [`Action`] carries `Type(char)` rather than the app inspecting
//! key events: the decision about what a letter means is made once, here, and
//! the app only ever receives the resolved intent.
//!
//! **And why the mode is two facts rather than one.** `j`/`k` used to resolve to
//! a cursor move from anywhere they were not a letter, so on the Today screen
//! they moved the thread highlight from all four focus states while
//! [`aside::threads`](super::screen::aside::threads) draws that highlight from
//! exactly one of them. `hint_today` promises `J/K a thread`; in three of the
//! four the keys did something the reader could not see, which is the same
//! broken promise as an unbound hint, reached from the other end. So the mode
//! carries whether the cursor is on screen as well as whether the draft is, and
//! `j`/`k` are [`Action::Ignore`] where nothing would move visibly. The hint
//! stays honest because `Tab panel` precedes it on the same rule.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::app::{Action, Mode};

/// Resolve `key` into an action.
///
/// `mode` is what the screen underneath makes of the keyboard — see
/// [`Mode`](super::app::Mode). It carries two facts rather than one because two
/// of the bindings are conditional: whether keystrokes reach the draft, and
/// whether the panel that draws the thread cursor holds focus.
pub fn action(key: KeyEvent, mode: Mode) -> Action {
    let Mode {
        typing,
        thread_cursor,
    } = mode;
    // Windows terminals report both press and release; acting on both would
    // double every keystroke.
    if key.kind == KeyEventKind::Release {
        return Action::Ignore;
    }

    let control = key.modifiers.contains(KeyModifiers::CONTROL);

    // Chords first: they mean the same thing wherever focus is, so a `^2` in the
    // middle of typing still shows the week rather than inserting a `2`.
    if control {
        return match key.code {
            KeyCode::Char('c') | KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('1') => Action::ShowToday,
            KeyCode::Char('2') => Action::ShowWeek,
            _ => Action::Ignore,
        };
    }

    // Alt is claimed for nothing, so every Alt chord is refused here rather than
    // falling through to the arms below — `Alt-h` moved the week's day cursor and
    // `Alt-Enter` put an invisible newline in the draft, neither of which anybody
    // asked for. Shift is *deliberately* not filtered: crossterm reports a capital as
    // `Char('J')` with the modifier set, and the design's hints are written `J/K`
    // and `H/L`, so both cases have to arrive at the movement arms below.
    if key.modifiers.contains(KeyModifiers::ALT) {
        return Action::Ignore;
    }

    match key.code {
        KeyCode::Tab => Action::NextPanel,
        KeyCode::BackTab => Action::PreviousPanel,
        KeyCode::Enter if typing => Action::Send,
        KeyCode::Backspace if typing => Action::Backspace,

        // The arrows work wherever focus is: they are unambiguous, and the
        // design's `H/L` and `J/K` are the letter equivalents for a hand already
        // on the home row.
        KeyCode::Down => Action::Next,
        KeyCode::Up => Action::Previous,
        KeyCode::Left => Action::Left,
        KeyCode::Right => Action::Right,

        // Esc leaves the app from a plain screen. The design gives it "back" on
        // the settings and first-run screens, which will claim it when those
        // land; from Today and the week there is nothing to go back to.
        KeyCode::Esc => Action::Quit,

        // A letter is a letter while typing, and a movement otherwise.
        KeyCode::Char(character) if typing => Action::Type(character),
        // `j`/`k` only where a cursor is drawn. They are the one binding whose
        // effect is invisible from the wrong screen: the Today screen draws the
        // thread cursor only on the focused panel, so from the other three focus
        // states these keys moved a highlight nobody could see while the rule
        // went on promising `J/K a thread`. The hint stays honest because `Tab
        // panel` precedes it on that rule — the reader is told how to reach the
        // panel the keys belong to.
        //
        // Folded to lowercase first, because the rules are written `J/K a
        // thread` and `H/L a day` and crossterm delivers a capital as its own
        // `Char('J')`. Matching only the lowercase arm left the two most
        // prominent hints in the app naming keystrokes that did nothing — the
        // same broken promise as an unbound `? keys`, reached from the other
        // end, and the reason `movement` takes the fold rather than each arm
        // listing two characters.
        KeyCode::Char(character) => movement(character, thread_cursor),
        _ => Action::Ignore,
    }
}

/// What a letter means when it is not being typed, in either case.
fn movement(character: char, thread_cursor: bool) -> Action {
    match character.to_ascii_lowercase() {
        'j' if thread_cursor => Action::Next,
        'k' if thread_cursor => Action::Previous,
        'h' => Action::Left,
        'l' => Action::Right,
        'q' => Action::Quit,
        _ => Action::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEvent;

    fn plain(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// The three modes the app actually produces: typing into the draft, on a
    /// screen that draws the thread cursor, and on one that draws neither.
    fn typing() -> Mode {
        Mode {
            typing: true,
            thread_cursor: false,
        }
    }

    fn cursored() -> Mode {
        Mode {
            typing: false,
            thread_cursor: true,
        }
    }

    fn plain_mode() -> Mode {
        Mode::default()
    }

    /// Every mode, for the bindings that must mean the same in all of them.
    fn all_modes() -> [Mode; 3] {
        [typing(), cursored(), plain_mode()]
    }

    /// The rules are written `J/K a thread` and `H/L a day`, so both cases have
    /// to work. Only the lowercase arms were bound, which left the two most
    /// prominent hints in the app promising keystrokes that did nothing.
    #[test]
    fn the_movement_keys_answer_in_either_case() {
        for (lower, upper, expected) in [
            ('j', 'J', Action::Next),
            ('k', 'K', Action::Previous),
            ('h', 'H', Action::Left),
            ('l', 'L', Action::Right),
        ] {
            for character in [lower, upper] {
                assert_eq!(
                    action(plain(KeyCode::Char(character)), cursored()),
                    expected,
                    "{character:?} is not bound"
                );
            }
        }
        // Crossterm delivers a capital with SHIFT set, which is the case that
        // actually arrives; the plain-modifier pass above is the belt.
        let shifted = KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT);
        assert_eq!(action(shifted, cursored()), Action::Next);
        let shifted = KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT);
        assert_eq!(action(shifted, plain_mode()), Action::Right);
    }

    /// A capital is still a capital in the draft, not a movement.
    #[test]
    fn a_capital_is_typed_when_typing() {
        let shifted = KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT);
        assert_eq!(action(shifted, typing()), Action::Type('J'));
    }

    /// A letter is a letter while typing, and a movement when it is not.
    #[test]
    fn letters_are_modal() {
        assert_eq!(
            action(plain(KeyCode::Char('j')), typing()),
            Action::Type('j')
        );
        assert_eq!(action(plain(KeyCode::Char('j')), cursored()), Action::Next);
        assert_eq!(
            action(plain(KeyCode::Char('k')), cursored()),
            Action::Previous
        );
        assert_eq!(action(plain(KeyCode::Char('h')), cursored()), Action::Left);
        assert_eq!(action(plain(KeyCode::Char('l')), cursored()), Action::Right);
    }

    /// `j`/`k` move the thread cursor only where that cursor is drawn.
    ///
    /// `hint_today` promises `J/K a thread` in all four of the Today screen's
    /// focus states, and `aside::threads` draws the highlight in exactly one of
    /// them — so from the other three the keys moved something invisible. They
    /// are refused there instead, and `Tab panel` on the same rule is how the
    /// reader reaches the panel they belong to.
    #[test]
    fn the_thread_keys_only_move_a_cursor_that_is_drawn() {
        assert_eq!(action(plain(KeyCode::Char('j')), cursored()), Action::Next);
        assert_eq!(
            action(plain(KeyCode::Char('k')), cursored()),
            Action::Previous
        );
        for key in ['j', 'k'] {
            assert_eq!(
                action(plain(KeyCode::Char(key)), plain_mode()),
                Action::Ignore,
                "{key} moved a cursor nothing draws"
            );
        }
        // The arrows are unconditional and stay so: they are the design's
        // unambiguous pair, and `H`/`L` still move the week's day cursor from
        // any screen the same way.
        assert_eq!(action(plain(KeyCode::Down), plain_mode()), Action::Next);
        assert_eq!(action(plain(KeyCode::Up), plain_mode()), Action::Previous);
    }

    /// Enter sends. `Alt-Enter` is bound to nothing: it pushed a `'\n'` the
    /// buffer filters out, into a composer with one text row, so it could never
    /// show — and the rule that advertised it no longer does either.
    #[test]
    fn enter_sends_and_alt_enter_is_unbound() {
        assert_eq!(action(plain(KeyCode::Enter), typing()), Action::Send);
        for mode in all_modes() {
            assert_eq!(
                action(with(KeyCode::Enter, KeyModifiers::ALT), mode),
                Action::Ignore,
                "Alt-Enter is bound in {mode:?}"
            );
        }
    }

    /// And no key can put a control character in the draft, invisible or not.
    #[test]
    fn no_binding_types_a_control_character() {
        let codes = [
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Esc,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Home,
            KeyCode::F(1),
        ];
        for code in codes {
            for modifiers in [
                KeyModifiers::NONE,
                KeyModifiers::ALT,
                KeyModifiers::CONTROL,
                KeyModifiers::SHIFT,
            ] {
                for mode in all_modes() {
                    if let Action::Type(character) = action(with(code, modifiers), mode) {
                        assert!(
                            !character.is_control(),
                            "{code:?} with {modifiers:?} types {character:?}"
                        );
                    }
                }
            }
        }
    }

    /// A chord means the same thing mid-sentence as it does anywhere else.
    #[test]
    fn chords_survive_typing() {
        for mode in all_modes() {
            assert_eq!(
                action(with(KeyCode::Char('2'), KeyModifiers::CONTROL), mode),
                Action::ShowWeek
            );
            assert_eq!(
                action(with(KeyCode::Char('1'), KeyModifiers::CONTROL), mode),
                Action::ShowToday
            );
            assert_eq!(
                action(with(KeyCode::Char('c'), KeyModifiers::CONTROL), mode),
                Action::Quit
            );
        }
        // And a digit on its own is still a digit in the draft.
        assert_eq!(
            action(plain(KeyCode::Char('2')), typing()),
            Action::Type('2')
        );
    }

    /// Tab cycles panels from either mode — it is the design's own binding and
    /// there is no tab character in a draft.
    #[test]
    fn tab_cycles_from_either_mode() {
        for mode in all_modes() {
            assert_eq!(action(plain(KeyCode::Tab), mode), Action::NextPanel);
            assert_eq!(action(plain(KeyCode::BackTab), mode), Action::PreviousPanel);
        }
    }

    /// The arrows move the cursor whatever has focus.
    #[test]
    fn the_arrows_work_in_either_mode() {
        for mode in all_modes() {
            assert_eq!(action(plain(KeyCode::Down), mode), Action::Next);
            assert_eq!(action(plain(KeyCode::Up), mode), Action::Previous);
            assert_eq!(action(plain(KeyCode::Left), mode), Action::Left);
            assert_eq!(action(plain(KeyCode::Right), mode), Action::Right);
        }
    }

    /// Backspace edits the draft while typing and does nothing otherwise, so it
    /// cannot silently delete something on a navigation screen.
    #[test]
    fn backspace_only_edits_the_draft() {
        assert_eq!(
            action(plain(KeyCode::Backspace), typing()),
            Action::Backspace
        );
        assert_eq!(
            action(plain(KeyCode::Backspace), plain_mode()),
            Action::Ignore
        );
    }

    /// `q` quits when it is not a letter being typed — and types when it is.
    #[test]
    fn q_quits_only_outside_the_draft() {
        assert_eq!(
            action(plain(KeyCode::Char('q')), plain_mode()),
            Action::Quit
        );
        assert_eq!(
            action(plain(KeyCode::Char('q')), typing()),
            Action::Type('q')
        );
    }

    /// A key release is ignored, so terminals that report both press and release
    /// do not double every keystroke.
    #[test]
    fn releases_are_ignored() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert_eq!(action(release, typing()), Action::Ignore);
        let press =
            KeyEvent::new_with_kind(KeyCode::Char('a'), KeyModifiers::NONE, KeyEventKind::Press);
        assert_eq!(action(press, typing()), Action::Type('a'));
    }

    /// An unmapped key does nothing rather than falling through to something.
    #[test]
    fn unmapped_keys_do_nothing() {
        assert_eq!(action(plain(KeyCode::F(7)), typing()), Action::Ignore);
        assert_eq!(action(plain(KeyCode::Insert), plain_mode()), Action::Ignore);
        assert_eq!(
            action(with(KeyCode::Char('z'), KeyModifiers::CONTROL), cursored()),
            Action::Ignore
        );
    }

    /// An Alt-modified letter is refused rather than treated as the bare letter.
    ///
    /// `Alt-H/L` was on the artboards' bottom rule as "resize" and is bound to
    /// nothing; falling through moved the week's day cursor instead, which is a
    /// key doing something the user did not ask for. Alt now claims nothing at
    /// all, `Alt-Enter` included.
    #[test]
    fn alt_modified_keys_are_refused() {
        for letter in ['h', 'l', 'j', 'k', 'q'] {
            for mode in all_modes() {
                assert_eq!(
                    action(with(KeyCode::Char(letter), KeyModifiers::ALT), mode),
                    Action::Ignore,
                    "Alt-{letter} did something in {mode:?}"
                );
            }
        }
    }
}
