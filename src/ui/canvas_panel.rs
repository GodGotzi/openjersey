//! The 2D editor panel: a toolbox down the left, a bar with the options of
//! the current tool on top, the canvas with its rulers, and a status bar
//! with the cursor position, the selection's size and the zoom.

use myth::egui::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, Vec2};

use crate::editor::{CanvasView, Editor, Tool};
use crate::i18n::tr;
use crate::render::geom;
use crate::ui::color_picker::{self, Swatches};
use crate::workspace::Workspace;

const TOOL_BUTTON: f32 = 34.0;
const STATUS_HEIGHT: f32 = 24.0;
const TOOLTIP_WIDTH: f32 = 260.0;
const ACCENT: Color32 = Color32::from_rgb(70, 140, 255);

/// Tools in the toolbox, grouped by what they do.
const TOOL_GROUPS: [&[Tool]; 5] = [
    &[Tool::Select, Tool::Hand],
    &[Tool::Brush, Tool::Eraser],
    &[Tool::Rect, Tool::Ellipse, Tool::Polygon],
    &[Tool::Text, Tool::Image, Tool::Placeholder],
    &[Tool::Fill, Tool::Eyedropper],
];

pub fn show(ui: &mut egui::Ui, editor: &mut Editor, ws: &mut Workspace, view: CanvasView<'_>) {
    options_bar(ui, editor, &ws.doc.used_colors());
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
    response.on_hover_ui(|ui| {
        ui.set_max_width(TOOLTIP_WIDTH);
        ui.label(RichText::new(tool.name()).strong());
        ui.label(tool.hint());
        ui.weak(format!("{}: {}", tr("tooltip.shortcut"), tool.shortcut()));
    })
}

/// The colour and the options of the current tool.
fn options_bar(ui: &mut egui::Ui, editor: &mut Editor, design_colors: &[crate::document::Rgb]) {
    let frame = egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(8, 5));
    let recent = editor.recent.clone();
    let swatches = Swatches { recent: &recent, design: design_colors };
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(editor.tool.name()).strong());
            ui.separator();
            let outcome = color_picker::button(ui, "paint-colour", &mut editor.color, &swatches);
            if outcome.committed {
                editor.remember(editor.color);
            }
            if outcome.eyedropper {
                editor.set_tool(Tool::Eyedropper);
            }
            for colour in color_picker::QUICK {
                if color_picker::swatch(ui, colour, editor.color == colour).clicked() {
                    editor.color = colour;
                    editor.remember(colour);
                }
            }
            tool_options(ui, editor, &swatches);
        });
    });
}

fn tool_options(ui: &mut egui::Ui, e: &mut Editor, swatches: &Swatches) {
    match e.tool {
        Tool::Brush | Tool::Eraser => {
            ui.separator();
            ui.label(tr("options.size"));
            ui.add(egui::Slider::new(&mut e.brush_mm, 1.0..=80.0).suffix(" mm"));
        }
        Tool::Rect | Tool::Ellipse | Tool::Polygon => {
            ui.separator();
            ui.checkbox(&mut e.fill_shapes, tr("options.fill"));
            ui.checkbox(&mut e.stroke_shapes, tr("options.outline"));
            if e.stroke_shapes {
                if color_picker::button(ui, "stroke-colour", &mut e.stroke_color, swatches).committed {
                    e.remember(e.stroke_color);
                }
                ui.add(egui::Slider::new(&mut e.brush_mm, 1.0..=40.0).suffix(" mm"));
            }
        }
        Tool::Text => {
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut e.text_input).desired_width(120.0));
            ui.label(tr("options.cap_height"));
            ui.add(egui::Slider::new(&mut e.text_mm, 5.0..=250.0).suffix(" mm"));
        }
        Tool::Placeholder => {
            ui.separator();
            ui.label(tr("options.label"));
            ui.add(egui::TextEdit::singleline(&mut e.placeholder_label).desired_width(80.0));
        }
        Tool::Image => {
            ui.separator();
            if ui.button(tr("options.choose_file")).clicked() {
                e.choose_image();
            }
            if let Some(a) = &e.pending_image {
                ui.weak(format!("{} – {}", a.name, tr("options.click_to_place")));
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
                ui.weak(format!("{} {:.1} × {:.1} mm", tr("status.selection"), (x1 - x0) / mm, (y1 - y0) / mm));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(tr("status.fit")).on_hover_text(tr("status.fit_hint")).clicked() {
                    editor.fit_view();
                }
                if ui.small_button("+").on_hover_text(tr("status.zoom_in")).clicked() {
                    editor.zoom_by(1.25);
                }
                ui.label(RichText::new(format!("{:.0}%", editor.zoom() * 100.0)).monospace());
                if ui.small_button("−").on_hover_text(tr("status.zoom_out")).clicked() {
                    editor.zoom_by(0.8);
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame of a panel holding a tool button, `time` seconds into the run;
    /// returns the button's response id and rectangle.
    #[allow(deprecated)]
    fn frame(ctx: &egui::Context, time: f64, events: Vec<egui::Event>) -> (egui::Id, egui::Rect) {
        let raw = egui::RawInput {
            time: Some(time),
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(400.0, 300.0))),
            ..Default::default()
        };
        let mut shown = (egui::Id::NULL, egui::Rect::NOTHING);
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = tool_button(ui, Tool::Brush, false);
                shown = (response.id, response.rect);
            });
        });
        shown
    }

    #[test]
    fn hovering_a_tool_button_shows_its_tooltip() {
        let ctx = egui::Context::default();
        ctx.global_style_mut(|style| style.interaction.tooltip_delay = 0.25);
        let (id, rect) = frame(&ctx, 0.0, vec![]);
        let hover = vec![egui::Event::PointerMoved(rect.center())];
        frame(&ctx, 0.1, hover);
        assert!(!egui::Tooltip::was_tooltip_open_last_frame(&ctx, id), "not yet: the pointer has only just arrived");
        for step in 2..12 {
            frame(&ctx, step as f64 * 0.1, vec![]);
        }
        frame(&ctx, 1.3, vec![]);
        assert!(egui::Tooltip::was_tooltip_open_last_frame(&ctx, id), "the pointer rested on the button; its tooltip should be showing");
    }

    #[test]
    fn every_tool_has_a_tooltip_text_and_a_shortcut() {
        for tool in Tool::ALL {
            assert!(!tool.hint().is_empty() && !tool.name().is_empty() && !tool.shortcut().is_empty());
            assert!(!tool.hint().starts_with("tool."), "{:?} has no hint text", tool.name());
        }
    }
}
