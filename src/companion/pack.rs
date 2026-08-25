use super::types::{Message, Role};
use super::CompanionError;

/// Conservative stand-in for a tokenizer: ceil(chars / 4), plus per-message overhead.
pub const CHARS_PER_TOKEN: usize = 4;
const PER_MESSAGE_OVERHEAD: usize = 8;

pub fn estimate_tokens(chars: usize) -> usize {
    if chars == 0 {
        PER_MESSAGE_OVERHEAD
    } else {
        chars.div_ceil(CHARS_PER_TOKEN) + PER_MESSAGE_OVERHEAD
    }
}

pub fn message_tokens(message: &Message) -> usize {
    estimate_tokens(message.token_chars())
}

pub trait RecallInjector: Send + Sync {
    fn inject(&self, dropped: &[Message], current_user: &str) -> Option<Message>;
}

pub struct NoopRecall;

impl RecallInjector for NoopRecall {
    fn inject(&self, _dropped: &[Message], _current_user: &str) -> Option<Message> {
        None
    }
}

#[derive(Debug)]
pub struct Packed {
    pub messages: Vec<Message>,
    pub dropped: Vec<Message>,
}

pub fn pack_messages<R: RecallInjector>(
    history: &[Message],
    window_tokens: u32,
    injector: &R,
) -> Result<Packed, CompanionError> {
    let window = window_tokens as usize;
    let (system, mut groups) = split_groups(history);
    if groups.is_empty() {
        return fit(system, Vec::new(), Vec::new(), window, "", injector);
    }
    let current_user = groups
        .last()
        .and_then(|group| group.iter().find(|m| m.role == Role::User))
        .map(|m| m.content.as_str())
        .unwrap_or("")
        .to_owned();
    let mut dropped = Vec::new();
    while groups.len() > 1 && tokens_of(&system, &groups) > window {
        dropped.extend(groups.remove(0));
    }
    if tokens_of(&system, &groups) > window {
        return Err(CompanionError::TurnTooLarge);
    }
    fit(system, groups, dropped, window, &current_user, injector)
}

fn fit<R: RecallInjector>(
    system: Vec<Message>,
    groups: Vec<Vec<Message>>,
    dropped: Vec<Message>,
    window: usize,
    current_user: &str,
    injector: &R,
) -> Result<Packed, CompanionError> {
    let rest: Vec<Message> = groups.into_iter().flatten().collect();
    let injection = if dropped.is_empty() {
        None
    } else {
        injector.inject(&dropped, current_user)
    };
    let mut messages = system.clone();
    if let Some(extra) = injection {
        let mut with_recall = system;
        with_recall.push(extra);
        with_recall.extend(rest.iter().cloned());
        if total_tokens(&with_recall) <= window {
            return Ok(Packed {
                messages: with_recall,
                dropped,
            });
        }
    }
    messages.extend(rest);
    if total_tokens(&messages) > window {
        return Err(CompanionError::TurnTooLarge);
    }
    Ok(Packed { messages, dropped })
}

fn split_groups(history: &[Message]) -> (Vec<Message>, Vec<Vec<Message>>) {
    let mut idx = 0;
    let mut system = Vec::new();
    while idx < history.len() && history[idx].role == Role::System {
        system.push(history[idx].clone());
        idx += 1;
    }
    let mut groups = Vec::new();
    let mut current = Vec::new();
    for message in &history[idx..] {
        if message.role == Role::User && !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
        current.push(message.clone());
    }
    if !current.is_empty() {
        groups.push(current);
    }
    (system, groups)
}

fn tokens_of(system: &[Message], groups: &[Vec<Message>]) -> usize {
    system.iter().map(message_tokens).sum::<usize>()
        + groups
            .iter()
            .flat_map(|g| g.iter())
            .map(message_tokens)
            .sum::<usize>()
}

