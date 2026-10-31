//! The 2D editor panel: a toolbox down the left, a bar with the options of
//! the current tool on top, the canvas with its rulers, and a status bar
//! with the cursor position, the selection's size and the zoom.

use myth::egui::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, Vec2};

use crate::editor::{CanvasView, Editor, Tool};
use crate::i18n::tr;
use crate::render::geom;
use crate::shapes::ShapeKind;
use crate::ui::color_picker::{self, Swatches};
use crate::workspace::Workspace;

const TOOL_BUTTON: f32 = 34.0;
const STATUS_HEIGHT: f32 = 24.0;
const TOOLTIP_WIDTH: f32 = 260.0;
const SHAPE_ICON_INSET: f32 = 9.0;
const SHAPE_MENU_WIDTH: f32 = 190.0;
const SHAPE_ROW_HEIGHT: f32 = 30.0;

/// Tools in the toolbox, grouped by what they do.
const TOOL_GROUPS: [&[Tool]; 5] = [
    &[Tool::Select, Tool::Hand],
    &[Tool::Brush, Tool::Eraser],
    &[Tool::OutlineShape, Tool::FilledShape, Tool::Polygon],
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
                if matches!(tool, Tool::OutlineShape | Tool::FilledShape) {
                    shape_tool_button(ui, editor, tool);
                } else if tool_button(ui, tool, editor.tool == tool).clicked() {
                    editor.set_tool(tool);
                }
            }
        }
    });
}

fn tool_button(ui: &mut egui::Ui, tool: Tool, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(TOOL_BUTTON), egui::Sense::click());
    paint_button_background(ui, rect, &response, selected);
    let color = if selected { crate::theme::on_accent() } else { ui.visuals().text_color() };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, tool.icon(), egui::FontId::proportional(17.0), color);
    tool_tooltip(response, tool)
}

fn paint_button_background(ui: &egui::Ui, rect: egui::Rect, response: &egui::Response, selected: bool) {
    let fill = if selected {
        crate::theme::accent()
    } else if response.hovered() {
        ui.visuals().widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
}

/// Name, what the tool does, and its shortcut.
fn tool_tooltip(response: egui::Response, tool: Tool) -> egui::Response {
    response.on_hover_ui(|ui| {
        ui.set_max_width(TOOLTIP_WIDTH);
        ui.label(RichText::new(tool.name()).strong());
        ui.label(tool.hint());
        ui.weak(format!("{}: {}", tr("tooltip.shortcut"), tool.shortcut()));
    })
}

/// A shape tool's button: it shows the shape currently chosen for the tool,
/// and a click opens a flyout beside it to choose another.
fn shape_tool_button(ui: &mut egui::Ui, editor: &mut Editor, tool: Tool) {
    let selected = editor.tool == tool;
    let filled = tool == Tool::FilledShape;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(TOOL_BUTTON), egui::Sense::click());
    paint_button_background(ui, rect, &response, selected);
    let color = if selected { crate::theme::on_accent() } else { ui.visuals().text_color() };
    paint_shape_icon(ui.painter(), editor.shape_of(tool), rect.shrink(SHAPE_ICON_INSET), filled, color);
    // a small triangle in the corner says there is more to choose from
    let corner = rect.right_bottom() - Vec2::splat(4.0);
    let marker = vec![corner, corner - Vec2::new(5.0, 0.0), corner - Vec2::new(0.0, 5.0)];
    ui.painter().add(egui::Shape::convex_polygon(marker, color.gamma_multiply(0.7), Stroke::NONE));
    if response.clicked() {
        editor.set_tool(tool);
    }
    let response = tool_tooltip(response, tool);
    egui::Popup::from_toggle_button_response(&response)
        .align(egui::RectAlign::RIGHT_START)
        .gap(6.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(SHAPE_MENU_WIDTH);
            for shape in shape_choices(tool) {
                if shape_row(ui, shape, filled, editor.shape_of(tool) == shape).clicked() {
                    editor.set_shape(tool, shape);
                    editor.set_tool(tool);
                    ui.close();
                }
            }
        });
}

/// What a shape tool's flyout offers: a line has no inside to fill.
fn shape_choices(tool: Tool) -> Vec<ShapeKind> {
    ShapeKind::ALL.into_iter().filter(|shape| shape.closed() || tool != Tool::FilledShape).collect()
}

/// One entry of a shape flyout: the shape and its name.
fn shape_row(ui: &mut egui::Ui, shape: ShapeKind, filled: bool, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), SHAPE_ROW_HEIGHT), egui::Sense::click());
    paint_button_background(ui, rect, &response, false);
    if selected {
        ui.painter().rect_filled(rect, CornerRadius::same(5), crate::theme::accent().gamma_multiply(0.4));
    }
    let icon =
        egui::Rect::from_center_size(rect.left_center() + Vec2::new(SHAPE_ROW_HEIGHT / 2.0, 0.0), Vec2::splat(SHAPE_ROW_HEIGHT - 12.0));
    paint_shape_icon(ui.painter(), shape, icon, filled, ui.visuals().text_color());
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.painter().text(
        rect.left_center() + Vec2::new(SHAPE_ROW_HEIGHT + 4.0, 0.0),
        egui::Align2::LEFT_CENTER,
        shape.name(),
        font,
        ui.visuals().text_color(),
    );
    response
}

