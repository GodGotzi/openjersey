//! The 2D canvas: the print template with every garment piece laid out like
//! the ninesquared template, a transform gizmo for the selection, and the
//! drawing tools.

use std::sync::Arc;

use myth::egui::egui::{self, Color32, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextureId, Vec2};

use crate::document::{map, unmap, Asset, Garments, Object, ObjectId, ObjectKind, Rgb, StrokeStyle, Xf};
use crate::render::geom;
use crate::template::pattern::{Pattern, Pt};
use crate::template::regions::hit_test;
use crate::workspace::{Guide, Workspace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Hand,
    Brush,
    Eraser,
    Rect,
    Ellipse,
    Polygon,
    Text,
    Image,
    Placeholder,
    Fill,
    Eyedropper,
}

impl Tool {
    pub const ALL: [Tool; 12] = [
        Tool::Select,
        Tool::Hand,
        Tool::Brush,
        Tool::Eraser,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Polygon,
        Tool::Text,
        Tool::Image,
        Tool::Placeholder,
        Tool::Fill,
        Tool::Eyedropper,
    ];

    pub fn icon(self) -> &'static str {
        match self {
            Tool::Select => "⬉",
            Tool::Hand => "✋",
            Tool::Brush => "✏",
            Tool::Eraser => "🗑",
            Tool::Rect => "⬜",
            Tool::Ellipse => "⭕",
            Tool::Polygon => "⬟",
            Tool::Text => "T",
            Tool::Image => "🖼",
            Tool::Placeholder => "🔲",
            Tool::Fill => "🎨",
            Tool::Eyedropper => "💧",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Auswahl / Transformieren (V)",
            Tool::Hand => "Hand: Ansicht verschieben (H)",
            Tool::Brush => "Pinsel (B)",
            Tool::Eraser => "Radierer: Pinselstriche löschen (E)",
            Tool::Rect => "Rechteck (R)",
            Tool::Ellipse => "Ellipse (O)",
            Tool::Polygon => "Polygon: klicken, Doppelklick/Enter schließt (P)",
            Tool::Text => "Text (T)",
            Tool::Image => "Bild / SVG importieren (I)",
            Tool::Placeholder => "Logo-Platzhalter aufziehen (L)",
            Tool::Fill => "Füllen: Teil oder Objekt einfärben (F)",
            Tool::Eyedropper => "Pipette (Y)",
        }
    }

    fn key(self) -> Key {
        match self {
            Tool::Select => Key::V,
            Tool::Hand => Key::H,
            Tool::Brush => Key::B,
            Tool::Eraser => Key::E,
            Tool::Rect => Key::R,
            Tool::Ellipse => Key::O,
            Tool::Polygon => Key::P,
            Tool::Text => Key::T,
            Tool::Image => Key::I,
            Tool::Placeholder => Key::L,
            Tool::Fill => Key::F,
            Tool::Eyedropper => Key::Y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Handle {
    Corner(usize),
    Rotate,
}

enum Drag {
    Pan,
    Move,
    Scale { id: ObjectId, start: Xf, corner: Pt, uniform: bool },
    Rotate { id: ObjectId, start: Xf, centre: Pt, start_angle: f32 },
    Marquee { start: Pt },
    Brush { id: ObjectId, points: Vec<Pt> },
    Box { start: Pt, tool: Tool },
    Erase,
}

pub struct Editor {
    pub tool: Tool,
    pub color: Rgb,
    pub stroke_color: Rgb,
    pub fill_shapes: bool,
    pub stroke_shapes: bool,
    pub brush_mm: f32,
    pub text_input: String,
    pub text_mm: f32,
    pub placeholder_label: String,
    pub pending_image: Option<Arc<Asset>>,
    polygon: Vec<Pt>,
    zoom: f32,
    pan: Vec2,
    drag: Option<Drag>,
    marquee_end: Pt,
    /// set by the eyedropper; read by the app to show it
    pub picked: Option<Rgb>,
}

/// What the canvas needs from the app besides the workspace.
pub struct CanvasView<'a> {
    pub texture: TextureId,
    /// last rendered template, for the eyedropper
    pub pixels: Option<&'a tiny_skia::Pixmap>,
}

const HANDLE: f32 = 5.0;
const ROTATE_OFFSET: f32 = 26.0;
const SELECTION: Color32 = Color32::from_rgb(70, 140, 255);

impl Editor {
    pub fn new() -> Self {
        Self {
            tool: Tool::Select,
            color: [0x2e, 0xa0, 0x4f],
            stroke_color: [0xff, 0xff, 0xff],
            fill_shapes: true,
            stroke_shapes: false,
            brush_mm: 8.0,
            text_input: "TEXT".into(),
            text_mm: 40.0,
            placeholder_label: "LOGO".into(),
            pending_image: None,
            polygon: Vec::new(),
            zoom: 1.0,
            pan: Vec2::ZERO,
            drag: None,
            marquee_end: [0.0, 0.0],
            picked: None,
        }
    }

    pub fn set_tool(&mut self, tool: Tool) {
        if tool != Tool::Polygon {
            self.polygon.clear();
        }
        self.tool = tool;
        if tool == Tool::Image {
            self.choose_image();
        }
    }

    pub fn choose_image(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Bild / Logo", &["svg", "png", "jpg", "jpeg"]).pick_file() {
            match Asset::from_file(&path) {
                Ok(asset) => self.pending_image = Some(Arc::new(asset)),
                Err(err) => eprintln!("{err}"),
            }
        }
    }

    /// Tool shortcuts (when no text field has focus).
    pub fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || ctx.input(|i| i.modifiers.command) {
            return;
        }
        for tool in Tool::ALL {
            if ctx.input(|i| i.key_pressed(tool.key())) {
                self.set_tool(tool);
            }
        }
    }

    pub fn canvas(&mut self, ui: &mut egui::Ui, ws: &mut Workspace, view: CanvasView<'_>) {
        let (tw, th) = ws.pattern.size();
        let template = Vec2::new(tw as f32, th as f32);
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        // fit the pieces that are shown (the shirt alone when the shorts are left out)
        let (focus_min, focus_size) = shown_area(&ws.pattern, &ws.doc.garments).unwrap_or((Vec2::ZERO, template));
        let focus_centre = focus_min + focus_size / 2.0;
        let fit = (rect.width() / focus_size.x).min(rect.height() / focus_size.y);
        let scale = fit * self.zoom;
        let origin = rect.center() - focus_centre * scale + self.pan;
        let to_t = |p: Pos2| -> Pt { [(p.x - origin.x) / scale, (p.y - origin.y) / scale] };
        let to_s = |p: Pt| Pos2::new(origin.x + p[0] * scale, origin.y + p[1] * scale);

        // zoom around the cursor
        if let Some(hover) = response.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let before = to_t(hover);
                self.zoom = (self.zoom * (scroll * 0.002).exp()).clamp(0.5, 16.0);
                let s = fit * self.zoom;
                let o = rect.center() - focus_centre * s + self.pan;
                self.pan += hover - Pos2::new(o.x + before[0] * s, o.y + before[1] * s);
            }
        }

        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let at = pointer.map(to_t);
        let mods = ui.input(|i| i.modifiers);
        let pan_button = ui.input(|i| i.pointer.secondary_down() || i.pointer.middle_down());

        // ---- input ----
        if response.drag_started() && (pan_button || self.tool == Tool::Hand || ui.input(|i| i.key_down(Key::Space))) {
            self.drag = Some(Drag::Pan);
        }
        if let Some(Drag::Pan) = self.drag {
            self.pan += response.drag_delta();
        } else if let Some(p) = at {
            self.tool_input(ui, ws, &response, p, scale, mods, view.pixels, &to_s);
        }
        if response.drag_stopped() {
            self.finish_drag(ws);
        }
        if response.hovered() {
            self.keys(ui, ws);
        }

        // ---- drawing ----
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(40));
        let image_rect = Rect::from_min_size(origin, template * scale);
        painter.image(view.texture, image_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        draw_labels(&painter, &ws.pattern, &ws.doc.garments, to_s, scale);

        for guide in &ws.guides {
            let c = Color32::from_rgb(255, 60, 200);
            match *guide {
                Guide::Vertical(x) => {
                    painter.line_segment([to_s([x, 0.0]), to_s([x, template.y])], Stroke::new(1.0_f32, c))
                }
                Guide::Horizontal(y) => {
                    painter.line_segment([to_s([0.0, y]), to_s([template.x, y])], Stroke::new(1.0_f32, c))
                }
            };
        }
        self.draw_selection(&painter, ws, &to_s);
        if let Some(Drag::Marquee { start }) = &self.drag {
            let r = Rect::from_two_pos(to_s(*start), to_s(self.marquee_end));
            painter.rect(r, 0.0, SELECTION.gamma_multiply(0.15), Stroke::new(1.0_f32, SELECTION), StrokeKind::Inside);
        }
        if let Some(Drag::Box { start, .. }) = &self.drag {
            let r = Rect::from_two_pos(to_s(*start), to_s(self.marquee_end));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::WHITE), StrokeKind::Inside);
        }
        if !self.polygon.is_empty() {
            let mut pts: Vec<Pos2> = self.polygon.iter().map(|&p| to_s(p)).collect();
            if let Some(h) = response.hover_pos() {
                pts.push(h);
            }
            painter.add(egui::Shape::line(pts, Stroke::new(1.5_f32, Color32::WHITE)));
        }
        if let Some(hover) = response.hover_pos() {
            match self.tool {
                Tool::Brush | Tool::Eraser => {
                    let r = self.brush_mm * ws.px_per_mm() * scale / 2.0;
                    painter.circle_stroke(hover, r.max(3.0), Stroke::new(1.0_f32, Color32::WHITE));
                }
                Tool::Fill => {
                    if let Some(p) = at {
                        let name = match ws.pick(p, 2.0 / scale, true) {
                            Some(id) => ws.object_name(id),
                            None => hit_test(&ws.pattern, &ws.regions, p).map(|r| r.label()).unwrap_or_default(),
                        };
                        tooltip(&painter, hover, &name);
                    }
                }
                Tool::Image if self.pending_image.is_some() => tooltip(&painter, hover, "Klicken zum Platzieren"),
                _ => {}
            }
        }
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, Color32::from_gray(25)), StrokeKind::Inside);
    }

    #[allow(clippy::too_many_arguments)]
    fn tool_input(
        &mut self,
        ui: &egui::Ui,
        ws: &mut Workspace,
        response: &egui::Response,
        p: Pt,
        scale: f32,
        mods: egui::Modifiers,
        pixels: Option<&tiny_skia::Pixmap>,
        to_s: &impl Fn(Pt) -> Pos2,
    ) {
        let tol = 4.0 / scale;
        let started = response.drag_started_by(egui::PointerButton::Primary);
        let clicked = response.clicked();
        let px_mm = ws.px_per_mm();
        self.marquee_end = p;
        match self.tool {
            Tool::Select => {
                if started {
                    if let Some((handle, id)) = self.handle_at(ws, response.interact_pointer_pos(), to_s) {
                        let Some(obj) = ws.doc.find(id).cloned() else { return };
                        let start = obj.xf;
                        ws.checkpoint();
                        let parent = ws.doc.parent_matrix(id);
                        self.drag = Some(match handle {
                            Handle::Corner(i) => {
                                let b = geom::local_bounds(&ws.raster.fonts, &obj).unwrap_or([0.0; 4]);
                                let corner = geom::corners(b)[i];
                                let uniform = !matches!(obj.kind, ObjectKind::Shape { .. } | ObjectKind::Placeholder { .. });
                                Drag::Scale { id, start, corner, uniform }
                            }
                            Handle::Rotate => {
                                let centre = map(&parent, start.pos);
                                let start_angle = (p[1] - centre[1]).atan2(p[0] - centre[0]);
                                Drag::Rotate { id, start, centre, start_angle }
                            }
                        });
                        return;
                    }
                    match ws.pick(p, tol, mods.command) {
                        Some(id) => {
                            if !ws.selection.contains(&id) {
                                if mods.shift {
                                    ws.selection.push(id);
                                } else {
                                    ws.selection = vec![id];
                                }
                            }
                            ws.begin_move(p);
                            self.drag = Some(Drag::Move);
                        }
                        None => self.drag = Some(Drag::Marquee { start: p }),
                    }
                } else if clicked {
                    match ws.pick(p, tol, mods.command) {
                        Some(id) if mods.shift => {
                            if let Some(i) = ws.selection.iter().position(|&s| s == id) {
                                ws.selection.remove(i);
                            } else {
                                ws.selection.push(id);
                            }
                        }
                        Some(id) => ws.selection = vec![id],
                        None if !mods.shift => ws.selection.clear(),
                        None => {}
                    }
                    if response.double_clicked()
                        && let Some(id) = ws.pick(p, tol, true)
                    {
                        ws.selection = vec![id];
                    }
                }
                match &self.drag {
                    Some(Drag::Move) if response.dragged() => ws.drag_move(p, 6.0 / scale),
                    Some(Drag::Scale { id, start, corner, uniform }) if response.dragged() => {
                        let (id, start, corner) = (*id, *start, *corner);
                        let uniform = *uniform != mods.shift;
                        let parent = ws.doc.parent_matrix(id);
                        // pointer in the object's unscaled frame
                        let frame = parent.pre_concat(Xf { scale: [1.0, 1.0], ..start }.matrix());
                        if let Some(q) = unmap(&frame, p) {
                            let sx = if corner[0].abs() > 1e-3 { q[0] / corner[0] } else { start.scale[0] };
                            let sy = if corner[1].abs() > 1e-3 { q[1] / corner[1] } else { start.scale[1] };
                            let scale = if uniform {
                                let f = (q[0].hypot(q[1]) / (corner[0] * start.scale[0]).hypot(corner[1] * start.scale[1])).max(0.01);
                                [start.scale[0] * f, start.scale[1] * f]
                            } else {
                                [sx.max(0.01), sy.max(0.01)]
                            };
                            if let Some(obj) = ws.doc.find_mut(id) {
                                obj.xf.scale = scale;
                            }
                            ws.dirty = true;
                        }
                    }
                    Some(Drag::Rotate { id, start, centre, start_angle }) if response.dragged() => {
                        let a = (p[1] - centre[1]).atan2(p[0] - centre[0]);
                        let mut rot = start.rotation + a - start_angle;
                        if mods.shift {
                            let step = 15f32.to_radians();
                            rot = (rot / step).round() * step;
                        }
                        let id = *id;
                        if let Some(obj) = ws.doc.find_mut(id) {
                            obj.xf.rotation = rot;
                        }
                        ws.dirty = true;
                    }
                    _ => {}
                }
            }
            Tool::Brush => {
                if started || clicked {
                    let width = self.brush_mm * px_mm;
                    let stroke = Some(StrokeStyle { color: self.color, width });
                    let id = ws.add(None, Object::new("Pinselstrich", Xf::default(), ObjectKind::Shape {
                        points: vec![p],
                        closed: false,
                        fill: None,
                        stroke,
                    }));
                    self.drag = Some(Drag::Brush { id, points: vec![p] });
                    if clicked {
                        self.finish_drag(ws);
                    }
                } else if let Some(Drag::Brush { id, points }) = &mut self.drag
                    && response.dragged()
                {
                    let last = *points.last().expect("starts with a point");
                    if (p[0] - last[0]).hypot(p[1] - last[1]) > 1.5 / scale {
                        points.push(p);
                        let pts = points.clone();
                        if let Some(ObjectKind::Shape { points, .. }) = ws.doc.find_mut(*id).map(|o| &mut o.kind) {
                            *points = pts;
                        }
                        ws.dirty = true;
                    }
                }
            }
            Tool::Eraser => {
                if started {
                    ws.checkpoint();
                    self.drag = Some(Drag::Erase);
                }
                if (matches!(self.drag, Some(Drag::Erase)) && response.dragged()) || clicked {
                    if clicked {
                        ws.checkpoint();
                    }
                    let reach = self.brush_mm * px_mm / 2.0;
                    if let Some(id) = ws.pick(p, reach, true)
                        && matches!(ws.doc.find(id).map(|o| &o.kind), Some(ObjectKind::Shape { closed: false, .. }))
                    {
                        ws.doc.remove(id);
                        ws.changed();
                    }
                }
            }
            Tool::Rect | Tool::Ellipse | Tool::Placeholder => {
                if started {
                    self.drag = Some(Drag::Box { start: p, tool: self.tool });
                }
            }
            Tool::Polygon => {
                if response.double_clicked() || (ui.input(|i| i.key_pressed(Key::Enter)) && self.polygon.len() > 2) {
                    self.finish_polygon(ws);
                } else if clicked {
                    self.polygon.push(p);
                }
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.polygon.clear();
                }
            }
            Tool::Text => {
                if clicked && !self.text_input.is_empty() {
                    ws.add(None, Object::new(self.text_input.clone(), Xf::at(p), ObjectKind::Text {
                        text: self.text_input.clone(),
                        size: self.text_mm * px_mm / 0.73,
                        color: Some(self.color),
                        fatten: 0.04,
                        italic: 0.0,
                        max_width: None,
                    }));
                    self.tool = Tool::Select;
                }
            }
            Tool::Image => {
                if clicked && let Some(asset) = self.pending_image.take() {
                    let [nw, nh] = asset.natural_size();
                    let w = 100.0 * px_mm; // 10 cm wide to start with
                    let name = asset.name.clone();
                    ws.add(None, Object::new(name, Xf::at(p), ObjectKind::Graphic {
                        asset,
                        size: [w, w * nh / nw.max(1.0)],
                        tint: None,
                    }));
                    self.tool = Tool::Select;
                }
            }
            Tool::Fill => {
                if clicked {
                    if let Some(id) = ws.pick(p, tol, true) {
                        ws.checkpoint();
                        let color = self.color;
                        if let Some(obj) = ws.doc.find_mut(id) {
                            match &mut obj.kind {
                                ObjectKind::Text { color: c, .. } => *c = Some(color),
                                ObjectKind::Shape { fill: Some(f), .. } => *f = color,
                                ObjectKind::Shape { stroke: Some(s), .. } => s.color = color,
                                ObjectKind::Graphic { tint, .. } => *tint = Some(color),
                                ObjectKind::Pattern { color: c, .. } => *c = color,
                                _ => {}
                            }
                        }
                        ws.changed();
                    } else if let Some(region) = hit_test(&ws.pattern, &ws.regions, p) {
                        ws.checkpoint();
                        ws.doc.base.set_region(region, self.color);
                        ws.changed();
                    }
                }
            }
            Tool::Eyedropper => {
                if clicked && let Some(pm) = pixels {
                    if let Some(c) = crate::render::Rasterizer::pick_color(pm, p) {
                        self.color = c;
                        self.picked = Some(c);
                    }
                }
            }
            Tool::Hand => {}
        }
    }

    fn finish_polygon(&mut self, ws: &mut Workspace) {
        let points = std::mem::take(&mut self.polygon);
        if points.len() < 3 {
            return;
        }
        let px = ws.px_per_mm();
        ws.add(None, self.shape_object("Polygon", points, px));
        self.tool = Tool::Select;
    }

    fn shape_object(&self, name: &str, points: Vec<Pt>, px_per_mm: f32) -> Object {
        let fill = (self.fill_shapes || !self.stroke_shapes).then_some(self.color);
        let stroke = self.stroke_shapes.then_some(StrokeStyle { color: self.stroke_color, width: self.brush_mm * px_per_mm });
        Object::shape(name, points, true, fill, stroke)
    }

    fn finish_drag(&mut self, ws: &mut Workspace) {
        match self.drag.take() {
            Some(Drag::Move) => ws.end_drag(),
            Some(Drag::Scale { .. } | Drag::Rotate { .. }) => ws.changed(),
            Some(Drag::Marquee { start }) => {
                let end = self.marquee_end;
                let sel = [start[0].min(end[0]), start[1].min(end[1]), start[0].max(end[0]), start[1].max(end[1])];
                let ids: Vec<ObjectId> = ws
                    .doc
                    .objects
                    .iter()
                    .filter(|o| o.visible && !o.locked)
                    .filter(|o| {
                        geom::world_bounds(&ws.raster.fonts, &ws.doc, &[o.id]).is_some_and(|[x0, y0, x1, y1]| {
                            x0 >= sel[0] && y0 >= sel[1] && x1 <= sel[2] && y1 <= sel[3]
                        })
                    })
                    .map(|o| o.id)
                    .collect();
                ws.selection = ids;
            }
            Some(Drag::Brush { id, points }) => {
                // re-centre the stroke on its points
                let replacement = ws.doc.find(id).map(|o| {
                    let ObjectKind::Shape { stroke, .. } = &o.kind else { return o.clone() };
                    let mut new = Object::shape("Pinselstrich", points.clone(), false, None, *stroke);
                    new.id = o.id;
                    new
                });
                if let Some(new) = replacement
                    && let Some(obj) = ws.doc.find_mut(id)
                {
                    *obj = new;
                }
                ws.selection.clear();
                ws.changed();
            }
            Some(Drag::Box { start, tool }) => {
                let end = self.marquee_end;
                let (x0, y0, x1, y1) = (start[0].min(end[0]), start[1].min(end[1]), start[0].max(end[0]), start[1].max(end[1]));
                if x1 - x0 < 2.0 || y1 - y0 < 2.0 {
                    return;
                }
                let centre = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
                let px = ws.px_per_mm();
                match tool {
                    Tool::Rect => {
                        ws.add(None, self.shape_object("Rechteck", vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], px));
                    }
                    Tool::Ellipse => {
                        let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
                        let pts = (0..96)
                            .map(|i| {
                                let a = i as f32 / 96.0 * std::f32::consts::TAU;
                                [centre[0] + rx * a.cos(), centre[1] + ry * a.sin()]
                            })
                            .collect();
                        ws.add(None, self.shape_object("Ellipse", pts, px));
                    }
                    _ => {
                        let label = self.placeholder_label.clone();
                        ws.add(None, Object::new(format!("Platzhalter {label}"), Xf::at(centre), ObjectKind::Placeholder {
                            label,
                            size: [x1 - x0, y1 - y0],
                        }));
                    }
                }
                self.tool = Tool::Select;
            }
            Some(Drag::Erase) | Some(Drag::Pan) | None => {}
        }
    }

    fn keys(&mut self, ui: &egui::Ui, ws: &mut Workspace) {
        if ui.ctx().egui_wants_keyboard_input() || ws.selection.is_empty() {
            return;
        }
        let step = ui.input(|i| if i.modifiers.shift { 10.0 } else { 1.0 }) * ws.px_per_mm();
        let d = ui.input(|i| {
            let mut d = [0.0f32, 0.0];
            if i.key_pressed(Key::ArrowLeft) {
                d[0] -= step;
            }
            if i.key_pressed(Key::ArrowRight) {
                d[0] += step;
            }
            if i.key_pressed(Key::ArrowUp) {
                d[1] -= step;
            }
            if i.key_pressed(Key::ArrowDown) {
                d[1] += step;
            }
            d
        });
        if d != [0.0, 0.0] {
            ws.checkpoint_tagged("nudge");
            ws.translate(&ws.selection.clone(), d);
            ws.changed();
        }
    }

    /// Handle under the pointer for a single selected object.
    fn handle_at(&self, ws: &Workspace, pos: Option<Pos2>, to_s: &impl Fn(Pt) -> Pos2) -> Option<(Handle, ObjectId)> {
        let pos = pos?;
        let [id] = ws.selection.as_slice() else { return None };
        if ws.doc.find(*id)?.locked {
            return None;
        }
        let quad = geom::world_quad(&ws.raster.fonts, &ws.doc, *id)?.map(to_s);
        if let Some(r) = rotate_handle(&quad)
            && r.distance(pos) <= HANDLE + 3.0
        {
            return Some((Handle::Rotate, *id));
        }
        quad.iter().position(|c| c.distance(pos) <= HANDLE + 3.0).map(|i| (Handle::Corner(i), *id))
    }

    fn draw_selection(&self, painter: &egui::Painter, ws: &Workspace, to_s: &impl Fn(Pt) -> Pos2) {
        for &id in &ws.selection {
            let Some(quad) = geom::world_quad(&ws.raster.fonts, &ws.doc, id) else { continue };
            let q: Vec<Pos2> = quad.iter().map(|&p| to_s(p)).collect();
            painter.add(egui::Shape::closed_line(q.clone(), Stroke::new(1.5_f32, SELECTION)));
            if ws.selection.len() == 1 && !ws.doc.find(id).is_some_and(|o| o.locked) {
                let quad = [q[0], q[1], q[2], q[3]];
                for c in quad {
                    painter.rect(Rect::from_center_size(c, Vec2::splat(HANDLE * 2.0)), 1.0, Color32::WHITE, Stroke::new(1.0_f32, SELECTION), StrokeKind::Middle);
                }
                if let Some(r) = rotate_handle(&quad) {
                    let top = Pos2::new((quad[0].x + quad[1].x) / 2.0, (quad[0].y + quad[1].y) / 2.0);
                    painter.line_segment([top, r], Stroke::new(1.0_f32, SELECTION));
                    painter.circle(r, HANDLE, Color32::WHITE, Stroke::new(1.0_f32, SELECTION));
                }
            }
        }
    }
}

