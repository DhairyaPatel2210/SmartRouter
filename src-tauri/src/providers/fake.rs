//! A tiny local HTTP server imitating an OpenAI-compatible API (`/v1/models`,
//! `/v1/chat/completions` incl. streaming and tool calls). Used by tests,
//! and via `<app> --fake-provider <port>` to record adapter fixtures without
//! calling a real cloud model.
//!
//! Behaviour: when the request offers a `write` tool and no tool result has
//! come back yet, it answers with a tool call that writes `hello.txt`;
//! otherwise it replies with a short final message.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const FAKE_KEY: &str = "sk-fake-test-key";

#[derive(Clone, Default)]
pub struct FakeStats {
    pub chat_requests: Arc<AtomicUsize>,
}

pub struct FakeServer {
    pub port: u16,
    pub stats: FakeStats,
    handle: tokio::task::JoinHandle<()>,
}

impl FakeServer {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

pub async fn start(port: u16) -> std::io::Result<FakeServer> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let port = listener.local_addr()?.port();
    let stats = FakeStats::default();
    let st = stats.clone();
    let handle = tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else { break };
            let st = st.clone();
            tokio::spawn(async move {
                let _ = handle_conn(sock, st).await;
            });
        }
    });
    Ok(FakeServer { port, stats, handle })
}

async fn handle_conn(mut sock: TcpStream, stats: FakeStats) -> std::io::Result<()> {
    loop {
        let Some((method, path, headers, body)) = read_request(&mut sock).await? else { return Ok(()) };
        let auth = headers
            .iter()
            .find(|(k, _)| k == "authorization" || k == "x-api-key")
            .map(|(_, v)| v.trim_start_matches("Bearer ").to_string())
            .unwrap_or_default();
        let authorized = !auth.is_empty() && auth != "bad";
        match (method.as_str(), path.split('?').next().unwrap_or("")) {
            (_, p) if !authorized && p != "/v1/models" => {
                respond(&mut sock, 401, "application/json", r#"{"error":{"message":"invalid api key"}}"#).await?;
            }
            ("GET", "/v1/models") | ("GET", "/models") => {
                let body = r#"{"object":"list","data":[
                    {"id":"fake-coder","object":"model","name":"Fake Coder","context_length":32768,
                     "pricing":{"prompt":"0.0000002","completion":"0.0000006"},"supported_parameters":["tools","temperature"]},
                    {"id":"fake-mini","object":"model","name":"Fake Mini","context_length":8192,
                     "pricing":{"prompt":"0.00000005","completion":"0.0000001"},"supported_parameters":["temperature"]}
                ]}"#;
                respond(&mut sock, 200, "application/json", body).await?;
            }
            ("GET", "/v1/key") | ("GET", "/api/v1/key") => {
                respond(&mut sock, 200, "application/json", r#"{"data":{"label":"fake","usage":0,"limit":null}}"#).await?;
            }
            ("POST", "/v1/chat/completions") | ("POST", "/chat/completions") => {
                stats.chat_requests.fetch_add(1, Ordering::SeqCst);
                let req: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
                chat(&mut sock, &req).await?;
            }
            _ => respond(&mut sock, 404, "application/json", r#"{"error":"not found"}"#).await?,
        }
        if headers.iter().any(|(k, v)| k == "connection" && v.eq_ignore_ascii_case("close")) {
            return Ok(());
        }
    }
}

async fn chat(sock: &mut TcpStream, req: &serde_json::Value) -> std::io::Result<()> {
    let stream = req.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let model = req.get("model").and_then(|m| m.as_str()).unwrap_or("fake-coder").to_string();
    let messages = req.get("messages").and_then(|m| m.as_array()).cloned().unwrap_or_default();
    let had_tool_result = messages.iter().any(|m| m.get("role").and_then(|r| r.as_str()) == Some("tool"));
    let write_tool = req.get("tools").and_then(|t| t.as_array()).and_then(|tools| {
        tools.iter().find_map(|t| {
            let name = t.pointer("/function/name").and_then(|n| n.as_str())?;
            (name == "write" || name == "write_file" || name == "Write").then(|| name.to_string())
        })
    });
    let cwd = messages.iter().find_map(|m| {
        let c = m.get("content")?.as_str()?;
        let i = c.find("Working directory: ")?;
        Some(c[i + 19..].lines().next()?.trim().to_string())
    });

    let tool_call = match (&write_tool, had_tool_result) {
        (Some(name), false) => {
            let path = cwd.map(|d| format!("{d}/hello.txt")).unwrap_or_else(|| "hello.txt".into());
            let args = serde_json::json!({ "filePath": path, "path": path, "content": "hello from the fake provider\n" });
            Some((name.clone(), args.to_string()))
        }
        _ => None,
    };
    let text = "Done. I wrote hello.txt as asked.";
    let usage = serde_json::json!({"prompt_tokens": 420, "completion_tokens": 24, "total_tokens": 444});

    if !stream {
        let message = match &tool_call {
            Some((name, args)) => serde_json::json!({"role":"assistant","content":null,
                "tool_calls":[{"id":"call_1","type":"function","function":{"name":name,"arguments":args}}]}),
            None => serde_json::json!({"role":"assistant","content":text}),
        };
        let finish = if tool_call.is_some() { "tool_calls" } else { "stop" };
        let body = serde_json::json!({"id":"chatcmpl-fake","object":"chat.completion","created":0,"model":model,
            "choices":[{"index":0,"message":message,"finish_reason":finish}],"usage":usage});
        return respond(sock, 200, "application/json", &body.to_string()).await;
    }

    let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n";
    sock.write_all(head.as_bytes()).await?;
    let chunk = |delta: serde_json::Value, finish: Option<&str>| {
        serde_json::json!({"id":"chatcmpl-fake","object":"chat.completion.chunk","created":0,"model":model,
            "choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
    };
    let mut events = vec![chunk(serde_json::json!({"role":"assistant","content":""}), None)];
    match &tool_call {
        Some((name, args)) => {
            events.push(chunk(
                serde_json::json!({"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":name,"arguments":args}}]}),
                None,
            ));
            events.push(chunk(serde_json::json!({}), Some("tool_calls")));
        }
        None => {
            for w in text.split_inclusive(' ') {
                events.push(chunk(serde_json::json!({"content": w}), None));
            }
            events.push(chunk(serde_json::json!({}), Some("stop")));
        }
    }
    let mut last = events.pop().unwrap();
    last["usage"] = usage;
    events.push(last);
    for e in events {
        sock.write_all(format!("data: {e}\n\n").as_bytes()).await?;
    }
    sock.write_all(b"data: [DONE]\n\n").await?;
    sock.flush().await?;
    sock.shutdown().await
}

type Request = (String, String, Vec<(String, String)>, Vec<u8>);

async fn read_request(sock: &mut TcpStream) -> std::io::Result<Option<Request>> {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 1 << 20 {
            return Ok(None);
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let headers: Vec<(String, String)> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let len: usize = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    let mut body = buf[header_end..].to_vec();
    while body.len() < len {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    Ok(Some((method, path, headers, body)))
}

async fn respond(sock: &mut TcpStream, code: u16, ctype: &str, body: &str) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Error",
    };
    let msg = format!("HTTP/1.1 {code} {reason}\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\n\r\n{body}", body.len());
    sock.write_all(msg.as_bytes()).await
}
