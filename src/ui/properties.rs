//! The properties of the selected object: transform in millimetres,
//! opacity, clipping to pieces, and the settings of its kind.

use crate::i18n::tr;
use std::sync::Arc;

use myth::egui::egui::{self, DragValue};

use crate::document::{Asset, Object, ObjectKind, Rgb, StrokeStyle, map};
use crate::template::pattern::piece_name;
use crate::template::regions::RegionId;
use crate::workspace::{Workspace, with_world_pos};

pub fn ui(ui: &mut egui::Ui, ws: &mut Workspace) {
    super::widgets::section_title(ui, tr("properties.title"));
    ui.add_space(4.0);
    let [id] = ws.selection.as_slice() else {
        if ws.selection.is_empty() {
            ui.weak(tr("properties.nothing"));
        } else {
            ui.weak(format!("{} {}", ws.selection.len(), tr("properties.many_selected")));
            if ui.button(tr("properties.group")).clicked() {
                ws.group_selection();
            }
        }
        return;
    };
    let id = *id;
    let Some(original) = ws.doc.find(id).cloned() else { return };
    let mut obj = original.clone();
    let mm = ws.px_per_mm();
    let world = map(&ws.doc.parent_matrix(id), obj.xf.pos);
    let mut pos_mm = [world[0] / mm, world[1] / mm];
    let mut replace_asset = false;
    let mut fit_logo: Option<Arc<Asset>> = None;
    // the model's pieces, for "only on"
    let pieces: Vec<_> = ws.pattern.pieces.iter().map(|p| (p.id, piece_name(p))).collect();

    egui::ScrollArea::vertical().id_salt("props").auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("props-grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut obj.name);
            ui.end_row();

            ui.label(tr("properties.type"));
            ui.label(obj.kind.type_name());
            ui.end_row();

            ui.label("Position");
            ui.horizontal(|ui| {
                ui.add(DragValue::new(&mut pos_mm[0]).speed(0.5).suffix(" mm").prefix("x "));
                ui.add(DragValue::new(&mut pos_mm[1]).speed(0.5).suffix(" mm").prefix("y "));
            });
            ui.end_row();

            ui.label(tr("properties.rotation"));
            let mut deg = obj.xf.rotation.to_degrees();
            if ui.add(DragValue::new(&mut deg).speed(0.5).suffix("°").range(-360.0..=360.0)).changed() {
                obj.xf.rotation = deg.to_radians();
            }
            ui.end_row();

            ui.label(tr("properties.scale"));
            ui.horizontal(|ui| {
                let mut sx = obj.xf.scale[0] * 100.0;
                let mut sy = obj.xf.scale[1] * 100.0;
                let rx = ui.add(DragValue::new(&mut sx).speed(0.5).suffix(" %").range(1.0..=2000.0));
                let ry = ui.add(DragValue::new(&mut sy).speed(0.5).suffix(" %").range(1.0..=2000.0));
                let uniform = ui.input(|i| i.modifiers.shift);
                if rx.changed() {
                    obj.xf.scale[0] = sx / 100.0;
                    if uniform {
                        obj.xf.scale[1] = obj.xf.scale[0];
                    }
                }
                if ry.changed() {
                    obj.xf.scale[1] = sy / 100.0;
                }
            });
            ui.end_row();

            ui.label(tr("properties.opacity"));
            ui.add(egui::Slider::new(&mut obj.opacity, 0.0..=1.0));
            ui.end_row();

            ui.label("");
            ui.horizontal(|ui| {
                ui.checkbox(&mut obj.visible, tr("properties.visible"));
                ui.checkbox(&mut obj.locked, tr("properties.locked"));
            });
            ui.end_row();

            ui.label(tr("properties.only_on"));
            let text = if obj.clip.is_empty() {
                tr("properties.all_pieces").to_string()
            } else {
                obj.clip.iter().map(|&c| RegionId::Piece(c).label(&ws.pattern)).collect::<Vec<_>>().join(", ")
            };
            ui.menu_button(text, |ui| {
                for (piece, name) in &pieces {
                    let piece = *piece;
                    let mut on = obj.clip.contains(&piece);
                    if ui.checkbox(&mut on, name.as_str()).changed() {
                        if on {
                            obj.clip.push(piece);
                        } else {
                            obj.clip.retain(|c| *c != piece);
                        }
                    }
                }
            });
            ui.end_row();
        });

        ui.separator();
        kind_ui(ui, &mut obj.kind, mm, &mut replace_asset);
    });

    if replace_asset
        && let Some(path) = rfd::FileDialog::new().add_filter(tr("dialog.filter.image"), &["svg", "png", "jpg", "jpeg"]).pick_file()
    {
        match Asset::from_file(&path) {
            Ok(asset) => {
                if let ObjectKind::Graphic { asset: a, size, .. } = &mut obj.kind {
                    let [nw, nh] = asset.natural_size();
                    size[1] = size[0] * nh / nw.max(1.0);
                    *a = Arc::new(asset);
                } else {
                    fit_logo = Some(Arc::new(asset));
                }
            }
            Err(err) => ws.status = err,
        }
    }
    // a placeholder becomes the logo, fitted into its box
    if let Some(asset) = fit_logo
        && let ObjectKind::Placeholder { size, .. } = obj.kind
    {
        let [nw, nh] = asset.natural_size();
        let f = (size[0] / nw).min(size[1] / nh);
        obj.kind = ObjectKind::Graphic { asset, size: [nw * f, nh * f], tint: None };
    }

    let world_now = map(&ws.doc.parent_matrix(id), obj.xf.pos);
    if (pos_mm[0] * mm - world_now[0]).abs() > 1e-3 || (pos_mm[1] * mm - world_now[1]).abs() > 1e-3 {
        obj.xf = with_world_pos(&ws.doc, id, obj.xf, [pos_mm[0] * mm, pos_mm[1] * mm]);
    }
    if !same(&obj, &original) {
        ws.checkpoint_tagged(&format!("props-{id}"));
        if let Some(target) = ws.doc.find_mut(id) {
            *target = obj;
        }
        ws.changed();
    }
}

