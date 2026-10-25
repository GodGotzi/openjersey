//! The flat design format saved before the object tree (role colours plus a
//! list of layers), read only to convert old files.

use serde::Deserialize;

use super::Rgb;
use crate::template::pattern::{PieceId, Pt};
use crate::template::regions::RegionId;

#[derive(Debug, Deserialize)]
pub struct Design {
    pub primary: Rgb,
    pub accent: Rgb,
    pub trim: Rgb,
    pub region_colors: Vec<(RegionId, Rgb)>,
    pub layers: Vec<Layer>,
}

#[derive(Debug, Deserialize)]
pub struct Layer {
    pub kind: LayerKind,
    #[serde(default)]
    pub clip: Vec<PieceId>,
}

#[derive(Debug, Deserialize)]
pub enum LayerKind {
    Stroke { points: Vec<Pt>, width: f32, color: Rgb },
    Text { text: String, pos: Pt, height: f32, rotation: f32, color: Option<Rgb>, fatten: f32 },
    Image { logo: LogoFile, pos: Pt, width: f32, rotation: f32 },
    Shape { points: Vec<Pt>, color: Rgb },
    Pattern {
        text: String,
        height: f32,
        line_spacing: f32,
        angle: f32,
        skew: f32,
        color: Rgb,
        opacity: f32,
        area: Vec<Vec<Pt>>,
    },
}

#[derive(Debug, Deserialize)]
pub struct LogoFile {
    pub file: Vec<u8>,
}
