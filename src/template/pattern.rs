//! The print template layout from `assets/pattern.json` (written by
//! `blender/build_jersey.py`): one atlas holding every garment piece side by
//! side like the ninesquared template (front, back, side panels, sleeves,
//! collar, legs). Coordinates are atlas pixels, y down.

use serde::Deserialize;

pub type Pt = [f32; 2];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceId {
    Front,
    Back,
    SideR,
    SideL,
    SleeveR,
    SleeveL,
    Collar,
    LegR,
    LegL,
}

impl PieceId {
    pub fn garment(self) -> Garment {
        match self {
            PieceId::LegR | PieceId::LegL => Garment::Shorts,
            _ => Garment::Shirt,
        }
    }
}

/// The two garments on the sheet; a design can leave one out (e.g. a
/// warm-up shirt without shorts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Garment {
    Shirt,
    Shorts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Front,
    Back,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub px_per_m: f32,
}

#[derive(Debug, Deserialize)]
pub struct Piece {
    pub id: PieceId,
    pub label: String,
    /// x, y, w, h
    pub rect: [f32; 4],
    pub polygons: Vec<Vec<Pt>>,
}

#[derive(Debug, Deserialize)]
pub struct Cuff {
    pub piece: PieceId,
    pub rect: [f32; 4],
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Placement {
    origin: Pt,
    x_sign: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leg {
    pub piece: PieceId,
    /// +1 if the front half of the leg piece is its right half, else -1;
    /// the outer side seam runs down the middle
    pub front_dir: f32,
    pub stripe: LegStripe,
}

/// The leg stripe continues the jersey's side panel: straight down from the
/// jersey's side seam and as wide as the panel, measured on the fabric at the
/// waist and at the hem. Fractions of the piece width.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegStripe {
    pub top_centre: f32,
    pub top_width: f32,
    pub hem_centre: f32,
    pub hem_width: f32,
}

#[derive(Debug, Deserialize)]
struct Placements {
    front: Placement,
    back: Placement,
}

#[derive(Debug, Deserialize)]
pub struct Pattern {
    pub atlas: Atlas,
    pub pieces: Vec<Piece>,
    pub cuffs: Vec<Cuff>,
    /// x, y, w, h of the plain square the fabric's inside is mapped to
    pub inside: [f32; 4],
    placement: Placements,
    pub legs: Vec<Leg>,
}

impl Pattern {
    pub fn load() -> Self {
        serde_json::from_str(include_str!("../../assets/pattern.json")).expect("assets/pattern.json is malformed")
    }

    pub fn size(&self) -> (u32, u32) {
        (self.atlas.width, self.atlas.height)
    }

    pub fn piece(&self, id: PieceId) -> &Piece {
        self.pieces.iter().find(|p| p.id == id).expect("every piece is in pattern.json")
    }

    /// Atlas pixel of a point on the front or back, given in front-view
    /// garment meters (x right = wearer's left, z up).
    pub fn place(&self, panel: Panel, [x, z]: Pt) -> Pt {
        let p = match panel {
            Panel::Front => &self.placement.front,
            Panel::Back => &self.placement.back,
        };
        let m = self.atlas.px_per_m;
        [p.origin[0] + p.x_sign * x * m, p.origin[1] - z * m]
    }

    /// Inverse of `place`: template pixel -> front-view garment meters.
    pub fn unplace(&self, panel: Panel, [px, py]: Pt) -> Pt {
        let p = match panel {
            Panel::Front => &self.placement.front,
            Panel::Back => &self.placement.back,
        };
        let m = self.atlas.px_per_m;
        [(px - p.origin[0]) / (p.x_sign * m), (p.origin[1] - py) / m]
    }

    /// Point on a leg piece: `across` from -0.5 (back edge) through 0 (outer
    /// side seam) to 0.5 (front edge), `up_m` meters above the hem.
    pub fn place_on_leg(&self, leg: &Leg, across: f32, up_m: f32) -> Pt {
        let [x, y, w, h] = self.piece(leg.piece).rect;
        [x + w * (0.5 + leg.front_dir * across), y + h - up_m * self.atlas.px_per_m]
    }
}

pub fn point_in_polygon(poly: &[Pt], [x, y]: Pt) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let ([xi, yi], [xj, yj]) = (poly[i], poly[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

impl Piece {
    pub fn contains(&self, p: Pt) -> bool {
        self.polygons.iter().any(|poly| point_in_polygon(poly, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_portrait_with_the_shirt_on_top() {
        let p = Pattern::load();
        let x = |id| p.piece(id).rect[0];
        let y = |id| p.piece(id).rect[1];
        assert!(p.atlas.height > p.atlas.width);
        // front and back, then side, sleeve, sleeve, side, then the collar
        assert!(x(PieceId::Front) < x(PieceId::Back) && y(PieceId::Front) == y(PieceId::Back));
        assert!(x(PieceId::SideR) < x(PieceId::SleeveR) && x(PieceId::SleeveL) < x(PieceId::SideL));
        assert!(y(PieceId::SleeveR) > y(PieceId::Front) && y(PieceId::Collar) > y(PieceId::SleeveR));
        // the shorts below everything of the shirt
        assert!(y(PieceId::LegR) > y(PieceId::Collar) && y(PieceId::LegL) > y(PieceId::LegR));
    }

    #[test]
    fn placement_lands_on_the_pieces() {
        let p = Pattern::load();
        assert!(p.piece(PieceId::Front).contains(p.place(Panel::Front, [0.075, 1.235])));
        assert!(p.piece(PieceId::Back).contains(p.place(Panel::Back, [0.0, 1.17])));
        let leg = &p.legs[0];
        assert!(p.piece(leg.piece).contains(p.place_on_leg(leg, 0.17, 0.075)));
    }
}
