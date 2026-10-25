//! Ready-made designs.
//!
//! Positions are front-view garment meters (x right = wearer's left, z up;
//! hem at z = 0.86, neck at about 1.46), placed on the template with
//! `Pattern::place`. Sizes in centimetres are converted with the template's
//! pixels per metre.

use std::sync::{Arc, OnceLock};

use crate::document::{Asset, BaseColors, Document, Object, ObjectId, ObjectKind, Rgb, StrokeStyle, Xf};
use crate::template::pattern::{Panel, Pattern, PieceId, Pt};
use crate::template::regions::{RegionId, Side};

pub const PRESETS: [(&str, fn(&Pattern) -> Document); 2] =
    [("Beispiel (ninesquared)", ninesquared_example), ("VBC Stainach Warm-up", vbc_warmup)];

pub fn by_name(pattern: &Pattern, name: &str) -> Option<Document> {
    let name = name.to_lowercase();
    PRESETS.iter().find(|(n, _)| n.to_lowercase().contains(&name)).map(|(_, build)| build(pattern))
}

const BLACK: Rgb = [0x1a, 0x1a, 0x1a];
const DARK_GREY: Rgb = [0x2f, 0x2f, 0x2f];
const MID_GREY: Rgb = [0x38, 0x38, 0x38];
const LIGHT_GREY: Rgb = [0x41, 0x41, 0x41];
const WHITE: Rgb = [0xff, 0xff, 0xff];
/// Room for the player name on the upper back, between the raglan seams.
const NAME_WIDTH_M: f32 = 0.20;

/// Font size for a given cap height (DejaVu caps are ~73% of the em).
fn font_for_cap(cap_m: f32) -> f32 {
    cap_m / 0.73
}

fn text(m: f32, name: &str, text: &str, at: Pt, cap_m: f32, color: Option<Rgb>, fatten: f32) -> Object {
    Object::new(name, Xf::at(at), ObjectKind::Text {
        text: text.into(),
        size: font_for_cap(cap_m) * m,
        color,
        fatten,
        italic: 0.0,
        max_width: None,
    })
}

/// Text that is condensed when it gets wider than `max_m` (player names).
fn fitted(mut obj: Object, max_m: f32, m: f32) -> Object {
    if let ObjectKind::Text { max_width, .. } = &mut obj.kind {
        *max_width = Some(max_m * m);
    }
    obj
}

fn placeholder(m: f32, label: &str, at: Pt, w_cm: f32, h_cm: f32) -> Object {
    Object::new(format!("Sponsor {label}"), Xf::at(at), ObjectKind::Placeholder {
        label: label.into(),
        size: [w_cm / 100.0 * m, h_cm / 100.0 * m],
    })
}

/// Adds objects and groups them under `name`, pivot at `centre`.
fn add_group(doc: &mut Document, name: &str, centre: Pt, objects: Vec<Object>) -> ObjectId {
    let ids: Vec<ObjectId> = objects.into_iter().map(|o| doc.add(None, o)).collect();
    doc.group(&ids, centre, name).expect("group of fresh objects")
}

/// White jersey with purple sleeves and side panels and a dark collar, like
/// the ninesquared mockups.
pub fn ninesquared_example(pattern: &Pattern) -> Document {
    let m = pattern.atlas.px_per_m;
    let mut doc = Document::new(BaseColors {
        primary: [0xff, 0xff, 0xff],
        accent: [0xb6, 0x2f, 0xc4],
        trim: [0x2b, 0x2b, 0x2e],
        overrides: Vec::new(),
    });
    let front = |p| pattern.place(Panel::Front, p);
    let back = |p| pattern.place(Panel::Back, p);
    let right_leg = &pattern.legs[0];
    doc.add(None, text(m, "Nummer Brust", "7", front([0.075, 1.235]), 0.095 * 0.73, None, 0.08));
    doc.add(None, text(m, "Sponsor", "SPONSOR", front([0.0, 1.08]), 0.045 * 0.73, None, 0.03));
    doc.add(None, fitted(text(m, "Name", "PLAYER", back([0.0, 1.335]), 0.04 * 0.73, None, 0.03), NAME_WIDTH_M, m));
    doc.add(None, text(m, "Nummer Rücken", "7", back([0.0, 1.17]), 0.19 * 0.73, None, 0.08));
    doc.add(None, text(m, "Nummer Hose", "7", pattern.place_on_leg(right_leg, 0.17, 0.075), 0.07 * 0.73, None, 0.08));
    doc
}

