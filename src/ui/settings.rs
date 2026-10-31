//! The Settings menu: language, colour theme and VSync. Language and theme are
//! submenus with one entry per choice. (A dropdown would not work in a menu:
//! egui closes a menu on any click inside it, and a dropdown's list counts as
//! outside of it.)

use myth::egui::egui;

use crate::i18n::{self, Language, tr, tr_args};
use crate::theme;

/// What the user changed. The caller applies it (and remembers it), so
/// showing the menu changes nothing by itself.
#[derive(Default, Debug, PartialEq)]
pub struct Changes {
    pub language: Option<Language>,
    pub theme: Option<usize>,
    pub vsync: Option<bool>,
}

/// Shows a folder in the system's file manager (best effort).
fn open_folder(folder: &std::path::Path) {
    const OPENER: &str = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::fs::create_dir_all(folder);
    let _ = std::process::Command::new(OPENER).arg(folder).spawn();
}

/// The contents of the Settings menu.
pub fn menu(ui: &mut egui::Ui, vsync: bool) -> Changes {
    let mut changes = Changes::default();
    ui.menu_button(tr("menu.settings.language"), |ui| {
        let current = i18n::current();
        for language in Language::ALL {
            if ui.radio(current == language, language.name()).clicked() {
                changes.language = Some(language);
                ui.close();
            }
        }
    });
    ui.menu_button(tr("menu.settings.theme"), |ui| {
        ui.set_max_width(280.0);
        let (entries, current) = (theme::all(), theme::current_index());
        for (index, entry) in entries.iter().enumerate().filter(|(_, e)| e.builtin) {
            if ui.radio(current == index, &entry.theme.name).clicked() {
                changes.theme = Some(index);
                ui.close();
            }
        }
        if entries.iter().any(|e| !e.builtin) {
            ui.separator();
            ui.weak(tr("menu.settings.your_themes"));
            for (index, entry) in entries.iter().enumerate().filter(|(_, e)| !e.builtin) {
                if ui.radio(current == index, &entry.theme.name).clicked() {
                    changes.theme = Some(index);
                    ui.close();
                }
            }
        }
        ui.separator();
        // the folder for the user's own themes, one click away
        if let Some(folder) = crate::config::themes_dir() {
            let hint = tr_args("menu.settings.theme_folder", &[&folder.display()]);
            if ui.button(tr("menu.settings.open_themes_folder")).on_hover_text(hint).clicked() {
                open_folder(&folder);
                ui.close();
            }
        }
        // what was wrong with theme files that can't be used: one line each, the whole message on hover
        for problem in theme::problems() {
            ui.add(egui::Label::new(egui::RichText::new(problem).small().color(*theme::colors().warn)).truncate()).on_hover_text(problem);
        }
    });
    ui.separator();
    let mut on = vsync;
    if ui.checkbox(&mut on, "VSync").changed() {
        changes.vsync = Some(on);
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame of a menu bar with the Settings menu: what changed, and every text drawn with where.
    #[allow(deprecated)]
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> (Changes, Vec<(String, egui::Rect)>) {
        let raw = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(900.0, 600.0))),
            ..Default::default()
        };
        let mut changes = Changes::default();
        let output = ctx.run(raw, |ctx| {
            egui::Panel::top("bar").show(ctx, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("Settings", |ui| changes = menu(ui, false));
                });
            });
        });
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

    /// Clicks the last text drawn as `label` (a submenu's entries come after its button).
    fn click(ctx: &egui::Context, label: &str) -> Changes {
        let (_, texts) = frame(ctx, vec![]);
        let (_, rect) = texts
            .iter()
            .rfind(|(text, _)| text == label)
            .cloned()
            .unwrap_or_else(|| panic!("no `{label}` on screen: {:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>()));
        let at = rect.center();
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(ctx, vec![egui::Event::PointerMoved(at)]);
        frame(ctx, vec![button(true)]);
        frame(ctx, vec![button(false)]).0
    }

    #[test]
    fn a_theme_is_chosen_through_the_submenus() {
        let ctx = egui::Context::default();
        frame(&ctx, vec![]);
        frame(&ctx, vec![]);
        click(&ctx, "Settings"); // the menu opens
        click(&ctx, tr("menu.settings.theme")); // the submenu opens, and the menu stays open
        let changes = click(&ctx, "Midnight");
        assert_eq!(changes.theme, theme::all().iter().position(|e| e.id == "midnight"), "choosing Midnight should be reported");
    }

    #[test]
    fn a_language_is_chosen_through_its_submenu() {
        let ctx = egui::Context::default();
        frame(&ctx, vec![]);
        frame(&ctx, vec![]);
        click(&ctx, "Settings");
        click(&ctx, tr("menu.settings.language"));
        let changes = click(&ctx, Language::German.name());
        assert_eq!(changes.language, Some(Language::German));
    }
}
