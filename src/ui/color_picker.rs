//! The colour control of the 2D editor: a button showing the colour that
//! opens a picker with the HSV square, a hex field, the design's own colours,
//! recently used colours and a palette, plus a shortcut to the eyedropper.

use std::hash::Hash;

use myth::egui::egui::color_picker::{Alpha, color_picker_color32};
use myth::egui::egui::{self, Color32, CornerRadius, Popup, PopupCloseBehavior, RichText, Stroke, StrokeKind, Vec2};

use crate::document::Rgb;
use crate::i18n::tr;

/// Preset colours: a grey ramp, then the hues, light to dark.
pub const PALETTE: [Rgb; 24] = [
    [0xff, 0xff, 0xff],
    [0xd9, 0xd9, 0xd9],
    [0x9a, 0x9a, 0x9a],
    [0x41, 0x41, 0x41],
    [0x1a, 0x1a, 0x1a],
    [0x00, 0x00, 0x00],
    [0xff, 0xb3, 0xba],
    [0xd7, 0x26, 0x38],
    [0x8b, 0x1a, 0x2b],
    [0xff, 0xd8, 0xa8],
    [0xf5, 0x82, 0x20],
    [0xb3, 0x4a, 0x00],
    [0xff, 0xf2, 0x9e],
    [0xf2, 0xc2, 0x1b],
    [0x9c, 0x7a, 0x00],
    [0xb9, 0xe8, 0xb0],
    [0x2e, 0xa0, 0x4f],
    [0x14, 0x5a, 0x2b],
    [0xa8, 0xd8, 0xf0],
    [0x1f, 0x5f, 0xd1],
    [0x0e, 0x2a, 0x6e],
    [0xe3, 0xb8, 0xf0],
    [0xb6, 0x2f, 0xc4],
    [0x5e, 0x14, 0x70],
];

/// The colours offered right in the options bar.
pub const QUICK: [Rgb; 8] = [
    [0xff, 0xff, 0xff],
    [0x1a, 0x1a, 0x1a],
    [0x41, 0x41, 0x41],
    [0xb6, 0x2f, 0xc4],
    [0xd7, 0x26, 0x38],
    [0x1f, 0x5f, 0xd1],
    [0x2e, 0xa0, 0x4f],
    [0xf2, 0xc2, 0x1b],
];

const SWATCH: f32 = 20.0;
const POPUP_WIDTH: f32 = 252.0;
const ACCENT: Color32 = Color32::from_rgb(70, 140, 255);

/// Colours offered in the picker besides the palette.
pub struct Swatches<'a> {
    pub recent: &'a [Rgb],
    pub design: &'a [Rgb],
}

/// What happened to the colour this frame.
#[derive(Default)]
pub struct Outcome {
    /// The colour was edited (also while dragging in the square).
    pub changed: bool,
    /// The colour was settled: a swatch was clicked, the hex was entered, or
    /// the picker closed after edits. This is when to remember it.
    pub committed: bool,
    /// The eyedropper was asked for.
    pub eyedropper: bool,
}

/// A button showing `color`, with the picker in its popup.
pub fn button(ui: &mut egui::Ui, id: impl Hash, color: &mut Rgb, swatches: &Swatches) -> Outcome {
    let id = ui.make_persistent_id(id);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(96.0, 24.0), egui::Sense::click());
    let visuals = ui.style().interact(&response);
    ui.painter().rect(rect, CornerRadius::same(5), visuals.weak_bg_fill, visuals.bg_stroke, StrokeKind::Inside);
    let chip = egui::Rect::from_min_size(rect.min + Vec2::new(4.0, 4.0), Vec2::new(22.0, 16.0));
    ui.painter().rect_filled(chip, CornerRadius::same(3), to_color32(*color));
    ui.painter().rect_stroke(chip, CornerRadius::same(3), Stroke::new(1.0_f32, Color32::from_gray(110)), StrokeKind::Inside);
    ui.painter().text(
        egui::pos2(chip.max.x + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        hex(*color),
        egui::FontId::monospace(12.0),
        visuals.text_color(),
    );
    let response = response.on_hover_text(tr("Choose a colour", "Farbe wählen"));

    let mut out = Outcome::default();
    let shown = Popup::from_toggle_button_response(&response)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| picker(ui, id, color, swatches));
    // edits made in the picker count as settled once it closes
    let pending_id = id.with("pending");
    let pending = ui.data(|d| d.get_temp::<bool>(pending_id)).unwrap_or(false);
    match shown {
        Some(popup) => {
            out = popup.inner;
            if out.committed {
                ui.data_mut(|d| d.insert_temp(pending_id, false));
            } else if out.changed {
                ui.data_mut(|d| d.insert_temp(pending_id, true));
            }
        }
        None if pending => {
            out.committed = true;
            ui.data_mut(|d| d.insert_temp(pending_id, false));
        }
        None => {}
    }
    out
}