/// Above the middle of the top edge, pointing away from the centre.
fn rotate_handle(q: &[Pos2; 4]) -> Option<Pos2> {
    let top = Pos2::new((q[0].x + q[1].x) / 2.0, (q[0].y + q[1].y) / 2.0);
    let bottom = Pos2::new((q[2].x + q[3].x) / 2.0, (q[2].y + q[3].y) / 2.0);
    let dir = (top - bottom).normalized();
    (dir.length() > 0.5).then(|| top + dir * ROTATE_OFFSET)
}

fn tooltip(painter: &egui::Painter, at: Pos2, text: &str) {
    if text.is_empty() {
        return;
    }
    let galley = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(13.0), Color32::WHITE);
    let r = Rect::from_min_size(at + Vec2::new(14.0, 14.0), galley.size() + Vec2::splat(8.0));
    painter.rect_filled(r, 3.0, Color32::from_black_alpha(200));
    painter.galley(r.min + Vec2::splat(4.0), galley, Color32::WHITE);
}

/// The shown pieces' bounds with room for their labels and a margin
/// (template pixels): top-left and size.
fn shown_area(pattern: &Pattern, garments: &Garments) -> Option<(Vec2, Vec2)> {
    let rects = pattern.pieces.iter().filter(|p| garments.shows_piece(p.id)).map(|p| p.rect);
    let [x0, y0, x1, y1] = rects.fold(None, |acc: Option<[f32; 4]>, [x, y, w, h]| {
        let [a, b, c, d] = acc.unwrap_or([x, y, x + w, y + h]);
        Some([a.min(x), b.min(y), c.max(x + w), d.max(y + h)])
    })?;
    let margin = 0.04 * pattern.atlas.px_per_m;
    let label = 0.06 * pattern.atlas.px_per_m;
    let (w, h) = pattern.size();
    let min = Vec2::new((x0 - margin).max(0.0), (y0 - label - margin).max(0.0));
    let max = Vec2::new((x1 + margin).min(w as f32), (y1 + margin).min(h as f32));
    Some((min, max - min))
}

