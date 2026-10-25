//! The in-app assistant: a tool-use loop against the Claude (Messages API) or
//! OpenAI (Responses API) HTTP API, using the same tools as the MCP server.
//! It runs on a background thread; tool calls are queued to the UI thread
//! like the MCP bridge's.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::commands::{self, Output};
use super::server::Request;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Claude,
    OpenAi,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude",
            Provider::OpenAi => "OpenAI",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub provider: Provider,
    pub claude_model: String,
    pub openai_model: String,
    /// Keys entered in the app; the environment variables win.
    #[serde(default)]
    pub anthropic_key: String,
    #[serde(default)]
    pub openai_key: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: Provider::Claude,
            claude_model: "claude-opus-5-5".into(),
            openai_model: "gpt-6-astra".into(),
            anthropic_key: String::new(),
            openai_key: String::new(),
        }
    }
}

fn settings_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("openjersey").join("settings.json")
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read(settings_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Saved readable by this user only (it may hold API keys).
    pub fn save(&self) {
        let path = settings_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let data = serde_json::to_vec_pretty(self).unwrap_or_default();
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path) {
                let _ = f.write_all(&data);
            }
        }
        #[cfg(not(unix))]
        let _ = std::fs::write(path, data);
    }

    pub fn key(&self, provider: Provider) -> Option<String> {
        let (env, saved) = match provider {
            Provider::Claude => ("ANTHROPIC_API_KEY", &self.anthropic_key),
            Provider::OpenAi => ("OPENAI_API_KEY", &self.openai_key),
        };
        std::env::var(env).ok().filter(|k| !k.is_empty()).or_else(|| (!saved.is_empty()).then(|| saved.clone()))
    }
}

pub enum Event {
    Text(String),
    Tool(String),
    Error(String),
    Done,
}

/// One conversation; the transcript is kept in the provider's own format so
/// it can be sent back unchanged.
pub struct Conversation {
    pub provider: Provider,
    claude_messages: Vec<Value>,
    openai_previous: Option<String>,
}

pub struct Running {
    pub events: Receiver<Event>,
    pub stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<Conversation>>,
}

impl Running {
    /// The conversation back, once the turn is over.
    pub fn finish(mut self) -> Option<Conversation> {
        self.handle.take()?.join().ok()
    }

    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().is_none_or(|h| h.is_finished())
    }
}

impl Conversation {
    pub fn new(provider: Provider) -> Self {
        Self { provider, claude_messages: Vec::new(), openai_previous: None }
    }

    /// Runs one user turn (with all tool calls) in the background.
    pub fn send(mut self, settings: &Settings, user_text: String, context: String, tools: Sender<Request>) -> Running {
        let (tx, events) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let settings = settings.clone();
        let stop2 = stop.clone();
        let handle = std::thread::spawn(move || {
            let result = match settings.key(self.provider) {
                None => Err(format!(
                    "Kein API-Key für {}. Setze {} oder trage ihn in den KI-Einstellungen ein.",
                    self.provider.label(),
                    if self.provider == Provider::Claude { "ANTHROPIC_API_KEY" } else { "OPENAI_API_KEY" }
                )),
                Some(key) => match self.provider {
                    Provider::Claude => self.claude_turn(&settings.claude_model, &key, user_text, &context, &tools, &tx, &stop2),
                    Provider::OpenAi => self.openai_turn(&settings.openai_model, &key, user_text, &context, &tools, &tx, &stop2),
                },
            };
            if let Err(e) = result {
                let _ = tx.send(Event::Error(e));
            }
            let _ = tx.send(Event::Done);
            self
        });
        Running { events, stop, handle: Some(handle) }
    }