/// "VBC Stainach – Warm-up Shirt Design Proposal" (Oct 2026): black shirt,
/// three-level grey Grimming panel over the wearer's left shoulder and sleeve
/// with a "VBC STAINACH" watermark, Grimming contour lines at the hem, white
/// trims, crest and back lettering, sponsor areas A–D as placeholders.
pub fn vbc_warmup(pattern: &Pattern) -> Document {
    let m = pattern.atlas.px_per_m;
    let mut doc = Document::new(BaseColors {
        primary: BLACK,
        accent: LIGHT_GREY,
        trim: WHITE,
        overrides: vec![
            // the grey wraps over the left side only; the right stays black
            (RegionId::Piece(PieceId::SleeveR), BLACK),
            (RegionId::Piece(PieceId::SideR), BLACK),
            (RegionId::LegStripe(Side::Left), BLACK),
            (RegionId::LegStripe(Side::Right), BLACK),
        ],
    });
    // a warm-up shirt: no shorts
    doc.garments.shorts = false;

    // watermark: white, ~1 cm caps, rows 2.3 cm apart, rotated 24 degrees
    let watermark = |name: &str, angle_deg: f32, opacity: f32, clip: PieceId, area: Vec<Pt>| {
        let c = crate::render::geom::points_bounds(area.iter().copied())
            .map(|[x0, y0, x1, y1]| [(x0 + x1) / 2.0, (y0 + y1) / 2.0])
            .unwrap_or_else(|| {
                let [x, y, w, h] = pattern.piece(clip).rect;
                [x + w / 2.0, y + h / 2.0]
            });
        let area = if area.is_empty() { vec![] } else { vec![area.iter().map(|p| [p[0] - c[0], p[1] - c[1]]).collect()] };
        Object::new(name, Xf::at(c), ObjectKind::Pattern {
            text: "VBC STAINACH".into(),
            size: font_for_cap(0.01) * m,
            line_spacing: 0.023 * m,
            angle: angle_deg.to_radians(),
            italic: 0.2,
            color: WHITE,
            area,
        })
        .with_opacity(opacity)
        .with_clip(&[clip])
    };

    // mountain panel: the Grimming skyline as the edge, three levels each
    // shifted towards the shoulder; the watermark only in the inner one
    for (panel, piece, label, angle) in
        [(Panel::Front, PieceId::Front, "vorne", -24.0), (Panel::Back, PieceId::Back, "hinten", 24.0)]
    {
        let mut objects = Vec::new();
        let mut inner = Vec::new();
        for (level, (color, name)) in
            [(DARK_GREY, "Stufe außen"), (MID_GREY, "Stufe Mitte"), (LIGHT_GREY, "Stufe innen")].into_iter().enumerate()
        {
            let mirrored = panel == Panel::Back;
            let points: Vec<Pt> = mountain_panel(level, mirrored).into_iter().map(|p| pattern.place(panel, p)).collect();
            if level == 2 {
                inner = points.clone();
            }
            objects.push(Object::shape(name, points, true, Some(color), None).with_clip(&[piece]));
        }
        objects.push(watermark("Wasserzeichen", angle, 0.14, piece, inner));
        add_group(&mut doc, &format!("Bergpanel {label}"), pattern.place(panel, [0.15, 1.3]), objects);
    }
    let sleeve_centre = {
        let [x, y, w, h] = pattern.piece(PieceId::SleeveL).rect;
        [x + w / 2.0, y + h / 2.0]
    };
    add_group(&mut doc, "Wasserzeichen Ärmel & Seiten", sleeve_centre, vec![
        watermark("Linker Ärmel", -24.0, 0.14, PieceId::SleeveL, vec![]),
        watermark("Linke Seite", -24.0, 0.14, PieceId::SideL, vec![]),
        watermark("Rechte Seite", -24.0, 0.07, PieceId::SideR, vec![]),
    ]);

    // Grimming contour lines at the hem; only the top line is white
    let contours = |panel, piece, half_width: f32, height: f32, spacing: f32, width: f32, colors: &[Rgb]| {
        colors
            .iter()
            .enumerate()
            .map(|(k, &color)| {
                let base = 0.88 + k as f32 * spacing;
                let points = (0..=200)
                    .map(|i| {
                        let t = i as f32 / 200.0;
                        pattern.place(panel, [-half_width + 2.0 * half_width * t, base + height * grimming(t)])
                    })
                    .collect();
                Object::shape(format!("Linie {}", k + 1), points, false, None, Some(StrokeStyle { color, width: width * m }))
                    .with_clip(&[piece])
            })
            .collect::<Vec<_>>()
    };
    let grey = |v: u8| [v, v, v];
    let lines = contours(Panel::Front, PieceId::Front, 0.205, 0.07, 0.02, 0.0035, &[grey(0x6e), grey(0x9e), WHITE]);
    add_group(&mut doc, "Höhenlinien vorne", pattern.place(Panel::Front, [0.0, 0.95]), lines);
    let lines =
        contours(Panel::Back, PieceId::Back, 0.26, 0.09, 0.02, 0.0045, &[grey(0x33), grey(0x4d), grey(0x6e), grey(0x9e), WHITE]);
    add_group(&mut doc, "Höhenlinien hinten", pattern.place(Panel::Back, [0.0, 0.98]), lines);

    // front: crest on the wearer's left chest, ninesquared logo on the right
    if let Ok(crest) = Asset::new("vbc-logo.svg", include_bytes!("../assets/vbc-logo.svg").to_vec()) {
        let [nw, nh] = crest.natural_size();
        let h = 0.09 * m; // ~9 cm tall
        doc.add(None, Object::new("Wappen", Xf::at(pattern.place(Panel::Front, [0.075, 1.27])), ObjectKind::Graphic {
            asset: Arc::new(crest),
            size: [h * nw / nh, h],
            tint: Some(WHITE),
        }));
    }
    if let Ok(logo) = Asset::new("ninesquared-logo.svg", include_bytes!("../assets/ninesquared-logo.svg").to_vec()) {
        let [nw, nh] = logo.natural_size();
        let w = 0.042 * m;
        doc.add(None, Object::new("ninesquared Logo", Xf::at(pattern.place(Panel::Front, [-0.075, 1.27])), ObjectKind::Graphic {
            asset: Arc::new(logo),
            size: [w, w * nh / nw],
            tint: Some(WHITE),
        }));
    }

    // back lettering; the back is narrow between the raglan seams up here, so
    // the wordmark sits a little lower and without the mockup's wide spacing
    let back = |p| pattern.place(Panel::Back, p);
    add_group(&mut doc, "Rücken-Typo", back([0.0, 1.25]), vec![
        text(m, "Vereinsschriftzug", "VBC STAINACH", back([0.0, 1.365]), 0.016, Some(WHITE), 0.0),
        // long names are condensed to the width between the raglan seams
        fitted(text(m, "Spielername", "PLAYER NAME", back([0.0, 1.30]), 0.05, Some(WHITE), 0.04), NAME_WIDTH_M, m),
        text(m, "Rückennummer", "99", back([0.0, 1.16]), 0.2, Some(WHITE), 0.04),
    ]);

    // sponsor areas, maximum print areas for size L
    let sleeve = |id: PieceId| {
        let [x, y, w, h] = pattern.piece(id).rect;
        [x + w / 2.0, y + h * 0.45]
    };
    add_group(&mut doc, "Sponsoren", pattern.place(Panel::Front, [0.0, 1.1]), vec![
        placeholder(m, "A", pattern.place(Panel::Front, [0.0, 1.14]), 34.0, 10.0),
        placeholder(m, "B", pattern.place(Panel::Front, [0.0, 1.03]), 23.0, 6.0),
        placeholder(m, "C", sleeve(PieceId::SleeveL), 7.0, 6.0),
        placeholder(m, "C", sleeve(PieceId::SleeveR), 7.0, 6.0),
        placeholder(m, "D", pattern.place(Panel::Back, [0.0, 0.905]), 36.0, 8.0),
    ]);
    doc
}

