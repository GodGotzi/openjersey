//! Ready-made designs, built for the open model's template.
//!
//! Positions are front-view garment meters (x right = wearer's left, z up;
//! hem at z = 0.86, neck at about 1.46), placed on the template with
//! `Pattern::place`. Sizes in centimetres are converted with the template's
//! pixels per metre.

use crate::document::{BaseColors, Document, Object, ObjectKind, Rgb, Xf};
use crate::i18n::tr;
use crate::template::pattern::{Panel, Pattern, PieceId, Pt};

/// A named design builder.
pub type Preset = (&'static str, fn(&Pattern) -> Document);

pub const PRESETS: [Preset; 1] = [("Example", example)];

pub fn by_name(pattern: &Pattern, name: &str) -> Option<Document> {
    let name = name.to_lowercase();
    PRESETS.iter().find(|(n, _)| n.to_lowercase().contains(&name)).map(|(_, build)| build(pattern))
}

/// Room for the player name on the upper back, between the raglan seams.
const NAME_WIDTH_M: f32 = 0.20;

/// Font size for a given cap height (DejaVu caps are ~73% of the em).
fn font_for_cap(cap_m: f32) -> f32 {
    cap_m / 0.73
}

fn text(m: f32, name: &str, text: &str, at: Pt, cap_m: f32, color: Option<Rgb>, fatten: f32) -> Object {
    Object::new(
        name,
        Xf::at(at),
        ObjectKind::Text { text: text.into(), size: font_for_cap(cap_m) * m, color, fatten, italic: 0.0, max_width: None },
    )
}

/// Text that is condensed when it gets wider than `max_m` (player names).
fn fitted(mut obj: Object, max_m: f32, m: f32) -> Object {
    if let ObjectKind::Text { max_width, .. } = &mut obj.kind {
        *max_width = Some(max_m * m);
    }
    obj
}

/// The sponsor across the stomach; higher up on short tops (the stomach
/// isn't on the front piece), on the chest beside the zip on zip jackets.
fn sponsor(pattern: &Pattern, m: f32) -> Object {
    let front = |p| pattern.place(Panel::Front, p);
    if pattern.get(PieceId::new("zipper")).is_some() {
        return text(m, "Sponsor", "SPONSOR", front([-0.09, 1.24]), 0.025 * 0.73, None, 0.03);
    }
    text(m, "Sponsor", "SPONSOR", on_fabric(pattern, Panel::Front, [0.0, 1.08]), 0.045 * 0.73, None, 0.03)
}

/// `at` placed on `panel`, moved up or down to the nearest height that is on
/// the piece (with a little room) for cuts that don't reach it (crop tops,
/// racerbacks).
fn on_fabric(pattern: &Pattern, panel: Panel, [x, z]: Pt) -> Pt {
    let piece = pattern.piece(if panel == Panel::Front { PieceId::FRONT } else { PieceId::BACK });
    let fits = |z: f32| [-0.02, 0.0, 0.02].iter().all(|dz| piece.contains(pattern.place(panel, [x, z + dz])));
    let z = (0..80).flat_map(|i| [z + i as f32 * 0.005, z - i as f32 * 0.005]).find(|&z| fits(z)).unwrap_or(z);
    pattern.place(panel, [x, z])
}

/// White jersey with purple sleeves and side panels and a dark collar (on a
/// tank top: white with dark bindings).
pub fn example(pattern: &Pattern) -> Document {
    let m = pattern.atlas.px_per_m;
    let mut doc = Document::for_sheet(
        pattern,
        BaseColors { primary: [0xff, 0xff, 0xff], accent: [0xb6, 0x2f, 0xc4], trim: [0x2b, 0x2b, 0x2e], overrides: Vec::new() },
    );
    let front = |p| on_fabric(pattern, Panel::Front, p);
    let back = |p| on_fabric(pattern, Panel::Back, p);
    doc.add(None, text(m, tr("preset.chest_number"), "7", front([0.075, 1.235]), 0.095 * 0.73, None, 0.08));
    doc.add(None, sponsor(pattern, m));
    doc.add(None, fitted(text(m, "Name", "PLAYER", back([0.0, 1.335]), 0.04 * 0.73, None, 0.03), NAME_WIDTH_M, m));
    doc.add(None, text(m, tr("preset.back_number"), "7", back([0.0, 1.17]), 0.19 * 0.73, None, 0.08));
    if let Some(right_leg) = pattern.legs.iter().find(|l| l.piece == PieceId::LEG_R) {
        doc.add(None, text(m, tr("preset.leg_number"), "7", pattern.place_on_leg(right_leg, 0.17, 0.075), 0.07 * 0.73, None, 0.08));
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::garments::{self, GARMENTS, Kind};

    #[test]
    fn presets_land_on_every_garment() {
        for g in GARMENTS {
            let (top, bottom) = match g.kind {
                Kind::Top => (g, garments::default(Kind::Bottom, g.body)),
                Kind::Bottom => (garments::default(Kind::Top, g.body), g),
            };
            let p = Pattern::of(top, bottom);
            for (_, build) in PRESETS {
                let doc = build(&p);
                assert!(doc.is_on(&p));
                for obj in &doc.objects {
                    assert!(p.pieces.iter().any(|piece| piece.contains(obj.xf.pos)), "{} off the sheet on {}", obj.name, g.id);
                }
            }
        }
    }
}