fn picker(ui: &mut egui::Ui, id: egui::Id, color: &mut Rgb, swatches: &Swatches) -> Outcome {
    let mut out = Outcome::default();
    ui.set_width(POPUP_WIDTH);
    // the square and the hue slider take their size from the slider width
    ui.spacing_mut().slider_width = POPUP_WIDTH - 2.0 * ui.spacing().item_spacing.x;
    let mut c32 = to_color32(*color);
    if color_picker_color32(ui, &mut c32, Alpha::Opaque) {
        *color = [c32.r(), c32.g(), c32.b()];
        out.changed = true;
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        hex_field(ui, id, color, &mut out);
        if ui.button(tr("Pick from sheet", "Vom Blatt aufnehmen")).clicked() {
            out.eyedropper = true;
            ui.close();
        }
    });
    section(ui, tr("Design colours", "Farben im Design"), swatches.design, color, &mut out);
    section(ui, tr("Recent", "Zuletzt verwendet"), swatches.recent, color, &mut out);
    section(ui, tr("Palette", "Palette"), &PALETTE, color, &mut out);
    out
}

/// An editable `#rrggbb` field that follows the colour and applies valid input.
fn hex_field(ui: &mut egui::Ui, id: egui::Id, color: &mut Rgb, out: &mut Outcome) {
    let text_id = id.with("hex");
    let (mut text, shown): (String, Rgb) = ui.data(|d| d.get_temp(text_id)).unwrap_or((hex(*color), *color));
    if shown != *color {
        text = hex(*color);
    }
    let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(76.0).font(egui::TextStyle::Monospace));
    if response.changed()
        && let Some(parsed) = parse_hex(&text)
    {
        *color = parsed;
        out.changed = true;
    }
    if response.lost_focus() && parse_hex(&text).is_some() {
        out.committed = true;
    }
    let remembered = if parse_hex(&text).is_some() { *color } else { shown };
    ui.data_mut(|d| d.insert_temp(text_id, (text, remembered)));
}

fn section(ui: &mut egui::Ui, title: &str, colors: &[Rgb], current: &mut Rgb, out: &mut Outcome) {
    if colors.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(RichText::new(title).weak().small());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(4.0);
        for &c in colors {
            if swatch(ui, c, *current == c).clicked() {
                *current = c;
                out.changed = true;
                out.committed = true;
            }
        }
    });
}

/// A small clickable colour chip, outlined when it is the active colour.
pub fn swatch(ui: &mut egui::Ui, color: Rgb, active: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(SWATCH), egui::Sense::click());
    ui.painter().rect_filled(rect, CornerRadius::same(4), to_color32(color));
    let stroke = if active { Stroke::new(2.0_f32, ACCENT) } else { Stroke::new(1.0_f32, Color32::from_gray(110)) };
    ui.painter().rect_stroke(rect, CornerRadius::same(4), stroke, StrokeKind::Inside);
    response.on_hover_text(hex(color))
}

fn to_color32([r, g, b]: Rgb) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub fn hex([r, g, b]: Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `#rrggbb` or `rrggbb`.
fn parse_hex(text: &str) -> Option<Rgb> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(digits, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs one frame of a panel holding the colour button.
    #[allow(deprecated)]
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, color: &mut Rgb) -> (egui::Rect, Outcome) {
        let raw = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0))),
            ..Default::default()
        };
        let mut button_rect = egui::Rect::NOTHING;
        let mut outcome = Outcome::default();
        let design = [[1, 2, 3], [4, 5, 6]];
        let swatches = Swatches { recent: &[[7, 8, 9]], design: &design };
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let before = ui.cursor().min;
                outcome = button(ui, "test-colour", color, &swatches);
                button_rect = egui::Rect::from_min_size(before, Vec2::new(96.0, 24.0));
            });
        });
        (button_rect, outcome)
    }

    #[test]
    fn the_picker_opens_without_changing_the_colour() {
        let ctx = egui::Context::default();
        let mut color: Rgb = [10, 20, 30];
        let (rect, _) = frame(&ctx, vec![], &mut color);
        let click = |pressed| egui::Event::PointerButton {
            pos: rect.center(),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&ctx, vec![egui::Event::PointerMoved(rect.center())], &mut color);
        frame(&ctx, vec![click(true)], &mut color);
        frame(&ctx, vec![click(false)], &mut color);
        // the popup is open now: it must lay out without panicking
        let (_, outcome) = frame(&ctx, vec![], &mut color);
        assert!(!outcome.committed);
        assert_eq!(color, [10, 20, 30]);
        assert!(egui::Popup::is_any_open(&ctx));
    }

    #[test]
    fn hex_round_trips_and_rejects_nonsense() {
        assert_eq!(parse_hex("#b62fc4"), Some([0xb6, 0x2f, 0xc4]));
        assert_eq!(parse_hex(" 1A1a1a "), Some([0x1a, 0x1a, 0x1a]));
        assert_eq!(hex([1, 2, 255]), "#0102ff");
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#gggggg"), None);
        assert_eq!(parse_hex(""), None);
    }
}
