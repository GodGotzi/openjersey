//! Imported images (PNG/JPG and SVG), kept as their original file bytes so a
//! saved design is self-contained; decoded and rasterised on demand.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tiny_skia::{IntSize, Pixmap};

use super::Rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetFormat {
    Raster,
    Svg,
}

pub struct Asset {
    pub name: String,
    pub format: AssetFormat,
    pub bytes: Vec<u8>,
    decoded: OnceLock<Option<Decoded>>,
    cache: Mutex<HashMap<(u32, u32, Option<Rgb>), Arc<Pixmap>>>,
}

enum Decoded {
    Raster(Pixmap),
    Svg(resvg::usvg::Tree),
}

impl std::fmt::Debug for Asset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Asset").field("name", &self.name).field("format", &self.format).finish()
    }
}

impl Asset {
    pub fn new(name: impl Into<String>, bytes: Vec<u8>) -> Result<Self, String> {
        let name = name.into();
        let lower = name.to_lowercase();
        let looks_svg = lower.ends_with(".svg") || bytes.starts_with(b"<?xml") || bytes.starts_with(b"<svg");
        let asset = Self {
            name,
            format: if looks_svg { AssetFormat::Svg } else { AssetFormat::Raster },
            bytes,
            decoded: OnceLock::new(),
            cache: Mutex::new(HashMap::new()),
        };
        if asset.decoded().is_none() {
            return Err(format!("{} konnte nicht gelesen werden", asset.name));
        }
        Ok(asset)
    }

    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("bild").to_string();
        Self::new(name, bytes)
    }

    fn decoded(&self) -> Option<&Decoded> {
        self.decoded
            .get_or_init(|| match self.format {
                AssetFormat::Svg => {
                    let opt = resvg::usvg::Options::default();
                    resvg::usvg::Tree::from_data(&self.bytes, &opt).ok().map(Decoded::Svg)
                }
                AssetFormat::Raster => {
                    let img = image::load_from_memory(&self.bytes).ok()?.to_rgba8();
                    let size = IntSize::from_wh(img.width(), img.height())?;
                    // tiny-skia wants premultiplied alpha
                    let data = img
                        .into_raw()
                        .chunks_exact(4)
                        .flat_map(|p| {
                            let a = p[3] as u16;
                            [(p[0] as u16 * a / 255) as u8, (p[1] as u16 * a / 255) as u8, (p[2] as u16 * a / 255) as u8, p[3]]
                        })
                        .collect();
                    Pixmap::from_vec(data, size).map(Decoded::Raster)
                }
            })
            .as_ref()
    }

    /// Natural size in the file's own units (pixels / SVG user units).
    pub fn natural_size(&self) -> [f32; 2] {
        match self.decoded() {
            Some(Decoded::Raster(p)) => [p.width() as f32, p.height() as f32],
            Some(Decoded::Svg(t)) => [t.size().width(), t.size().height()],
            None => [1.0, 1.0],
        }
    }

    /// The graphic as a premultiplied pixmap of about `w` x `h` pixels
    /// (SVGs are rendered at exactly that size, rasters returned as they are),
    /// optionally recoloured to `tint`.
    pub fn raster(&self, w: u32, h: u32, tint: Option<Rgb>) -> Option<Arc<Pixmap>> {
        let (w, h) = (w.clamp(1, 4096), h.clamp(1, 4096));
        let key = match self.format {
            AssetFormat::Svg => (w, h, tint),
            AssetFormat::Raster => (0, 0, tint),
        };
        if let Some(hit) = self.cache.lock().ok()?.get(&key) {
            return Some(hit.clone());
        }
        let mut pm = match self.decoded()? {
            Decoded::Raster(p) => p.clone(),
            Decoded::Svg(tree) => {
                let mut pm = Pixmap::new(w, h)?;
                let size = tree.size();
                let t = tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
                resvg::render(tree, t, &mut pm.as_mut());
                pm
            }
        };
        if let Some([r, g, b]) = tint {
            for px in pm.data_mut().chunks_exact_mut(4) {
                let a = px[3] as u16;
                px[0] = (r as u16 * a / 255) as u8;
                px[1] = (g as u16 * a / 255) as u8;
                px[2] = (b as u16 * a / 255) as u8;
            }
        }
        let pm = Arc::new(pm);
        let mut cache = self.cache.lock().ok()?;
        if cache.len() > 16 {
            cache.clear();
        }
        cache.insert(key, pm.clone());
        Some(pm)
    }
}

#[derive(Serialize, Deserialize)]
struct AssetFile {
    name: String,
    data: String,
}

impl Serialize for Asset {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        AssetFile { name: self.name.clone(), data: base64::engine::general_purpose::STANDARD.encode(&self.bytes) }
            .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Asset {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let f = AssetFile::deserialize(d)?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(f.data).map_err(serde::de::Error::custom)?;
        Asset::new(f.name, bytes).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_renders_at_the_requested_size_and_tint() {
        let a = Asset::new("vbc-logo.svg", include_bytes!("../../assets/vbc-logo.svg").to_vec()).unwrap();
        assert_eq!(a.format, AssetFormat::Svg);
        let pm = a.raster(200, 280, Some([255, 0, 0])).unwrap();
        assert_eq!((pm.width(), pm.height()), (200, 280));
        let opaque = pm.pixels().iter().filter(|p| p.alpha() == 255).count();
        assert!(opaque > 1000);
        assert!(pm.pixels().iter().filter(|p| p.alpha() == 255).all(|p| p.red() == 255 && p.green() == 0));
    }
}
