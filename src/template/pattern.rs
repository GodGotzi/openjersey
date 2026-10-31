//! The print sheet of a design: the top's atlas with the bottom's below it,
//! from the garments' `pattern.json` files (written by `blender/build.py`,
//! see blender/GARMENTS.md). Each atlas holds every piece of its garment side
//! by side like a sublimation print template (front, back, side panels, sleeves,
//! collar, … ; legs, waistband, …). Coordinates are sheet pixels, y down.

use std::collections::HashSet;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::regions::ColorRole;
use crate::garments::{self, Garment, Kind};

pub type Pt = [f32; 2];

/// A garment piece, by the id its pattern.json gives it ("front", "sleeve_l",
/// "leg_r", "waistband", …). Ids are unique within a top and bottom.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PieceId(&'static str);

impl PieceId {
    pub const FRONT: Self = Self("front");
    pub const BACK: Self = Self("back");
    pub const SIDE_L: Self = Self("side_l");
    pub const SIDE_R: Self = Self("side_r");
    pub const SLEEVE_L: Self = Self("sleeve_l");
    pub const SLEEVE_R: Self = Self("sleeve_r");
    pub const COLLAR: Self = Self("collar");
    pub const LEG_L: Self = Self("leg_l");
    pub const LEG_R: Self = Self("leg_r");

    pub fn new(id: &str) -> Self {
        static IDS: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
        let mut ids = IDS.lock().expect("not poisoned");
        let ids = ids.get_or_insert_with(HashSet::new);
        match ids.get(id) {
            Some(&s) => Self(s),
            None => {
                let s: &'static str = Box::leak(id.to_owned().into_boxed_str());
                ids.insert(s);
                Self(s)
            }
        }
    }

    pub fn as_str(self) -> &'static str {
        self.0
    }

    /// The wearer's side the piece is on, by its `_l` / `_r` suffix.
    pub fn side(self) -> Option<super::regions::Side> {
        use super::regions::Side;
        if self.0.ends_with("_l") {
            Some(Side::Left)
        } else if self.0.ends_with("_r") {
            Some(Side::Right)
        } else {
            None
        }
    }
}

