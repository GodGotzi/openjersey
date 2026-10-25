//! The design as a tree of objects (text, graphics, shapes, watermark
//! patterns, logo placeholders, groups) over the garment pieces' base
//! colours. All coordinates are template (atlas) pixels; every object has a
//! transform, and its content is stored around its own origin, so moving,
//! rotating and scaling work the same for every kind.

mod asset;
mod history;
pub mod io;
pub mod legacy;
mod relayout;

use serde::{Deserialize, Serialize};
use tiny_skia::Transform;

pub use asset::Asset;
pub use history::History;

use crate::template::pattern::{Garment, PieceId, Pt};
use crate::template::regions::{ColorRole, RegionId};

pub type Rgb = [u8; 3];
pub type ObjectId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Xf {
    pub pos: Pt,
    /// radians, clockwise on screen (y points down)
    pub rotation: f32,
    pub scale: [f32; 2],
}

impl Default for Xf {
    fn default() -> Self {
        Self { pos: [0.0, 0.0], rotation: 0.0, scale: [1.0, 1.0] }
    }
}

impl Xf {
    pub fn at(pos: Pt) -> Self {
        Self { pos, ..Self::default() }
    }

    /// Local -> parent space: scale, then rotate, then move.
    pub fn matrix(&self) -> Transform {
        Transform::from_scale(self.scale[0], self.scale[1])
            .post_rotate(self.rotation.to_degrees())
            .post_translate(self.pos[0], self.pos[1])
    }
}

pub fn map(m: &Transform, [x, y]: Pt) -> Pt {
    [m.sx * x + m.kx * y + m.tx, m.ky * x + m.sy * y + m.ty]
}

pub fn unmap(m: &Transform, p: Pt) -> Option<Pt> {
    Some(map(&m.invert()?, p))
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrokeStyle {
    pub color: Rgb,
    pub width: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObjectKind {
    /// A line of text centred on the origin. `color: None` picks white or the
    /// trim colour, whichever reads on the base colour. `fatten` thickens the
    /// glyphs (fraction of the size), `italic` slants them. Text wider than
    /// `max_width` is condensed horizontally to fit (long player names).
    Text {
        text: String,
        size: f32,
        color: Option<Rgb>,
        fatten: f32,
        italic: f32,
        #[serde(default)]
        max_width: Option<f32>,
    },
    /// An imported PNG/JPG/SVG, `size` (w, h) centred on the origin. `tint`
    /// recolours the whole graphic (e.g. a white logo on a dark shirt).
    Graphic { asset: std::sync::Arc<Asset>, size: [f32; 2], tint: Option<Rgb> },
    /// Polygon or polyline (brush strokes, contour lines, shapes).
    Shape { points: Vec<Pt>, closed: bool, fill: Option<Rgb>, stroke: Option<StrokeStyle> },
    /// Text repeated in staggered rows (a watermark), only inside `area`, or
    /// inside the object's clip pieces when `area` is empty.
    Pattern { text: String, size: f32, line_spacing: f32, angle: f32, italic: f32, color: Rgb, area: Vec<Vec<Pt>> },
    /// A reserved area for a logo that will come later. Shown in the editor,
    /// left out of the print file.
    Placeholder { label: String, size: [f32; 2] },
    Group { children: Vec<Object> },
}

impl ObjectKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            ObjectKind::Text { .. } => "Text",
            ObjectKind::Graphic { .. } => "Grafik",
            ObjectKind::Shape { closed: true, .. } => "Form",
            ObjectKind::Shape { closed: false, .. } => "Linie",
            ObjectKind::Pattern { .. } => "Muster",
            ObjectKind::Placeholder { .. } => "Logo-Platzhalter",
            ObjectKind::Group { .. } => "Gruppe",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Object {
    pub id: ObjectId,
    pub name: String,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "one")]
    pub opacity: f32,
    /// Restricts the object to these pieces (empty = anywhere).
    #[serde(default)]
    pub clip: Vec<PieceId>,
    #[serde(default)]
    pub xf: Xf,
    pub kind: ObjectKind,
}

fn yes() -> bool {
    true
}

fn one() -> f32 {
    1.0
}

impl Object {
    /// A new object (id assigned on insertion).
    pub fn new(name: impl Into<String>, xf: Xf, kind: ObjectKind) -> Self {
        Self { id: 0, name: name.into(), visible: true, locked: false, opacity: 1.0, clip: Vec::new(), xf, kind }
    }

