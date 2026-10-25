//! Colour regions of the print template: each garment piece, plus printed
//! details on top of a piece (cuff bands on the sleeves, the stripe and hem
//! band on each leg). Atlas pixels.

use serde::{Deserialize, Serialize};

use super::pattern::{point_in_polygon, Pattern, PieceId, Pt};

const LEG_HEM_BAND: f32 = 0.018;

/// The wearer's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RegionId {
    Piece(PieceId),
    Cuff(Side),
    LegStripe(Side),
    LegHem(Side),
}

fn side_of(piece: PieceId) -> Side {
    match piece {
        PieceId::SideL | PieceId::SleeveL | PieceId::LegL => Side::Left,
        _ => Side::Right,
    }
}

impl RegionId {
    pub fn label(self) -> String {
        let side = |s: Side| if s == Side::Left { "links" } else { "rechts" };
        match self {
            RegionId::Piece(p) => match p {
                PieceId::Front => "Vorderteil".into(),
                PieceId::Back => "Rückenteil".into(),
                PieceId::SideR | PieceId::SideL => format!("Seitenteil {}", side(side_of(p))),
                PieceId::SleeveR | PieceId::SleeveL => format!("Ärmel {}", side(side_of(p))),
                PieceId::Collar => "Kragen".into(),
                PieceId::LegR | PieceId::LegL => format!("Hosenbein {}", side(side_of(p))),
            },
            RegionId::Cuff(s) => format!("Bündchen {}", side(s)),
            RegionId::LegStripe(s) => format!("Hosenstreifen {}", side(s)),
            RegionId::LegHem(s) => format!("Hosensaum {}", side(s)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRole {
    Primary,
    Accent,
    Trim,
}

#[derive(Debug, Clone)]
pub struct Region {
    pub id: RegionId,
    pub role: ColorRole,
    pub polygons: Vec<Vec<Pt>>,
    /// Printed details are clipped to the piece they sit on.
    pub clip: Option<PieceId>,
}

/// All regions in draw order: pieces first, details on top.
pub fn regions(pattern: &Pattern) -> Vec<Region> {
    let mut out: Vec<Region> = pattern
        .pieces
        .iter()
        .map(|piece| Region {
            id: RegionId::Piece(piece.id),
            role: match piece.id {
                PieceId::Front | PieceId::Back | PieceId::LegR | PieceId::LegL => ColorRole::Primary,
                PieceId::SideR | PieceId::SideL | PieceId::SleeveR | PieceId::SleeveL => ColorRole::Accent,
                PieceId::Collar => ColorRole::Trim,
            },
            polygons: piece.polygons.clone(),
            clip: None,
        })
        .collect();

    let rect = |[x, y, w, h]: [f32; 4]| vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
    for cuff in &pattern.cuffs {
        out.push(Region {
            id: RegionId::Cuff(side_of(cuff.piece)),
            role: ColorRole::Trim,
            polygons: vec![rect(cuff.rect)],
            clip: Some(cuff.piece),
        });
    }
    let m = pattern.atlas.px_per_m;
    for leg in &pattern.legs {
        let [x, y, w, h] = pattern.piece(leg.piece).rect;
        let s = &leg.stripe;
        let (top, top_half) = (x + w * s.top_centre, w * s.top_width / 2.0);
        let (hem, hem_half) = (x + w * s.hem_centre, w * s.hem_width / 2.0);
        // a little past the piece so the stripe reaches the waist and hem edges
        out.push(Region {
            id: RegionId::LegStripe(side_of(leg.piece)),
            role: ColorRole::Accent,
            polygons: vec![vec![
                [top - top_half, y - 4.0],
                [top + top_half, y - 4.0],
                [hem + hem_half, y + h + 4.0],
                [hem - hem_half, y + h + 4.0],
            ]],
            clip: Some(leg.piece),
        });
        out.push(Region {
            id: RegionId::LegHem(side_of(leg.piece)),
            role: ColorRole::Trim,
            polygons: vec![rect([x - 20.0, y + h - LEG_HEM_BAND * m, w + 40.0, LEG_HEM_BAND * m + 20.0])],
            clip: Some(leg.piece),
        });
    }
    out
}

/// Topmost region under an atlas pixel.
pub fn hit_test(pattern: &Pattern, regions: &[Region], at: Pt) -> Option<RegionId> {
    regions
        .iter()
        .rev()
        .find(|r| {
            r.polygons.iter().any(|poly| point_in_polygon(poly, at))
                && r.clip.is_none_or(|piece| pattern.piece(piece).contains(at))
        })
        .map(|r| r.id)
}

pub fn distance_to_segment(p: Pt, a: Pt, b: Pt) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0)
    };
    (p[0] - (a[0] + ab[0] * t)).hypot(p[1] - (a[1] + ab[1] * t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::pattern::Panel;

    #[test]
    fn hit_test_finds_pieces_and_details() {
        let p = Pattern::load();
        let r = regions(&p);
        let hit = |at| hit_test(&p, &r, at);
        assert_eq!(hit(p.place(Panel::Front, [0.0, 1.1])), Some(RegionId::Piece(PieceId::Front)));
        assert_eq!(hit(p.place(Panel::Back, [0.0, 1.1])), Some(RegionId::Piece(PieceId::Back)));
        let [x, y, w, h] = p.piece(PieceId::SleeveL).rect;
        assert_eq!(hit([x + w / 2.0, y + h * 0.4]), Some(RegionId::Piece(PieceId::SleeveL)));
        assert_eq!(hit([x + w / 2.0, y + h - 3.0]), Some(RegionId::Cuff(Side::Left)));
        let leg = &p.legs[0];
        assert_eq!(hit(p.place_on_leg(leg, 0.35, 0.2)), Some(RegionId::Piece(leg.piece)));
        assert_eq!(hit(p.place_on_leg(leg, 0.0, 0.003)), Some(RegionId::LegHem(Side::Right)));
        // the template background between pieces is no region
        assert_eq!(hit([2.0, 2.0]), None);
    }
}
