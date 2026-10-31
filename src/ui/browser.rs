//! The garment browser: a window with a searchable grid of cards, one per
//! garment, each showing the outline of its print template. Replaces the
//! old nested menu.

use std::collections::HashMap;

use myth::egui::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use serde::Deserialize;

use crate::garments::{self, Body, Garment, Kind};
use crate::i18n::tr;
use crate::workspace::Workspace;

const CARD: Vec2 = Vec2::new(116.0, 150.0);
const THUMB_PAD: f32 = 10.0;

/// The outline of a garment's print template, in 0..1 coordinates of its
/// atlas (so it can be drawn at any size).
struct Outline {
    aspect: f32,
    loops: Vec<Vec<Pos2>>,
}

#[derive(Deserialize)]
struct SheetFile {
    atlas: AtlasFile,
    pieces: Vec<PieceFile>,
}

#[derive(Deserialize)]
struct AtlasFile {
    width: f32,
    height: f32,
}

#[derive(Deserialize)]
struct PieceFile {
    polygons: Vec<Vec<[f32; 2]>>,
}

impl Outline {
    fn of(garment: &Garment) -> Outline {
        let Ok(sheet) = serde_json::from_str::<SheetFile>(garment.pattern_json) else {
            return Outline { aspect: 1.0, loops: Vec::new() };
        };
        let (w, h) = (sheet.atlas.width, sheet.atlas.height);
        let loops = sheet
            .pieces
            .iter()
            .flat_map(|p| &p.polygons)
            .map(|poly| poly.iter().map(|&[x, y]| Pos2::new(x / w, y / h)).collect())
            .collect();
        Outline { aspect: w / h, loops }
    }

    fn paint(&self, painter: &egui::Painter, area: Rect, stroke: Stroke) {
        let size = if area.width() / area.height() > self.aspect {
            Vec2::new(area.height() * self.aspect, area.height())
        } else {
            Vec2::new(area.width(), area.width() / self.aspect)
        };
        let origin = area.center() - size / 2.0;
        for points in &self.loops {
            let mapped: Vec<Pos2> = points.iter().map(|p| origin + p.to_vec2() * size).collect();
            painter.add(egui::Shape::closed_line(mapped, stroke));
        }
    }
}

pub struct GarmentBrowser {
    pub open: bool,
    query: String,
    kind: Kind,
    outlines: HashMap<&'static str, Outline>,
}

impl GarmentBrowser {
    pub fn new() -> Self {
        Self { open: false, query: String::new(), kind: Kind::Top, outlines: HashMap::new() }
    }

    /// The sidebar block that names the current garments; each row opens
    /// the browser on its kind.
    pub fn sidebar(&mut self, ui: &mut egui::Ui, ws: &mut Workspace) {
        super::widgets::section_title(ui, tr("browser.model"));
        ui.add_space(2.0);
        let mut body = ws.body();
        let choices = [(Body::Man, Body::Man.label()), (Body::Women, Body::Women.label())];
        if super::widgets::segmented(ui, "body", &mut body, &choices, true) {
            ws.set_body(body);
        }
        ui.add_space(4.0);
        let rows = [(Kind::Top, tr("garment.top"), ws.pattern.top().name), (Kind::Bottom, tr("garment.bottom"), ws.pattern.bottom().name)];
        for (kind, label, name) in rows {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(56.0, 24.0), Sense::hover());
                let font = egui::TextStyle::Body.resolve(ui.style());
                ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, label, font, ui.visuals().weak_text_color());
                let selected = self.open && self.kind == kind;
                let mut button = egui::Button::new(name).min_size(Vec2::new(ui.available_width(), 0.0));
                if selected {
                    button = button.fill(ui.visuals().selection.bg_fill);
                }
                if ui.add(button).on_hover_text(tr("browser.browse")).clicked() {
                    self.kind = kind;
                    self.open = !selected;
                }
            });
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, ws: &mut Workspace) {
        let mut open = self.open;
        egui::Window::new(tr("browser.title"))
            .open(&mut open)
            .default_size([5.0 * CARD.x + 40.0, 520.0])
            .min_width(CARD.x + 40.0)
            .show(ctx, |ui| self.contents(ui, ws));
        self.open = open;
    }

    fn contents(&mut self, ui: &mut egui::Ui, ws: &mut Workspace) {
        ui.horizontal_wrapped(|ui| {
            for body in Body::ALL {
                if ui.selectable_label(ws.body() == body, body.label()).clicked() {
                    ws.set_body(body);
                }
            }
            ui.separator();
            for (kind, label) in [(Kind::Top, tr("garment.tops")), (Kind::Bottom, tr("garment.bottoms"))] {
                ui.selectable_value(&mut self.kind, kind, label);
            }
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.query).hint_text(tr("browser.search")).desired_width(140.0));
        });
        ui.weak(format!("{}: {} · {}: {}", tr("garment.top"), ws.pattern.top().name, tr("garment.bottom"), ws.pattern.bottom().name));
        ui.separator();

        let query = self.query.trim().to_lowercase();
        let wanted: Vec<&'static Garment> = garments::GARMENTS
            .iter()
            .filter(|g| g.kind == self.kind && g.body == ws.body())
            .filter(|g| query.split_whitespace().all(|w| g.name.to_lowercase().contains(w)))
            .collect();
        if wanted.is_empty() {
            ui.weak(tr("browser.no_match"));
            return;
        }
        let current = if self.kind == Kind::Top { ws.pattern.top().id } else { ws.pattern.bottom().id };
        let mut chosen = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(8.0);
                for g in wanted {
                    let outline = self.outlines.entry(g.id).or_insert_with(|| Outline::of(g));
                    if card(ui, g, outline, g.id == current).clicked() {
                        chosen = Some(g);
                    }
                }
            });
        });
        if let Some(g) = chosen {
            ws.set_garment(g);
        }
    }
}

fn card(ui: &mut egui::Ui, g: &Garment, outline: &Outline, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(CARD, Sense::click());
    let visuals = ui.visuals();
    let (fill, stroke) = if selected {
        (visuals.selection.bg_fill.gamma_multiply(0.35), visuals.selection.stroke)
    } else if response.hovered() {
        (visuals.widgets.hovered.weak_bg_fill, visuals.widgets.hovered.bg_stroke)
    } else {
        (visuals.faint_bg_color, Stroke::new(1.0_f32, visuals.widgets.noninteractive.bg_stroke.color))
    };
    let line = if selected { visuals.selection.stroke.color } else { visuals.text_color().gamma_multiply(0.7) };
    let text = visuals.text_color();
    let painter = ui.painter_at(rect);
    painter.rect(rect, 6.0, fill, stroke, StrokeKind::Inside);
    let thumb = Rect::from_min_max(rect.min + Vec2::splat(THUMB_PAD), Pos2::new(rect.max.x - THUMB_PAD, rect.max.y - 44.0));
    outline.paint(&painter, thumb, Stroke::new(1.0_f32, line));
    let name = painter.layout(g.name.to_string(), egui::FontId::proportional(13.0), text, CARD.x - 12.0);
    painter.galley(Pos2::new(rect.center().x - name.size().x / 2.0, rect.max.y - 40.0), name, text);
    if g.unisex {
        let tag = painter.layout_no_wrap(tr("browser.unisex").to_string(), egui::FontId::proportional(11.0), Color32::GRAY);
        painter.galley(Pos2::new(rect.center().x - tag.size().x / 2.0, rect.max.y - 18.0), tag, Color32::GRAY);
    }
    response.on_hover_text(g.name)
}
