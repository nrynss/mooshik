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

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::app::Action;

/// Resolve `key` into an action.
///
/// `typing` says whether keystrokes should reach the draft — see
/// [`App::is_typing`](super::app::App::is_typing).
pub fn action(key: KeyEvent, typing: bool) -> Action {
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
    // asked for. Shift is not filtered: crossterm reports a capital as
    // `Char('J')` with the modifier set, and the design's hints are written `J/K`.
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
        KeyCode::Char('j') => Action::Next,
        KeyCode::Char('k') => Action::Previous,
        KeyCode::Char('h') => Action::Left,
        KeyCode::Char('l') => Action::Right,
        KeyCode::Char('q') => Action::Quit,
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

    /// A letter is a letter while typing, and a movement when it is not.
    #[test]
    fn letters_are_modal() {
        assert_eq!(action(plain(KeyCode::Char('j')), true), Action::Type('j'));
        assert_eq!(action(plain(KeyCode::Char('j')), false), Action::Next);
        assert_eq!(action(plain(KeyCode::Char('k')), false), Action::Previous);
        assert_eq!(action(plain(KeyCode::Char('h')), false), Action::Left);
        assert_eq!(action(plain(KeyCode::Char('l')), false), Action::Right);
    }

    /// Enter sends. `Alt-Enter` is bound to nothing: it pushed a `'\n'` the
    /// buffer filters out, into a composer with one text row, so it could never
    /// show — and the rule that advertised it no longer does either.
    #[test]
    fn enter_sends_and_alt_enter_is_unbound() {
        assert_eq!(action(plain(KeyCode::Enter), true), Action::Send);
        for typing in [true, false] {
            assert_eq!(
                action(with(KeyCode::Enter, KeyModifiers::ALT), typing),
                Action::Ignore,
                "Alt-Enter is bound while typing={typing}"
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
                for typing in [true, false] {
                    if let Action::Type(character) = action(with(code, modifiers), typing) {
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
        for typing in [true, false] {
            assert_eq!(
                action(with(KeyCode::Char('2'), KeyModifiers::CONTROL), typing),
                Action::ShowWeek
            );
            assert_eq!(
                action(with(KeyCode::Char('1'), KeyModifiers::CONTROL), typing),
                Action::ShowToday
            );
            assert_eq!(
                action(with(KeyCode::Char('c'), KeyModifiers::CONTROL), typing),
                Action::Quit
            );
        }
        // And a digit on its own is still a digit in the draft.
        assert_eq!(action(plain(KeyCode::Char('2')), true), Action::Type('2'));
    }

    /// Tab cycles panels from either mode — it is the design's own binding and
    /// there is no tab character in a draft.
    #[test]
    fn tab_cycles_from_either_mode() {
        for typing in [true, false] {
            assert_eq!(action(plain(KeyCode::Tab), typing), Action::NextPanel);
            assert_eq!(
                action(plain(KeyCode::BackTab), typing),
                Action::PreviousPanel
            );
        }
    }

    /// The arrows move the cursor whatever has focus.
    #[test]
    fn the_arrows_work_in_either_mode() {
        for typing in [true, false] {
            assert_eq!(action(plain(KeyCode::Down), typing), Action::Next);
            assert_eq!(action(plain(KeyCode::Up), typing), Action::Previous);
            assert_eq!(action(plain(KeyCode::Left), typing), Action::Left);
            assert_eq!(action(plain(KeyCode::Right), typing), Action::Right);
        }
    }

    /// Backspace edits the draft while typing and does nothing otherwise, so it
    /// cannot silently delete something on a navigation screen.
    #[test]
    fn backspace_only_edits_the_draft() {
        assert_eq!(action(plain(KeyCode::Backspace), true), Action::Backspace);
        assert_eq!(action(plain(KeyCode::Backspace), false), Action::Ignore);
    }

    /// `q` quits when it is not a letter being typed — and types when it is.
    #[test]
    fn q_quits_only_outside_the_draft() {
        assert_eq!(action(plain(KeyCode::Char('q')), false), Action::Quit);
        assert_eq!(action(plain(KeyCode::Char('q')), true), Action::Type('q'));
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
        assert_eq!(action(release, true), Action::Ignore);
        let press =
            KeyEvent::new_with_kind(KeyCode::Char('a'), KeyModifiers::NONE, KeyEventKind::Press);
        assert_eq!(action(press, true), Action::Type('a'));
    }

    /// An unmapped key does nothing rather than falling through to something.
    #[test]
    fn unmapped_keys_do_nothing() {
        assert_eq!(action(plain(KeyCode::F(7)), true), Action::Ignore);
        assert_eq!(action(plain(KeyCode::Insert), false), Action::Ignore);
        assert_eq!(
            action(with(KeyCode::Char('z'), KeyModifiers::CONTROL), false),
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
            for typing in [true, false] {
                assert_eq!(
                    action(with(KeyCode::Char(letter), KeyModifiers::ALT), typing),
                    Action::Ignore,
                    "Alt-{letter} did something while typing={typing}"
                );
            }
        }
    }
}
