use super::types::ChatChunk;
use super::CompanionError;

#[derive(Debug, PartialEq, Eq)]
pub enum SseEvent {
    Data(String),
    Done,
}

pub struct SseParser {
    buf: Vec<u8>,
    data: String,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            data: String::new(),
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(end) = find_line_end(&self.buf) {
            let mut line: Vec<u8> = self.buf.drain(..end.drop).collect();
            line.truncate(end.content);
            self.handle_line(&line, &mut events);
        }
        events
    }

    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        if !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            self.handle_line(&line, &mut events);
        }
        if let Some(event) = self.take_event() {
            events.push(event);
        }
        events
    }

    fn handle_line(&mut self, line: &[u8], events: &mut Vec<SseEvent>) {
        if line.is_empty() {
            if let Some(event) = self.take_event() {
                events.push(event);
            }
            return;
        }
        if line.first() == Some(&b':') {
            return;
        }
        let Some(rest) = strip_prefix(line, b"data:") else {
            return;
        };
        let rest = match rest.first() {
            Some(b' ') => &rest[1..],
            _ => rest,
        };
        if !self.data.is_empty() {
            self.data.push('\n');
        }
        self.data.push_str(&String::from_utf8_lossy(rest));
    }

    fn take_event(&mut self) -> Option<SseEvent> {
        if self.data.is_empty() {
            return None;
        }
        let data = std::mem::take(&mut self.data);
        if data.trim() == "[DONE]" {
            Some(SseEvent::Done)
        } else {
            Some(SseEvent::Data(data))
        }
    }
}

struct LineEnd {
    content: usize,
    drop: usize,
}

fn find_line_end(buf: &[u8]) -> Option<LineEnd> {
    let n = buf.iter().position(|b| *b == b'\n')?;
    let content = if n > 0 && buf[n - 1] == b'\r' {
        n - 1
    } else {
        n
    };
    Some(LineEnd {
        content,
        drop: n + 1,
    })
}

fn strip_prefix<'a>(bytes: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    bytes.strip_prefix(prefix)
}

pub fn parse_chunk(data: &str) -> Result<ChatChunk, CompanionError> {
    serde_json::from_str(data).map_err(|_| CompanionError::InvalidResponse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_data_done_and_ignores_comments() {
        let mut parser = SseParser::new();
        let events = parser.push(b": comment\n\ndata: {\"choices\":[]}\n\ndata: [DONE]\n\n");
        assert_eq!(
            events,
            vec![SseEvent::Data("{\"choices\":[]}".into()), SseEvent::Done]
        );
    }

    #[test]
    fn splits_across_chunks_and_strips_crlf() {
        let mut parser = SseParser::new();
        assert!(parser.push(b"data: hel").is_empty());
        let events = parser.push(b"lo\r\n\r\n");
        assert_eq!(events, vec![SseEvent::Data("hello".into())]);
    }
}
