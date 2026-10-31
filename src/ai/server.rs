//! The running app accepts tool calls on a local socket, so the MCP bridge
//! (`openjersey mcp`, used by Codex and Claude Code) can edit the open design
//! live. Protocol: one JSON object per line, `{"token", "client", "tool", "args"}` in,
//! `{"ok", "text", "images" (base64 PNGs) | "error"}` out. Only localhost, and
//! a random token from the session file (readable by this user only) is
//! required.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::commands::Output;

/// A tool call waiting for the UI thread.
pub struct Request {
    /// Who is calling: the MCP client's name and version, as it introduced itself.
    pub client: String,
    pub tool: String,
    pub args: Value,
    pub reply: Sender<Result<Output, String>>,
}

/// What the socket tells the UI thread: tool calls, and who comes and goes.
pub enum Incoming {
    Connected,
    Disconnected,
    /// A message that was turned away (a wrong token, a line that isn't JSON).
    Rejected(String),
    Call(Request),
}

/// The tool, arguments and caller of a message, if it is a call.
pub fn parse_call(msg: &Value) -> (String, Value, String) {
    let text = |key: &str| msg.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    (text("tool"), msg.get("args").cloned().unwrap_or(json!({})), text("client"))
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

/// Starts listening; what happens at the socket arrives on the returned channel.
pub fn start() -> Receiver<Incoming> {
    let (tx, rx) = mpsc::channel::<Incoming>();
    let Ok(listener) = TcpListener::bind("127.0.0.1:0") else { return rx };
    let Ok(port) = listener.local_addr().map(|a| a.port()) else { return rx };
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        return rx;
    }
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let path = session_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let session = Session { port, token: token.clone(), pid: std::process::id() };
    if write_private(&path, &serde_json::to_vec(&session).unwrap_or_default()).is_err() {
        return rx;
    }
    let accept_tx = tx.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = accept_tx.clone();
            let token = token.clone();
            std::thread::spawn(move || serve(stream, &token, tx));
        }
    });
    rx
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

fn serve(stream: TcpStream, token: &str, tx: Sender<Incoming>) {
    let _ = tx.send(Incoming::Connected);
    serve_lines(stream, token, &tx);
    let _ = tx.send(Incoming::Disconnected);
}

fn serve_lines(stream: TcpStream, token: &str, tx: &Sender<Incoming>) {
    let Ok(mut out) = stream.try_clone() else { return };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) if msg.get("token").and_then(Value::as_str) == Some(token) => {
                let (rtx, rrx) = mpsc::channel();
                let (tool, args, client) = parse_call(&msg);
                if tx.send(Incoming::Call(Request { client, tool, args, reply: rtx })).is_err() {
                    return;
                }
                match rrx.recv() {
                    Ok(result) => encode(result),
                    Err(_) => json!({"ok": false, "error": "app closed"}),
                }
            }
            Ok(_) => {
                let _ = tx.send(Incoming::Rejected("a request with a wrong token".into()));
                json!({"ok": false, "error": "bad token"})
            }
            Err(e) => {
                let _ = tx.send(Incoming::Rejected(format!("a line that is not JSON ({e})")));
                json!({"ok": false, "error": e.to_string()})
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_names_its_tool_arguments_and_caller() {
        let msg = json!({"token": "t", "client": "Claude Code 2.1", "tool": "add_text", "args": {"text": "HI"}});
        assert_eq!(parse_call(&msg), ("add_text".to_string(), json!({"text": "HI"}), "Claude Code 2.1".to_string()));
        // a caller that doesn't introduce itself, and a call without arguments
        let bare = json!({"token": "t", "tool": "get_overview"});
        assert_eq!(parse_call(&bare), ("get_overview".to_string(), json!({}), String::new()));
    }
}
