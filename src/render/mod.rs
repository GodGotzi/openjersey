//! Renders the print template: garment pieces in their base colours, then
//! the object tree on top, with tiny-skia.
//!
//! The base layer and the per-piece clip masks are cached. While an object is
//! being dragged, `Rasterizer::frame` keeps everything below and above it as
//! two cached layers, so a frame only redraws the moving object.

pub mod geom;
pub mod text;

use std::cell::RefCell;
use std::collections::HashMap;

use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash, Transform,
};

use crate::document::{BaseColors, Document, Object, ObjectId, ObjectKind, Rgb};
use crate::template::pattern::{Pattern, PieceId, Pt};
use crate::template::regions::Region;
use text::Fonts;

/// Template background between the pieces (the grey of the ninesquared sheets).
pub const BACKGROUND: Rgb = [0xe4, 0xe4, 0xe7];
const BLEED: f32 = 3.0;
const SELECTION: Rgb = [0x46, 0x8c, 0xff];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The print file: no placeholders, no selection.
    Print,
    /// For the editor and the 3D view: placeholder boxes shown.
    Display,
}

pub struct Rasterizer {
    pub fonts: Fonts,
    base: RefCell<Option<(BaseColors, Pixmap)>>,
    masks: RefCell<HashMap<Vec<PieceId>, Mask>>,
    split: RefCell<Option<Split>>,
}

/// Cached layers around the object being dragged.
struct Split {
    key: (ObjectId, u64),
    below: Pixmap,
    above: Pixmap,
}

impl Rasterizer {
    pub fn new() -> Self {
        Self {
            fonts: Fonts::new(),
            base: RefCell::new(None),
            masks: RefCell::new(HashMap::new()),
            split: RefCell::new(None),
        }
    }

    /// The whole template (opaque, so premultiplied = straight RGBA).
    pub fn render(&self, pattern: &Pattern, regions: &[Region], doc: &Document, mode: Mode) -> Pixmap {
        let mut pm = self.base(pattern, regions, &doc.base);
        self.draw_objects(&mut pm, pattern, doc, &doc.objects, Transform::identity(), mode);
        self.blank_left_out(&mut pm, pattern, doc);
        draw_inside(&mut pm, pattern, doc, mode);
        pm
    }

    /// Paints the pieces of left-out garments (and whatever lies on them)
    /// over with the sheet background.
    fn blank_left_out(&self, pm: &mut Pixmap, pattern: &Pattern, doc: &Document) {
        // a bit wider than the bleed, so no anti-aliased fringe stays behind
        let cover = Stroke { width: BLEED * 2.0 + 4.0, line_join: LineJoin::Round, ..Stroke::default() };
        let bg = paint(BACKGROUND, 1.0);
        for piece in pattern.pieces.iter().filter(|p| !doc.garments.shows_piece(p.id)) {
            for path in piece.polygons.iter().filter_map(|poly| polygon_path(poly, true)) {
                pm.fill_path(&path, &bg, FillRule::Winding, Transform::identity(), None);
                pm.stroke_path(&path, &bg, &cover, Transform::identity(), None);
            }
        }
    }

