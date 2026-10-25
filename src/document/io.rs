//! Saving and loading designs as JSON (graphics embedded), including designs
//! saved in the old flat format.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::legacy::{self, LayerKind};
use super::{Asset, BaseColors, Document, Object, ObjectKind, StrokeStyle, Xf};

/// 3: the portrait sheet layout.
const VERSION: u32 = 3;

#[derive(Serialize, Deserialize)]
struct File {
    openjersey: u32,
    document: Document,
}

pub fn to_json(doc: &Document) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(&File { openjersey: VERSION, document: doc.clone() }).map_err(|e| e.to_string())
}

pub fn from_json(bytes: &[u8]) -> Result<Document, String> {
    let (mut doc, version) = match serde_json::from_slice::<File>(bytes) {
        Ok(file) => (file.document, file.openjersey),
        Err(_) => {
            let old: legacy::Design = serde_json::from_slice(bytes).map_err(|e| format!("kein OpenJersey-Design: {e}"))?;
            (migrate(old), 1)
        }
    };
    if version < 3 {
        super::relayout::from_landscape(&mut doc, &crate::template::pattern::Pattern::load());
    }
    Ok(doc)
}

/// Old flat layers become top-level objects, bottom to top.
fn migrate(old: legacy::Design) -> Document {
    let mut doc = Document::new(BaseColors {
        primary: old.primary,
        accent: old.accent,
        trim: old.trim,
        overrides: old.region_colors,
    });
    for layer in old.layers {
        let obj = match layer.kind {
            LayerKind::Stroke { points, width, color } => {
                Object::shape("Pinselstrich", points, false, None, Some(StrokeStyle { color, width }))
            }
            LayerKind::Shape { points, color } => Object::shape("Form", points, true, Some(color), None),
            LayerKind::Text { text, pos, height, rotation, color, fatten } => Object::new(
                text.clone(),
                Xf { pos, rotation, scale: [1.0, 1.0] },
                ObjectKind::Text { text, size: height, color, fatten, italic: 0.0, max_width: None },
            ),
            LayerKind::Image { logo, pos, width, rotation } => {
                let Ok(asset) = Asset::new("logo.png", logo.file) else { continue };
                let [nw, nh] = asset.natural_size();
                Object::new(
                    "Logo",
                    Xf { pos, rotation, scale: [1.0, 1.0] },
                    ObjectKind::Graphic { asset: Arc::new(asset), size: [width, width * nh / nw], tint: None },
                )
            }
            LayerKind::Pattern { text, height, line_spacing, angle, skew, color, opacity, area } => Object::new(
                "Muster",
                Xf::default(),
                ObjectKind::Pattern { text, size: height, line_spacing, angle, italic: skew, color, area },
            )
            .with_opacity(opacity),
        };
        doc.add(None, obj.with_clip(&layer.clip));
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_flat_designs_load_as_objects() {
        let old = r#"{"primary":[255,255,255],"accent":[1,2,3],"trim":[4,5,6],
            "region_colors":[[{"Piece":"collar"},[9,9,9]]],
            "layers":[{"kind":{"Text":{"text":"7","pos":[10.0,20.0],"height":50.0,"rotation":0.0,"color":null,"fatten":0.08}}},
                      {"kind":{"Stroke":{"points":[[0.0,0.0],[10.0,10.0]],"width":4.0,"color":[1,1,1]}},"clip":["front"]}]}"#;
        let doc = from_json(old.as_bytes()).unwrap();
        assert_eq!(doc.objects.len(), 2);
        assert_eq!(doc.base.accent, [1, 2, 3]);
        // old files are on the landscape sheet; the stroke moves with the front
        let front = crate::template::pattern::Pattern::load().piece(crate::template::pattern::PieceId::Front).rect;
        // shapes are re-centred on their points
        assert_eq!(doc.objects[1].xf.pos, [5.0 + front[0] - 184.24, 5.0 + front[1] - 120.0]);
        let again = from_json(&to_json(&doc).unwrap()).unwrap();
        assert_eq!(again.objects.len(), 2);
        assert_eq!(again.objects[1].xf.pos, doc.objects[1].xf.pos);
        assert_eq!(again.objects[1].clip, doc.objects[1].clip);
    }
}
