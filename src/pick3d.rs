//! The garment mesh on the CPU: picking (which template pixel is under the
//! pointer in the 3D view) and a small software renderer for previews the AI
//! tools can look at. Myth has no raycasting, so this is our own; it loads the
//! same `assets/jersey.glb` the 3D view shows.

use myth::math::{Mat3, Mat4, Vec2, Vec3, Vec4};
use tiny_skia::Pixmap;

use crate::document::Garments;
use crate::template::pattern::{Garment, Pt};

pub struct GarmentMesh {
    tris: Vec<[Vec3; 3]>,
    uvs: Vec<[Vec2; 3]>,
    /// which garment each triangle belongs to
    garment: Vec<Garment>,
    /// template size in pixels, to turn UVs into template points
    atlas: [f32; 2],
}

impl GarmentMesh {
    pub fn load(atlas: (u32, u32)) -> Self {
        let (doc, buffers, _) = gltf::import_slice(include_bytes!("../assets/jersey.glb")).expect("bundled glb");
        let mut tris = Vec::new();
        let mut uvs = Vec::new();
        let mut garment = Vec::new();
        for node in doc.nodes() {
            let Some(mesh) = node.mesh() else { continue };
            let g = if node.name() == Some("shorts") { Garment::Shorts } else { Garment::Shirt };
            let m = Mat4::from_cols_array_2d(&node.transform().matrix());
            for prim in mesh.primitives() {
                let reader = prim.reader(|b| Some(&buffers[b.index()]));
                let (Some(pos), Some(tex)) = (reader.read_positions(), reader.read_tex_coords(0)) else { continue };
                let pos: Vec<Vec3> = pos.map(|p| m.transform_point3(Vec3::from(p))).collect();
                let tex: Vec<Vec2> = tex.into_f32().map(Vec2::from).collect();
                let idx: Vec<u32> = match reader.read_indices() {
                    Some(i) => i.into_u32().collect(),
                    None => (0..pos.len() as u32).collect(),
                };
                for t in idx.chunks_exact(3) {
                    let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
                    tris.push([pos[a], pos[b], pos[c]]);
                    uvs.push([tex[a], tex[b], tex[c]]);
                    garment.push(g);
                }
            }
        }
        Self { tris, uvs, garment, atlas: [atlas.0 as f32, atlas.1 as f32] }
    }

    /// Nearest hit of a ray: (distance, template point).
    pub fn pick(&self, origin: Vec3, dir: Vec3, shown: &Garments) -> Option<(f32, Pt)> {
        let mut best: Option<(f32, Pt)> = None;
        for (tri, uv) in self.shown_tris(shown) {
            let Some((t, u, v)) = intersect(origin, dir, tri) else { continue };
            if best.is_some_and(|(bt, _)| bt <= t) {
                continue;
            }
            let w = 1.0 - u - v;
            let st = uv[0] * w + uv[1] * u + uv[2] * v;
            best = Some((t, [st.x * self.atlas[0], st.y * self.atlas[1]]));
        }
        best
    }

