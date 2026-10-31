//! Preset shapes for the shape tool and the MCP tools: each is an outline
//! fitted to a box (for a line, to its two ends), as sheet points.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use crate::i18n::tr;
use crate::template::pattern::Pt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    RoundedRect,
    Triangle,
    Diamond,
    Pentagon,
    Hexagon,
    Star,
    Arrow,
    Chevron,
    Cross,
    Line,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 12] = [
        ShapeKind::Rectangle,
        ShapeKind::Ellipse,
        ShapeKind::RoundedRect,
        ShapeKind::Triangle,
        ShapeKind::Diamond,
        ShapeKind::Pentagon,
        ShapeKind::Hexagon,
        ShapeKind::Star,
        ShapeKind::Arrow,
        ShapeKind::Chevron,
        ShapeKind::Cross,
        ShapeKind::Line,
    ];

    /// The name MCP clients use.
    pub fn id(self) -> &'static str {
        match self {
            ShapeKind::Rectangle => "rectangle",
            ShapeKind::Ellipse => "ellipse",
            ShapeKind::RoundedRect => "rounded_rect",
            ShapeKind::Triangle => "triangle",
            ShapeKind::Diamond => "diamond",
            ShapeKind::Pentagon => "pentagon",
            ShapeKind::Hexagon => "hexagon",
            ShapeKind::Star => "star",
            ShapeKind::Arrow => "arrow",
            ShapeKind::Chevron => "chevron",
            ShapeKind::Cross => "cross",
            ShapeKind::Line => "line",
        }
    }

    pub fn from_id(id: &str) -> Option<ShapeKind> {
        ShapeKind::ALL.into_iter().find(|kind| kind.id() == id)
    }

    /// The name shown in the UI.
    pub fn name(self) -> &'static str {
        match self {
            ShapeKind::Rectangle => tr("shape.rectangle"),
            ShapeKind::Ellipse => tr("shape.ellipse"),
            ShapeKind::RoundedRect => tr("shape.rounded_rect"),
            ShapeKind::Triangle => tr("shape.triangle"),
            ShapeKind::Diamond => tr("shape.diamond"),
            ShapeKind::Pentagon => tr("shape.pentagon"),
            ShapeKind::Hexagon => tr("shape.hexagon"),
            ShapeKind::Star => tr("shape.star"),
            ShapeKind::Arrow => tr("shape.arrow"),
            ShapeKind::Chevron => tr("shape.chevron"),
            ShapeKind::Cross => tr("shape.cross"),
            ShapeKind::Line => tr("shape.line"),
        }
    }

    /// A line is open: it has a stroke and no fill.
    pub fn closed(self) -> bool {
        self != ShapeKind::Line
    }

    /// The outline for the box with corners `from` and `to`; a line runs from
    /// `from` to `to`.
    pub fn outline(self, from: Pt, to: Pt) -> Vec<Pt> {
        let (x0, x1) = (from[0].min(to[0]), from[0].max(to[0]));
        let (y0, y1) = (from[1].min(to[1]), from[1].max(to[1]));
        let fit = |unit: &[Pt]| -> Vec<Pt> { unit.iter().map(|&[u, v]| [x0 + u * (x1 - x0), y0 + v * (y1 - y0)]).collect() };
        match self {
            ShapeKind::Line => vec![from, to],
            ShapeKind::Rectangle => fit(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
            ShapeKind::Ellipse => fit(&ellipse()),
            ShapeKind::RoundedRect => rounded_rect(x0, y0, x1, y1),
            ShapeKind::Triangle => fit(&[[0.5, 0.0], [1.0, 1.0], [0.0, 1.0]]),
            ShapeKind::Diamond => fit(&[[0.5, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 0.5]]),
            ShapeKind::Pentagon => fit(&regular_polygon(5)),
            ShapeKind::Hexagon => fit(&regular_polygon(6)),
            ShapeKind::Star => fit(&star(5, 0.42)),
            // pointing right: shaft, then the head
            ShapeKind::Arrow => fit(&[[0.0, 0.35], [0.6, 0.35], [0.6, 0.0], [1.0, 0.5], [0.6, 1.0], [0.6, 0.65], [0.0, 0.65]]),
            ShapeKind::Chevron => fit(&[[0.0, 0.0], [0.6, 0.0], [1.0, 0.5], [0.6, 1.0], [0.0, 1.0], [0.4, 0.5]]),
            ShapeKind::Cross => fit(&[
                [0.35, 0.0],
                [0.65, 0.0],
                [0.65, 0.35],
                [1.0, 0.35],
                [1.0, 0.65],
                [0.65, 0.65],
                [0.65, 1.0],
                [0.35, 1.0],
                [0.35, 0.65],
                [0.0, 0.65],
                [0.0, 0.35],
                [0.35, 0.35],
            ]),
        }
    }
}

