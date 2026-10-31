//! Small widgets the panels share: section titles and a segmented control.

use std::hash::Hash;

use myth::egui::egui::{self, Align2, CornerRadius, RichText, Sense, Stroke, StrokeKind, TextStyle, Vec2};

use crate::theme;

/// A small, quiet title above a section of a panel.
pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).small().strong().color(ui.visuals().weak_text_color()));
}

/// Options in one rounded control, one of them chosen. With `fill` the
/// segments share the width of the panel. Returns true when the choice changed.
pub fn segmented<T: Copy + PartialEq>(ui: &mut egui::Ui, id: impl Hash, value: &mut T, items: &[(T, &str)], fill: bool) -> bool {
    const HEIGHT: f32 = 26.0;
    const PADDING: f32 = 14.0;
    const INSET: f32 = 2.0;
    let font = TextStyle::Button.resolve(ui.style());
    let natural: Vec<f32> = items
        .iter()
        .map(|(_, label)| ui.painter().layout_no_wrap(label.to_string(), font.clone(), egui::Color32::WHITE).size().x + 2.0 * PADDING)
        .collect();
    let total = if fill { ui.available_width() } else { natural.iter().sum::<f32>() + 2.0 * INSET };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, HEIGHT), Sense::hover());
    let visuals = ui.visuals().widgets.inactive;
    let rounding = visuals.corner_radius;
    ui.painter().rect(
        rect,
        rounding,
        theme::mix(*theme::colors().panel, *theme::colors().input, 0.7),
        Stroke::new(1.0, *theme::colors().border),
        StrokeKind::Inside,
    );

    let inner = rect.shrink(INSET);
    let share = if fill { inner.width() / items.len() as f32 } else { 0.0 };
    let (mut changed, mut x) = (false, inner.min.x);
    let segment_rounding = CornerRadius::same(rounding.nw.saturating_sub(1));
    for (i, ((item, label), width)) in items.iter().zip(&natural).enumerate() {
        let width = if fill { share } else { *width };
        let segment = egui::Rect::from_min_size(egui::pos2(x, inner.min.y), Vec2::new(width, inner.height()));
        let response = ui.interact(segment, ui.id().with((&id_hash(&id), i)), Sense::click());
        let selected = *value == *item;
        if selected {
            ui.painter().rect_filled(segment, segment_rounding, theme::accent());
        } else if response.hovered() {
            ui.painter().rect_filled(segment, segment_rounding, *theme::colors().surface_hover);
        }
        let ink = if selected { theme::on_accent() } else { ui.visuals().text_color() };
        ui.painter().text(segment.center(), Align2::CENTER_CENTER, label, font.clone(), ink);
        if response.clicked() && !selected {
            *value = *item;
            changed = true;
        }
        x += width;
    }
    changed
}

/// A dropdown of the themes: the bundled ones, then the user's own under a
/// heading of their own. It changes `chosen`; the caller applies the theme.
pub fn theme_dropdown(ui: &mut egui::Ui, chosen: &mut usize) -> egui::Response {
    let entries = theme::all();
    let selected = entries[(*chosen).min(entries.len() - 1)].theme.name.clone();
    egui::ComboBox::from_id_salt("theme")
        .width(180.0)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for (index, entry) in entries.iter().enumerate().filter(|(_, e)| e.builtin) {
                ui.selectable_value(chosen, index, &entry.theme.name);
            }
            if entries.iter().any(|e| !e.builtin) {
                ui.separator();
                ui.weak(crate::i18n::tr("menu.settings.your_themes"));
                for (index, entry) in entries.iter().enumerate().filter(|(_, e)| !e.builtin) {
                    ui.selectable_value(chosen, index, &entry.theme.name);
                }
            }
        })
        .response
}

fn id_hash(id: &impl Hash) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Side {
        Left,
        Right,
    }

    /// One frame of a panel with a segmented control; returns its rectangle and whether the choice changed.
    #[allow(deprecated)]
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, value: &mut Side) -> (egui::Rect, bool) {
        let raw = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(400.0, 200.0))),
            ..Default::default()
        };
        let (mut rect, mut changed) = (egui::Rect::NOTHING, false);
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let top_left = ui.cursor().min;
                changed = segmented(ui, "test", value, &[(Side::Left, "Left"), (Side::Right, "Right")], true);
                rect = egui::Rect::from_min_size(top_left, Vec2::new(ui.min_rect().width(), 26.0));
            });
        });
        (rect, changed)
    }

    /// One frame of the theme dropdown: its rectangle and every text drawn, with where.
    #[allow(deprecated)]
    fn dropdown_frame(ctx: &egui::Context, events: Vec<egui::Event>, chosen: &mut usize) -> (egui::Rect, Vec<(String, egui::Rect)>) {
        let raw = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(500.0, 500.0))),
            ..Default::default()
        };
        let mut rect = egui::Rect::NOTHING;
        let output = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| rect = theme_dropdown(ui, chosen).rect);
        });
        let texts = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some((text.galley.text().to_string(), text.galley.rect.translate(text.pos.to_vec2()))),
                _ => None,
            })
            .collect();
        (rect, texts)
    }

    #[test]
    fn the_theme_dropdown_lists_the_themes_and_chooses_one() {
        let ctx = egui::Context::default();
        let mut chosen = 0;
        let (button, _) = dropdown_frame(&ctx, vec![], &mut chosen);
        let click = |pos: egui::Pos2, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        dropdown_frame(&ctx, vec![egui::Event::PointerMoved(button.center())], &mut chosen);
        dropdown_frame(&ctx, vec![click(button.center(), true)], &mut chosen);
        dropdown_frame(&ctx, vec![click(button.center(), false)], &mut chosen);
        // the list is open: every bundled theme is in it
        let (_, texts) = dropdown_frame(&ctx, vec![], &mut chosen);
        for entry in theme::all().iter().filter(|e| e.builtin) {
            assert!(texts.iter().any(|(text, _)| *text == entry.theme.name), "{} is not in the list", entry.theme.name);
        }
        // choose Midnight
        let target = theme::all().iter().position(|e| e.id == "midnight").unwrap();
        let (_, entry_rect) = texts.iter().rfind(|(text, _)| *text == "Midnight").cloned().unwrap();
        dropdown_frame(&ctx, vec![egui::Event::PointerMoved(entry_rect.center())], &mut chosen);
        dropdown_frame(&ctx, vec![click(entry_rect.center(), true)], &mut chosen);
        dropdown_frame(&ctx, vec![click(entry_rect.center(), false)], &mut chosen);
        assert_eq!(chosen, target, "clicking Midnight should choose it");
    }

    #[test]
    fn clicking_a_segment_chooses_it() {
        let ctx = egui::Context::default();
        let mut value = Side::Left;
        let (rect, _) = frame(&ctx, vec![], &mut value);
        let right_half = egui::pos2(rect.max.x - rect.width() / 4.0, rect.center().y);
        let click = |pressed| egui::Event::PointerButton {
            pos: right_half,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&ctx, vec![egui::Event::PointerMoved(right_half)], &mut value);
        frame(&ctx, vec![click(true)], &mut value);
        let (_, changed) = frame(&ctx, vec![click(false)], &mut value);
        assert!(changed && value == Side::Right, "the right segment should be chosen, got {value:?}");
        // clicking the chosen segment again changes nothing
        frame(&ctx, vec![click(true)], &mut value);
        let (_, changed) = frame(&ctx, vec![click(false)], &mut value);
        assert!(!changed && value == Side::Right);
    }
}