fn total_tokens(messages: &[Message]) -> usize {
    messages.iter().map(message_tokens).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Recording {
        dropped: Mutex<Vec<Vec<Message>>>,
        users: Mutex<Vec<String>>,
    }

    impl RecallInjector for Recording {
        fn inject(&self, dropped: &[Message], current_user: &str) -> Option<Message> {
            self.dropped.lock().unwrap().push(dropped.to_vec());
            self.users.lock().unwrap().push(current_user.to_owned());
            None
        }
    }

    #[test]
    fn context_pressure_drops_oldest_turns_and_invokes_injector() {
        let history = vec![
            Message::system("s"),
            Message::user("UNIQUE_OLD_TURN_xyz"),
            Message::assistant("old-reply", Vec::new()),
            Message::user("current-turn"),
        ];
        let window = (message_tokens(&history[0]) + message_tokens(&history[3])) as u32;
        let recorder = Recording {
            dropped: Mutex::new(Vec::new()),
            users: Mutex::new(Vec::new()),
        };
        let packed = pack_messages(&history, window, &recorder).unwrap();
        let contents: Vec<&str> = packed.messages.iter().map(|m| m.content.as_str()).collect();
        assert!(!contents.iter().any(|c| c.contains("UNIQUE_OLD_TURN_xyz")));
        assert!(!contents.iter().any(|c| c.contains("old-reply")));
        assert!(contents.contains(&"current-turn"));
        let dropped = recorder.dropped.lock().unwrap();
        assert_eq!(dropped.len(), 1);
        assert!(dropped[0]
            .iter()
            .any(|m| m.content.contains("UNIQUE_OLD_TURN_xyz")));
        assert_eq!(recorder.users.lock().unwrap().as_slice(), ["current-turn"]);
        assert!(packed
            .dropped
            .iter()
            .any(|m| m.content.contains("UNIQUE_OLD_TURN_xyz")));
    }

    #[test]
    fn packing_does_not_summarize_dropped_turns() {
        let history = vec![
            Message::system("s"),
            Message::user("alpha-secret-turn"),
            Message::assistant("beta-secret-turn", Vec::new()),
            Message::user("now"),
        ];
        let summary = Message::system("summary: alpha-secret-turn beta-secret-turn");
        let window = (message_tokens(&history[0])
            + message_tokens(&history[3])
            + message_tokens(&summary)) as u32;
        let packed = pack_messages(&history, window, &NoopRecall).unwrap();
        let joined = packed
            .messages
            .iter()
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!joined.contains("alpha-secret-turn"));
        assert!(!joined.contains("beta-secret-turn"));
        assert!(!joined.contains("summary"));
    }

    #[test]
    fn current_user_turn_that_exceeds_window_fails() {
        let system = Message::system("s");
        let huge = Message::user("x".repeat(400));
        let history = vec![system.clone(), huge.clone()];
        let window = message_tokens(&system) as u32;
        assert!(message_tokens(&system) <= window as usize);
        assert!(message_tokens(&system) + message_tokens(&huge) > window as usize);
        let err = pack_messages(&history, window, &NoopRecall).unwrap_err();
        assert!(matches!(err, CompanionError::TurnTooLarge));
    }

    struct MarkerRecall;

    impl RecallInjector for MarkerRecall {
        fn inject(&self, dropped: &[Message], _current_user: &str) -> Option<Message> {
            assert!(!dropped.is_empty());
            Some(Message::system("RECALL_MARKER"))
        }
    }

    #[test]
    fn injector_some_is_packed_and_dropped_turns_stay_out() {
        let history = vec![
            Message::system("s"),
            Message::user("UNIQUE_OLD_TURN_xyz"),
            Message::assistant("old-reply", Vec::new()),
            Message::user("now"),
        ];
        let recall = Message::system("RECALL_MARKER");
        let window = (message_tokens(&history[0])
            + message_tokens(&history[3])
            + message_tokens(&recall)) as u32;
        let packed = pack_messages(&history, window, &MarkerRecall).unwrap();
        let contents: Vec<&str> = packed.messages.iter().map(|m| m.content.as_str()).collect();
        assert!(contents.contains(&"RECALL_MARKER"));
        assert!(contents.contains(&"now"));
        assert!(!contents.iter().any(|c| c.contains("UNIQUE_OLD_TURN_xyz")));
        assert!(!contents.iter().any(|c| c.contains("old-reply")));
    }
}
