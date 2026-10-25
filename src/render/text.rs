//! Glyph outlines from the bundled font.

use ab_glyph::{Font, FontArc, OutlineCurve, PxScale, ScaleFont};
use tiny_skia::PathBuilder;

pub struct Fonts {
    font: FontArc,
}

impl Fonts {
    pub fn new() -> Self {
        let font = FontArc::try_from_slice(include_bytes!("../../assets/DejaVuSansCondensed-Bold.ttf"))
            .expect("bundled font is valid");
        Self { font }
    }

    /// (width, height) of a line of text at font size `size`.
    pub fn text_size(&self, text: &str, size: f32) -> (f32, f32) {
        let scaled = self.font.as_scaled(PxScale::from(size));
        let mut width = 0.0;
        let mut prev = None;
        for ch in text.chars() {
            let id = self.font.glyph_id(ch);
            if let Some(p) = prev {
                width += scaled.kern(p, id);
            }
            width += scaled.h_advance(id);
            prev = Some(id);
        }
        (width, scaled.ascent() - scaled.descent())
    }

    /// Outline of a line of text, centred on the origin (horizontally, and
    /// vertically on the middle of the em box like canvas `textBaseline = "middle"`).
    pub fn text_path(&self, text: &str, size: f32) -> Option<tiny_skia::Path> {
        if text.is_empty() || size <= 0.0 {
            return None;
        }
        let scaled = self.font.as_scaled(PxScale::from(size));
        let (hs, vs) = (scaled.h_scale_factor(), scaled.v_scale_factor());
        let (width, _) = self.text_size(text, size);
        let baseline = (scaled.ascent() + scaled.descent()) / 2.0;
        let mut pen = -width / 2.0;

        let mut pb = PathBuilder::new();
        let mut prev = None;
        for ch in text.chars() {
            let id = self.font.glyph_id(ch);
            if let Some(p) = prev {
                pen += scaled.kern(p, id);
            }
            if let Some(outline) = self.font.outline(id) {
                let map = |p: ab_glyph::Point| (pen + p.x * hs, baseline - p.y * vs);
                let mut last: Option<ab_glyph::Point> = None;
                for curve in &outline.curves {
                    let start = match curve {
                        OutlineCurve::Line(a, _) | OutlineCurve::Quad(a, _, _) | OutlineCurve::Cubic(a, _, _, _) => *a,
                    };
                    if last.is_none_or(|l| l != start) {
                        if last.is_some() {
                            pb.close();
                        }
                        let (x, y) = map(start);
                        pb.move_to(x, y);
                    }
                    last = Some(match curve {
                        OutlineCurve::Line(_, b) => {
                            let (x, y) = map(*b);
                            pb.line_to(x, y);
                            *b
                        }
                        OutlineCurve::Quad(_, c, b) => {
                            let ((cx, cy), (x, y)) = (map(*c), map(*b));
                            pb.quad_to(cx, cy, x, y);
                            *b
                        }
                        OutlineCurve::Cubic(_, c1, c2, b) => {
                            let ((ax, ay), (bx, by), (x, y)) = (map(*c1), map(*c2), map(*b));
                            pb.cubic_to(ax, ay, bx, by, x, y);
                            *b
                        }
                    });
                }
                if last.is_some() {
                    pb.close();
                }
            }
            pen += scaled.h_advance(id);
            prev = Some(id);
        }
        pb.finish()
    }
}
