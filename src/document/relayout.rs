//! Moving a design to another sheet layout: designs saved with an older
//! layout (before the sheet became portrait, file version < 3, and before the
//! raglan seams bowed out, version 3, which made front and back taller), and
//! designs switched to another model. Every object moves by the offset of
//! the piece it sits on: front and back by their garment origin, so things
//! stay put on the body, the other pieces by their corner. Objects on a piece
//! the new layout doesn't have (a sleeve on a tank top) are hidden and
//! remember their piece, so they come back on the next layout that has it.

use tiny_skia::Transform;

use super::{Document, Object, ObjectKind, Parked, map};
use crate::template::pattern::{Panel, Pattern, PieceId, Pt};

/// A sheet layout: piece rects and the front/back garment origins.
pub struct OldLayout<'a> {
    rects: &'a [(PieceId, [f32; 4])],
    front: Pt,
    back: Pt,
}

/// The rects of a model's current layout, for `OldLayout::of`.
pub fn piece_rects(pattern: &Pattern) -> Vec<(PieceId, [f32; 4])> {
    pattern.pieces.iter().map(|p| (p.id, p.rect)).collect()
}

impl<'a> OldLayout<'a> {
    /// The layout of `pattern`, with `rects` from `piece_rects(pattern)`.
    pub fn of(pattern: &Pattern, rects: &'a [(PieceId, [f32; 4])]) -> Self {
        Self { rects, front: pattern.place(Panel::Front, [0.0, 0.0]), back: pattern.place(Panel::Back, [0.0, 0.0]) }
    }

    /// The point a piece's objects move with: the garment origin on front
    /// and back, else the rect's corner.
    fn corner(&self, piece: PieceId) -> Option<Pt> {
        match piece {
            PieceId::FRONT => Some(self.front),
            PieceId::BACK => Some(self.back),
            _ => self.rects.iter().find(|(p, _)| *p == piece).map(|(_, r)| [r[0], r[1]]),
        }
    }
}

/// `corner` of `pattern`'s layout.
fn new_corner(pattern: &Pattern, piece: PieceId) -> Option<Pt> {
    match piece {
        PieceId::FRONT => Some(pattern.place(Panel::Front, [0.0, 0.0])),
        PieceId::BACK => Some(pattern.place(Panel::Back, [0.0, 0.0])),
        _ => pattern.get(piece).map(|p| [p.rect[0], p.rect[1]]),
    }
}

/// Landscape: side_r, front, side_l, back / sleeve_r, sleeve_l, leg_r, leg_l / collar.
pub const LANDSCAPE: OldLayout<'static> = OldLayout { rects: &LANDSCAPE_RECTS, front: [458.5, 1825.75], back: [1201.47, 1846.03] };

/// The jersey and shorts on one sheet, before tops and bottoms were chosen
/// separately (file version 4).
pub const JERSEY_AND_SHORTS: OldLayout<'static> = OldLayout {
    rects: &[
        (PieceId::FRONT, [48.0, 120.0, 552.17, 692.01]),
        (PieceId::BACK, [660.17, 120.0, 550.38, 700.96]),
        (PieceId::SIDE_R, [58.31, 952.96, 73.16, 411.48]),
        (PieceId::SLEEVE_R, [191.47, 952.96, 407.8, 528.98]),
        (PieceId::SLEEVE_L, [659.28, 952.96, 407.8, 528.98]),
        (PieceId::SIDE_L, [1127.08, 952.96, 73.16, 411.48]),
        (PieceId::COLLAR, [468.19, 1613.94, 322.18, 16.8]),
        (PieceId::LEG_R, [196.94, 1762.74, 864.67, 468.0]),
        (PieceId::LEG_L, [196.94, 2362.74, 864.67, 468.0]),
    ],
    front: [324.08, 1844.01],
    back: [935.36, 1852.96],
};

/// Portrait with straight raglan seams (file version 3).
pub const PORTRAIT_STRAIGHT_RAGLAN: OldLayout<'static> = OldLayout {
    rects: &[
        (PieceId::FRONT, [48.0, 120.0, 548.52, 673.75]),
        (PieceId::BACK, [656.52, 120.0, 544.94, 694.03]),
        (PieceId::SIDE_R, [50.69, 946.03, 76.24, 410.64]),
        (PieceId::SLEEVE_R, [186.93, 946.03, 407.8, 528.98]),
        (PieceId::SLEEVE_L, [654.73, 946.03, 407.8, 528.98]),
        (PieceId::SIDE_L, [1122.53, 946.03, 76.24, 410.64]),
        (PieceId::COLLAR, [463.64, 1607.01, 322.18, 16.8]),
        (PieceId::LEG_R, [192.39, 1755.81, 864.67, 468.0]),
        (PieceId::LEG_L, [192.39, 2355.81, 864.67, 468.0]),
    ],
    front: [322.26, 1825.75],
    back: [928.99, 1846.03],
};

const LANDSCAPE_RECTS: [(PieceId, [f32; 4]); 9] = [
    (PieceId::SIDE_R, [48.0, 120.0, 76.24, 410.64]),
    (PieceId::FRONT, [184.24, 120.0, 548.52, 673.75]),
    (PieceId::SIDE_L, [792.76, 120.0, 76.24, 410.64]),
    (PieceId::BACK, [929.0, 120.0, 544.94, 694.03]),
    (PieceId::SLEEVE_R, [48.0, 946.03, 407.8, 528.98]),
    (PieceId::SLEEVE_L, [515.8, 946.03, 407.8, 528.98]),
    (PieceId::LEG_R, [983.6, 946.03, 864.67, 468.0]),
    (PieceId::LEG_L, [1908.28, 946.03, 864.67, 468.0]),
    (PieceId::COLLAR, [48.0, 1607.01, 322.18, 16.8]),
];

