//! The 2D editor panel: a toolbox down the left, a bar with the options of
//! the current tool on top, the canvas with its rulers, and a status bar
//! with the cursor position, the selection's size and the zoom.

use myth::egui::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, StrokeKind, Vec2};

use crate::document::Rgb;
use crate::editor::{CanvasView, Editor, Tool};
use crate::i18n::tr;
use crate::render::geom;
use crate::workspace::Workspace;

const TOOL_BUTTON: f32 = 34.0;
const STATUS_HEIGHT: f32 = 24.0;
const ACCENT: Color32 = Color32::from_rgb(70, 140, 255);

const PALETTE: [Rgb; 8] = [
    [0xff, 0xff, 0xff],
    [0x1a, 0x1a, 0x1a],
    [0x41, 0x41, 0x41],
    [0xb6, 0x2f, 0xc4],
    [0xd7, 0x26, 0x38],
    [0x1f, 0x5f, 0xd1],
    [0x2e, 0xa0, 0x4f],
    [0xf2, 0xc2, 0x1b],
];

/// Tools in the toolbox, grouped by what they do.
const TOOL_GROUPS: [&[Tool]; 5] = [
    &[Tool::Select, Tool::Hand],
    &[Tool::Brush, Tool::Eraser],
    &[Tool::Rect, Tool::Ellipse, Tool::Polygon],
    &[Tool::Text, Tool::Image, Tool::Placeholder],
    &[Tool::Fill, Tool::Eyedropper],
];

pub fn show(ui: &mut egui::Ui, editor: &mut Editor, ws: &mut Workspace, view: CanvasView<'_>) {
    options_bar(ui, editor);
    ui.add_space(4.0);
    let free = ui.available_size();
    let toolbox_width = TOOL_BUTTON + 2.0 * ui.spacing().item_spacing.x;
    ui.horizontal_top(|ui| {
        toolbox(ui, editor);
        let size = Vec2::new((free.x - toolbox_width).max(50.0), (free.y - STATUS_HEIGHT - 6.0).max(50.0));
        ui.allocate_ui(size, |ui| editor.canvas(ui, ws, view));
    });
    status_bar(ui, editor, ws);
}

fn toolbox(ui: &mut egui::Ui, editor: &mut Editor) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        for (i, group) in TOOL_GROUPS.iter().enumerate() {
            if i > 0 {
                ui.add_space(5.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(TOOL_BUTTON, 1.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 0.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
                ui.add_space(2.0);
            }
            for &tool in *group {
                if tool_button(ui, tool, editor.tool == tool).clicked() {
                    editor.set_tool(tool);
                }
            }
        }
    });
}

fn tool_button(ui: &mut egui::Ui, tool: Tool, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(TOOL_BUTTON), egui::Sense::click());
    let visuals = ui.visuals();
    let fill = if selected {
        ACCENT
    } else if response.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    let color = if selected { Color32::WHITE } else { visuals.text_color() };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, tool.icon(), egui::FontId::proportional(17.0), color);
    response.on_hover_text(tool.label())
}

/// The colours and the options of the current tool.
fn options_bar(ui: &mut egui::Ui, editor: &mut Editor) {
    let frame = egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(8, 5));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(editor.tool.name()).strong());
            ui.separator();
            ui.color_edit_button_srgb(&mut editor.color).on_hover_text(tr("Colour", "Farbe"));
            for colour in PALETTE {
                if swatch(ui, colour, editor.color == colour).clicked() {
                    editor.color = colour;
                }
            }
            tool_options(ui, editor);
        });
    });
}

fn tool_options(ui: &mut egui::Ui, e: &mut Editor) {
    match e.tool {
        Tool::Brush | Tool::Eraser => {
            ui.separator();
            ui.label(tr("Size", "Größe"));
            ui.add(egui::Slider::new(&mut e.brush_mm, 1.0..=80.0).suffix(" mm"));
        }
        Tool::Rect | Tool::Ellipse | Tool::Polygon => {
            ui.separator();
            ui.checkbox(&mut e.fill_shapes, tr("Fill", "Füllung"));
            ui.checkbox(&mut e.stroke_shapes, tr("Outline", "Kontur"));
            if e.stroke_shapes {
                ui.color_edit_button_srgb(&mut e.stroke_color);
                ui.add(egui::Slider::new(&mut e.brush_mm, 1.0..=40.0).suffix(" mm"));
            }
        }
        Tool::Text => {
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut e.text_input).desired_width(120.0));
            ui.label(tr("Cap height", "Versalhöhe"));
            ui.add(egui::Slider::new(&mut e.text_mm, 5.0..=250.0).suffix(" mm"));
        }
        Tool::Placeholder => {
            ui.separator();
            ui.label(tr("Text", "Beschriftung"));
            ui.add(egui::TextEdit::singleline(&mut e.placeholder_label).desired_width(80.0));
        }
        Tool::Image => {
            ui.separator();
            if ui.button(tr("Choose file…", "Datei wählen…")).clicked() {
                e.choose_image();
            }
            if let Some(a) = &e.pending_image {
                ui.weak(format!("{} – {}", a.name, tr("click to place", "klicken zum Platzieren")));
            }
        }
        Tool::Select | Tool::Hand | Tool::Fill | Tool::Eyedropper => {}
    }
}

fn status_bar(ui: &mut egui::Ui, editor: &mut Editor, ws: &Workspace) {
    ui.allocate_ui(Vec2::new(ui.available_width(), STATUS_HEIGHT), |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let cursor = match editor.cursor_mm {
                Some([x, y]) => format!("x {x:7.1}   y {y:7.1} mm"),
                None => String::new(),
            };
            ui.add_sized([150.0, STATUS_HEIGHT], egui::Label::new(RichText::new(cursor).monospace().weak()));
            if let Some([x0, y0, x1, y1]) = geom::world_bounds(&ws.raster.fonts, &ws.doc, &ws.selection) {
                let mm = ws.px_per_mm();
                ui.weak(format!("{} {:.1} × {:.1} mm", tr("Selection", "Auswahl"), (x1 - x0) / mm, (y1 - y0) / mm));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(tr("Fit", "Einpassen")).on_hover_text(tr("Fit the garment to the view", "Modell einpassen")).clicked() {
                    editor.fit_view();
                }
                if ui.small_button("+").clicked() {
                    editor.zoom_by(1.25);
                }
                ui.label(RichText::new(format!("{:.0}%", editor.zoom() * 100.0)).monospace());
                if ui.small_button("−").clicked() {
                    editor.zoom_by(0.8);
                }
            });
        });
    });
}

fn swatch(ui: &mut egui::Ui, [r, g, b]: Rgb, active: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    ui.painter().rect_filled(rect, 4.0, Color32::from_rgb(r, g, b));
    let stroke = if active { Stroke::new(2.0_f32, ACCENT) } else { Stroke::new(1.0_f32, Color32::from_gray(110)) };
    ui.painter().rect_stroke(rect, 4.0, stroke, StrokeKind::Inside);
    response
}
