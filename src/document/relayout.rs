//! Designs saved before the sheet became portrait (file version < 3) have
//! their objects on the old landscape layout. The pieces kept their size and
//! shape, only their place on the sheet changed, so every object moves by the
//! offset of the piece it sits on.

use tiny_skia::Transform;

use super::{map, Document, Object, ObjectKind};
use crate::template::pattern::{Pattern, PieceId, Pt};

/// Top-left corners in the landscape layout (side_r, front, side_l, back /
/// sleeve_r, sleeve_l, leg_r, leg_l / collar), with the piece sizes.
const LANDSCAPE: [(PieceId, [f32; 4]); 9] = [
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

pub fn from_landscape(doc: &mut Document, pattern: &Pattern) {
    let offset = |piece: PieceId| {
        let (_, old) = LANDSCAPE.iter().find(|(p, _)| *p == piece).expect("all pieces listed");
        let new = pattern.piece(piece).rect;
        [new[0] - old[0], new[1] - old[1]]
    };
    for obj in &mut doc.objects {
        relayout(obj, Transform::identity(), &offset);
    }
}

/// Moves `obj` (or, if its parts sit on different pieces, each part) by its
/// piece's offset. `parent` maps the object's parent space to the sheet.
fn relayout(obj: &mut Object, parent: Transform, offset: &impl Fn(PieceId) -> Pt) {
    let mut pieces = Vec::new();
    collect_pieces(obj, parent, &mut pieces);
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
                relayout(child, world, offset);
            }
        }
        _ => {}
    }
}

/// The pieces the leaves of `obj` sit on, in order.
fn collect_pieces(obj: &Object, parent: Transform, out: &mut Vec<PieceId>) {
    let world = parent.pre_concat(obj.xf.matrix());
    if let ObjectKind::Group { children } = &obj.kind
        && obj.clip.is_empty()
    {
        for child in children {
            collect_pieces(child, world, out);
        }
        return;
    }
    let piece = obj.clip.first().copied().unwrap_or_else(|| piece_at(map(&world, [0.0, 0.0])));
    if !out.contains(&piece) {
        out.push(piece);
    }
}

/// The landscape piece under (or nearest to) a sheet point.
fn piece_at([x, y]: Pt) -> PieceId {
    let distance = |[rx, ry, rw, rh]: [f32; 4]| {
        let dx = (rx - x).max(x - (rx + rw)).max(0.0);
        let dy = (ry - y).max(y - (ry + rh)).max(0.0);
        dx.hypot(dy)
    };
    LANDSCAPE.iter().min_by(|a, b| distance(a.1).total_cmp(&distance(b.1))).map(|(p, _)| *p).expect("non-empty")
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
        from_landscape(&mut doc, &p);
        let at = |id| map(&doc.world_matrix(id), [0.0, 0.0]);
        let moved = |piece: PieceId, old: Pt| {
            let (_, r) = LANDSCAPE.iter().find(|(q, _)| *q == piece).unwrap();
            let n = p.piece(piece).rect;
            [old[0] + n[0] - r[0], old[1] + n[1] - r[1]]
        };
        let close = |a: Pt, b: Pt| (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3;
        assert!(close(at(a), moved(PieceId::Front, [400.0, 500.0])));
        assert!(close(at(b), moved(PieceId::SleeveL, [700.0, 1200.0])));
        assert!(close(at(c), moved(PieceId::Back, [1200.0, 400.0])));
        assert!(doc.find(g).is_some());
    }
}