pub fn from_layout(doc: &mut Document, old: &OldLayout, pattern: &Pattern) {
    let to_new = |piece: PieceId| new_corner(pattern, piece);
    for obj in &mut doc.objects {
        relayout(obj, Transform::identity(), &to_new, old);
    }
}

/// Moves `obj` (a position in parent space) by a sheet-space offset.
fn shift(obj: &mut Object, parent: Transform, [dx, dy]: Pt) {
    let inv = Transform::from_row(parent.sx, parent.ky, parent.kx, parent.sy, 0.0, 0.0).invert();
    let d = inv.map_or([dx, dy], |m| map(&m, [dx, dy]));
    obj.xf.pos = [obj.xf.pos[0] + d[0], obj.xf.pos[1] + d[1]];
}

/// Moves `obj` (or, if its parts sit on different pieces, each part) by its
/// piece's offset, or hides (parks) it if its piece is gone; a parked object
/// comes back once its piece is there again. `to_new` is a piece's corner
/// on the new layout; `parent` maps the object's parent space to the sheet.
fn relayout(obj: &mut Object, parent: Transform, to_new: &impl Fn(PieceId) -> Option<Pt>, old: &OldLayout) {
    if let Some(parked) = obj.parked {
        if let Some(n) = to_new(parked.piece) {
            shift(obj, parent, [n[0] - parked.corner[0], n[1] - parked.corner[1]]);
            obj.visible = parked.visible;
            obj.parked = None;
        }
        return;
    }
    let mut pieces = Vec::new();
    collect_pieces(obj, parent, &mut pieces, old);
    pieces.dedup();
    let world = parent.pre_concat(obj.xf.matrix());
    match (pieces.as_slice(), &mut obj.kind) {
        ([only], _) => {
            let Some(o) = old.corner(*only) else { return };
            match to_new(*only) {
                Some(n) => shift(obj, parent, [n[0] - o[0], n[1] - o[1]]),
                None => {
                    obj.parked = Some(Parked { piece: *only, corner: o, visible: obj.visible });
                    obj.visible = false;
                }
            }
        }
        (_, ObjectKind::Group { children }) => {
            for child in children {
                relayout(child, world, to_new, old);
            }
        }
        _ => {}
    }
}

/// The pieces the leaves of `obj` sit on, in order.
fn collect_pieces(obj: &Object, parent: Transform, out: &mut Vec<PieceId>, old: &OldLayout) {
    let world = parent.pre_concat(obj.xf.matrix());
    if let ObjectKind::Group { children } = &obj.kind
        && obj.clip.is_empty()
    {
        for child in children {
            collect_pieces(child, world, out, old);
        }
        return;
    }
    let piece = obj.clip.first().copied().unwrap_or_else(|| piece_at(map(&world, [0.0, 0.0]), old));
    if !out.contains(&piece) {
        out.push(piece);
    }
}

/// The old layout's piece under (or nearest to) a sheet point.
fn piece_at([x, y]: Pt, old: &OldLayout) -> PieceId {
    let distance = |[rx, ry, rw, rh]: [f32; 4]| {
        let dx = (rx - x).max(x - (rx + rw)).max(0.0);
        let dy = (ry - y).max(y - (ry + rh)).max(0.0);
        dx.hypot(dy)
    };
    old.rects.iter().min_by(|a, b| distance(a.1).total_cmp(&distance(b.1))).map(|(p, _)| *p).expect("non-empty")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{BaseColors, Xf};

    fn placeholder(name: &str, at: Pt) -> Object {
        Object::new(name, Xf::at(at), ObjectKind::Placeholder { label: name.into(), size: [10.0, 10.0] })
    }

    #[test]
    fn objects_follow_their_pieces() {
        let p = Pattern::load();
        let mut doc = Document::new(BaseColors { primary: [0; 3], accent: [0; 3], trim: [0; 3], overrides: vec![] });
        // a group spanning the front and a sleeve, and a lone object on the back
        let a = doc.add(None, placeholder("front", [400.0, 500.0]));
        let b = doc.add(None, placeholder("sleeve", [700.0, 1200.0]));
        let g = doc.group(&[a, b], [550.0, 850.0], "g").unwrap();
        let c = doc.add(None, placeholder("back", [1200.0, 400.0]));
        from_layout(&mut doc, &LANDSCAPE, &p);
        let at = |id| map(&doc.world_matrix(id), [0.0, 0.0]);
        let close = |a: Pt, b: Pt| (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3;
        // front and back keep their place on the body, the sleeve its place on the piece
        let front = p.place(Panel::Front, [0.0, 0.0]);
        assert!(close(at(a), [400.0 + front[0] - LANDSCAPE.front[0], 500.0 + front[1] - LANDSCAPE.front[1]]));
        let (_, r) = LANDSCAPE.rects.iter().find(|(q, _)| *q == PieceId::SLEEVE_L).unwrap();
        let n = p.piece(PieceId::SLEEVE_L).rect;
        assert!(close(at(b), [700.0 + n[0] - r[0], 1200.0 + n[1] - r[1]]));
        let back = p.place(Panel::Back, [0.0, 0.0]);
        assert!(close(at(c), [1200.0 + back[0] - LANDSCAPE.back[0], 400.0 + back[1] - LANDSCAPE.back[1]]));
        assert!(doc.find(g).is_some());
    }
}