/// A shape's outline, or filled, inside `rect`.
fn paint_shape_icon(painter: &egui::Painter, shape: ShapeKind, rect: egui::Rect, filled: bool, color: Color32) {
    let outline = shape.outline([rect.min.x, rect.min.y], [rect.max.x, rect.max.y]);
    if filled && shape.closed() {
        let mut mesh = egui::Mesh::default();
        for [a, b, c] in crate::shapes::triangulate(&outline) {
            for [x, y] in [a, b, c] {
                mesh.colored_vertex(egui::pos2(x, y), color);
            }
            let first = mesh.vertices.len() as u32 - 3;
            mesh.add_triangle(first, first + 1, first + 2);
        }
        painter.add(egui::Shape::mesh(mesh));
    } else {
        let points: Vec<egui::Pos2> = outline.into_iter().map(|[x, y]| egui::pos2(x, y)).collect();
        let stroke = Stroke::new(1.6_f32, color);
        painter.add(if shape.closed() { egui::Shape::closed_line(points, stroke) } else { egui::Shape::line(points, stroke) });
    }
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
        Tool::Polygon => {
            ui.separator();
            fill_and_outline(ui, e, swatches);
        }
        Tool::OutlineShape | Tool::FilledShape => {
            ui.separator();
            ui.label(RichText::new(e.shape_of(e.tool).name()).weak());
            if e.tool == Tool::OutlineShape {
                ui.label(tr("options.size"));
                ui.add(egui::Slider::new(&mut e.brush_mm, 1.0..=40.0).suffix(" mm"));
            } else {
                outline_options(ui, e, swatches);
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

/// The fill switch and the outline options, as a polygon has them.
fn fill_and_outline(ui: &mut egui::Ui, e: &mut Editor, swatches: &Swatches) {
    ui.checkbox(&mut e.fill_shapes, tr("options.fill"));
    outline_options(ui, e, swatches);
}

/// The outline switch and, when it is on, the outline's colour and width.
fn outline_options(ui: &mut egui::Ui, e: &mut Editor, swatches: &Swatches) {
    ui.checkbox(&mut e.stroke_shapes, tr("options.outline"));
    if e.stroke_shapes {
        if color_picker::button(ui, "stroke-colour", &mut e.stroke_color, swatches).committed {
            e.remember(e.stroke_color);
        }
        ui.add(egui::Slider::new(&mut e.brush_mm, 1.0..=40.0).suffix(" mm"));
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
    fn the_filled_flyout_has_no_line_and_the_outline_flyout_has_every_shape() {
        assert_eq!(shape_choices(Tool::OutlineShape), ShapeKind::ALL.to_vec());
        let filled = shape_choices(Tool::FilledShape);
        assert_eq!(filled.len(), ShapeKind::ALL.len() - 1);
        assert!(!filled.contains(&ShapeKind::Line));
    }

    #[test]
    fn clicking_a_shape_button_selects_its_tool_and_opens_the_flyout() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new();
        let mut rect = egui::Rect::NOTHING;
        #[allow(deprecated)]
        let run = |events: Vec<egui::Event>, editor: &mut Editor, rect: &mut egui::Rect| {
            let raw = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(500.0, 400.0))),
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let top = ui.cursor().min;
                    shape_tool_button(ui, editor, Tool::OutlineShape);
                    *rect = egui::Rect::from_min_size(top, Vec2::splat(TOOL_BUTTON));
                });
            });
        };
        run(vec![], &mut editor, &mut rect);
        let centre = rect.center();
        let click = |pressed| egui::Event::PointerButton {
            pos: centre,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(editor.tool, Tool::Select);
        run(vec![egui::Event::PointerMoved(centre)], &mut editor, &mut rect);
        run(vec![click(true)], &mut editor, &mut rect);
        run(vec![click(false)], &mut editor, &mut rect);
        run(vec![], &mut editor, &mut rect);
        assert_eq!(editor.tool, Tool::OutlineShape);
        assert!(egui::Popup::is_any_open(&ctx), "the flyout should be open beside the button");
    }

    #[test]
    fn every_tool_has_a_tooltip_text_and_a_shortcut() {
        for tool in Tool::ALL {
            assert!(!tool.hint().is_empty() && !tool.name().is_empty() && !tool.shortcut().is_empty());
            assert!(!tool.hint().starts_with("tool."), "{:?} has no hint text", tool.name());
        }
    }

    /// All text painted by one frame of the options bar for `tool`.
    #[allow(deprecated)]
    fn options_bar_texts(tool: Tool, outline_on: bool) -> Vec<String> {
        let ctx = egui::Context::default();
        let mut editor = Editor::new();
        editor.tool = tool;
        editor.stroke_shapes = outline_on;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 300.0))),
            ..Default::default()
        };
        let output = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| options_bar(ui, &mut editor, &[]));
        });
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_filled_shape_tool_offers_an_outline_and_the_outline_tool_does_not() {
        let outline = tr("options.outline");
        let has = |texts: &[String], wanted: &str| texts.iter().any(|t| t == wanted);
        let filled = options_bar_texts(Tool::FilledShape, false);
        assert!(has(&filled, outline), "no Outline switch for filled shapes in: {filled:?}");
        assert!(!has(&options_bar_texts(Tool::OutlineShape, false), outline), "the outline tool must not offer an Outline switch");
        // switched on, the outline's width shows up as well
        assert!(options_bar_texts(Tool::FilledShape, true).iter().any(|t| t.contains("mm")));
        assert!(!filled.iter().any(|t| t.contains("mm")));
    }
}