    /// Like `render`, but while `moving` (a top-level object) is dragged,
    /// everything else is rendered once and reused. `generation` must change
    /// whenever anything but the moving object's transform changes.
    pub fn frame(
        &self,
        pattern: &Pattern,
        regions: &[Region],
        doc: &Document,
        mode: Mode,
        moving: Option<(ObjectId, u64)>,
    ) -> Pixmap {
        let Some((id, generation)) = moving else {
            self.split.replace(None);
            return self.render(pattern, regions, doc, mode);
        };
        let Some(index) = doc.objects.iter().position(|o| o.id == id) else {
            return self.render(pattern, regions, doc, mode);
        };
        let key = (id, generation);
        if self.split.borrow().as_ref().map(|s| s.key) != Some(key) {
            let mut below = self.base(pattern, regions, &doc.base);
            self.draw_objects(&mut below, pattern, doc, &doc.objects[..index], Transform::identity(), mode);
            let (w, h) = pattern.size();
            let mut above = Pixmap::new(w, h).expect("template size is non-zero");
            self.draw_objects(&mut above, pattern, doc, &doc.objects[index + 1..], Transform::identity(), mode);
            self.split.replace(Some(Split { key, below, above }));
        }
        let split = self.split.borrow();
        let split = split.as_ref().expect("just filled");
        let mut pm = split.below.clone();
        self.draw_objects(&mut pm, pattern, doc, std::slice::from_ref(&doc.objects[index]), Transform::identity(), mode);
        pm.draw_pixmap(0, 0, split.above.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        self.blank_left_out(&mut pm, pattern, doc);
        draw_inside(&mut pm, pattern, doc, mode);
        pm
    }

    fn base(&self, pattern: &Pattern, regions: &[Region], colors: &BaseColors) -> Pixmap {
        if let Some((cached, pm)) = self.base.borrow().as_ref()
            && cached == colors
        {
            return pm.clone();
        }
        let (w, h) = pattern.size();
        let mut pm = Pixmap::new(w, h).expect("template size is non-zero");
        pm.fill(rgb(BACKGROUND, 1.0));
        for region in regions {
            let color = colors.region(region.id, region.role);
            let mask = region.clip.map(|piece| self.mask(pattern, &[piece]));
            for poly in &region.polygons {
                let Some(path) = polygon_path(poly, true) else { continue };
                let mask = mask.as_ref();
                pm.fill_path(&path, &paint(color, 1.0), FillRule::Winding, Transform::identity(), mask);
                if region.clip.is_none() {
                    // bleed past the piece edge, so texture filtering on the
                    // model never mixes in the background colour at seams
                    let bleed = Stroke { width: BLEED * 2.0, line_join: LineJoin::Round, ..Stroke::default() };
                    pm.stroke_path(&path, &paint(color, 1.0), &bleed, Transform::identity(), None);
                }
            }
        }
        self.base.replace(Some((colors.clone(), pm.clone())));
        pm
    }

    /// Clip mask of some pieces, including their bleed. Not anti-aliased: a
    /// piece can be several polygons sharing an edge (e.g. the two halves of a
    /// sleeve), and anti-aliased edges would leave a line along that seam.
    fn mask(&self, pattern: &Pattern, pieces: &[PieceId]) -> Mask {
        let mut key = pieces.to_vec();
        key.sort_by_key(|p| *p as u8);
        if let Some(m) = self.masks.borrow().get(&key) {
            return m.clone();
        }
        let (w, h) = pattern.size();
        let mut mask = Mask::new(w, h).expect("template size is non-zero");
        let mut bleed = Pixmap::new(w, h).expect("template size is non-zero");
        let stroke = Stroke { width: BLEED * 2.0, line_join: LineJoin::Round, ..Stroke::default() };
        for &id in &key {
            for poly in &pattern.piece(id).polygons {
                if let Some(path) = polygon_path(poly, true) {
                    mask.fill_path(&path, FillRule::Winding, false, Transform::identity());
                    bleed.stroke_path(&path, &paint([0, 0, 0], 1.0), &stroke, Transform::identity(), None);
                }
            }
        }
        let bleed = Mask::from_pixmap(bleed.as_ref(), tiny_skia::MaskType::Alpha);
        for (m, b) in mask.data_mut().iter_mut().zip(bleed.data()) {
            *m = (*m).max(*b);
        }
        self.masks.borrow_mut().insert(key, mask.clone());
        mask
    }

    fn draw_objects(&self, pm: &mut Pixmap, pattern: &Pattern, doc: &Document, objs: &[Object], parent: Transform, mode: Mode) {
        for obj in objs {
            self.draw_object(pm, pattern, doc, obj, parent, 1.0, mode);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_object(
        &self,
        pm: &mut Pixmap,
        pattern: &Pattern,
        doc: &Document,
        obj: &Object,
        parent: Transform,
        parent_opacity: f32,
        mode: Mode,
    ) {
        if !obj.visible || (mode == Mode::Print && matches!(obj.kind, ObjectKind::Placeholder { .. })) {
            return;
        }
        let m = parent.pre_concat(obj.xf.matrix());
        let opacity = (obj.opacity * parent_opacity).clamp(0.0, 1.0);
        let mask = (!obj.clip.is_empty()).then(|| self.mask(pattern, &obj.clip));
        let mask = mask.as_ref();
        match &obj.kind {
            ObjectKind::Group { children } => {
                if mask.is_none() {
                    for child in children {
                        self.draw_object(pm, pattern, doc, child, m, opacity, mode);
                    }
                } else {
                    // a clipped group: draw into a layer, then clip that
                    let mut layer = Pixmap::new(pm.width(), pm.height()).expect("same size");
                    for child in children {
                        self.draw_object(&mut layer, pattern, doc, child, m, opacity, mode);
                    }
                    pm.draw_pixmap(0, 0, layer.as_ref(), &PixmapPaint::default(), Transform::identity(), mask);
                }
            }
            ObjectKind::Text { text, size, color, fatten, italic, max_width } => {
                let Some(path) = self.fonts.text_path(text, *size) else { return };
                // condense text that is wider than its maximum width
                let (w, _) = self.fonts.text_size(text, *size);
                let squeeze = max_width.filter(|&mw| w > mw && w > 0.0).map_or(1.0, |mw| mw / w);
                let t = m.pre_concat(Transform::from_scale(squeeze, 1.0)).pre_concat(Transform::from_skew(-italic, 0.0));
                let p = paint(color.unwrap_or_else(|| doc.base.auto_ink()), opacity);
                pm.fill_path(&path, &p, FillRule::Winding, t, mask);
                // the bundled font is fairly light; an outline makes numbers as
                // chunky as on a real jersey
                if *fatten > 0.0 {
                    let stroke = Stroke { width: size * fatten, line_join: LineJoin::Round, ..Stroke::default() };
                    pm.stroke_path(&path, &p, &stroke, t, mask);
                }
            }
            ObjectKind::Graphic { asset, size, tint } => {
                // render SVGs at the size they end up on the template
                let scale = (m.sx * m.sy - m.kx * m.ky).abs().sqrt();
                let (w, h) = ((size[0] * scale).ceil() as u32, (size[1] * scale).ceil() as u32);
                let Some(src) = asset.raster(w, h, *tint) else { return };
                let t = m.pre_concat(
                    Transform::from_translate(-size[0] / 2.0, -size[1] / 2.0)
                        .pre_scale(size[0] / src.width() as f32, size[1] / src.height() as f32),
                );
                let paint = PixmapPaint { opacity, quality: tiny_skia::FilterQuality::Bicubic, ..PixmapPaint::default() };
                pm.draw_pixmap(0, 0, src.as_ref().as_ref(), &paint, t, mask);
            }
            ObjectKind::Shape { points, closed, fill, stroke } => {
                if let [only] = points.as_slice() {
                    // a click without dragging leaves a dot
                    if let (Some(s), Some(path)) = (stroke, PathBuilder::from_circle(only[0], only[1], 0.5)) {
                        let st = Stroke { width: s.width, ..Stroke::default() };
                        pm.stroke_path(&path, &paint(s.color, opacity), &st, m, mask);
                    }
                    return;
                }
                let Some(path) = polygon_path(points, *closed) else { return };
                if let Some(f) = fill {
                    pm.fill_path(&path, &paint(*f, opacity), FillRule::Winding, m, mask);
                }
                if let Some(s) = stroke {
                    let st = Stroke { width: s.width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
                    pm.stroke_path(&path, &paint(s.color, opacity), &st, m, mask);
                }
            }
            ObjectKind::Pattern { text, size, line_spacing, angle, italic, color, area } => {
                let mut area_mask = None;
                if !area.is_empty() {
                    let mut am = Mask::new(pm.width(), pm.height()).expect("same size");
                    for poly in area {
                        if let Some(path) = polygon_path(poly, true) {
                            am.fill_path(&path, FillRule::Winding, true, m);
                        }
                    }
                    if let Some(clip) = mask {
                        for (a, b) in am.data_mut().iter_mut().zip(clip.data()) {
                            *a = ((*a as u16 * *b as u16) / 255) as u8;
                        }
                    }
                    area_mask = Some(am);
                }
                let mask = area_mask.as_ref().or(mask);
                // world bounds to cover with rows
                let world_pts: Vec<Pt> = if area.is_empty() {
                    obj.clip.iter().flat_map(|&id| pattern.piece(id).polygons.iter().flatten().copied()).collect()
                } else {
                    area.iter().flatten().map(|&p| crate::document::map(&m, p)).collect()
                };
                let Some(bounds) = geom::points_bounds(world_pts.into_iter()) else { return };
                let spec = PatternSpec { text, size: *size, line_spacing: *line_spacing, angle: *angle + obj.xf.rotation, italic: *italic };
                self.draw_pattern(pm, &spec, bounds, *color, opacity, mask);
            }
            ObjectKind::Placeholder { label, size } => {
                if mode == Mode::Print {
                    return;
                }
                let rect = tiny_skia::Rect::from_xywh(-size[0] / 2.0, -size[1] / 2.0, size[0], size[1]);
                let Some(rect) = rect else { return };
                let path = PathBuilder::from_rect(rect);
                pm.fill_path(&path, &paint([0x80, 0x80, 0x88], 0.35 * opacity), FillRule::Winding, m, mask);
                let unit = size[0].min(size[1]) / 10.0;
                let st = Stroke {
                    width: (unit * 0.35).max(1.5),
                    dash: StrokeDash::new(vec![unit, unit * 0.7], 0.0),
                    ..Stroke::default()
                };
                pm.stroke_path(&path, &paint([0xf0, 0xf0, 0xf0], 0.9 * opacity), &st, m, mask);
                let (w, h) = self.fonts.text_size(label, 1.0);
                let font = (size[1] * 0.45).min(size[0] * 0.8 / w.max(0.01)).min(size[1] * 0.8 / h.max(0.01));
                if let Some(path) = self.fonts.text_path(label, font) {
                    pm.fill_path(&path, &paint([0xf0, 0xf0, 0xf0], 0.9 * opacity), FillRule::Winding, m, mask);
                }
            }
        }
    }

    /// Text repeated in staggered rows across `bounds` (template space).
    fn draw_pattern(&self, pm: &mut Pixmap, spec: &PatternSpec<'_>, bounds: [f32; 4], color: Rgb, opacity: f32, mask: Option<&Mask>) {
        let [x0, y0, x1, y1] = bounds;
        if spec.text.is_empty() || spec.line_spacing <= 0.0 || spec.size <= 0.0 {
            return;
        }
        let centre = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        let radius = (x1 - x0).hypot(y1 - y0) / 2.0;

        // one long row, built once and placed for every line
        let word = format!("{}   ", spec.text);
        let (word_width, _) = self.fonts.text_size(&word, spec.size);
        let repeats = (2.0 * radius / word_width).ceil() as usize + 2;
        let Some(row) = self.fonts.text_path(&word.repeat(repeats), spec.size) else { return };

        let p = paint(color, opacity);
        let rows = (radius / spec.line_spacing).ceil() as i32 + 1;
        for k in -rows..=rows {
            let shift = if k.rem_euclid(2) == 1 { word_width / 2.0 } else { 0.0 };
            let transform = Transform::from_skew(-spec.italic, 0.0)
                .post_translate(shift, k as f32 * spec.line_spacing)
                .post_rotate(spec.angle.to_degrees())
                .post_translate(centre[0], centre[1]);
            pm.fill_path(&row, &p, FillRule::Winding, transform, mask);
        }
    }

    /// Outlines of the selected objects, for the 3D view.
    pub fn draw_selection(&self, pm: &mut Pixmap, doc: &Document, ids: &[ObjectId]) {
        let st = Stroke { width: 4.0, line_join: LineJoin::Round, ..Stroke::default() };
        for &id in ids {
            let Some(q) = geom::world_quad(&self.fonts, doc, id) else { continue };
            if let Some(path) = polygon_path(&q, true) {
                pm.stroke_path(&path, &paint(SELECTION, 1.0), &st, Transform::identity(), None);
            }
        }
    }

    /// Colour of the template at `p` (for the eyedropper).
    pub fn pick_color(pm: &Pixmap, [x, y]: Pt) -> Option<Rgb> {
        let c = pm.pixel(x as u32, y as u32)?.demultiply();
        Some([c.red(), c.green(), c.blue()])
    }
}

struct PatternSpec<'a> {
    text: &'a str,
    size: f32,
    line_spacing: f32,
    angle: f32,
    italic: f32,
}

fn rgb([r, g, b]: Rgb, a: f32) -> Color {
    Color::from_rgba8(r, g, b, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn paint(color: Rgb, opacity: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(rgb(color, opacity));
    p.anti_alias = true;
    p
}

/// The plain square the model's inside is mapped to (not on the print file).
fn draw_inside(pm: &mut Pixmap, pattern: &Pattern, doc: &Document, mode: Mode) {
    if mode == Mode::Print {
        return;
    }
    let [x, y, w, h] = pattern.inside;
    if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
        pm.fill_rect(rect, &paint(doc.inside_color(), 1.0), Transform::identity(), None);
    }
}

pub fn polygon_path(pts: &[Pt], closed: bool) -> Option<tiny_skia::Path> {
    let (first, rest) = pts.split_first()?;
    let mut pb = PathBuilder::new();
    pb.move_to(first[0], first[1]);
    for p in rest {
        pb.line_to(p[0], p[1]);
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{BaseColors, Xf};
    use crate::template::pattern::Panel;
    use crate::template::regions::regions;

    fn pixel(pm: &Pixmap, [x, y]: Pt) -> [u8; 3] {
        let c = pm.pixel(x as u32, y as u32).unwrap();
        [c.red(), c.green(), c.blue()]
    }

    #[test]
    fn objects_render_over_the_base_and_placeholders_stay_out_of_print() {
        let p = Pattern::load();
        let r = Rasterizer::new();
        let mut doc = Document::new(BaseColors { primary: [255; 3], accent: [1, 2, 3], trim: [9; 3], overrides: vec![] });
        let at = p.place(Panel::Front, [0.0, 1.1]);
        doc.add(None, Object::shape(
            "box",
            vec![[at[0] - 20.0, at[1] - 20.0], [at[0] + 20.0, at[1] - 20.0], [at[0] + 20.0, at[1] + 20.0], [at[0] - 20.0, at[1] + 20.0]],
            true,
            Some([200, 10, 10]),
            None,
        ));
        let below = p.place(Panel::Front, [0.0, 1.0]);
        doc.add(None, Object::new("A", Xf::at(below), ObjectKind::Placeholder { label: "A".into(), size: [100.0, 40.0] }));
        let regions = regions(&p);
        let print = r.render(&p, &regions, &doc, Mode::Print);
        assert_eq!(pixel(&print, at), [200, 10, 10]);
        assert_eq!(pixel(&print, [below[0] - 45.0, below[1]]), [255; 3]);
        let display = r.render(&p, &regions, &doc, Mode::Display);
        assert_ne!(pixel(&display, [below[0] - 45.0, below[1]]), [255; 3]);
        // the split frame matches a full render (up to rounding where
        // translucent layers are composited in a different order)
        let id = doc.objects[0].id;
        let split = r.frame(&p, &regions, &doc, Mode::Display, Some((id, 1)));
        let off = split.data().iter().zip(display.data()).filter(|(a, b)| a.abs_diff(**b) > 2).count();
        assert_eq!(off, 0);
    }
}