/// Piece names above each piece, like the ninesquared template; a leg piece
/// gets its back and front half named.
fn draw_labels(painter: &egui::Painter, pattern: &Pattern, garments: &Garments, to_screen: impl Fn(Pt) -> Pos2, scale: f32) {
    // labels only once there is room for them
    if scale < 0.22 {
        return;
    }
    let font = egui::FontId::proportional((22.0 * scale).clamp(9.0, 16.0));
    let color = Color32::from_gray(90);
    let left = to_screen([0.0, 0.0]).x;
    let label = |at: Pt, text: &str| {
        let pos = to_screen(at);
        let half = painter.layout_no_wrap(text.into(), font.clone(), color).size().x / 2.0;
        let pos = Pos2::new(pos.x.max(left + half + 2.0), pos.y);
        painter.text(pos, egui::Align2::CENTER_BOTTOM, text, font.clone(), color);
    };
    for piece in pattern.pieces.iter().filter(|p| garments.shows_piece(p.id)) {
        let [x, y, w, _] = piece.rect;
        match pattern.legs.iter().find(|l| l.piece == piece.id) {
            Some(leg) => {
                let name = piece.label.replace(" LEG", "");
                for (half, prefix) in [(0.25, "FRONT"), (-0.25, "BACK")] {
                    label([x + w * (0.5 + leg.front_dir * half), y - 8.0], &format!("{prefix} {name} LEG"));
                }
            }
            None => label([x + w / 2.0, y - 8.0], &piece.label),
        }
    }
}
