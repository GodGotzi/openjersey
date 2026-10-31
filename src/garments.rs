//! The garment library: every top and bottom a design can be made of, by
//! body, each a 3D model and the print sheet laid out for it.
//! Found in `garments/` at build time (see build.rs and
//! blender/GARMENTS.md).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Body {
    Man,
    Women,
}

impl Body {
    pub const ALL: [Body; 2] = [Body::Man, Body::Women];

    pub fn label(self) -> &'static str {
        match self {
            Body::Man => crate::i18n::tr("Men", "Herren"),
            Body::Women => crate::i18n::tr("Women", "Damen"),
        }
    }
}

#[derive(Debug)]
pub struct Garment {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    pub body: Body,
    pub unisex: bool,
    /// the 3D model, for the 3D view (loaded from disk by the engine)
    pub glb_path: &'static str,
    /// the same model, for picking and previews on the CPU
    pub glb: &'static [u8],
    pub pattern_json: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/garments.rs"));

/// What a design is made of when it doesn't say (designs from before the garment library).
pub const DEFAULT_TOP: &str = "raglan-tshirt-man";
pub const DEFAULT_BOTTOM: &str = "game-shorts-man";

impl Garment {
    /// "Raglan T-Shirt (Women)"
    pub fn label(&self) -> String {
        format!("{} ({})", self.name, self.body.label())
    }

    /// The same product for the other body, if there is one.
    pub fn for_body(&'static self, body: Body) -> Option<&'static Garment> {
        if self.body == body {
            return Some(self);
        }
        GARMENTS.iter().find(|g| g.body == body && g.kind == self.kind && g.name == self.name)
    }
}

pub fn get(id: &str) -> Option<&'static Garment> {
    GARMENTS.iter().find(|g| g.id == id)
}

/// The garment with this id, or the default of its kind (for designs
/// naming a garment this build doesn't have).
pub fn get_or_default(id: &str, kind: Kind) -> &'static Garment {
    get(id).filter(|g| g.kind == kind).unwrap_or_else(|| default(kind, Body::Man))
}

/// The first garment of a kind for a body.
pub fn default(kind: Kind, body: Body) -> &'static Garment {
    let preferred = if kind == Kind::Top { DEFAULT_TOP } else { DEFAULT_BOTTOM };
    get(preferred)
        .and_then(|g| g.for_body(body))
        .or_else(|| GARMENTS.iter().find(|g| g.kind == kind && g.body == body))
        .or_else(|| GARMENTS.iter().find(|g| g.kind == kind))
        .expect("the library has tops and bottoms")
}

/// A garment by id or by part of its name ("bikini bottom", "tank top women").
pub fn find(query: &str) -> Option<&'static Garment> {
    let q = query.to_lowercase();
    get(&q).or_else(|| {
        GARMENTS.iter().find(|g| {
            let hay = format!("{} {}", g.id, g.label()).to_lowercase();
            q.split_whitespace().all(|w| hay.contains(w))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_garment_is_complete() {
        assert!(GARMENTS.iter().any(|g| g.kind == Kind::Top) && GARMENTS.iter().any(|g| g.kind == Kind::Bottom));
        for g in GARMENTS {
            assert!(!g.glb.is_empty() && !g.pattern_json.is_empty(), "{}", g.id);
            assert_eq!(get(g.id).map(|x| x.id), Some(g.id));
        }
        assert_eq!(default(Kind::Top, Body::Man).kind, Kind::Top);
        assert!(find("no such garment").is_none());
    }
}
