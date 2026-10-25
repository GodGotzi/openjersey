//! The "Ebenen" panel: the object tree (top of the stack first) with
//! visibility, lock, renaming, drag-and-drop reordering and a context menu,
//! plus the garment pieces' base colours pinned at the bottom.

use std::collections::HashSet;

use myth::egui::egui::{self, Color32, RichText, Sense};

use crate::document::{Object, ObjectId, ObjectKind};
use crate::workspace::Workspace;

pub struct LayersPanel {
    expanded: HashSet<ObjectId>,
    renaming: Option<(ObjectId, String)>,
}

enum Action {
    Select(ObjectId, bool),
    ToggleVisible(ObjectId),
    ToggleLocked(ObjectId),
    Rename(ObjectId, String),
    Drop { id: ObjectId, target: ObjectId, place: DropPlace },
}

#[derive(Clone, Copy)]
enum DropPlace {
    Above,
    Below,
    Into,
}

impl LayersPanel {
    pub fn new() -> Self {
        Self { expanded: HashSet::new(), renaming: None }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ws: &mut Workspace) {
        ui.horizontal(|ui| {
            ui.strong("Ebenen");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let any = !ws.selection.is_empty();
                if ui.add_enabled(any, egui::Button::new("🗑").small()).on_hover_text("Löschen (Entf)").clicked() {
                    ws.delete_selection();
                }
                if ui.add_enabled(any, egui::Button::new("⧉").small()).on_hover_text("Duplizieren (Strg+D)").clicked() {
                    ws.duplicate_selection();
                }
                if ui.add_enabled(any, egui::Button::new("⏷").small()).on_hover_text("Nach hinten").clicked() {
                    ws.restack_selection(-1);
                }
                if ui.add_enabled(any, egui::Button::new("⏶").small()).on_hover_text("Nach vorne").clicked() {
                    ws.restack_selection(1);
                }
            });
        });
        ui.separator();

        // keep the selection's groups open
        for &id in &ws.selection {
            self.expanded.extend(ws.doc.ancestors(id));
        }

        let mut actions = Vec::new();
        egui::ScrollArea::vertical().id_salt("layers").auto_shrink([false, true]).max_height(ui.available_height() * 0.55).show(
            ui,
            |ui| {
                let objects = ws.doc.objects.clone();
                for obj in objects.iter().rev() {
                    self.row(ui, ws, obj, 0, &mut actions);
                }
                if ws.doc.objects.is_empty() {
                    ui.weak("Noch keine Objekte. Werkzeug rechts wählen oder Vorlage laden.");
                }
                ui.add_space(6.0);
                self.base_colors(ui, ws);
            },
        );

        for action in actions {
            match action {
                Action::Select(id, add) => {
                    if add {
                        if let Some(i) = ws.selection.iter().position(|&s| s == id) {
                            ws.selection.remove(i);
                        } else {
                            ws.selection.push(id);
                        }
                    } else {
                        ws.selection = vec![id];
                    }
                }
                Action::ToggleVisible(id) => {
                    ws.checkpoint();
                    if let Some(o) = ws.doc.find_mut(id) {
                        o.visible = !o.visible;
                    }
                    ws.changed();
                }
                Action::ToggleLocked(id) => {
                    ws.checkpoint();
                    if let Some(o) = ws.doc.find_mut(id) {
                        o.locked = !o.locked;
                    }
                    ws.changed();
                }
                Action::Rename(id, name) => {
                    ws.checkpoint();
                    if let Some(o) = ws.doc.find_mut(id) {
                        o.name = name;
                    }
                    ws.changed();
                }
                Action::Drop { id, target, place } => {
                    ws.checkpoint();
                    match place {
                        DropPlace::Above => ws.doc.move_next_to(id, target, true),
                        DropPlace::Below => ws.doc.move_next_to(id, target, false),
                        DropPlace::Into => {
                            ws.doc.move_into(id, target);
                            self.expanded.insert(target);
                        }
                    }
                    ws.changed();
                }
            }
        }
    }

    fn row(&mut self, ui: &mut egui::Ui, ws: &mut Workspace, obj: &Object, depth: usize, actions: &mut Vec<Action>) {
        let selected = ws.selection.contains(&obj.id);
        let is_group = matches!(obj.kind, ObjectKind::Group { .. });
        let row = ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 14.0);
            if is_group {
                let open = self.expanded.contains(&obj.id);
                if ui.add(egui::Button::new(if open { "⏷" } else { "⏵" }).small().frame(false)).clicked() {
                    if open {
                        self.expanded.remove(&obj.id);
                    } else {
                        self.expanded.insert(obj.id);
                    }
                }
            } else {
                ui.add_space(18.0);
            }
            let eye = if obj.visible { "👁" } else { "–" };
            if ui.add(egui::Button::new(eye).small().frame(false)).on_hover_text("Sichtbar").clicked() {
                actions.push(Action::ToggleVisible(obj.id));
            }
            let lock = if obj.locked { "🔒" } else { "🔓" };
            let lock_text = RichText::new(lock).color(if obj.locked { Color32::LIGHT_YELLOW } else { Color32::from_gray(90) });
            if ui.add(egui::Button::new(lock_text).small().frame(false)).on_hover_text("Sperren").clicked() {
                actions.push(Action::ToggleLocked(obj.id));
            }
            ui.label(RichText::new(kind_icon(&obj.kind)).color(Color32::from_gray(140)));

            if let Some((id, name)) = &mut self.renaming
                && *id == obj.id
            {
                let edit = ui.add(egui::TextEdit::singleline(name).desired_width(140.0));
                edit.request_focus();
                if edit.lost_focus() {
                    let (id, name) = self.renaming.take().expect("renaming");
                    if !name.trim().is_empty() {
                        actions.push(Action::Rename(id, name));
                    }
                }
                return None;
            }
            let mut text = RichText::new(&obj.name);
            if !obj.visible {
                text = text.weak();
            }
            let label = ui.add(egui::Button::selectable(selected, text).sense(Sense::click_and_drag()));
            Some(label)
        });

        if let Some(label) = row.inner {
            if label.clicked() {
                actions.push(Action::Select(obj.id, ui.input(|i| i.modifiers.shift || i.modifiers.command)));
            }
            if label.double_clicked() {
                self.renaming = Some((obj.id, obj.name.clone()));
            }
            label.dnd_set_drag_payload(obj.id);
            if let Some(dragged) = label.dnd_hover_payload::<ObjectId>()
                && *dragged != obj.id
            {
                let place = drop_place(ui, &label, is_group);
                let r = label.rect;
                let y = match place {
                    DropPlace::Above => r.top(),
                    DropPlace::Below => r.bottom(),
                    DropPlace::Into => r.center().y,
                };
                let color = Color32::from_rgb(70, 140, 255);
                if let DropPlace::Into = place {
                    ui.painter().rect_stroke(r, 2.0, egui::Stroke::new(1.5_f32, color), egui::StrokeKind::Inside);
                } else {
                    ui.painter().hline(r.x_range(), y, egui::Stroke::new(2.0_f32, color));
                }
            }
            if let Some(dragged) = label.dnd_release_payload::<ObjectId>()
                && *dragged != obj.id
            {
                let place = drop_place(ui, &label, is_group);
                actions.push(Action::Drop { id: *dragged, target: obj.id, place });
            }
            label.context_menu(|ui| {
                if !ws.selection.contains(&obj.id) {
                    ws.selection = vec![obj.id];
                }
                context_menu(ui, ws);
            });
        }

        if is_group && self.expanded.contains(&obj.id) {
            for child in obj.children().iter().rev() {
                self.row(ui, ws, child, depth + 1, actions);
            }
        }
    }

    fn base_colors(&mut self, ui: &mut egui::Ui, ws: &mut Workspace) {
        let mut garments = ws.doc.garments;
        let mut inside = ws.doc.inside_color();
        let mut inside_changed = false;
        ui.horizontal(|ui| {
            ui.label("Kleidung:");
            ui.checkbox(&mut garments.shirt, "Trikot");
            ui.checkbox(&mut garments.shorts, "Hose");
            ui.separator();
            inside_changed = ui.color_edit_button_srgb(&mut inside).on_hover_text("Farbe der Innenseite am Modell").changed();
            ui.label("Innen");
        });
        if inside_changed {
            garments.inside = Some(inside);
        }
        if garments != ws.doc.garments {
            // dragging in the colour picker is one undo step
            if inside_changed { ws.checkpoint_tagged("inside-color") } else { ws.checkpoint() }
            ws.doc.garments = garments;
            ws.changed();
        }
        egui::CollapsingHeader::new(RichText::new("Schnittteile (Grundfarben)").strong()).default_open(false).show(ui, |ui| {
            let mut roles = [ws.doc.base.primary, ws.doc.base.accent, ws.doc.base.trim];
            let mut changed = false;
            ui.horizontal(|ui| {
                for (c, label) in roles.iter_mut().zip(["Grund", "Akzent", "Besatz"]) {
                    changed |= ui.color_edit_button_srgb(c).changed();
                    ui.label(label);
                }
            });
            if changed {
                ws.checkpoint_tagged("base-roles");
                [ws.doc.base.primary, ws.doc.base.accent, ws.doc.base.trim] = roles;
                ws.changed();
            }
            ui.separator();
            let regions: Vec<_> = ws.regions.iter().map(|r| (r.id, r.role)).collect();
            for (id, role) in regions {
                ui.horizontal(|ui| {
                    let mut c = ws.doc.base.region(id, role);
                    if ui.color_edit_button_srgb(&mut c).changed() {
                        ws.checkpoint_tagged(&format!("base-{id:?}"));
                        ws.doc.base.set_region(id, c);
                        ws.changed();
                    }
                    ui.label(id.label());
                });
            }
        });
    }
}

