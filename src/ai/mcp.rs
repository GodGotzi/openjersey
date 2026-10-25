//! `openjersey mcp`: a stdio MCP server for ChatGPT Codex and Claude Code.
//! It lists the editor tools and forwards calls to the running OpenJersey
//! window (starting it if needed), so the assistant edits the design you
//! see.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::commands;
use super::server::{session_path, Session};

const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

pub fn run() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut app: Option<Connection> = None;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = msg.get("id").cloned() else { continue }; // notifications need no answer
        let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result = match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOLS[0]);
                let version = if PROTOCOLS.contains(&asked) { asked } else { PROTOCOLS[0] };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "openjersey", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": commands::INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": commands::specs().into_iter().map(|t| json!({
                    "name": t.name, "description": t.description, "inputSchema": t.schema,
                })).collect::<Vec<_>>()
            })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                Ok(call(&mut app, name, &args))
            }
            other => Err(json!({"code": -32601, "message": format!("unknown method {other}")})),
        };
        let response = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
        };
        if writeln!(stdout, "{response}").and_then(|_| stdout.flush()).is_err() {
            break;
        }
    }
}

struct Connection {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    token: String,
}

/// A tool call as MCP content; failures are reported to the model, not as
/// protocol errors.
fn call(app: &mut Option<Connection>, name: &str, args: &Value) -> Value {
    let reply = (0..2).find_map(|_| {
        if app.is_none() {
            *app = connect();
        }
        let conn = app.as_mut()?;
        let request = json!({"token": conn.token, "tool": name, "args": args});
        let mut line = String::new();
        let ok = writeln!(conn.stream, "{request}").is_ok() && conn.reader.read_line(&mut line).is_ok_and(|n| n > 0);
        if !ok {
            *app = None; // the app was closed; reconnect once
            return None;
        }
        serde_json::from_str::<Value>(&line).ok()
    });
    let Some(reply) = reply else {
        return json!({"isError": true, "content": [{"type": "text", "text": "OpenJersey could not be reached or started."}]});
    };
    if reply.get("ok").and_then(Value::as_bool) != Some(true) {
        let err = reply.get("error").and_then(Value::as_str).unwrap_or("failed");
        return json!({"isError": true, "content": [{"type": "text", "text": err}]});
    }
    let mut content = vec![json!({"type": "text", "text": reply.get("text").and_then(Value::as_str).unwrap_or("")})];
    for img in reply.get("images").and_then(Value::as_array).into_iter().flatten() {
        content.push(json!({"type": "image", "data": img, "mimeType": "image/png"}));
    }
    json!({"content": content})
}

/// Connects to the running app, starting it when there is none.
fn connect() -> Option<Connection> {
    if let Some(c) = try_connect() {
        return Some(c);
    }
    let exe = std::env::current_exe().ok()?;
    std::process::Command::new(exe)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(300));
        if let Some(c) = try_connect() {
            return Some(c);
        }
    }
    None
}

fn try_connect() -> Option<Connection> {
    let session: Session = serde_json::from_slice(&std::fs::read(session_path()).ok()?).ok()?;
    let stream = TcpStream::connect(("127.0.0.1", session.port)).ok()?;
    let reader = BufReader::new(stream.try_clone().ok()?);
    Some(Connection { stream, reader, token: session.token })
}
