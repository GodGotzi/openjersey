//! Object bounds and hit testing (what's under the pointer), in template pixels.

use tiny_skia::Transform;

use super::text::Fonts;
use crate::document::{map, unmap, Document, Object, ObjectId, ObjectKind};
use crate::template::pattern::{point_in_polygon, Pattern, Pt};
use crate::template::regions::distance_to_segment;

/// Axis-aligned box in an object's local space: [x0, y0, x1, y1].
pub type Bounds = [f32; 4];

/// Local bounds of an object's own content (groups: of their children, in
/// the group's space).
pub fn local_bounds(fonts: &Fonts, obj: &Object) -> Option<Bounds> {
    match &obj.kind {
        ObjectKind::Text { text, size, max_width, .. } => {
            let (w, h) = fonts.text_size(text, *size);
            let w = max_width.map_or(w, |m| w.min(m));
            Some([-w / 2.0, -h / 2.0, w / 2.0, h / 2.0])
        }
        ObjectKind::Graphic { size, .. } | ObjectKind::Placeholder { size, .. } => {
            Some([-size[0] / 2.0, -size[1] / 2.0, size[0] / 2.0, size[1] / 2.0])
        }
        ObjectKind::Shape { points, stroke, .. } => {
            let r = stroke.map(|s| s.width / 2.0).unwrap_or(0.0);
            points_bounds(points.iter().copied()).map(|[a, b, c, d]| [a - r, b - r, c + r, d + r])
        }
        ObjectKind::Pattern { area, .. } => points_bounds(area.iter().flatten().copied()),
        ObjectKind::Group { children } => {
            let mut out: Option<Bounds> = None;
            for child in children {
                let Some(b) = local_bounds(fonts, child) else { continue };
                let m = child.xf.matrix();
                for corner in corners(b) {
                    out = Some(grow(out, map(&m, corner)));
                }
            }
            out
        }
    }
}

pub fn points_bounds(points: impl Iterator<Item = Pt>) -> Option<Bounds> {
    let mut out: Option<Bounds> = None;
    for p in points {
        out = Some(grow(out, p));
    }
    out
}

fn grow(b: Option<Bounds>, p: Pt) -> Bounds {
    match b {
        None => [p[0], p[1], p[0], p[1]],
        Some([x0, y0, x1, y1]) => [x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1])],
    }
}