    #[allow(clippy::too_many_arguments)]
    fn claude_turn(
        &mut self,
        model: &str,
        key: &str,
        user_text: String,
        context: &str,
        tools: &Sender<Request>,
        events: &Sender<Event>,
        stop: &AtomicBool,
    ) -> Result<(), String> {
        let tool_defs: Vec<Value> = commands::specs()
            .into_iter()
            .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.schema}))
            .collect();
        self.claude_messages.push(json!({"role": "user", "content": format!("{context}\n\n{user_text}")}));
        for _ in 0..30 {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let body = json!({
                "model": model,
                "max_tokens": 16000,
                "system": commands::INSTRUCTIONS,
                "tools": tool_defs,
                "messages": self.claude_messages,
                "output_config": {"effort": "medium"},
                "fallbacks": "default",
            });
            let resp = post(
                "https://api.anthropic.com/v1/messages",
                &[
                    ("x-api-key", key),
                    ("anthropic-version", "2026-10-21"),
                    ("anthropic-beta", "server-side-fallback-2026-07-01"),
                ],
                &body,
            )?;
            let content = resp.get("content").cloned().unwrap_or(json!([]));
            // the whole assistant turn goes back unchanged (thinking blocks included)
            self.claude_messages.push(json!({"role": "assistant", "content": content}));
            let mut results = Vec::new();
            for block in content.as_array().into_iter().flatten() {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let _ = events.send(Event::Text(block["text"].as_str().unwrap_or_default().to_string()));
                    }
                    Some("tool_use") => {
                        let name = block["name"].as_str().unwrap_or_default().to_string();
                        let out = run_tool(tools, events, &name, block["input"].clone());
                        let (content, is_error) = match out {
                            Ok(o) => (claude_content(&o), false),
                            Err(e) => (json!([{"type": "text", "text": e}]), true),
                        };
                        results.push(json!({"type": "tool_result", "tool_use_id": block["id"], "content": content, "is_error": is_error}));
                    }
                    _ => {}
                }
            }
            match resp.get("stop_reason").and_then(Value::as_str) {
                Some("tool_use") => self.claude_messages.push(json!({"role": "user", "content": results})),
                Some("refusal") => return Err("Claude hat die Anfrage abgelehnt.".into()),
                Some("max_tokens") => return Err("Antwort wurde abgeschnitten (max_tokens).".into()),
                _ => return Ok(()),
            }
        }
        Err("Zu viele Werkzeugaufrufe in einer Runde.".into())
    }

    #[allow(clippy::too_many_arguments)]
    fn openai_turn(
        &mut self,
        model: &str,
        key: &str,
        user_text: String,
        context: &str,
        tools: &Sender<Request>,
        events: &Sender<Event>,
        stop: &AtomicBool,
    ) -> Result<(), String> {
        let tool_defs: Vec<Value> = commands::specs()
            .into_iter()
            .map(|t| json!({"type": "function", "name": t.name, "description": t.description, "parameters": t.schema, "strict": false}))
            .collect();
        let mut input = json!([{"role": "user", "content": format!("{context}\n\n{user_text}")}]);
        let auth = format!("Bearer {key}");
        for _ in 0..30 {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let mut body = json!({
                "model": model,
                "instructions": commands::INSTRUCTIONS,
                "input": input,
                "tools": tool_defs,
            });
            if let Some(prev) = &self.openai_previous {
                body["previous_response_id"] = json!(prev);
            }
            let resp = post("https://api.openai.com/v1/responses", &[("authorization", &auth)], &body)?;
            self.openai_previous = resp.get("id").and_then(Value::as_str).map(str::to_string);
            let mut outputs = Vec::new();
            for item in resp.get("output").and_then(Value::as_array).into_iter().flatten() {
                match item.get("type").and_then(Value::as_str) {
                    Some("message") => {
                        for part in item["content"].as_array().into_iter().flatten() {
                            if let Some(t) = part.get("text").and_then(Value::as_str) {
                                let _ = events.send(Event::Text(t.to_string()));
                            }
                        }
                    }
                    Some("function_call") => {
                        let name = item["name"].as_str().unwrap_or_default().to_string();
                        let args: Value = item["arguments"].as_str().and_then(|a| serde_json::from_str(a).ok()).unwrap_or(json!({}));
                        let output = match run_tool(tools, events, &name, args) {
                            Ok(o) if o.images.is_empty() => json!(o.text),
                            Ok(o) => {
                                let mut parts = vec![json!({"type": "input_text", "text": o.text})];
                                for png in &o.images {
                                    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
                                    parts.push(json!({"type": "input_image", "image_url": format!("data:image/png;base64,{b64}")}));
                                }
                                json!(parts)
                            }
                            Err(e) => json!(format!("error: {e}")),
                        };
                        outputs.push(json!({"type": "function_call_output", "call_id": item["call_id"], "output": output}));
                    }
                    _ => {}
                }
            }
            if outputs.is_empty() {
                return Ok(());
            }
            input = json!(outputs);
        }
        Err("Zu viele Werkzeugaufrufe in einer Runde.".into())
    }
}

fn claude_content(o: &Output) -> Value {
    let mut parts = vec![json!({"type": "text", "text": o.text})];
    for png in &o.images {
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        parts.push(json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": data}}));
    }
    json!(parts)
}

fn run_tool(tools: &Sender<Request>, events: &Sender<Event>, name: &str, args: Value) -> Result<Output, String> {
    let _ = events.send(Event::Tool(describe(name, &args)));
    let (tx, rx) = mpsc::channel();
    tools.send(Request { tool: name.to_string(), args, reply: tx }).map_err(|_| "App geschlossen".to_string())?;
    rx.recv().map_err(|_| "App geschlossen".to_string())?
}

/// A short German line for the chat log.
fn describe(name: &str, args: &Value) -> String {
    let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    match name {
        "add_text" => format!("Text „{}“ hinzugefügt", s("text")),
        "add_placeholder" => format!("Platzhalter „{}“ hinzugefügt", s("label")),
        "add_shape" => "Form hinzugefügt".into(),
        "import_image" => format!("Bild {} eingefügt", s("path")),
        "update_object" => format!("Objekt {} geändert", args.get("id").unwrap_or(&json!("?"))),
        "delete_object" => format!("Objekt {} gelöscht", args.get("id").unwrap_or(&json!("?"))),
        "set_piece_color" => format!("{} → {}", s("target"), s("color")),
        "render_preview" => "Vorschau angesehen".into(),
        "get_overview" | "list_objects" => "Design gelesen".into(),
        other => other.replace('_', " "),
    }
}

fn post(url: &str, headers: &[(&str, &str)], body: &Value) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .into();
    let mut req = agent.post(url).header("content-type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut resp = req.send_json(body).map_err(|e| format!("Netzwerkfehler: {e}"))?;
    let status = resp.status();
    let json: Value = resp.body_mut().read_json().map_err(|e| format!("Antwort unlesbar ({status}): {e}"))?;
    if !status.is_success() {
        let msg = json.pointer("/error/message").and_then(Value::as_str).unwrap_or("unbekannter Fehler");
        return Err(format!("API-Fehler {status}: {msg}"));
    }
    Ok(json)
}