impl std::fmt::Debug for PieceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl Serialize for PieceId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for PieceId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(PieceId::new(&String::deserialize(d)?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Front,
    Back,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub px_per_m: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Piece {
    pub id: PieceId,
    /// printed on the sheet
    pub label: String,
    /// shown in the editor
    pub name: String,
    /// the base colour it takes by default
    pub role: ColorRole,
    /// x, y, w, h
    pub rect: [f32; 4],
    pub polygons: Vec<Vec<Pt>>,
    /// top or bottom
    #[serde(skip, default = "top")]
    pub kind: Kind,
}

fn top() -> Kind {
    Kind::Top
}

#[derive(Debug, Clone, Deserialize)]
pub struct Cuff {
    pub piece: PieceId,
    pub rect: [f32; 4],
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Placement {
    origin: Pt,
    x_sign: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leg {
    pub piece: PieceId,
    /// +1 if the front half of the leg piece is its right half, else -1;
    /// the outer side seam runs down the middle
    pub front_dir: f32,
    pub stripe: LegStripe,
}

/// The leg stripe runs down the outer side seam, measured on the fabric at
/// the waist and at the hem. Fractions of the piece width.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegStripe {
    pub top_centre: f32,
    pub top_width: f32,
    pub hem_centre: f32,
    pub hem_width: f32,
}

#[derive(Debug, Clone, Deserialize)]
struct Placements {
    front: Placement,
    back: Placement,
}

/// One garment's pattern.json.
#[derive(Debug, Deserialize)]
struct File {
    atlas: Atlas,
    pieces: Vec<Piece>,
    #[serde(default)]
    cuffs: Vec<Cuff>,
    inside: [f32; 4],
    placement: Option<Placements>,
    #[serde(default)]
    legs: Vec<Leg>,
}

/// Where a garment's own atlas lies on the sheet (its model's UVs cover it).
#[derive(Debug, Clone)]
pub struct Part {
    pub garment: &'static Garment,
    /// x, y, w, h in sheet pixels
    pub rect: [f32; 4],
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub atlas: Atlas,
    pub pieces: Vec<Piece>,
    pub cuffs: Vec<Cuff>,
    /// x, y, w, h of the plain squares the fabric's inside is mapped to
    pub insides: Vec<[f32; 4]>,
    placement: Placements,
    pub legs: Vec<Leg>,
    /// top, then bottom
    pub parts: [Part; 2],
}

/// The key of a piece's name in `locales/` (`Left Sleeve` is `left_sleeve`
/// under `piece.`).
pub fn piece_key(name: &str) -> String {
    let mut key = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            key.push(c.to_ascii_lowercase());
        } else if !key.ends_with('_') {
            key.push('_');
        }
    }
    key.trim_matches('_').to_string()
}

/// A piece's name in the current language: the pattern's own name when the
/// locale has none.
pub fn piece_name(piece: &Piece) -> String {
    crate::i18n::lookup(&format!("piece.{}", piece_key(&piece.name))).map_or_else(|| piece.name.clone(), str::to_string)
}

impl Pattern {
    /// The sheet of a top and a bottom: the top's atlas, the bottom's centred below.
    pub fn of(top: &'static Garment, bottom: &'static Garment) -> Self {
        let parse = |g: &Garment| -> File {
            serde_json::from_str(g.pattern_json).unwrap_or_else(|e| panic!("{}: pattern.json is malformed: {e}", g.id))
        };
        let (t, b) = (parse(top), parse(bottom));
        let width = t.atlas.width.max(b.atlas.width);
        let height = t.atlas.height + b.atlas.height;
        let offsets = [[((width - t.atlas.width) / 2) as f32, 0.0], [((width - b.atlas.width) / 2) as f32, t.atlas.height as f32]];
        let parts = [
            Part { garment: top, rect: [offsets[0][0], 0.0, t.atlas.width as f32, t.atlas.height as f32] },
            Part { garment: bottom, rect: [offsets[1][0], offsets[1][1], b.atlas.width as f32, b.atlas.height as f32] },
        ];
        let mv = |[x, y]: Pt, [dx, dy]: Pt| [x + dx, y + dy];
        let mv_rect = |[x, y, w, h]: [f32; 4], d: Pt| [x + d[0], y + d[1], w, h];
        let placement = t.placement.clone().expect("a top has a front/back placement");
        let placement = Placements {
            front: Placement { origin: mv(placement.front.origin, offsets[0]), ..placement.front },
            back: Placement { origin: mv(placement.back.origin, offsets[0]), ..placement.back },
        };
        let mut pattern = Pattern {
            atlas: Atlas { width, height, px_per_m: t.atlas.px_per_m },
            pieces: Vec::new(),
            cuffs: Vec::new(),
            insides: Vec::new(),
            placement,
            legs: Vec::new(),
            parts,
        };
        for (file, kind, d) in [(t, Kind::Top, offsets[0]), (b, Kind::Bottom, offsets[1])] {
            for mut piece in file.pieces {
                piece.rect = mv_rect(piece.rect, d);
                for poly in &mut piece.polygons {
                    for p in poly.iter_mut() {
                        *p = mv(*p, d);
                    }
                }
                piece.kind = kind;
                pattern.pieces.push(piece);
            }
            pattern.cuffs.extend(file.cuffs.into_iter().map(|c| Cuff { rect: mv_rect(c.rect, d), ..c }));
            pattern.insides.push(mv_rect(file.inside, d));
            pattern.legs.extend(file.legs);
        }
        pattern
    }

    /// The default top and bottom.
    pub fn load() -> Self {
        Self::of(garments::default(Kind::Top, garments::Body::Man), garments::default(Kind::Bottom, garments::Body::Man))
    }

    pub fn top(&self) -> &'static Garment {
        self.parts[0].garment
    }

    pub fn bottom(&self) -> &'static Garment {
        self.parts[1].garment
    }

    pub fn size(&self) -> (u32, u32) {
        (self.atlas.width, self.atlas.height)
    }

    /// A piece every top has (front, back).
    pub fn piece(&self, id: PieceId) -> &Piece {
        self.get(id).unwrap_or_else(|| panic!("no piece {id:?}"))
    }

    /// A piece of this top or bottom, if it has one.
    pub fn get(&self, id: PieceId) -> Option<&Piece> {
        self.pieces.iter().find(|p| p.id == id)
    }

    /// Is `p` on piece `id` (false if there is no such piece)?
    pub fn on_piece(&self, id: PieceId, p: Pt) -> bool {
        self.get(id).is_some_and(|piece| piece.contains(p))
    }

    /// Sheet pixel of a point on the front or back, given in front-view
    /// garment meters (x right = wearer's left, z up).
    pub fn place(&self, panel: Panel, [x, z]: Pt) -> Pt {
        let p = match panel {
            Panel::Front => &self.placement.front,
            Panel::Back => &self.placement.back,
        };
        let m = self.atlas.px_per_m;
        [p.origin[0] + p.x_sign * x * m, p.origin[1] - z * m]
    }

    /// Inverse of `place`: sheet pixel -> front-view garment meters.
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
    use crate::garments::GARMENTS;

    #[test]
    fn a_hood_covers_the_neck_band_so_the_band_wears_the_jackets_own_colour() {
        use super::super::regions::ColorRole;
        let bottom = garments::default(Kind::Bottom, garments::Body::Man);
        for top in GARMENTS.iter().filter(|g| g.id.starts_with("hooded-jacket")) {
            let sheet = Pattern::of(top, bottom);
            assert!(sheet.get(PieceId::new("hood")).is_some(), "{} has a hood", top.id);
            // in the trim colour the band would show as a stripe around the back of the neck, under the hood
            assert_eq!(sheet.piece(PieceId::COLLAR).role, ColorRole::Primary, "{}", top.id);
        }
    }

    #[test]
    fn every_top_and_bottom_make_a_sheet() {
        let bottom = garments::default(Kind::Bottom, garments::Body::Man);
        for top in GARMENTS.iter().filter(|g| g.kind == Kind::Top) {
            let p = Pattern::of(top, bottom);
            // the top's pieces above the bottom's, inside the sheet
            let lowest_top = p.pieces.iter().filter(|q| q.kind == Kind::Top).map(|q| q.rect[1] + q.rect[3]).fold(0.0, f32::max);
            let highest_bottom = p.pieces.iter().filter(|q| q.kind == Kind::Bottom).map(|q| q.rect[1]).fold(f32::MAX, f32::min);
            assert!(lowest_top < highest_bottom, "{}", top.id);
            for q in &p.pieces {
                assert!(q.rect[0] >= 0.0 && q.rect[0] + q.rect[2] <= p.atlas.width as f32, "{} {:?}", top.id, q.id);
                assert!(q.rect[1] + q.rect[3] <= p.atlas.height as f32, "{} {:?}", top.id, q.id);
            }
            assert!(p.piece(PieceId::FRONT).contains(p.place(Panel::Front, [0.075, 1.235])), "{}", top.id);
            assert!(p.piece(PieceId::BACK).contains(p.place(Panel::Back, [0.0, 1.17])), "{}", top.id);
        }
        for bottom in GARMENTS.iter().filter(|g| g.kind == Kind::Bottom) {
            let p = Pattern::of(garments::default(Kind::Top, bottom.body), bottom);
            for leg in &p.legs {
                assert!(p.piece(leg.piece).contains(p.place_on_leg(leg, 0.17, 0.075)), "{}", bottom.id);
            }
        }
    }

    #[test]
    fn piece_ids_are_interned() {
        assert_eq!(PieceId::new("front"), PieceId::FRONT);
        assert_eq!(PieceId::new("waistband").as_str(), "waistband");
        let json = serde_json::to_string(&PieceId::SLEEVE_L).unwrap();
        assert_eq!(serde_json::from_str::<PieceId>(&json).unwrap(), PieceId::SLEEVE_L);
    }
}