/// The Grimming's skyline from `assets/grimming.svg` (west to east), height
/// 0..1 over t = 0..1. The SVG draws the ridge as a thick line; its centre
/// line is taken column by column from a rendering of it.
fn grimming(t: f32) -> f32 {
    static PROFILE: OnceLock<Vec<f32>> = OnceLock::new();
    let profile = PROFILE.get_or_init(|| {
        let asset = Asset::new("grimming.svg", include_bytes!("../assets/grimming.svg").to_vec()).expect("bundled svg");
        let [nw, nh] = asset.natural_size();
        let w = 800u32;
        let h = (w as f32 * nh / nw) as u32;
        let pm = asset.raster(w, h, None).expect("renders");
        let mut centre: Vec<Option<f32>> = (0..w)
            .map(|x| {
                let ys: Vec<u32> = (0..h).filter(|&y| pm.pixel(x, y).is_some_and(|p| p.alpha() > 128)).collect();
                Some((*ys.first()? + *ys.last()?) as f32 / 2.0)
            })
            .collect();
        // keep the drawn span only, fill small gaps
        let first = centre.iter().position(|c| c.is_some()).unwrap_or(0);
        let last = centre.iter().rposition(|c| c.is_some()).unwrap_or(w as usize - 1);
        centre = centre[first..=last].to_vec();
        let mut prev = centre.iter().flatten().next().copied().unwrap_or(0.0);
        let ys: Vec<f32> = centre
            .into_iter()
            .map(|c| {
                prev = c.unwrap_or(prev);
                prev
            })
            .collect();
        let (top, bottom) = ys.iter().fold((f32::MAX, f32::MIN), |(a, b), &y| (a.min(y), b.max(y)));
        ys.iter().map(|y| (bottom - y) / (bottom - top).max(1.0)).collect()
    });
    let x = t.clamp(0.0, 1.0) * (profile.len() - 1) as f32;
    let i = (x as usize).min(profile.len() - 2);
    profile[i] + (profile[i + 1] - profile[i]) * (x - i as f32)
}

