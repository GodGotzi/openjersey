//! The open design and everything the editors share: selection, undo,
//! rendering state, and the drag logic used by both the 2D canvas and the 3D
//! view.

use crate::document::{map, unmap, Document, History, Object, ObjectId, ObjectKind, Xf};
use crate::render::geom::{self, Bounds};
use crate::render::Rasterizer;
use crate::template::pattern::{Panel, Pattern, PieceId, Pt};
use crate::template::regions::{regions, Region};

pub struct Workspace {
    pub doc: Document,
    pub history: History,
    pub pattern: Pattern,
    pub regions: Vec<Region>,
    pub raster: Rasterizer,
    pub selection: Vec<ObjectId>,
    /// Bumped on every change except moving the dragged object.
    pub rev: u64,
    /// The template must be re-rendered.
    pub dirty: bool,
    /// While dragging: the top-level object that moves and the revision when
    /// the drag started (lets the renderer reuse everything else).
    pub moving: Option<(ObjectId, u64)>,
    drag: Option<MoveDrag>,
    pub snap: bool,
    /// Snap guide to draw (template x or y line) while moving.
    pub guides: Vec<Guide>,
    pub status: String,
}

#[derive(Debug, Clone, Copy)]
pub enum Guide {
    Vertical(f32),
    Horizontal(f32),
}

struct MoveDrag {
    start: Pt,
    /// (id, parent matrix, start position in template space)
    items: Vec<(ObjectId, tiny_skia::Transform, Pt)>,
    start_bounds: Option<Bounds>,
}

impl Workspace {
    pub fn new(doc: Document) -> Self {
        let pattern = Pattern::load();
        let regions = regions(&pattern);
        Self {
            doc,
            history: History::new(),
            pattern,
            regions,
            raster: Rasterizer::new(),
            selection: Vec::new(),
            rev: 1,
            dirty: true,
            moving: None,
            drag: None,
            snap: true,
            guides: Vec::new(),
            status: String::new(),
        }
    }

    pub fn px_per_mm(&self) -> f32 {
        self.pattern.atlas.px_per_m / 1000.0
    }

    /// Call before changing the document (one undo step).
    pub fn checkpoint(&mut self) {
        self.history.checkpoint(&self.doc);
    }

    /// Like `checkpoint`, but repeated calls with the same tag merge.
    pub fn checkpoint_tagged(&mut self, tag: &str) {
        self.history.checkpoint_tagged(&self.doc, tag);
    }

    /// Call after changing the document.
    pub fn changed(&mut self) {
        self.rev += 1;
        self.dirty = true;
        self.selection.retain(|&id| self.doc.find(id).is_some());
    }

    pub fn replace(&mut self, doc: Document) {
        self.checkpoint();
        self.doc = doc;
        self.selection.clear();
        self.changed();
    }

    pub fn undo(&mut self) {
        if self.history.undo(&mut self.doc) {
            self.changed();
        }
    }

    pub fn redo(&mut self) {
        if self.history.redo(&mut self.doc) {
            self.changed();
        }
    }

    pub fn add(&mut self, parent: Option<ObjectId>, obj: Object) -> ObjectId {
        self.checkpoint();
        let id = self.doc.add(parent, obj);
        self.selection = vec![id];
        self.changed();
        id
    }

