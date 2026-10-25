//! The "KI-Assistent" window: chat with Claude or OpenAI, which edit the
//! design through the editor tools; plus how to connect Codex / Claude Code.

use std::sync::mpsc::Sender;
use std::sync::atomic::Ordering;

use myth::egui::egui::{self, Color32, RichText};

use crate::ai::chat::{Conversation, Event, Provider, Running, Settings};
use crate::ai::server::Request;
use crate::workspace::Workspace;

enum Line {
    User(String),
    Assistant(String),
    Tool(String),
    Error(String),
}

pub struct ChatPanel {
    pub open: bool,
    pub connect_open: bool,
    settings: Settings,
    show_settings: bool,
    conversation: Option<Conversation>,
    running: Option<Running>,
    log: Vec<Line>,
    input: String,
}

impl ChatPanel {
    pub fn new() -> Self {
        Self {
            open: false,
            connect_open: false,
            settings: Settings::load(),
            show_settings: false,
            conversation: None,
            running: None,
            log: Vec::new(),
            input: String::new(),
        }
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    pub fn ui(&mut self, ctx: &egui::Context, ws: &Workspace, tools: &Sender<Request>) {
        self.poll();
        let mut open = self.open;
        egui::Window::new("KI-Assistent").open(&mut open).default_size([380.0, 520.0]).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let before = self.settings.provider;
                egui::ComboBox::from_id_salt("provider").selected_text(self.settings.provider.label()).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.settings.provider, Provider::Claude, "Claude");
                    ui.selectable_value(&mut self.settings.provider, Provider::OpenAi, "OpenAI");
                });
                if before != self.settings.provider {
                    self.conversation = None; // transcripts are provider-specific
                    self.settings.save();
                }
                if ui.button("Neues Gespräch").clicked() && !self.busy() {
                    self.conversation = None;
                    self.log.clear();
                }
                ui.toggle_value(&mut self.show_settings, "⚙");
            });
            if self.show_settings {
                self.settings_ui(ui);
            }
            ui.separator();

            let input_height = 70.0;
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .max_height(ui.available_height() - input_height)
                .show(ui, |ui| {
                    if self.log.is_empty() {
                        ui.weak("Beschreibe, was am Trikot passieren soll, z. B. „Setz das Sponsorlogo aus ~/logo.svg in Feld A“ oder „Mach die Ärmel dunkelblau“. Ausgewählte Objekte werden mitgeschickt.");
                    }
                    for line in &self.log {
                        match line {
                            Line::User(t) => {
                                ui.label(RichText::new(t).strong());
                            }
                            Line::Assistant(t) => {
                                ui.label(t);
                            }
                            Line::Tool(t) => {
                                ui.label(RichText::new(format!("🔧 {t}")).small().color(Color32::from_gray(150)));
                            }
                            Line::Error(t) => {
                                ui.label(RichText::new(t).color(Color32::from_rgb(255, 120, 120)));
                            }
                        }
                        ui.add_space(4.0);
                    }
                    if self.busy() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.weak("arbeitet …");
                        });
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut self.input)
                        .desired_rows(2)
                        .desired_width(ui.available_width() - 70.0)
                        .hint_text("Nachricht (Enter sendet, Shift+Enter neue Zeile)"),
                );
                let enter = edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                if self.busy() {
                    if ui.button("Stopp").clicked()
                        && let Some(r) = &self.running
                    {
                        r.stop.store(true, Ordering::Relaxed);
                    }
                } else if (ui.button("Senden").clicked() || enter) && !self.input.trim().is_empty() {
                    let text = self.input.trim().to_string();
                    self.input.clear();
                    self.send(text, ws, tools);
                }
            });
        });
        self.open = open;
        self.connect_window(ctx);
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        egui::Grid::new("ai-settings").num_columns(2).show(ui, |ui| {
            ui.label("Claude-Modell");
            changed |= ui.text_edit_singleline(&mut self.settings.claude_model).changed();
            ui.end_row();
            ui.label("OpenAI-Modell");
            changed |= ui.text_edit_singleline(&mut self.settings.openai_model).changed();
            ui.end_row();
            ui.label("Anthropic-Key");
            changed |= ui.add(egui::TextEdit::singleline(&mut self.settings.anthropic_key).password(true).hint_text("oder ANTHROPIC_API_KEY")).changed();
            ui.end_row();
            ui.label("OpenAI-Key");
            changed |= ui.add(egui::TextEdit::singleline(&mut self.settings.openai_key).password(true).hint_text("oder OPENAI_API_KEY")).changed();
            ui.end_row();
        });
        ui.small("Keys werden nur lokal gespeichert (~/.config/openjersey/settings.json, nur für dich lesbar).");
        if changed {
            self.settings.save();
        }
    }

    fn send(&mut self, text: String, ws: &Workspace, tools: &Sender<Request>) {
        let selection: Vec<String> = ws.selection.iter().map(|&id| format!("{id} „{}“", ws.object_name(id))).collect();
        let context = if selection.is_empty() {
            "(Nichts ausgewählt.)".to_string()
        } else {
            format!("Ausgewählt im Editor: {}", selection.join(", "))
        };
        self.log.push(Line::User(text.clone()));
        let conversation = self.conversation.take().unwrap_or_else(|| Conversation::new(self.settings.provider));
        self.running = Some(conversation.send(&self.settings, text, context, tools.clone()));
    }

    fn poll(&mut self) {
        let Some(running) = &self.running else { return };
        while let Ok(event) = running.events.try_recv() {
            match event {
                Event::Text(t) if !t.trim().is_empty() => self.log.push(Line::Assistant(t)),
                Event::Text(_) => {}
                Event::Tool(t) => self.log.push(Line::Tool(t)),
                Event::Error(e) => self.log.push(Line::Error(e)),
                Event::Done => {}
            }
        }
        if running.is_finished() {
            self.conversation = self.running.take().and_then(Running::finish);
        }
    }

    fn connect_window(&mut self, ctx: &egui::Context) {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "openjersey".into());
        let mut open = self.connect_open;
        egui::Window::new("Codex / Claude Code verbinden").open(&mut open).default_width(520.0).show(ctx, |ui| {
            ui.label("OpenJersey ist ein MCP-Server: Codex und Claude Code können das offene Design über dieselben Werkzeuge bearbeiten wie der Assistent hier. Die App muss dafür laufen (sonst wird sie gestartet).");
            ui.add_space(6.0);
            ui.strong("Claude Code");
            let claude = format!("claude mcp add openjersey -- {exe} mcp");
            copy_line(ui, &claude);
            ui.add_space(6.0);
            ui.strong("ChatGPT Codex (~/.codex/config.toml)");
            let codex = format!("[mcp_servers.openjersey]\ncommand = \"{exe}\"\nargs = [\"mcp\"]");
            copy_line(ui, &codex);
        });
        self.connect_open = open;
    }
}

fn copy_line(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(RichText::new(text).monospace()).wrap());
        if ui.small_button("kopieren").clicked() {
            ui.ctx().copy_text(text.to_string());
        }
    });
}
