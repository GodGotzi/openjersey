//! The Agent panel (Agent mode): a log of what an AI assistant does through
//! MCP, with the details the rest of the UI doesn't show: who is connected,
//! every call with its arguments, result, duration and what it did to the
//! design, and what was turned away.

use myth::egui::egui::text::{LayoutJob, TextFormat, TextWrapping};
use myth::egui::egui::{self, Color32, RichText};

use crate::ai::log::{Call, Entry, Event, Log, Outcome, format_time};
use crate::i18n::{tr, tr_args};
use crate::theme;

use super::widgets::section_title;

pub fn show(ui: &mut egui::Ui, log: &mut Log) {
    header(ui, log);
    ui.add_space(4.0);
    if log.is_empty() {
        connect_help(ui);
        return;
    }
    egui::ScrollArea::vertical().id_salt("agent-log").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        for entry in log.entries() {
            row(ui, entry);
        }
    });
}

/// The command that starts the bridge, with the path of this program.
fn exe_path() -> String {
    let path = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "openjersey".into());
    if path.contains(' ') { format!("\"{path}\"") } else { path }
}

/// The empty state: how to add OpenJersey's MCP server to an assistant.
fn connect_help(ui: &mut egui::Ui) {
    let exe = exe_path();
    ui.add(egui::Label::new(RichText::new(tr("agent.connect_title")).strong()));
    ui.add_space(2.0);
    ui.add(egui::Label::new(RichText::new(tr("agent.connect_intro")).weak()).wrap());
    ui.add_space(8.0);
    command_card(ui, "Claude Code", &format!("claude mcp add openjersey -- {exe} mcp"));
    ui.add_space(8.0);
    command_card(ui, "Codex", &format!("codex mcp add openjersey -- {exe} mcp"));
    ui.add_space(8.0);
    ui.add(egui::Label::new(RichText::new(tr_args("agent.connect_other", &[&"openjersey"])).small().weak()).wrap());
}

/// A client's name above a command in a box, with a button that copies it.
fn command_card(ui: &mut egui::Ui, name: &str, command: &str) {
    let colors = theme::colors();
    ui.label(RichText::new(name).small().color(*colors.text_weak));
    egui::Frame::new().fill(*colors.input).stroke(egui::Stroke::new(1.0, *colors.border)).corner_radius(6.0).inner_margin(8.0).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(command).monospace().size(11.5)).wrap());
            ui.add_space(4.0);
            if ui.add(egui::Button::new(tr("agent.copy_command")).small()).clicked() {
                ui.ctx().copy_text(command.to_owned());
            }
        },
    );
}

/// The title, the counts, and the buttons to copy or clear the log.
fn header(ui: &mut egui::Ui, log: &mut Log) {
    ui.horizontal(|ui| {
        section_title(ui, tr("agent.title"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(!log.is_empty(), egui::Button::new(tr("agent.clear")).small()).clicked() {
                log.clear();
            }
            if ui.add_enabled(!log.is_empty(), egui::Button::new(tr("agent.copy")).small()).on_hover_text(tr("agent.copy_hint")).clicked() {
                ui.ctx().copy_text(log.to_text());
            }
        });
    });
    let (clients, calls, errors) = (log.clients(), log.calls(), log.errors());
    let dot = if clients > 0 { theme::accent() } else { ui.visuals().weak_text_color() };
    ui.horizontal(|ui| {
        ui.label(RichText::new("⏺").color(dot));
        let status = tr_args("agent.status", &[&clients, &calls, &errors]);
        ui.label(RichText::new(status).small().color(ui.visuals().weak_text_color()));
    });
}

fn row(ui: &mut egui::Ui, entry: &Entry) {
    let time = format_time(entry.at);
    let weak = ui.visuals().weak_text_color();
    match &entry.event {
        Event::Connected => {
            ui.label(RichText::new(format!("{time}  {}", tr("agent.connected"))).small().color(weak));
        }
        Event::Disconnected => {
            ui.label(RichText::new(format!("{time}  {}", tr("agent.disconnected"))).small().color(weak));
        }
        Event::Rejected(why) => {
            ui.label(RichText::new(format!("{time}  {}", tr_args("agent.rejected", &[why]))).small().color(*theme::colors().warn));
        }
        Event::Call(call) => call_row(ui, entry.id, &time, call),
    }
}

