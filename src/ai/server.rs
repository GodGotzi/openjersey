//! The running app accepts tool calls on a local socket, so the MCP bridge
//! (`openjersey mcp`, used by Codex and Claude Code) can edit the open design
//! live. Protocol: one JSON object per line, `{"token", "tool", "args"}` in,
//! `{"ok", "text", "images" (base64 PNGs) | "error"}` out. Only localhost, and
//! a random token from the session file (readable by this user only) is
//! required.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::commands::Output;

/// A tool call waiting for the UI thread.
pub struct Request {
    pub tool: String,
    pub args: Value,
    pub reply: Sender<Result<Output, String>>,
}

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

pub fn session_path() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("openjersey").join("session.json")
}

/// Starts listening; tool calls arrive on the returned channel. The sender
/// is handed out too, so the in-app chat can queue calls the same way.
pub fn start() -> (Sender<Request>, Receiver<Request>) {
    let (tx, rx) = mpsc::channel::<Request>();
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else { return (tx, rx) };
    let Ok(port) = listener.local_addr().map(|a| a.port()) else { return (tx, rx) };
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        return (tx, rx);
    }
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let path = session_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let session = Session { port, token: token.clone(), pid: std::process::id() };
    if write_private(&path, &serde_json::to_vec(&session).unwrap_or_default()).is_err() {
        return (tx, rx);
    }
    let accept_tx = tx.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = accept_tx.clone();
            let token = token.clone();
            std::thread::spawn(move || serve(stream, &token, tx));
        }
    });
    (tx, rx)
}

fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        f.write_all(data)
    }
    #[cfg(not(unix))]
    std::fs::write(path, data)
}

/// Removes the session file if it still belongs to this process.
pub fn stop() {
    let path = session_path();
    if let Ok(bytes) = std::fs::read(&path)
        && let Ok(s) = serde_json::from_slice::<Session>(&bytes)
        && s.pid == std::process::id()
    {
        let _ = std::fs::remove_file(path);
    }
}

fn serve(stream: TcpStream, token: &str, tx: Sender<Request>) {
    let Ok(mut out) = stream.try_clone() else { return };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) if msg.get("token").and_then(Value::as_str) == Some(token) => {
                let (rtx, rrx) = mpsc::channel();
                let request = Request {
                    tool: msg.get("tool").and_then(Value::as_str).unwrap_or_default().to_string(),
                    args: msg.get("args").cloned().unwrap_or(json!({})),
                    reply: rtx,
                };
                if tx.send(request).is_err() {
                    return;
                }
                match rrx.recv() {
                    Ok(result) => encode(result),
                    Err(_) => json!({"ok": false, "error": "app closed"}),
                }
            }
            Ok(_) => json!({"ok": false, "error": "bad token"}),
            Err(e) => json!({"ok": false, "error": e.to_string()}),
        };
        if writeln!(out, "{reply}").is_err() {
            return;
        }
    }
}

pub fn encode(result: Result<Output, String>) -> Value {
    match result {
        Ok(o) => json!({
            "ok": true,
            "text": o.text,
            "images": o.images.iter().map(|png| base64::engine::general_purpose::STANDARD.encode(png)).collect::<Vec<_>>(),
        }),
        Err(e) => json!({"ok": false, "error": e}),
    }
}
