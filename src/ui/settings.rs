//! The Settings window: language, colour theme and VSync. It is a window and
//! not a menu because a dropdown inside a menu can't work: egui closes a menu
//! on any click inside it, and the dropdown's list counts as outside of it.

use myth::egui::egui;

use crate::i18n::{self, Language, tr, tr_args};
use crate::theme;

use super::widgets::theme_dropdown;

#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
}

/// What the user changed this frame. The caller applies it (and remembers
/// it), so showing the window changes nothing by itself.
#[derive(Default, Debug, PartialEq)]
pub struct Changes {
    pub language: Option<Language>,
    pub theme: Option<usize>,
    pub vsync: Option<bool>,
}

impl SettingsWindow {
    pub fn show(&mut self, ctx: &egui::Context, vsync: bool) -> Changes {
        let mut changes = Changes::default();
        let mut open = self.open;
        let screen = ctx.content_rect();
        egui::Window::new(tr("menu.settings.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(egui::pos2(screen.right() - 360.0, screen.top() + 52.0))
            .show(ctx, |ui| {
                egui::Grid::new("settings").num_columns(2).spacing([20.0, 12.0]).show(ui, |ui| {
                    ui.label(tr("menu.settings.language"));
                    let mut language = i18n::current();
                    egui::ComboBox::from_id_salt("language").width(180.0).selected_text(language.name()).show_ui(ui, |ui| {
                        for option in Language::ALL {
                            ui.selectable_value(&mut language, option, option.name());
                        }
                    });
                    if language != i18n::current() {
                        changes.language = Some(language);
                    }
                    ui.end_row();

                    ui.label(tr("menu.settings.theme"));
                    let mut chosen = theme::current_index();
                    let dropdown = theme_dropdown(ui, &mut chosen);
                    if let Some(folder) = crate::config::themes_dir() {
                        dropdown.on_hover_text(tr_args("menu.settings.theme_folder", &[&folder.display()]));
                    }
                    if chosen != theme::current_index() {
                        changes.theme = Some(chosen);
                    }
                    ui.end_row();

                    ui.label("VSync");
                    let mut on = vsync;
                    if ui.checkbox(&mut on, "").changed() {
                        changes.vsync = Some(on);
                    }
                    ui.end_row();
                });
                for problem in theme::problems() {
                    ui.add(egui::Label::new(egui::RichText::new(problem).small().color(*theme::colors().warn)).wrap());
                }
            });
        self.open = open;
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame of the window: what the user changed, and every text drawn with where.
    #[allow(deprecated)]
    fn frame(ctx: &egui::Context, window: &mut SettingsWindow, events: Vec<egui::Event>) -> (Changes, Vec<(String, egui::Rect)>) {
        let raw = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(900.0, 600.0))),
            ..Default::default()
        };
        let mut changes = Changes::default();
        let output = ctx.run(raw, |ctx| changes = window.show(ctx, false));
        let texts = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some((text.galley.text().to_string(), text.galley.rect.translate(text.pos.to_vec2()))),
                _ => None,
            })
            .collect();
        (changes, texts)
    }

    fn click(ctx: &egui::Context, window: &mut SettingsWindow, at: egui::Pos2) -> Changes {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(ctx, window, vec![egui::Event::PointerMoved(at)]);
        frame(ctx, window, vec![button(true)]);
        frame(ctx, window, vec![button(false)]).0
    }

    #[test]
    fn choosing_a_theme_in_the_window_works_and_leaves_the_window_open() {
        let ctx = egui::Context::default();
        let mut window = SettingsWindow { open: true };
        let current = theme::all()[theme::current_index()].theme.name.clone();
        // a window draws nothing in its first frame, while it measures itself
        frame(&ctx, &mut window, vec![]);
        // the closed dropdown shows the current theme's name (the Midnight entry is not on screen yet)
        let (_, texts) = frame(&ctx, &mut window, vec![]);
        assert!(texts.iter().all(|(text, _)| text != "Midnight") || current == "Midnight");
        let (_, dropdown) = texts.iter().find(|(text, _)| *text == current).cloned().expect("the dropdown shows the current theme");
        // opening the list must not close the window
        click(&ctx, &mut window, dropdown.center());
        assert!(window.open, "opening the dropdown closed the window");
        let (_, texts) = frame(&ctx, &mut window, vec![]);
        let (_, midnight) = texts.iter().rfind(|(text, _)| *text == "Midnight").cloned().expect("the open list shows Midnight");
        let changes = click(&ctx, &mut window, midnight.center());
        let wanted = theme::all().iter().position(|e| e.id == "midnight");
        assert_eq!(changes.theme, wanted, "choosing Midnight should be reported");
        assert!(window.open, "choosing a theme closed the window");
    }

    #[test]
    fn a_closed_window_draws_and_changes_nothing() {
        let ctx = egui::Context::default();
        let mut window = SettingsWindow::default();
        let (changes, texts) = frame(&ctx, &mut window, vec![]);
        assert_eq!(changes, Changes::default());
        assert!(texts.is_empty(), "a closed window should draw nothing, drew {texts:?}");
    }
}
