//! The 3D side: studio lighting, the garment models (top and bottom) and the camera.

use myth::ToneMappingMode;
use myth::prelude::*;
use myth::render::FrameComposer;
use myth::scene::{Frustum, RenderCamera, ShadowConfig};

use crate::document::Garments;
use crate::template::pattern::Pattern;

const ENV_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/studio.hdr");

/// Orbit target and start position, in model meters (the mannequin is
/// 1.69 m tall; jersey and shorts span about 0.56..1.47 m).
const TARGET: Vec3 = Vec3::new(0.0, 1.02, 0.0);
const START: Vec3 = Vec3::new(0.0, 1.1, 2.7);
/// Where the front key light is; the back one mirrors it across z = 0.
const KEY_DISTANCE: Vec3 = Vec3::new(-2.5, 3.5, 4.0);
/// Light levels: the environment only fills in; the key lights shape.
const ENV_INTENSITY: f32 = 0.28;
const KEY_INTENSITY: f32 = 2.3;

/// One garment model (top or bottom) in the scene.
struct Slot {
    prefab: PrefabHandle,
    path: &'static str,
    /// the instantiated model's nodes, root first; empty until loaded
    nodes: Vec<NodeHandle>,
    /// the print texture it wears; `nodes` are rebuilt when this changes
    texture: Option<TextureHandle>,
    failed: bool,
}

pub struct Studio {
    /// top, bottom
    slots: [Slot; 2],
    camera: NodeHandle,
    pub controls: OrbitControls,
}
impl Studio {
    pub fn new(engine: &mut Engine, pattern: &Pattern) -> Self {
        let env = engine.assets.load_hdr_texture(ENV_PATH);
        let slots = pattern.parts.clone().map(|part| Slot {
            prefab: engine.assets.load_gltf(part.garment.glb_path),
            path: part.garment.glb_path,
            nodes: Vec::new(),
            texture: None,
            failed: false,
        });

        let scene = engine.scene_manager.create_active();
        scene.environment.set_env_map(Some(env));
        // a dim environment keeps the shaded side dark enough to show the folds
        scene.environment.set_intensity(ENV_INTENSITY);
        // Myth prefilters the environment light once, with 4096 samples per
        // texel; at its default 512 px cube faces that keeps an integrated
        // GPU (and the whole desktop) busy for ~9 s. The background is a
        // plain colour and the fabric is matte, so small cubes look the same.
        scene.environment.set_base_cube_size(256);
        scene.environment.set_pmrem_size(128);
        let [r, g, b] = crate::theme::viewport_background();
        scene.background.set_mode(BackgroundMode::color(r, g, b));
        scene.tone_mapping.set_mode(ToneMappingMode::Neutral);

        // a strong key light from the upper left of the front, casting shadows
        // so the sleeves and hem shade the fabric under them, and the same
        // light mirrored to the back so both sides look alike
        for z in [KEY_DISTANCE.z, -KEY_DISTANCE.z] {
            let mut key = Light::new_directional(Vec3::ONE, KEY_INTENSITY);
            key.cast_shadows = std::env::var("OJ_SHADOWS").as_deref() != Ok("0");
            // the scene is a few metres across: a sharp, close shadow map
            key.shadow =
                Some(ShadowConfig { map_size: 4096, cascade_count: 2, max_shadow_distance: 12.0, normal_bias: 0.01, ..Default::default() });
            let key = scene.add_light(key);
            if let Some(node) = scene.get_node_mut(key) {
                node.transform.position = Vec3::new(KEY_DISTANCE.x, KEY_DISTANCE.y, z);
                node.transform.look_at(TARGET, Vec3::Y);
            }
        }

        let start = View::from_env().camera_position();
        let mut camera = Camera::new_perspective(30.0, 16.0 / 9.0, 0.05);
        camera.set_aa_mode(aa_mode_from_env());
        let camera = scene.add_camera(camera);
        if let Some(node) = scene.get_node_mut(camera) {
            node.transform.position = start;
            node.transform.look_at(TARGET, Vec3::Y);
        }
        scene.active_camera = Some(camera);

        let mut controls = OrbitControls::new(start, TARGET);
        controls.min_distance = 1.2;
        controls.max_distance = 6.0;
        Self { slots, camera, controls }
    }