/// An ellipse inscribed in the unit square.
fn ellipse() -> Vec<Pt> {
    const STEPS: usize = 96;
    (0..STEPS)
        .map(|i| {
            let angle = i as f32 / STEPS as f32 * TAU;
            [0.5 + 0.5 * angle.cos(), 0.5 + 0.5 * angle.sin()]
        })
        .collect()
}

/// Twice the signed area of a triangle (positive when it turns counter-clockwise).
fn cross(a: Pt, b: Pt, c: Pt) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Splits a simple outline into triangles by ear clipping, so any of the
/// shapes here can be filled (the icons are drawn as meshes, which have no
/// fill rule). An outline that cannot be split gives what could be.
pub fn triangulate(points: &[Pt]) -> Vec<[Pt; 3]> {
    let doubled_area: f32 = (0..points.len()).map(|i| cross([0.0, 0.0], points[i], points[(i + 1) % points.len()])).sum();
    let counter_clockwise = doubled_area > 0.0;
    let strictly_inside = |p: Pt, a: Pt, b: Pt, c: Pt| {
        let signs = [cross(a, b, p), cross(b, c, p), cross(c, a, p)];
        signs.iter().all(|&s| s > 1e-6) || signs.iter().all(|&s| s < -1e-6)
    };
    let mut remaining: Vec<usize> = (0..points.len()).collect();
    let mut triangles = Vec::new();
    while remaining.len() > 3 {
        let count = remaining.len();
        // a vertex on a straight line between its neighbours adds no corner
        let straight = (0..count).find(|&i| {
            let (a, b, c) = (remaining[(i + count - 1) % count], remaining[i], remaining[(i + 1) % count]);
            cross(points[a], points[b], points[c]).abs() <= 1e-9
        });
        if let Some(i) = straight {
            remaining.remove(i);
            continue;
        }
        let ear = (0..count).find(|&i| {
            let (a, b, c) = (remaining[(i + count - 1) % count], remaining[i], remaining[(i + 1) % count]);
            let turn = cross(points[a], points[b], points[c]);
            // a corner that turns the wrong way is a notch, not an ear
            (turn > 1e-9) == counter_clockwise
                && turn.abs() > 1e-9
                && !remaining.iter().any(|&j| ![a, b, c].contains(&j) && strictly_inside(points[j], points[a], points[b], points[c]))
        });
        let Some(i) = ear else { return triangles };
        let (a, b, c) = (remaining[(i + count - 1) % count], remaining[i], remaining[(i + 1) % count]);
        triangles.push([points[a], points[b], points[c]]);
        remaining.remove(i);
    }
    if remaining.len() == 3 {
        let last = [points[remaining[0]], points[remaining[1]], points[remaining[2]]];
        if cross(last[0], last[1], last[2]).abs() > 1e-9 {
            triangles.push(last);
        }
    }
    triangles
}