fn same(a: &Object, b: &Object) -> bool {
    serde_json::to_string(a).ok() == serde_json::to_string(b).ok()
}

fn color_opt(ui: &mut egui::Ui, label: &str, color: &mut Option<Rgb>, default: Rgb) {
    ui.horizontal(|ui| {
        let mut on = color.is_some();
        if ui.checkbox(&mut on, label).changed() {
            *color = on.then_some(color.unwrap_or(default));
        }
        if let Some(c) = color {
            ui.color_edit_button_srgb(c);
        }
    });
}

fn kind_ui(ui: &mut egui::Ui, kind: &mut ObjectKind, mm: f32, replace: &mut bool) {
    match kind {
        ObjectKind::Text { text, size, color, fatten, italic, max_width } => {
            ui.label("Text");
            ui.text_edit_singleline(text);
            ui.horizontal(|ui| {
                let mut on = max_width.is_some();
                if ui.checkbox(&mut on, tr("properties.max_width")).on_hover_text(tr("properties.max_width_hint")).changed() {
                    *max_width = on.then_some(200.0 * mm);
                }
                if let Some(w) = max_width {
                    let mut w_mm = *w / mm;
                    if ui.add(DragValue::new(&mut w_mm).speed(1.0).suffix(" mm").range(10.0..=1000.0)).changed() {
                        *w = w_mm * mm;
                    }
                }
            });
            let mut cap = *size * 0.73 / mm;
            if ui.add(egui::Slider::new(&mut cap, 3.0..=300.0).logarithmic(true).text(tr("properties.cap_height_mm"))).changed() {
                *size = cap * mm / 0.73;
            }
            ui.add(egui::Slider::new(fatten, 0.0..=0.15).text(tr("properties.bolder")));
            ui.add(egui::Slider::new(italic, -0.4..=0.4).text(tr("properties.italic")));
            let mut auto = color.is_none();
            ui.horizontal(|ui| {
                if ui.checkbox(&mut auto, tr("properties.auto_colour")).changed() {
                    *color = if auto { None } else { Some([255, 255, 255]) };
                }
                if let Some(c) = color {
                    ui.color_edit_button_srgb(c);
                }
            });
        }
        ObjectKind::Graphic { asset, size, tint } => {
            ui.label(format!("{} {}", tr("properties.file"), asset.name));
            let mut w = size[0] / mm;
            if ui.add(DragValue::new(&mut w).speed(0.5).suffix(tr("properties.mm_wide")).range(2.0..=1000.0)).changed() {
                let ratio = size[1] / size[0];
                size[0] = w * mm;
                size[1] = size[0] * ratio;
            }
            color_opt(ui, tr("properties.tint"), tint, [255, 255, 255]);
            if ui.button(tr("properties.replace_image")).clicked() {
                *replace = true;
            }
        }
        ObjectKind::Shape { closed, fill, stroke, .. } => {
            ui.checkbox(closed, tr("properties.closed"));
            color_opt(ui, tr("options.fill"), fill, [255, 255, 255]);
            ui.horizontal(|ui| {
                let mut on = stroke.is_some();
                if ui.checkbox(&mut on, tr("options.outline")).changed() {
                    *stroke = on.then_some(stroke.unwrap_or(StrokeStyle { color: [255, 255, 255], width: 3.0 * mm }));
                }
                if let Some(s) = stroke {
                    ui.color_edit_button_srgb(&mut s.color);
                    let mut w = s.width / mm;
                    if ui.add(DragValue::new(&mut w).speed(0.1).suffix(" mm").range(0.1..=100.0)).changed() {
                        s.width = w * mm;
                    }
                }
            });
        }
        ObjectKind::Pattern { text, size, line_spacing, angle, italic, color, .. } => {
            ui.label("Text");
            ui.text_edit_singleline(text);
            let mut cap = *size * 0.73 / mm;
            if ui.add(egui::Slider::new(&mut cap, 2.0..=60.0).text(tr("properties.cap_height_mm"))).changed() {
                *size = cap * mm / 0.73;
            }
            let mut sp = *line_spacing / mm;
            if ui.add(egui::Slider::new(&mut sp, 3.0..=100.0).text(tr("properties.line_spacing_mm"))).changed() {
                *line_spacing = sp * mm;
            }
            let mut deg = angle.to_degrees();
            if ui.add(egui::Slider::new(&mut deg, -90.0..=90.0).text(tr("properties.angle"))).changed() {
                *angle = deg.to_radians();
            }
            ui.add(egui::Slider::new(italic, -0.4..=0.4).text(tr("properties.italic")));
            ui.horizontal(|ui| {
                ui.label(tr("properties.colour"));
                ui.color_edit_button_srgb(color);
            });
        }
        ObjectKind::Placeholder { label, size } => {
            ui.label(tr("options.label"));
            ui.text_edit_singleline(label);
            ui.horizontal(|ui| {
                let (mut w, mut h) = (size[0] / mm / 10.0, size[1] / mm / 10.0);
                if ui.add(DragValue::new(&mut w).speed(0.1).suffix(" cm").range(0.5..=100.0)).changed() {
                    size[0] = w * 10.0 * mm;
                }
                ui.label("×");
                if ui.add(DragValue::new(&mut h).speed(0.1).suffix(" cm").range(0.5..=100.0)).changed() {
                    size[1] = h * 10.0 * mm;
                }
            });
            if ui.button(tr("properties.insert_logo")).on_hover_text(tr("properties.insert_logo_hint")).clicked() {
                *replace = true;
            }
        }
        ObjectKind::Group { children } => {
            ui.label(format!("{} {}", children.len(), tr("properties.objects")));
        }
    }
}