pub fn corners([x0, y0, x1, y1]: Bounds) -> [Pt; 4] {
    [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

/// The object's bounds as a (possibly rotated) quad in template space.
pub fn world_quad(fonts: &Fonts, doc: &Document, id: ObjectId) -> Option<[Pt; 4]> {
    let obj = doc.find(id)?;
    let b = local_bounds(fonts, obj)?;
    let m = doc.world_matrix(id);
    Some(corners(b).map(|c| map(&m, c)))
}

/// Axis-aligned template-space bounds of several objects.
pub fn world_bounds(fonts: &Fonts, doc: &Document, ids: &[ObjectId]) -> Option<Bounds> {
    let mut out: Option<Bounds> = None;
    for &id in ids {
        for p in world_quad(fonts, doc, id).into_iter().flatten() {
            out = Some(grow(out, p));
        }
    }
    out
}

/// Does the object's own content cover template point `p`?
fn covers(fonts: &Fonts, pattern: &Pattern, obj: &Object, world: &Transform, p: Pt, tolerance: f32) -> bool {
    if !obj.clip.is_empty() && !obj.clip.iter().any(|&id| pattern.piece(id).contains(p)) {
        return false;
    }
    let Some(local) = unmap(world, p) else { return false };
    // pick tolerance in local units
    let scale = (world.sx * world.sy - world.kx * world.ky).abs().sqrt().max(1e-6);
    let tol = tolerance / scale;
    match &obj.kind {
        ObjectKind::Text { .. } | ObjectKind::Graphic { .. } | ObjectKind::Placeholder { .. } => {
            local_bounds(fonts, obj).is_some_and(|[x0, y0, x1, y1]| {
                local[0] >= x0 - tol && local[0] <= x1 + tol && local[1] >= y0 - tol && local[1] <= y1 + tol
            })
        }
        ObjectKind::Shape { points, closed, fill, stroke } => {
            if *closed && fill.is_some() && point_in_polygon(points, local) {
                return true;
            }
            let reach = stroke.map(|s| s.width / 2.0).unwrap_or(0.0) + tol;
            let mut segs: Vec<(Pt, Pt)> = points.windows(2).map(|w| (w[0], w[1])).collect();
            if *closed && points.len() > 2 {
                segs.push((points[points.len() - 1], points[0]));
            }
            match points.as_slice() {
                [only] => (local[0] - only[0]).hypot(local[1] - only[1]) <= reach,
                _ => segs.iter().any(|(a, b)| distance_to_segment(local, *a, *b) <= reach),
            }
        }
        // a watermark covers whole pieces; picking it would block everything
        // under it (select it in the layer list instead)
        ObjectKind::Pattern { .. } => false,
        ObjectKind::Group { .. } => false,
    }
}

/// The topmost visible, unlocked leaf object under `p` with the chain of its
/// group ancestors (outermost first).
pub fn hit(fonts: &Fonts, pattern: &Pattern, doc: &Document, p: Pt, tolerance: f32) -> Option<(ObjectId, Vec<ObjectId>)> {
    fn go(
        fonts: &Fonts,
        pattern: &Pattern,
        objs: &[Object],
        parent: Transform,
        path: &mut Vec<ObjectId>,
        p: Pt,
        tol: f32,
    ) -> Option<ObjectId> {
        for obj in objs.iter().rev() {
            if !obj.visible || obj.locked {
                continue;
            }
            let m = parent.pre_concat(obj.xf.matrix());
            if let ObjectKind::Group { children } = &obj.kind {
                if !obj.clip.is_empty() && !obj.clip.iter().any(|&id| pattern.piece(id).contains(p)) {
                    continue;
                }
                path.push(obj.id);
                if let Some(found) = go(fonts, pattern, children, m, path, p, tol) {
                    return Some(found);
                }
                path.pop();
            } else if covers(fonts, pattern, obj, &m, p, tol) {
                return Some(obj.id);
            }
        }
        None
    }
    let mut path = Vec::new();
    let leaf = go(fonts, pattern, &doc.objects, Transform::identity(), &mut path, p, tolerance)?;
    Some((leaf, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{BaseColors, Xf};

    #[test]
    fn hit_finds_the_topmost_and_respects_rotation() {
        let fonts = Fonts::new();
        let pattern = Pattern::load();
        let mut doc = Document::new(BaseColors { primary: [0; 3], accent: [0; 3], trim: [0; 3], overrides: vec![] });
        let under = doc.add(None, Object::new("u", Xf::at([500.0, 500.0]), ObjectKind::Placeholder {
            label: "A".into(),
            size: [200.0, 40.0],
        }));
        let over = doc.add(None, Object::new(
            "o",
            Xf { pos: [500.0, 500.0], rotation: std::f32::consts::FRAC_PI_2, scale: [1.0, 1.0] },
            ObjectKind::Placeholder { label: "B".into(), size: [200.0, 40.0] },
        ));
        // inside both: the top one wins
        assert_eq!(hit(&fonts, &pattern, &doc, [505.0, 505.0], 0.0).map(|h| h.0), Some(over));
        // only inside the unrotated one
        assert_eq!(hit(&fonts, &pattern, &doc, [590.0, 500.0], 0.0).map(|h| h.0), Some(under));
        // only inside the rotated one (tall now)
        assert_eq!(hit(&fonts, &pattern, &doc, [500.0, 590.0], 0.0).map(|h| h.0), Some(over));
        doc.find_mut(over).unwrap().locked = true;
        assert_eq!(hit(&fonts, &pattern, &doc, [505.0, 505.0], 0.0).map(|h| h.0), Some(under));
    }
}