    /// A shape from points in parent space; the object's origin is put at
    /// their centre so it rotates and scales about the middle.
    pub fn shape(name: impl Into<String>, points: Vec<Pt>, closed: bool, fill: Option<Rgb>, stroke: Option<StrokeStyle>) -> Self {
        let c = centre(&points);
        let points = points.iter().map(|p| [p[0] - c[0], p[1] - c[1]]).collect();
        Self::new(name, Xf::at(c), ObjectKind::Shape { points, closed, fill, stroke })
    }

    pub fn with_clip(mut self, clip: &[PieceId]) -> Self {
        self.clip = clip.to_vec();
        self
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    pub fn children(&self) -> &[Object] {
        match &self.kind {
            ObjectKind::Group { children } => children,
            _ => &[],
        }
    }
}

fn centre(points: &[Pt]) -> Pt {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    if x0 > x1 { [0.0, 0.0] } else { [(x0 + x1) / 2.0, (y0 + y1) / 2.0] }
}

/// The garment pieces' colours: three role colours plus per-region overrides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaseColors {
    pub primary: Rgb,
    pub accent: Rgb,
    pub trim: Rgb,
    pub overrides: Vec<(RegionId, Rgb)>,
}

impl BaseColors {
    pub fn role(&self, role: ColorRole) -> Rgb {
        match role {
            ColorRole::Primary => self.primary,
            ColorRole::Accent => self.accent,
            ColorRole::Trim => self.trim,
        }
    }

    pub fn region(&self, id: RegionId, role: ColorRole) -> Rgb {
        self.overrides.iter().find(|(r, _)| *r == id).map(|(_, c)| *c).unwrap_or_else(|| self.role(role))
    }

    pub fn set_region(&mut self, id: RegionId, color: Rgb) {
        self.overrides.retain(|(r, _)| *r != id);
        self.overrides.push((id, color));
    }

    /// Text colour that reads on the base colour.
    pub fn auto_ink(&self) -> Rgb {
        let [r, g, b] = self.primary.map(|c| srgb_to_linear(c as f32 / 255.0));
        if 0.2126 * r + 0.7152 * g + 0.0722 * b < 0.5 { [255, 255, 255] } else { self.trim }
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Which garments the design includes. A left-out garment is hidden on the
/// model and blanked on the print sheet. `inside` is the plain colour of the
/// fabric's inside on the model (None = the primary colour).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Garments {
    pub shirt: bool,
    pub shorts: bool,
    #[serde(default)]
    pub inside: Option<Rgb>,
}

impl Default for Garments {
    fn default() -> Self {
        Self { shirt: true, shorts: true, inside: None }
    }
}

impl Garments {
    pub fn shows(&self, g: Garment) -> bool {
        match g {
            Garment::Shirt => self.shirt,
            Garment::Shorts => self.shorts,
        }
    }

    pub fn shows_piece(&self, p: PieceId) -> bool {
        self.shows(p.garment())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub base: BaseColors,
    #[serde(default)]
    pub garments: Garments,
    /// Bottom to top.
    pub objects: Vec<Object>,
    pub next_id: ObjectId,
}

impl Document {
    pub fn new(base: BaseColors) -> Self {
        Self { base, garments: Garments::default(), objects: Vec::new(), next_id: 1 }
    }

    /// The inside colour of shirt and shorts on the model.
    pub fn inside_color(&self) -> Rgb {
        self.garments.inside.unwrap_or(self.base.primary)
    }

    fn assign_ids(&mut self, obj: &mut Object) {
        obj.id = self.next_id;
        self.next_id += 1;
        if let ObjectKind::Group { children } = &mut obj.kind {
            for child in children {
                self.assign_ids(child);
            }
        }
    }

    /// Adds on top of `parent` (None = the top level). Returns the new id.
    pub fn add(&mut self, parent: Option<ObjectId>, mut obj: Object) -> ObjectId {
        self.assign_ids(&mut obj);
        let id = obj.id;
        self.container_mut(parent).push(obj);
        id
    }

    pub fn container_mut(&mut self, parent: Option<ObjectId>) -> &mut Vec<Object> {
        let group = parent.filter(|&id| self.find(id).is_some_and(|o| matches!(o.kind, ObjectKind::Group { .. })));
        if let Some(id) = group {
            // returning unconditionally from here keeps the borrow checker happy
            match self.find_mut(id) {
                Some(Object { kind: ObjectKind::Group { children }, .. }) => return children,
                _ => unreachable!("checked above"),
            }
        }
        &mut self.objects
    }

    pub fn find(&self, id: ObjectId) -> Option<&Object> {
        fn walk(objs: &[Object], id: ObjectId) -> Option<&Object> {
            objs.iter().find_map(|o| if o.id == id { Some(o) } else { walk(o.children(), id) })
        }
        walk(&self.objects, id)
    }