/// Mountain panel outline for one level (0 = outer), front-view meters.
/// Its edge is the Grimming skyline itself, as in the contour lines at the
/// hem and in the same proportions, laid from the side seam (west) up to the
/// neck (east) with the peaks pointing towards the centre; the panel is the
/// mountain below it. Each level is the same ridge moved further towards the
/// shoulder. `mirrored` runs it east to west, so on the back it reads the
/// right way round from behind. The rest is generous and gets clipped to the
/// piece.
fn mountain_panel(level: usize, mirrored: bool) -> Vec<Pt> {
    const SEAM: Pt = [0.235, 1.03];
    const NECK: Pt = [-0.035, 1.47];
    const HEIGHT: f32 = 0.17; // of the length, like the hem contour lines
    const STEP: f32 = 0.018; // between levels
    let len = (NECK[0] - SEAM[0]).hypot(NECK[1] - SEAM[1]);
    let d = [(NECK[0] - SEAM[0]) / len, (NECK[1] - SEAM[1]) / len];
    let towards_centre = [-d[1], d[0]];
    let shift = -STEP * level as f32;
    let at = |s: f32, up: f32| {
        let off = up + shift;
        [SEAM[0] + d[0] * s + towards_centre[0] * off, SEAM[1] + d[1] * s + towards_centre[1] * off]
    };
    let ridge = |t: f32| HEIGHT * len * grimming(if mirrored { 1.0 - t } else { t });
    let n = 400;
    let mut pts: Vec<Pt> = (0..=n).map(|i| i as f32 / n as f32).map(|t| at(t * len, ridge(t))).collect();
    // carry the ends on past the seam and the neck, then close over the shoulder
    let (start, end) = (at(-0.15, ridge(0.0)), at(len + 0.15, ridge(1.0)));
    let far = |p: Pt| [p[0] - towards_centre[0] * 0.6, p[1] - towards_centre[1] * 0.6];
    pts.insert(0, start);
    pts.extend([end, far(end), far(start)]);
    pts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::pattern::point_in_polygon;

    #[test]
    fn mountain_levels_step_towards_the_shoulder() {
        let outer = mountain_panel(0, false);
        let inner = mountain_panel(2, false);
        // just inside the outer ridge, towards the shoulder
        let edge = outer[200];
        let p = [edge[0] + 0.005, edge[1] + 0.003];
        assert!(point_in_polygon(&outer, p) && !point_in_polygon(&inner, p));
        assert!(point_in_polygon(&inner, [0.2, 1.38]));
        // the centre of the chest stays free
        assert!(!point_in_polygon(&outer, [0.0, 1.2]));
    }

    #[test]
    fn mountain_edge_is_the_grimming_profile() {
        // distance of the ridge from the seam-neck line follows the skyline
        let pts = mountain_panel(0, false);
        let (seam, neck) = ([0.235f32, 1.03f32], [-0.035f32, 1.47f32]);
        let len = (neck[0] - seam[0]).hypot(neck[1] - seam[1]);
        let n = [-(neck[1] - seam[1]) / len, (neck[0] - seam[0]) / len];
        for (i, t) in [(1, 0.0), (101, 0.25), (201, 0.5), (301, 0.75)] {
            let p = pts[i];
            let off = (p[0] - seam[0]) * n[0] + (p[1] - seam[1]) * n[1];
            assert!((off - 0.17 * len * grimming(t)).abs() < 1e-4);
        }
    }

    #[test]
    fn grimming_profile_comes_from_the_svg() {
        let samples: Vec<f32> = (0..=100).map(|i| grimming(i as f32 / 100.0)).collect();
        assert!(samples.iter().all(|h| (0.0..=1.0).contains(h)));
        // low at both ends, the summit in the middle part
        let top = samples.iter().cloned().fold(0.0, f32::max);
        assert!(top > 0.95 && samples[0] < 0.3 && samples[100] < 0.3);
        let summit = samples.iter().position(|&h| h == top).unwrap();
        assert!((35..=85).contains(&summit));
    }

    #[test]
    fn presets_build_named_groups() {
        let p = Pattern::load();
        let doc = vbc_warmup(&p);
        let names: Vec<_> = doc.objects.iter().map(|o| o.name.as_str()).collect();
        for expected in ["Bergpanel vorne", "Bergpanel hinten", "Höhenlinien vorne", "Wappen", "Rücken-Typo", "Sponsoren"] {
            assert!(names.contains(&expected), "{expected} missing in {names:?}");
        }
    }
}