    pub fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        for id in std::mem::take(&mut self.selection) {
            self.doc.remove(id);
        }
        self.changed();
    }

    pub fn duplicate_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let offset = 5.0 * self.px_per_mm();
        let new: Vec<ObjectId> =
            self.selection.clone().into_iter().filter_map(|id| self.doc.duplicate(id, [offset, offset])).collect();
        self.selection = new;
        self.changed();
    }

    pub fn group_selection(&mut self) {
        if self.selection.len() < 2 {
            return;
        }
        let Some([x0, y0, x1, y1]) = geom::world_bounds(&self.raster.fonts, &self.doc, &self.selection) else { return };
        // group siblings of the first selected object, in parent space
        let parent = self.doc.locate(self.selection[0]).and_then(|l| l.0);
        let centre = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        let centre = parent.and_then(|p| unmap(&self.doc.world_matrix(p), centre)).unwrap_or(centre);
        self.checkpoint();
        if let Some(id) = self.doc.group(&self.selection.clone(), centre, "Gruppe") {
            self.selection = vec![id];
        }
        self.changed();
    }

    pub fn ungroup_selection(&mut self) {
        let groups: Vec<ObjectId> = self
            .selection
            .iter()
            .copied()
            .filter(|&id| matches!(self.doc.find(id).map(|o| &o.kind), Some(ObjectKind::Group { .. })))
            .collect();
        if groups.is_empty() {
            return;
        }
        self.checkpoint();
        let mut new = Vec::new();
        for g in groups {
            new.extend(self.doc.ungroup(g));
        }
        self.selection = new;
        self.changed();
    }

    pub fn restack_selection(&mut self, step: isize) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        for id in self.selection.clone() {
            self.doc.restack(id, step);
        }
        self.changed();
    }

    /// Copies the selection onto the other side (front <-> back), at the same
    /// garment position.
    pub fn mirror_selection_to_other_side(&mut self) {
        let mut new = Vec::new();
        self.checkpoint();
        for id in self.selection.clone() {
            let Some(obj) = self.doc.find(id) else { continue };
            let world = map(&self.doc.world_matrix(id), [0.0, 0.0]);
            let (from, to) = if self.pattern.piece(PieceId::Front).contains(world) {
                (Panel::Front, Panel::Back)
            } else if self.pattern.piece(PieceId::Back).contains(world) {
                (Panel::Back, Panel::Front)
            } else {
                continue;
            };
            let mut copy = obj.clone();
            copy.xf.pos = self.pattern.place(to, self.pattern.unplace(from, world));
            copy.xf.rotation = -copy.xf.rotation;
            for piece in &mut copy.clip {
                *piece = match *piece {
                    PieceId::Front => PieceId::Back,
                    PieceId::Back => PieceId::Front,
                    other => other,
                };
            }
            copy.name = format!("{} ({})", obj.name, if to == Panel::Back { "hinten" } else { "vorne" });
            new.push(self.doc.add(None, copy));
        }
        if !new.is_empty() {
            self.selection = new;
        }
        self.changed();
    }

    /// Moves the selection's centre onto the centre line of the piece it is on.
    pub fn centre_selection_on_piece(&mut self) {
        let Some([x0, y0, x1, y1]) = geom::world_bounds(&self.raster.fonts, &self.doc, &self.selection) else { return };
        let c = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        let target_x = if let Some(panel) = [Panel::Front, Panel::Back]
            .into_iter()
            .find(|&p| self.pattern.piece(if p == Panel::Front { PieceId::Front } else { PieceId::Back }).contains(c))
        {
            self.pattern.place(panel, [0.0, 1.0])[0]
        } else if let Some(piece) = self.pattern.pieces.iter().find(|p| p.contains(c)) {
            piece.rect[0] + piece.rect[2] / 2.0
        } else {
            return;
        };
        self.checkpoint();
        self.translate(&self.selection.clone(), [target_x - c[0], 0.0]);
        self.changed();
    }

    /// Moves objects by a template-space offset.
    pub fn translate(&mut self, ids: &[ObjectId], delta: Pt) {
        for &id in ids {
            let parent = self.doc.parent_matrix(id);
            let Some(obj) = self.doc.find(id) else { continue };
            let world = map(&parent, obj.xf.pos);
            let target = [world[0] + delta[0], world[1] + delta[1]];
            let local = unmap(&parent, target).unwrap_or(target);
            if let Some(obj) = self.doc.find_mut(id) {
                obj.xf.pos = local;
            }
        }
    }

    // ---- dragging selected objects (2D canvas and 3D view) ----

    pub fn begin_move(&mut self, at: Pt) {
        let ids: Vec<ObjectId> =
            self.selection.iter().copied().filter(|&id| self.doc.find(id).is_some_and(|o| !o.locked)).collect();
        if ids.is_empty() {
            return;
        }
        self.checkpoint();
        let items = ids
            .iter()
            .filter_map(|&id| {
                let parent = self.doc.parent_matrix(id);
                let pos = self.doc.find(id)?.xf.pos;
                Some((id, parent, map(&parent, pos)))
            })
            .collect();
        let start_bounds = geom::world_bounds(&self.raster.fonts, &self.doc, &ids);
        self.drag = Some(MoveDrag { start: at, items, start_bounds });
        let tops: Vec<ObjectId> = ids.iter().map(|&id| self.doc.top_level(id)).collect();
        self.moving = (tops.iter().all(|&t| t == tops[0])).then(|| (tops[0], self.rev));
    }

    /// `snap_px`: snapping distance in template pixels (0 = off).
    pub fn drag_move(&mut self, at: Pt, snap_px: f32) {
        let Some(drag) = &self.drag else { return };
        let mut delta = [at[0] - drag.start[0], at[1] - drag.start[1]];
        self.guides.clear();
        if self.snap && snap_px > 0.0
            && let Some([x0, y0, x1, y1]) = drag.start_bounds
        {
            let moved = [x0 + delta[0], y0 + delta[1], x1 + delta[0], y1 + delta[1]];
            let (cx, cy) = ((moved[0] + moved[2]) / 2.0, (moved[1] + moved[3]) / 2.0);
            let ids: Vec<ObjectId> = drag.items.iter().map(|i| i.0).collect();
            let (xs, ys) = self.snap_targets(&ids);
            if let Some(&x) = xs.iter().min_by(|a, b| (*a - cx).abs().total_cmp(&(*b - cx).abs()))
                && (x - cx).abs() < snap_px
            {
                delta[0] += x - cx;
                self.guides.push(Guide::Vertical(x));
            }
            if let Some(&y) = ys.iter().min_by(|a, b| (*a - cy).abs().total_cmp(&(*b - cy).abs()))
                && (y - cy).abs() < snap_px
            {
                delta[1] += y - cy;
                self.guides.push(Guide::Horizontal(y));
            }
        }
        let items: Vec<_> = drag.items.clone();
        for (id, parent, start) in items {
            let target = [start[0] + delta[0], start[1] + delta[1]];
            let local = unmap(&parent, target).unwrap_or(target);
            if let Some(obj) = self.doc.find_mut(id) {
                obj.xf.pos = local;
            }
        }
        self.dirty = true;
    }

    pub fn end_drag(&mut self) {
        if self.drag.take().is_some() {
            self.moving = None;
            self.guides.clear();
            self.changed();
        }
    }

    /// Centre lines of the front and back, piece centres, and the centres of
    /// other top-level objects.
    fn snap_targets(&self, moving: &[ObjectId]) -> (Vec<f32>, Vec<f32>) {
        let mut xs = vec![self.pattern.place(Panel::Front, [0.0, 1.0])[0], self.pattern.place(Panel::Back, [0.0, 1.0])[0]];
        let mut ys = Vec::new();
        for p in &self.pattern.pieces {
            xs.push(p.rect[0] + p.rect[2] / 2.0);
            ys.push(p.rect[1] + p.rect[3] / 2.0);
        }
        let skip: Vec<ObjectId> = moving.iter().map(|&id| self.doc.top_level(id)).collect();
        for obj in &self.doc.objects {
            if skip.contains(&obj.id) || !obj.visible {
                continue;
            }
            if let Some([x0, y0, x1, y1]) = geom::world_bounds(&self.raster.fonts, &self.doc, &[obj.id]) {
                xs.push((x0 + x1) / 2.0);
                ys.push((y0 + y1) / 2.0);
            }
        }
        (xs, ys)
    }

    // ---- picking ----

    /// What a click at `p` selects: the outermost group of the hit object,
    /// or with `deep` the object itself.
    pub fn pick(&self, p: Pt, tolerance: f32, deep: bool) -> Option<ObjectId> {
        let (leaf, path) = geom::hit(&self.raster.fonts, &self.pattern, &self.doc, p, tolerance)?;
        if deep {
            return Some(leaf);
        }
        // inside a selected group, a click picks the next level down
        for (i, g) in path.iter().enumerate() {
            if self.selection.contains(g) {
                return Some(path.get(i + 1).copied().unwrap_or(leaf));
            }
        }
        Some(path.first().copied().unwrap_or(leaf))
    }

    pub fn object_name(&self, id: ObjectId) -> String {
        self.doc.find(id).map(|o| o.name.clone()).unwrap_or_default()
    }
}

/// An object's xf with a new position in template space.
pub fn with_world_pos(doc: &Document, id: ObjectId, xf: Xf, world: Pt) -> Xf {
    let parent = doc.parent_matrix(id);
    Xf { pos: unmap(&parent, world).unwrap_or(world), ..xf }
}