    /// Changes the 3D view's background (a theme's `viewport` colour).
    pub fn set_background(&self, engine: &mut Engine, color: [f32; 3]) {
        if let Some(scene) = engine.scene_manager.active_scene_mut() {
            let [r, g, b] = color;
            scene.background.set_mode(BackgroundMode::color(r, g, b));
        }
    }

    /// Both models are in the scene.
    pub fn loaded(&self) -> bool {
        self.slots.iter().all(|s| !s.nodes.is_empty() || s.failed)
    }

    /// Swaps in the models of `pattern`'s top and bottom where they changed;
    /// `try_instantiate` puts them in once loaded.
    pub fn set_garments(&mut self, engine: &mut Engine, pattern: &Pattern) {
        for (slot, part) in self.slots.iter_mut().zip(&pattern.parts) {
            if slot.path == part.garment.glb_path {
                continue;
            }
            if let Some(scene) = engine.scene_manager.active_scene_mut()
                && let Some(&root) = slot.nodes.first()
            {
                scene.remove_node(root);
            }
            *slot = Slot {
                prefab: engine.assets.load_gltf(part.garment.glb_path),
                path: part.garment.glb_path,
                nodes: Vec::new(),
                texture: None,
                failed: false,
            };
        }
    }

    /// Instantiates each model once loaded (again, if its texture changed),
    /// with a fabric material carrying its part of the print sheet.
    pub fn try_instantiate(&mut self, engine: &mut Engine, textures: [TextureHandle; 2]) {
        let assets = engine.assets.clone();
        let Some(scene) = engine.scene_manager.active_scene_mut() else { return };
        for (slot, texture) in self.slots.iter_mut().zip(textures) {
            if slot.failed || (!slot.nodes.is_empty() && slot.texture == Some(texture)) {
                continue;
            }
            let Some(prefab) = assets.prefabs.get(slot.prefab) else {
                if let Some(err) = assets.prefabs.get_error(slot.prefab) {
                    eprintln!("failed to load {}: {err}", slot.path);
                    slot.failed = true;
                }
                continue;
            };
            if let Some(&root) = slot.nodes.first() {
                scene.remove_node(root);
            }
            let root = scene.instantiate(prefab.as_ref());
            scene.update_subtree(root);
            let mut nodes = vec![root];
            let mut i = 0;
            while i < nodes.len() {
                if let Some(node) = scene.get_node(nodes[i]) {
                    nodes.extend(node.children().iter().copied());
                }
                i += 1;
            }
            // the fabric's inside is mapped to a plain swatch on the sheet,
            // so one material shows both print and inside (the solidified
            // fabric has an inner surface of its own)
            let fabric = assets.materials.add(
                PhysicalMaterial::new(Vec4::ONE)
                    .with_map(texture)
                    .with_roughness(0.95)
                    .with_side(Side::Double)
                    .with_sheen(Vec3::splat(0.45), 0.9),
            );
            for &node in &nodes {
                if let Some(mesh) = scene.get_mesh_mut(node) {
                    mesh.material = fabric;
                    mesh.cast_shadows = true;
                    mesh.receive_shadows = true;
                }
            }
            slot.nodes = nodes;
            slot.texture = Some(texture);
        }
    }

    /// Hides the garments the design leaves out.
    pub fn show_garments(&self, engine: &mut Engine, shown: &Garments) {
        let Some(scene) = engine.scene_manager.active_scene_mut() else { return };
        for (slot, visible) in self.slots.iter().zip([shown.shirt, shown.shorts]) {
            for &node in &slot.nodes {
                if let Some(node) = scene.get_node_mut(node) {
                    node.visible = visible;
                }
            }
        }
    }

