//! What the agent does, as the app saw it: who connected, every tool call
//! with its arguments and result, how long it took and whether it changed the
//! design, and what was turned away. Shown in the Agent panel (Agent mode).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::commands::Output;

/// How many entries are kept; the oldest go first.
const CAPACITY: usize = 500;
/// How much of a result is kept for display (results can be long).
const RESULT_LIMIT: usize = 4000;
/// How much of the arguments the one-line summary shows.
const SUMMARY_LIMIT: usize = 110;

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Done { text: String, images: usize },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    /// The MCP client's name and version (empty if it did not say).
    pub client: String,
    pub tool: String,
    pub args: Value,
    pub outcome: Outcome,
    pub took: Duration,
    /// The design was different afterwards.
    pub changed: bool,
    pub objects_before: usize,
    pub objects_after: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Connected,
    Disconnected,
    /// A message that was turned away, and why.
    Rejected(String),
    Call(Box<Call>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// A number that stays with the entry, even when older ones are dropped.
    pub id: u64,
    /// Time since the app started.
    pub at: Duration,
    pub event: Event,
}

pub struct Log {
    entries: VecDeque<Entry>,
    started: Instant,
    next_id: u64,
    clients: usize,
    calls: usize,
    errors: usize,
}

impl Default for Log {
    fn default() -> Self {
        Self { entries: VecDeque::new(), started: Instant::now(), next_id: 0, clients: 0, calls: 0, errors: 0 }
    }
}

impl Log {
    pub fn push(&mut self, event: Event) {
        match &event {
            Event::Connected => self.clients += 1,
            Event::Disconnected => self.clients = self.clients.saturating_sub(1),
            Event::Call(call) => {
                self.calls += 1;
                self.errors += usize::from(matches!(call.outcome, Outcome::Failed(_)));
            }
            Event::Rejected(_) => {}
        }
        self.next_id += 1;
        self.entries.push_back(Entry { id: self.next_id, at: self.started.elapsed(), event });
        while self.entries.len() > CAPACITY {
            self.entries.pop_front();
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Empties the list (the counts start again too).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.calls = 0;
        self.errors = 0;
    }

    /// Clients connected right now.
    pub fn clients(&self) -> usize {
        self.clients
    }

    pub fn calls(&self) -> usize {
        self.calls
    }

    pub fn errors(&self) -> usize {
        self.errors
    }

    /// The whole log as text, to paste somewhere.
    pub fn to_text(&self) -> String {
        self.entries.iter().map(Entry::to_line).collect::<Vec<_>>().join("\n")
    }
}

impl Entry {
    /// "0:12.3  add_rectangle  ok  4 ms  {…}"
    pub fn to_line(&self) -> String {
        let time = format_time(self.at);
        match &self.event {
            Event::Connected => format!("{time}  client connected"),
            Event::Disconnected => format!("{time}  client disconnected"),
            Event::Rejected(why) => format!("{time}  rejected: {why}"),
            Event::Call(call) => {
                let status = match &call.outcome {
                    Outcome::Done { .. } => "ok".to_string(),
                    Outcome::Failed(error) => format!("FAILED: {}", shorten(error, 200)),
                };
                let changed = if call.changed { "  changed the design" } else { "" };
                format!("{time}  {}  {status}  {} ms{changed}  {}", call.tool, call.took.as_millis(), call.args)
            }
        }
    }
}

impl Call {
    /// A finished call: what was asked, what came back, and what it did to the design.
    #[allow(clippy::too_many_arguments)]
    pub fn finished(
        client: &str,
        tool: &str,
        args: &Value,
        result: &Result<Output, String>,
        took: Duration,
        changed: bool,
        objects_before: usize,
        objects_after: usize,
    ) -> Self {
        let outcome = match result {
            Ok(output) => Outcome::Done { text: shorten(&output.text, RESULT_LIMIT), images: output.images.len() },
            Err(error) => Outcome::Failed(error.clone()),
        };
        Self {
            client: client.to_string(),
            tool: tool.to_string(),
            args: args.clone(),
            outcome,
            took,
            changed,
            objects_before,
            objects_after,
        }
    }