    pub fn find_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        fn walk(objs: &mut [Object], id: ObjectId) -> Option<&mut Object> {
            for o in objs {
                if o.id == id {
                    return Some(o);
                }
                if let ObjectKind::Group { children } = &mut o.kind
                    && let Some(found) = walk(children, id)
                {
                    return Some(found);
                }
            }
            None
        }
        walk(&mut self.objects, id)
    }

    /// (parent, index) of an object.
    pub fn locate(&self, id: ObjectId) -> Option<(Option<ObjectId>, usize)> {
        fn walk(objs: &[Object], parent: Option<ObjectId>, id: ObjectId) -> Option<(Option<ObjectId>, usize)> {
            for (i, o) in objs.iter().enumerate() {
                if o.id == id {
                    return Some((parent, i));
                }
                if let Some(found) = walk(o.children(), Some(o.id), id) {
                    return Some(found);
                }
            }
            None
        }
        walk(&self.objects, None, id)
    }

    /// Ancestors from the top level down to (excluding) `id`.
    pub fn ancestors(&self, id: ObjectId) -> Vec<ObjectId> {
        let mut out = Vec::new();
        let mut current = id;
        while let Some((Some(parent), _)) = self.locate(current) {
            out.push(parent);
            current = parent;
        }
        out.reverse();
        out
    }

    /// The top-level object containing `id` (itself if top level).
    pub fn top_level(&self, id: ObjectId) -> ObjectId {
        self.ancestors(id).first().copied().unwrap_or(id)
    }

    /// Parent space -> template space for an object's parent.
    pub fn parent_matrix(&self, id: ObjectId) -> Transform {
        self.ancestors(id)
            .iter()
            .filter_map(|&a| self.find(a))
            .fold(Transform::identity(), |m, a| m.pre_concat(a.xf.matrix()))
    }

    /// Local -> template space.
    pub fn world_matrix(&self, id: ObjectId) -> Transform {
        let parent = self.parent_matrix(id);
        match self.find(id) {
            Some(o) => parent.pre_concat(o.xf.matrix()),
            None => parent,
        }
    }

    pub fn remove(&mut self, id: ObjectId) -> Option<Object> {
        let (parent, index) = self.locate(id)?;
        Some(self.container_mut(parent).remove(index))
    }

    pub fn insert(&mut self, parent: Option<ObjectId>, index: usize, obj: Object) {
        let c = self.container_mut(parent);
        let index = index.min(c.len());
        c.insert(index, obj);
    }

    /// Moves `id` to sit directly above (`above = true`) or below `target`,
    /// in the target's group.
    pub fn move_next_to(&mut self, id: ObjectId, target: ObjectId, above: bool) {
        if id == target || self.ancestors(target).contains(&id) {
            return; // can't move a group into itself
        }
        let Some(obj) = self.remove(id) else { return };
        let Some((parent, index)) = self.locate(target) else {
            self.objects.push(obj);
            return;
        };
        self.insert(parent, if above { index + 1 } else { index }, obj);
    }

    /// Moves `id` into group `group` (on top).
    pub fn move_into(&mut self, id: ObjectId, group: ObjectId) {
        if id == group || self.ancestors(group).contains(&id) {
            return;
        }
        if !self.find(group).is_some_and(|g| matches!(g.kind, ObjectKind::Group { .. })) {
            return;
        }
        // keep the object where it is on the template
        let world = self.world_matrix(id);
        let Some(mut obj) = self.remove(id) else { return };
        if let Some(inv) = self.world_matrix(group).invert() {
            obj.xf = decompose(&inv.pre_concat(world)).unwrap_or(obj.xf);
        }
        self.container_mut(Some(group)).push(obj);
    }

    /// One step up (+1) or down (-1) among its siblings.
    pub fn restack(&mut self, id: ObjectId, step: isize) {
        let Some((parent, index)) = self.locate(id) else { return };
        let c = self.container_mut(parent);
        let to = (index as isize + step).clamp(0, c.len() as isize - 1) as usize;
        let obj = c.remove(index);
        c.insert(to, obj);
    }

    pub fn duplicate(&mut self, id: ObjectId, offset: Pt) -> Option<ObjectId> {
        let (parent, index) = self.locate(id)?;
        let mut copy = self.find(id)?.clone();
        copy.xf.pos = [copy.xf.pos[0] + offset[0], copy.xf.pos[1] + offset[1]];
        copy.name = format!("{} Kopie", copy.name);
        self.assign_ids(&mut copy);
        let new_id = copy.id;
        self.insert(parent, index + 1, copy);
        Some(new_id)
    }