/// One call: a line with its tool and arguments, opening into the details.
fn call_row(ui: &mut egui::Ui, id: u64, time: &str, call: &Call) {
    let colors = theme::colors();
    let failed = matches!(call.outcome, Outcome::Failed(_));
    // red when it failed, the accent when it changed the design, quiet when it only looked
    let dot = if failed {
        *colors.error
    } else if call.changed {
        *colors.accent
    } else {
        *colors.text_weak
    };
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::FontId::monospace(11.0);
    let mut line = LayoutJob::default();
    let mut add = |text: &str, color: Color32, font: &egui::FontId| {
        line.append(text, 0.0, TextFormat { color, font_id: font.clone(), ..Default::default() })
    };
    add("⏺ ", dot, &body);
    add(time, *colors.text_weak, &small);
    add("  ", *colors.text_weak, &small);
    add(&call.tool, if failed { *colors.error } else { *colors.text }, &body);
    add("  ", *colors.text_weak, &small);
    add(&call.summary(), *colors.text_weak, &small);
    line.wrap = TextWrapping::truncate_at_width((ui.available_width() - 28.0).max(60.0));

    egui::CollapsingHeader::new(line).id_salt(id).show(ui, |ui| details(ui, call));
}

fn details(ui: &mut egui::Ui, call: &Call) {
    let weak = ui.visuals().weak_text_color();
    let note = |ui: &mut egui::Ui, label: &str, value: String| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(label).small().color(weak));
            ui.label(RichText::new(value).small());
        });
    };
    if !call.client.is_empty() {
        note(ui, tr("agent.client"), call.client.clone());
    }
    note(ui, tr("agent.took"), format!("{} ms", call.took.as_millis()));
    let design = if call.changed { tr("agent.changed") } else { tr("agent.unchanged") };
    note(ui, tr("agent.design"), format!("{design} ({} ➡ {} {})", call.objects_before, call.objects_after, tr("agent.objects")));
    ui.label(RichText::new(tr("agent.arguments")).small().color(weak));
    let args = serde_json::to_string_pretty(&call.args).unwrap_or_default();
    ui.add(egui::Label::new(RichText::new(args).monospace().size(11.0)).selectable(true).wrap());
    match &call.outcome {
        Outcome::Done { text, images } => {
            ui.label(RichText::new(tr("agent.result")).small().color(weak));
            ui.add(egui::Label::new(RichText::new(text).monospace().size(11.0)).selectable(true).wrap());
            if *images > 0 {
                note(ui, "", tr_args("agent.images", &[images]));
            }
        }
        Outcome::Failed(error) => {
            ui.label(RichText::new(tr("agent.error")).small().color(weak));
            ui.add(egui::Label::new(RichText::new(error).monospace().size(11.0).color(*theme::colors().error)).selectable(true).wrap());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::ai::commands::Output;

    fn call(tool: &str, args: serde_json::Value, result: Result<Output, String>, changed: bool) -> Event {
        Event::Call(Box::new(Call::finished("Claude Code 2.1", tool, &args, &result, Duration::from_millis(12), changed, 3, 4)))
    }

    /// Every text one frame of the panel draws.
    #[allow(deprecated)]
    fn texts(log: &mut Log) -> Vec<String> {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(500.0, 400.0))),
            ..Default::default()
        };
        let mut last = Vec::new();
        for _ in 0..2 {
            let output = ctx.run(raw.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show(ui, log));
            });
            last = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_string()),
                    _ => None,
                })
                .collect();
        }
        last
    }

    #[test]
    fn an_empty_log_explains_itself() {
        let shown = texts(&mut Log::default());
        assert!(shown.iter().any(|t| t == tr("agent.connect_title")), "{shown:?}");
    }

    #[test]
    fn calls_show_their_tool_and_arguments_and_the_counts() {
        let mut log = Log::default();
        log.push(Event::Connected);
        log.push(call("add_text", json!({"text": "HI"}), Ok(Output { text: "{\"id\":7}".into(), images: vec![] }), true));
        log.push(call("delete_object", json!({"id": 9}), Err("no object 9".into()), false));
        log.push(Event::Rejected("a request with a wrong token".into()));
        let shown = texts(&mut log);
        let all = shown.join("\n");
        assert!(all.contains("add_text") && all.contains("{\"text\":\"HI\"}"), "{all}");
        assert!(all.contains("delete_object"), "{all}");
        assert!(all.contains(&tr_args("agent.status", &[&1usize, &2usize, &1usize])), "{all}");
        assert!(all.contains("wrong token"), "{all}");
    }
}