    fn shown_tris<'a>(&'a self, shown: &'a Garments) -> impl Iterator<Item = (&'a [Vec3; 3], &'a [Vec2; 3])> {
        self.tris.iter().zip(&self.uvs).zip(&self.garment).filter(|(_, g)| shown.shows(**g)).map(|(t, _)| t)
    }

    /// A ray from window NDC through the camera (`inv_view_proj` is the
    /// inverse of the projection actually used, lens shift included).
    pub fn ray(inv_view_proj: &Mat4, ndc: [f32; 2]) -> (Vec3, Vec3) {
        // reverse-Z: the near plane is at z = 1
        let near = inv_view_proj.project_point3(Vec3::new(ndc[0], ndc[1], 1.0));
        let mid = inv_view_proj.project_point3(Vec3::new(ndc[0], ndc[1], 0.1));
        (near, (mid - near).normalize())
    }

    /// Software render of the garment with the print template, seen from the
    /// front and from the back, side by side (for AI previews).
    pub fn preview(&self, template: &Pixmap, height: u32, shown: &Garments) -> Pixmap {
        let w = height * 3 / 4;
        let mut out = Pixmap::new(w * 2, height).expect("non-zero");
        out.fill(tiny_skia::Color::from_rgba8(0xe9, 0xe9, 0xeb, 255));
        // OJ_PREVIEW_YAW turns the camera around the body, e.g. 90 for both sides
        let yaw = std::env::var("OJ_PREVIEW_YAW").ok().and_then(|y| y.parse::<f32>().ok()).unwrap_or(0.0).to_radians();
        for (k, back) in [false, true].into_iter().enumerate() {
            let eye = Mat3::from_rotation_y(yaw) * Vec3::new(0.0, 1.1, if back { -2.4 } else { 2.4 });
            let view = Mat4::look_at_rh(eye, Vec3::new(0.0, 1.02, 0.0), Vec3::Y);
            let proj = Mat4::perspective_rh(30f32.to_radians(), w as f32 / height as f32, 0.1, 10.0);
            self.raster_view(&mut out, template, proj * view, k as u32 * w, w, height, eye, shown);
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn raster_view(&self, out: &mut Pixmap, tex: &Pixmap, vp: Mat4, x0: u32, w: u32, h: u32, eye: Vec3, shown: &Garments) {
        let mut depth = vec![f32::MAX; (w * h) as usize];
        let light = Vec3::new(-0.4, 0.6, 0.7).normalize();
        let stride = out.width();
        let data = out.data_mut();
        for (tri, uv) in self.shown_tris(shown) {
            let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
            // two-sided: face the normal towards the viewer
            let n = if n.dot(eye - tri[0]) < 0.0 { -n } else { n };
            let shade = 0.55 + 0.45 * n.dot(light).max(0.0);
            let s: Vec<Vec4> = tri.iter().map(|p| vp * p.extend(1.0)).collect();
            if s.iter().any(|c| c.w <= 0.0) {
                continue;
            }
            let p: Vec<Vec3> = s
                .iter()
                .map(|c| Vec3::new((c.x / c.w * 0.5 + 0.5) * w as f32, (0.5 - c.y / c.w * 0.5) * h as f32, c.z / c.w))
                .collect();
            let (minx, maxx) = (p.iter().map(|q| q.x).fold(f32::MAX, f32::min), p.iter().map(|q| q.x).fold(f32::MIN, f32::max));
            let (miny, maxy) = (p.iter().map(|q| q.y).fold(f32::MAX, f32::min), p.iter().map(|q| q.y).fold(f32::MIN, f32::max));
            let area = edge(p[0], p[1], p[2]);
            if area.abs() < 1e-9 {
                continue;
            }
            for y in (miny.max(0.0) as u32)..=(maxy.min(h as f32 - 1.0) as u32) {
                for x in (minx.max(0.0) as u32)..=(maxx.min(w as f32 - 1.0) as u32) {
                    let q = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, 0.0);
                    let (b0, b1, b2) = (edge(p[1], p[2], q) / area, edge(p[2], p[0], q) / area, edge(p[0], p[1], q) / area);
                    if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                        continue;
                    }
                    let z = b0 * p[0].z + b1 * p[1].z + b2 * p[2].z;
                    let di = (y * w + x) as usize;
                    if z >= depth[di] {
                        continue;
                    }
                    depth[di] = z;
                    let st = uv[0] * b0 + uv[1] * b1 + uv[2] * b2;
                    let tx = (st.x * tex.width() as f32).clamp(0.0, tex.width() as f32 - 1.0) as u32;
                    let ty = (st.y * tex.height() as f32).clamp(0.0, tex.height() as f32 - 1.0) as u32;
                    let c = tex.pixel(tx, ty).expect("clamped");
                    let o = (((y * stride) + x0 + x) * 4) as usize;
                    data[o] = (c.red() as f32 * shade) as u8;
                    data[o + 1] = (c.green() as f32 * shade) as u8;
                    data[o + 2] = (c.blue() as f32 * shade) as u8;
                    data[o + 3] = 255;
                }
            }
        }
    }
}

fn edge(a: Vec3, b: Vec3, c: Vec3) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Möller–Trumbore: (t, u, v) of a ray-triangle hit.
fn intersect(origin: Vec3, dir: Vec3, tri: &[Vec3; 3]) -> Option<(f32, f32, f32)> {
    let e1 = tri[1] - tri[0];
    let e2 = tri[2] - tri[0];
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - tri[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 1e-5).then_some((t, u, v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::pattern::{Pattern, PieceId};

    #[test]
    fn a_ray_at_the_chest_lands_on_the_front_piece() {
        let pattern = Pattern::load();
        let mesh = GarmentMesh::load(pattern.size());
        let (_, at) = mesh.pick(Vec3::new(0.05, 1.2, 2.0), Vec3::new(0.0, 0.0, -1.0), &Garments::default()).expect("hits the shirt");
        assert!(pattern.piece(PieceId::Front).contains(at), "{at:?}");
        let (_, at) = mesh.pick(Vec3::new(0.05, 1.2, -2.0), Vec3::new(0.0, 0.0, 1.0), &Garments::default()).expect("hits the back");
        assert!(pattern.piece(PieceId::Back).contains(at), "{at:?}");
    }
}