/// Scales points so their bounding box is the unit square.
fn normalised(points: Vec<Pt>) -> Vec<Pt> {
    let min = points.iter().fold([f32::MAX; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
    let max = points.iter().fold([f32::MIN; 2], |m, p| [m[0].max(p[0]), m[1].max(p[1])]);
    points.into_iter().map(|[x, y]| [(x - min[0]) / (max[0] - min[0]), (y - min[1]) / (max[1] - min[1])]).collect()
}

/// A regular polygon with a vertex at the top, filling the unit square.
fn regular_polygon(sides: usize) -> Vec<Pt> {
    normalised((0..sides).map(|i| point_on_circle(1.0, i as f32 / sides as f32)).collect())
}

/// A star with `points` tips; `inner` is the radius of the notches (0..1).
fn star(points: usize, inner: f32) -> Vec<Pt> {
    let vertices = 2 * points;
    normalised((0..vertices).map(|i| point_on_circle(if i % 2 == 0 { 1.0 } else { inner }, i as f32 / vertices as f32)).collect())
}

/// The point at `turn` (0..1) round a circle of `radius`, starting at the top.
fn point_on_circle(radius: f32, turn: f32) -> Pt {
    let angle = -FRAC_PI_2 + turn * TAU;
    [radius * angle.cos(), radius * angle.sin()]
}

fn rounded_rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Pt> {
    const STEPS: usize = 8;
    let radius = 0.22 * (x1 - x0).min(y1 - y0);
    // each corner: its arc's centre and the angle the arc starts at
    let corners = [
        (x1 - radius, y0 + radius, -FRAC_PI_2),
        (x1 - radius, y1 - radius, 0.0),
        (x0 + radius, y1 - radius, FRAC_PI_2),
        (x0 + radius, y0 + radius, PI),
    ];
    corners
        .into_iter()
        .flat_map(|(cx, cy, start)| {
            (0..=STEPS).map(move |i| {
                let angle = start + FRAC_PI_2 * i as f32 / STEPS as f32;
                [cx + radius * angle.cos(), cy + radius * angle.sin()]
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_closed_shape_fills_its_box() {
        let (from, to) = ([100.0, 50.0], [300.0, 250.0]);
        for kind in ShapeKind::ALL.into_iter().filter(|k| k.closed()) {
            let points = kind.outline(from, to);
            assert!(points.len() >= 3, "{kind:?}");
            let inside =
                points.iter().all(|p| (100.0 - 0.01..=300.0 + 0.01).contains(&p[0]) && (50.0 - 0.01..=250.0 + 0.01).contains(&p[1]));
            assert!(inside, "{kind:?} leaves its box");
            let reaches = |axis: usize, lo: f32, hi: f32| {
                points.iter().any(|p| (p[axis] - lo).abs() < 0.01) && points.iter().any(|p| (p[axis] - hi).abs() < 0.01)
            };
            assert!(reaches(0, 100.0, 300.0) && reaches(1, 50.0, 250.0), "{kind:?} does not touch all four sides of its box");
        }
    }

    #[test]
    fn shapes_have_their_vertices() {
        let count = |kind: ShapeKind| kind.outline([0.0, 0.0], [10.0, 10.0]).len();
        assert_eq!(count(ShapeKind::Rectangle), 4);
        assert_eq!(count(ShapeKind::Ellipse), 96);
        assert_eq!(count(ShapeKind::Triangle), 3);
        assert_eq!(count(ShapeKind::Diamond), 4);
        assert_eq!(count(ShapeKind::Pentagon), 5);
        assert_eq!(count(ShapeKind::Hexagon), 6);
        assert_eq!(count(ShapeKind::Star), 10);
        assert_eq!(count(ShapeKind::Arrow), 7);
        assert_eq!(count(ShapeKind::Chevron), 6);
        assert_eq!(count(ShapeKind::Cross), 12);
    }

    #[test]
    fn the_triangles_cover_every_closed_shape_exactly() {
        for kind in ShapeKind::ALL.into_iter().filter(|k| k.closed()) {
            let points = kind.outline([0.0, 0.0], [10.0, 10.0]);
            let doubled_area: f32 = (0..points.len()).map(|i| cross([0.0, 0.0], points[i], points[(i + 1) % points.len()])).sum();
            let triangles = triangulate(&points);
            assert!(!triangles.is_empty() && triangles.len() <= points.len() - 2, "{kind:?}");
            let covered: f32 = triangles.iter().map(|&[a, b, c]| cross(a, b, c)).sum();
            assert!((covered - doubled_area).abs() < 1e-2, "{kind:?}: triangles cover {covered}, the outline {doubled_area}");
            // none of them is flipped, so none overlaps another
            assert!(triangles.iter().all(|&[a, b, c]| cross(a, b, c) * doubled_area > 0.0), "{kind:?} has a flipped triangle");
        }
    }

    #[test]
    fn a_line_runs_from_one_end_to_the_other_and_is_open() {
        assert_eq!(ShapeKind::Line.outline([5.0, 9.0], [1.0, 2.0]), vec![[5.0, 9.0], [1.0, 2.0]]);
        assert!(!ShapeKind::Line.closed());
        assert!(ShapeKind::Star.closed());
    }

    #[test]
    fn ids_round_trip_and_are_unique() {
        for kind in ShapeKind::ALL {
            assert_eq!(ShapeKind::from_id(kind.id()), Some(kind));
        }
        assert_eq!(ShapeKind::from_id("blob"), None);
        let mut ids: Vec<_> = ShapeKind::ALL.iter().map(|k| k.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ShapeKind::ALL.len());
    }
}