    pub fn update_camera(&mut self, engine: &mut Engine, dt: f32) {
        let Some(scene) = engine.scene_manager.active_scene_mut() else { return };
        if let Some(node) = scene.get_node_mut(self.camera) {
            self.controls.update(&mut node.transform, &engine.input, 30.0, dt);
        }
    }

    /// Moves the camera to look at the model from `view`, keeping the
    /// distance of the start position.
    pub fn set_view(&mut self, view: View) {
        self.controls.set_target(TARGET);
        self.controls.set_position(view.camera_position());
    }
}

/// A side of the model the camera can be moved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Front,
    Left,
    Back,
    Right,
}

impl View {
    /// The side to start at: `OJ_VIEW` = front | left | back | right.
    fn from_env() -> View {
        match std::env::var("OJ_VIEW").as_deref() {
            Ok("left") => View::Left,
            Ok("back") => View::Back,
            Ok("right") => View::Right,
            _ => View::Front,
        }
    }

    /// The camera position for this side: the start position turned around
    /// the model's vertical axis.
    fn camera_position(self) -> Vec3 {
        let from_front = START - TARGET;
        let (sin, cos) = self.azimuth().sin_cos();
        TARGET + Vec3::new(from_front.z * sin, from_front.y, from_front.z * cos)
    }

    /// Angle around the vertical axis, from the front, towards the wearer's left.
    fn azimuth(self) -> f32 {
        let quarter_turns = match self {
            View::Front => 0.0,
            View::Left => 1.0,
            View::Back => 2.0,
            View::Right => 3.0,
        };
        quarter_turns * std::f32::consts::FRAC_PI_2
    }
}

/// Anti-aliasing, overridable for benchmarking with `OJ_AA` = none | fxaa | msaa | msaa-fxaa.
fn aa_mode_from_env() -> AntiAliasingMode {
    match std::env::var("OJ_AA").as_deref() {
        Ok("none") => AntiAliasingMode::None,
        Ok("fxaa") => AntiAliasingMode::FXAA(FxaaSettings::default()),
        Ok("msaa") => AntiAliasingMode::MSAA(4),
        _ => AntiAliasingMode::MSAA_FXAA(4, FxaaSettings::default()),
    }
}

/// Like `Engine::compose_frame`, but shifts the projection centre to
/// `centre_ndc_x` (lens shift), so the model sits in the middle of the 3D
/// pane between the side panels instead of the middle of the window.
/// `inv_view_proj` receives the inverse of the matrix used, for picking.
pub fn compose_frame_shifted<'a>(engine: &'a mut Engine, centre_ndc_x: f32, inv_view_proj: &mut Mat4) -> Option<FrameComposer<'a>> {
    let frame_time = engine.frame_time();
    let handle = engine.scene_manager.active_handle()?;
    let scene = engine.scene_manager.get_scene_mut(handle)?;
    let camera = scene.cameras.get_mut(scene.active_camera?)?;
    let mut render_camera = camera.extract_render_camera();
    shift_projection(&mut render_camera, centre_ndc_x);
    *inv_view_proj = (render_camera.unjittered_projection * render_camera.view_matrix).inverse();
    engine.renderer.begin_frame(scene, render_camera, &engine.assets, frame_time)
}

fn shift_projection(camera: &mut RenderCamera, centre_ndc_x: f32) {
    // NDC x = clip.x / clip.w with clip.w = -z_view, so subtracting from the
    // z column moves everything by +centre_ndc_x (same trick as TAA jitter)
    camera.projection_matrix.z_axis.x -= centre_ndc_x;
    camera.unjittered_projection.z_axis.x -= centre_ndc_x;
    camera.view_projection_matrix = camera.projection_matrix * camera.view_matrix;
    camera.frustum = Frustum::from_matrix(camera.view_projection_matrix);
}
