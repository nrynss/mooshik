use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct Captured {
    pub path: String,
    pub body: String,
    pub authorization: Option<String>,
}

pub struct Frame {
    pub delay: Duration,
    pub data: String,
}

impl Frame {
    pub fn data(payload: &str) -> Self {
        Self {
            delay: Duration::ZERO,
            data: format!("data: {payload}\n\n"),
        }
    }

    pub fn done() -> Self {
        Self {
            delay: Duration::ZERO,
            data: "data: [DONE]\n\n".into(),
        }
    }

    pub fn content(text: &str) -> Self {
        Self::data(&serde_json::json!({"choices":[{"delta":{"content":text}}]}).to_string())
    }

    /// Envelope a real OpenAI-compat endpoint sends; extra keys must not fail parse.
    pub fn content_openai(text: &str) -> Self {
        Self::data(
            &serde_json::json!({
                "id": "chatcmpl-x",
                "object": "chat.completion.chunk",
                "model": "local-model",
                "choices": [{
                    "index": 0,
                    "delta": {"role": "assistant", "content": text},
                    "finish_reason": null
                }],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 1,
                    "total_tokens": 2
                }
            })
            .to_string(),
        )
    }

    pub fn content_delayed(delay: Duration, text: &str) -> Self {
        let mut frame = Self::content(text);
        frame.delay = delay;
        frame
    }

    pub fn finish(reason: &str) -> Self {
        Self::data(
            &serde_json::json!({"choices":[{"delta":{},"finish_reason":reason}]}).to_string(),
        )
    }

    pub fn tool_head(index: usize, id: &str, name: &str) -> Self {
        Self::data(
            &serde_json::json!({
                "choices":[{
                    "delta":{"tool_calls":[{
                        "index": index,
                        "id": id,
                        "type": "function",
                        "function": {"name": name, "arguments": ""}
                    }]}
                }]
            })
            .to_string(),
        )
    }

    pub fn tool_args(index: usize, chunk: &str) -> Self {
        Self::data(
            &serde_json::json!({
                "choices":[{
                    "delta":{"tool_calls":[{
                        "index": index,
                        "function": {"arguments": chunk}
                    }]}
                }]
            })
            .to_string(),
        )
    }
}

pub struct Script {
    pub status: u16,
    pub frames: Vec<Frame>,
    pub body: Option<String>,
}

impl Script {
    pub fn sse(frames: Vec<Frame>) -> Self {
        Self {
            status: 200,
            frames,
            body: None,
        }
    }

    pub fn error(status: u16, body: &str) -> Self {
        Self {
            status,
            frames: Vec::new(),
            body: Some(body.to_owned()),
        }
    }
}

pub struct MockServer {
    pub base_url: String,
    pub captured: Arc<Mutex<Vec<Captured>>>,
    pub aborted: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl MockServer {
    pub async fn spawn(scripts: Vec<Script>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let aborted = Arc::new(AtomicBool::new(false));
        let queue = Arc::new(Mutex::new(VecDeque::from(scripts)));
        let captured_bg = captured.clone();
        let aborted_bg = aborted.clone();
        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let queue = queue.clone();
                let captured = captured_bg.clone();
                let aborted = aborted_bg.clone();
                tokio::spawn(async move {
                    handle_conn(stream, queue, captured, aborted).await;
                });
            }
        });
        Self {
            base_url: format!("http://{addr}/v1"),
            captured,
            aborted,
            handle,
        }
    }

    pub fn requests(&self) -> Vec<Captured> {
        self.captured.lock().unwrap().clone()
    }

    pub fn aborted(&self) -> bool {
        self.aborted.load(Ordering::SeqCst)
    }

    pub fn assert_all_streaming(&self) {
        for req in self.requests() {
            let body: serde_json::Value = serde_json::from_str(&req.body).unwrap();
            assert_eq!(
                body.get("stream"),
                Some(&serde_json::Value::Bool(true)),
                "{}",
                req.body
            );
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn handle_conn(
    mut stream: TcpStream,
    queue: Arc<Mutex<VecDeque<Script>>>,
    captured: Arc<Mutex<Vec<Captured>>>,
    aborted: Arc<AtomicBool>,
) {
    let Ok(req) = read_http(&mut stream).await else {
        return;
    };
    captured.lock().unwrap().push(req.clone());
    if !is_stream_true(&req.body) {
        let _ = stream
            .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 24\r\nConnection: close\r\n\r\n{\"error\":\"stream false\"}")
            .await;
        return;
    }
    let script = queue.lock().unwrap().pop_front();
    let Some(script) = script else {
        return;
    };
    if let Some(body) = script.body {
        let head = format!(
            "HTTP/1.1 {} ERR\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            script.status,
            body.len()
        );
        if stream.write_all(head.as_bytes()).await.is_err()
            || stream.write_all(body.as_bytes()).await.is_err()
        {
            aborted.store(true, Ordering::SeqCst);
        }
        return;
    }
    let head = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        script.status
    );
    if stream.write_all(head.as_bytes()).await.is_err() {
        aborted.store(true, Ordering::SeqCst);
        return;
    }
    for frame in script.frames {
        if !frame.delay.is_zero() {
            tokio::time::sleep(frame.delay).await;
        }
        if stream.write_all(frame.data.as_bytes()).await.is_err() || stream.flush().await.is_err() {
            aborted.store(true, Ordering::SeqCst);
            return;
        }
    }
}

async fn read_http(stream: &mut TcpStream) -> std::io::Result<Captured> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&buf[..pos]);
            let mut path = String::new();
            if let Some(line) = headers.lines().next() {
                let mut parts = line.split_whitespace();
                let _ = parts.next();
                if let Some(value) = parts.next() {
                    path = value.to_owned();
                }
            }
            let mut authorization = None;
            let mut content_length = 0usize;
            for line in headers.lines() {
                let Some((name, value)) = line.split_once(':') else {
                    continue;
                };
                if name.eq_ignore_ascii_case("authorization") {
                    authorization = Some(value.trim().to_owned());
                }
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            let mut body = buf[pos + 4..].to_vec();
            while body.len() < content_length {
                let n = stream.read(&mut tmp).await?;
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&tmp[..n]);
            }
            body.truncate(content_length);
            return Ok(Captured {
                path,
                body: String::from_utf8_lossy(&body).into_owned(),
                authorization,
            });
        }
        if buf.len() > 64 * 1024 {
            break;
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "eof",
    ))
}

fn is_stream_true(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("stream").and_then(serde_json::Value::as_bool))
        == Some(true)
}