    /// The arguments on one line; for a batch, how many operations and which.
    pub fn summary(&self) -> String {
        if self.tool == "batch"
            && let Some(operations) = self.args.get("operations").and_then(Value::as_array)
        {
            let mut names: Vec<&str> = Vec::new();
            for tool in operations.iter().filter_map(|op| op.get("tool").and_then(Value::as_str)) {
                if !names.contains(&tool) {
                    names.push(tool);
                }
            }
            return shorten(&format!("{} operations: {}", operations.len(), names.join(", ")), SUMMARY_LIMIT);
        }
        shorten(&self.args.to_string(), SUMMARY_LIMIT)
    }
}

/// "1:05.3" for a duration.
pub fn format_time(time: Duration) -> String {
    let seconds = time.as_secs_f32();
    format!("{}:{:04.1}", seconds as u64 / 60, seconds % 60.0)
}

/// At most `max` characters, with an ellipsis when something was cut.
pub fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn call(tool: &str, args: Value, result: Result<Output, String>) -> Event {
        Event::Call(Box::new(Call::finished("Claude Code 2.1", tool, &args, &result, Duration::from_millis(7), true, 3, 4)))
    }

    fn ok(text: &str) -> Result<Output, String> {
        Ok(Output { text: text.to_string(), images: vec![vec![1, 2, 3]] })
    }

    #[test]
    fn the_log_counts_clients_calls_and_errors() {
        let mut log = Log::default();
        assert!(log.is_empty());
        log.push(Event::Connected);
        log.push(call("get_overview", json!({}), ok("{}")));
        log.push(call("delete_object", json!({"id": 9}), Err("no object 9".into())));
        log.push(Event::Rejected("a request with a wrong token".into()));
        assert_eq!((log.clients(), log.calls(), log.errors()), (1, 2, 1));
        log.push(Event::Disconnected);
        log.push(Event::Disconnected); // never below zero
        assert_eq!(log.clients(), 0);
        log.clear();
        assert!(log.is_empty() && log.calls() == 0 && log.errors() == 0);
    }

    #[test]
    fn the_oldest_entries_are_dropped_and_ids_keep_counting() {
        let mut log = Log::default();
        for _ in 0..CAPACITY + 25 {
            log.push(Event::Connected);
        }
        assert_eq!(log.entries().count(), CAPACITY);
        let ids: Vec<u64> = log.entries().map(|e| e.id).collect();
        assert_eq!(ids.first(), Some(&26));
        assert_eq!(ids.last(), Some(&(CAPACITY as u64 + 25)));
    }

    #[test]
    fn a_finished_call_keeps_what_came_back_shortened() {
        let long = "x".repeat(RESULT_LIMIT + 50);
        let done = Call::finished("c", "t", &json!({}), &ok(&long), Duration::ZERO, false, 0, 0);
        match done.outcome {
            Outcome::Done { text, images } => {
                assert_eq!(text.chars().count(), RESULT_LIMIT + 1);
                assert!(text.ends_with('…') && images == 1);
            }
            other => panic!("{other:?}"),
        }
        let failed = Call::finished("c", "t", &json!({}), &Err("boom".into()), Duration::ZERO, false, 0, 0);
        assert_eq!(failed.outcome, Outcome::Failed("boom".into()));
    }

    #[test]
    fn a_batch_is_summarised_by_its_operations() {
        let args = json!({"operations": [{"tool": "add_rectangle"}, {"tool": "add_text"}, {"tool": "add_rectangle"}]});
        let Event::Call(batch) = call("batch", args, ok("{}")) else { unreachable!() };
        assert_eq!(batch.summary(), "3 operations: add_rectangle, add_text");
        let Event::Call(plain) = call("add_text", json!({"text": "HI"}), ok("{}")) else { unreachable!() };
        assert_eq!(plain.summary(), "{\"text\":\"HI\"}");
    }

    #[test]
    fn shortening_cuts_on_characters_not_bytes() {
        assert_eq!(shorten("äöü", 5), "äöü");
        assert_eq!(shorten("äöüß", 2), "äö…");
        assert_eq!(format_time(Duration::from_millis(65_300)), "1:05.3");
        assert_eq!(format_time(Duration::from_millis(900)), "0:00.9");
    }

    #[test]
    fn the_log_reads_as_text() {
        let mut log = Log::default();
        log.push(Event::Connected);
        log.push(call("add_text", json!({"text": "HI"}), ok("{}")));
        log.push(call("delete_object", json!({"id": 9}), Err("no object 9".into())));
        let text = log.to_text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].ends_with("client connected"));
        assert!(lines[1].contains("add_text  ok  7 ms  changed the design") && lines[1].ends_with("{\"text\":\"HI\"}"), "{}", lines[1]);
        assert!(lines[2].contains("delete_object  FAILED: no object 9"), "{}", lines[2]);
    }
}
