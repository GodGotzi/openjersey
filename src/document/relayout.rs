//! Designs saved with an older sheet layout: before the sheet became portrait
//! (file version < 3), and before the raglan seams bowed out (version 3,
//! which made front and back taller). Every object moves by the offset of
//! the piece it sits on: front and back by their garment origin, so things
//! stay put on the body, the other pieces by their corner.

use tiny_skia::Transform;

use super::{map, Document, Object, ObjectKind};
use crate::template::pattern::{Panel, Pattern, PieceId, Pt};

/// An old sheet layout: piece rects and the front/back garment origins.
pub struct OldLayout {
    rects: [(PieceId, [f32; 4]); 9],
    front: Pt,
    back: Pt,
}

/// Landscape: side_r, front, side_l, back / sleeve_r, sleeve_l, leg_r, leg_l / collar.
pub const LANDSCAPE: OldLayout = OldLayout { rects: LANDSCAPE_RECTS, front: [458.5, 1825.75], back: [1201.47, 1846.03] };

/// Portrait with straight raglan seams (file version 3).
pub const PORTRAIT_STRAIGHT_RAGLAN: OldLayout = OldLayout {
    rects: [
        (PieceId::Front, [48.0, 120.0, 548.52, 673.75]),
        (PieceId::Back, [656.52, 120.0, 544.94, 694.03]),
        (PieceId::SideR, [50.69, 946.03, 76.24, 410.64]),
        (PieceId::SleeveR, [186.93, 946.03, 407.8, 528.98]),
        (PieceId::SleeveL, [654.73, 946.03, 407.8, 528.98]),
        (PieceId::SideL, [1122.53, 946.03, 76.24, 410.64]),
        (PieceId::Collar, [463.64, 1607.01, 322.18, 16.8]),
        (PieceId::LegR, [192.39, 1755.81, 864.67, 468.0]),
        (PieceId::LegL, [192.39, 2355.81, 864.67, 468.0]),
    ],
    front: [322.26, 1825.75],
    back: [928.99, 1846.03],
};

const LANDSCAPE_RECTS: [(PieceId, [f32; 4]); 9] = [
    (PieceId::SideR, [48.0, 120.0, 76.24, 410.64]),
    (PieceId::Front, [184.24, 120.0, 548.52, 673.75]),
    (PieceId::SideL, [792.76, 120.0, 76.24, 410.64]),
    (PieceId::Back, [929.0, 120.0, 544.94, 694.03]),
    (PieceId::SleeveR, [48.0, 946.03, 407.8, 528.98]),
    (PieceId::SleeveL, [515.8, 946.03, 407.8, 528.98]),
    (PieceId::LegR, [983.6, 946.03, 864.67, 468.0]),
    (PieceId::LegL, [1908.28, 946.03, 864.67, 468.0]),
    (PieceId::Collar, [48.0, 1607.01, 322.18, 16.8]),
];

pub fn from_layout(doc: &mut Document, old: &OldLayout, pattern: &Pattern) {
    let offset = |piece: PieceId| {
        let (o, n) = match piece {
            PieceId::Front => (old.front, pattern.place(Panel::Front, [0.0, 0.0])),
            PieceId::Back => (old.back, pattern.place(Panel::Back, [0.0, 0.0])),
            _ => {
                let (_, r) = old.rects.iter().find(|(p, _)| *p == piece).expect("all pieces listed");
                let n = pattern.piece(piece).rect;
                ([r[0], r[1]], [n[0], n[1]])
            }
        };
        [n[0] - o[0], n[1] - o[1]]
    };
    for obj in &mut doc.objects {
        relayout(obj, Transform::identity(), &offset, old);
    }
}

/// Moves `obj` (or, if its parts sit on different pieces, each part) by its
/// piece's offset. `parent` maps the object's parent space to the sheet.
fn relayout(obj: &mut Object, parent: Transform, offset: &impl Fn(PieceId) -> Pt, old: &OldLayout) {
    let mut pieces = Vec::new();
    collect_pieces(obj, parent, &mut pieces, old);
    pieces.dedup();
    let world = parent.pre_concat(obj.xf.matrix());
    match (pieces.as_slice(), &mut obj.kind) {
        ([only], _) => {
            let [dx, dy] = offset(*only);
            // the offset is in sheet space; the position in parent space
            let inv = Transform::from_row(parent.sx, parent.ky, parent.kx, parent.sy, 0.0, 0.0).invert();
            let d = inv.map_or([dx, dy], |m| map(&m, [dx, dy]));
            obj.xf.pos = [obj.xf.pos[0] + d[0], obj.xf.pos[1] + d[1]];
        }
        (_, ObjectKind::Group { children }) => {
            for child in children {
                relayout(child, world, offset, old);
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
        let (_, r) = LANDSCAPE.rects.iter().find(|(q, _)| *q == PieceId::SleeveL).unwrap();
        let n = p.piece(PieceId::SleeveL).rect;
        assert!(close(at(b), [700.0 + n[0] - r[0], 1200.0 + n[1] - r[1]]));
        let back = p.place(Panel::Back, [0.0, 0.0]);
        assert!(close(at(c), [1200.0 + back[0] - LANDSCAPE.back[0], 400.0 + back[1] - LANDSCAPE.back[1]]));
        assert!(doc.find(g).is_some());
    }
}