fn drop_place(ui: &egui::Ui, label: &egui::Response, is_group: bool) -> DropPlace {
    let y = ui.input(|i| i.pointer.hover_pos()).map(|p| p.y).unwrap_or(label.rect.center().y);
    let t = (y - label.rect.top()) / label.rect.height().max(1.0);
    // the list shows the top of the stack first, so "above" in the list is
    // "above" in the stack
    if is_group && (0.3..0.7).contains(&t) {
        DropPlace::Into
    } else if t < 0.5 {
        DropPlace::Above
    } else {
        DropPlace::Below
    }
}

pub fn kind_icon(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Text { .. } => "T",
        ObjectKind::Graphic { .. } => "🖼",
        ObjectKind::Shape { closed: true, .. } => "⬟",
        ObjectKind::Shape { closed: false, .. } => "〰",
        ObjectKind::Pattern { .. } => "▦",
        ObjectKind::Placeholder { .. } => "🔲",
        ObjectKind::Group { .. } => "🗀",
    }
}

/// Shared by the layer list and the canvas.
pub fn context_menu(ui: &mut egui::Ui, ws: &mut Workspace) {
    if ui.button("Duplizieren (Strg+D)").clicked() {
        ws.duplicate_selection();
        ui.close();
    }
    if ui.button("Löschen (Entf)").clicked() {
        ws.delete_selection();
        ui.close();
    }
    ui.separator();
    if ui.add_enabled(ws.selection.len() > 1, egui::Button::new("Gruppieren (Strg+G)")).clicked() {
        ws.group_selection();
        ui.close();
    }
    if ui.button("Gruppe auflösen (Strg+Shift+G)").clicked() {
        ws.ungroup_selection();
        ui.close();
    }
    ui.separator();
    if ui.button("Nach vorne").clicked() {
        ws.restack_selection(1);
        ui.close();
    }
    if ui.button("Nach hinten").clicked() {
        ws.restack_selection(-1);
        ui.close();
    }
    ui.separator();
    if ui.button("Auf Teil zentrieren").clicked() {
        ws.centre_selection_on_piece();
        ui.close();
    }
    if ui.button("Auf andere Seite spiegeln (vorne ↔ hinten)").clicked() {
        ws.mirror_selection_to_other_side();
        ui.close();
    }
}