    /// Groups siblings (they must share a parent). The group's origin is the
    /// centre of their bounds, so it rotates and scales about its middle.
    pub fn group(&mut self, ids: &[ObjectId], centre: Pt, name: &str) -> Option<ObjectId> {
        let parent = self.locate(*ids.first()?)?.0;
        let mut members: Vec<(usize, ObjectId)> = ids
            .iter()
            .filter_map(|&id| self.locate(id).filter(|(p, _)| *p == parent).map(|(_, i)| (i, id)))
            .collect();
        members.sort();
        let insert_at = members.first()?.0;
        let mut children = Vec::new();
        for (_, id) in &members {
            if let Some(mut obj) = self.remove(*id) {
                obj.xf.pos = [obj.xf.pos[0] - centre[0], obj.xf.pos[1] - centre[1]];
                children.push(obj);
            }
        }
        let mut group = Object::new(name, Xf::at(centre), ObjectKind::Group { children: Vec::new() });
        group.id = self.next_id;
        self.next_id += 1;
        group.kind = ObjectKind::Group { children };
        let id = group.id;
        self.insert(parent, insert_at, group);
        Some(id)
    }

    /// Dissolves a group, keeping its children where they are.
    pub fn ungroup(&mut self, id: ObjectId) -> Vec<ObjectId> {
        let Some((parent, index)) = self.locate(id) else { return vec![] };
        if !matches!(self.find(id).map(|o| &o.kind), Some(ObjectKind::Group { .. })) {
            return vec![];
        }
        let Some(group) = self.remove(id) else { return vec![] };
        let gm = group.xf.matrix();
        let ObjectKind::Group { children } = group.kind else { return vec![] };
        let mut ids = Vec::new();
        for (k, mut child) in children.into_iter().enumerate() {
            child.xf = decompose(&gm.pre_concat(child.xf.matrix())).unwrap_or(child.xf);
            child.opacity *= group.opacity;
            ids.push(child.id);
            self.insert(parent, index + k, child);
        }
        ids
    }

}

/// Splits a matrix back into move/rotate/scale (no shear).
pub fn decompose(m: &Transform) -> Option<Xf> {
    let sx = m.sx.hypot(m.ky);
    if sx == 0.0 {
        return None;
    }
    let rotation = m.ky.atan2(m.sx);
    let det = m.sx * m.sy - m.kx * m.ky;
    let sy = det / sx;
    Some(Xf { pos: [m.tx, m.ty], rotation, scale: [sx, sy] })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(name: &str) -> Object {
        Object::new(name, Xf::at([10.0, 20.0]), ObjectKind::Text {
            text: name.into(),
            size: 10.0,
            color: None,
            fatten: 0.0,
            italic: 0.0,
            max_width: None,
        })
    }

    fn doc() -> Document {
        Document::new(BaseColors { primary: [0; 3], accent: [0; 3], trim: [255; 3], overrides: vec![] })
    }

    #[test]
    fn tree_operations_keep_ids_and_order() {
        let mut d = doc();
        let a = d.add(None, text("a"));
        let b = d.add(None, text("b"));
        let c = d.add(None, text("c"));
        d.move_next_to(a, c, true);
        let names: Vec<_> = d.objects.iter().map(|o| o.name.clone()).collect();
        assert_eq!(names, ["b", "c", "a"]);
        let g = d.group(&[b, c], [10.0, 20.0], "g").unwrap();
        assert_eq!(d.locate(b), Some((Some(g), 0)));
        assert_eq!(d.top_level(c), g);
        // group content keeps its place on the template
        assert_eq!(map(&d.world_matrix(b), [0.0, 0.0]), [10.0, 20.0]);
        let dup = d.duplicate(g, [5.0, 0.0]).unwrap();
        assert_ne!(dup, g);
        assert_eq!(d.find(dup).unwrap().children().len(), 2);
        let ids = d.ungroup(g);
        assert_eq!(ids, vec![b, c]);
        assert_eq!(d.find(b).unwrap().xf.pos, [10.0, 20.0]);
    }

    #[test]
    fn moving_into_a_rotated_group_keeps_the_position() {
        let mut d = doc();
        let a = d.add(None, text("a"));
        let mut g = Object::new("g", Xf { pos: [100.0, 50.0], rotation: 0.7, scale: [2.0, 2.0] }, ObjectKind::Group {
            children: vec![],
        });
        g.name = "g".into();
        let g = d.add(None, g);
        let before = map(&d.world_matrix(a), [3.0, 4.0]);
        d.move_into(a, g);
        let after = map(&d.world_matrix(a), [3.0, 4.0]);
        assert!((before[0] - after[0]).abs() < 1e-3 && (before[1] - after[1]).abs() < 1e-3);
    }
}
