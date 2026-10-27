//! The 3D side: studio lighting, the jersey + shorts model and the camera.

use myth::prelude::*;
use myth::render::FrameComposer;
use myth::scene::{Frustum, RenderCamera};
use myth::ToneMappingMode;

use crate::document::Garments;

const MODEL_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/jersey.glb");
const ENV_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/studio.hdr");

/// Orbit target and start position, in model meters (the mannequin is
/// 1.69 m tall; jersey and shorts span about 0.56..1.47 m).
const TARGET: Vec3 = Vec3::new(0.0, 1.02, 0.0);
const START: Vec3 = Vec3::new(0.0, 1.1, 2.7);
const BACKGROUND: [f32; 3] = [0.913, 0.913, 0.922]; // #e9e9eb like the Tauri app

pub struct Studio {
    prefab: PrefabHandle,
    pub loaded: bool,
    /// the "jersey" and "shorts" mesh nodes, once instantiated
    garment_nodes: [Option<NodeHandle>; 2],
    camera: NodeHandle,
    pub controls: OrbitControls,
}

impl Studio {
    pub fn new(engine: &mut Engine) -> Self {
        let env = engine.assets.load_hdr_texture(ENV_PATH);
        let prefab = engine.assets.load_gltf(MODEL_PATH);

        let scene = engine.scene_manager.create_active();
        scene.environment.set_env_map(Some(env));
        scene.environment.set_intensity(1.0);
        // Myth prefilters the environment light once, with 4096 samples per
        // texel; at its default 512 px cube faces that keeps an integrated
        // GPU (and the whole desktop) busy for ~9 s. The background is a
        // plain colour and the fabric is matte, so small cubes look the same.
        scene.environment.set_base_cube_size(256);
        scene.environment.set_pmrem_size(128);
        let [r, g, b] = BACKGROUND;
        scene.background.set_mode(BackgroundMode::color(r, g, b));
        scene.tone_mapping.set_mode(ToneMappingMode::Neutral);

        // soft key light from the upper left front, casting shadows so the
        // sleeves and hem shade the fabric under them
        let mut key = Light::new_directional(Vec3::ONE, 2.0);
        key.cast_shadows = std::env::var("OJ_SHADOWS").as_deref() != Ok("0");
        let key = scene.add_light(key);
        if let Some(node) = scene.get_node_mut(key) {
            node.transform.position = Vec3::new(-2.5, 3.5, 4.0);
            node.transform.look_at(TARGET, Vec3::Y);
        }

        let mut camera = Camera::new_perspective(30.0, 16.0 / 9.0, 0.05);
        camera.set_aa_mode(aa_mode_from_env());
        let camera = scene.add_camera(camera);
        if let Some(node) = scene.get_node_mut(camera) {
            node.transform.position = START;
            node.transform.look_at(TARGET, Vec3::Y);
        }
        scene.active_camera = Some(camera);

        let mut controls = OrbitControls::new(START, TARGET);
        controls.min_distance = 1.2;
        controls.max_distance = 6.0;
        Self { prefab, loaded: false, garment_nodes: [None; 2], camera, controls }
    }

    /// Instantiates the model once loaded and gives jersey and shorts a
    /// fabric material carrying the print template. Returns true when it happened.
    pub fn try_instantiate(&mut self, engine: &mut Engine, print: TextureHandle) -> bool {
        if self.loaded {
            return false;
        }
        let assets = engine.assets.clone();
        let Some(scene) = engine.scene_manager.active_scene_mut() else { return false };
        let Some(prefab) = assets.prefabs.get(self.prefab) else {
            if let Some(err) = assets.prefabs.get_error(self.prefab) {
                eprintln!("failed to load {MODEL_PATH}: {err}");
                self.loaded = true;
            }
            return false;
        };
        let root = scene.instantiate(prefab.as_ref());
        scene.update_subtree(root);

        // the fabric's inside is mapped to a plain swatch on the template,
        // so one material shows both print and inside (the solidified
        // fabric has an inner surface of its own)
        let fabric = assets.materials.add(
            PhysicalMaterial::new(Vec4::ONE)
                .with_map(print)
                .with_roughness(0.95)
                .with_side(Side::Double)
                .with_sheen(Vec3::splat(0.45), 0.9),
        );
        let meshes: Vec<NodeHandle> = scene.meshes.iter().map(|(node, _)| node).collect();
        for node in meshes {
            if let Some(mesh) = scene.get_mesh_mut(node) {
                mesh.material = fabric;
                mesh.cast_shadows = true;
                mesh.receive_shadows = true;
            }
        }
        self.garment_nodes = ["jersey", "shorts"].map(|name| scene.find_node_by_name(name));
        self.loaded = true;
        true
    }

    /// Hides the garments the design leaves out.
    pub fn show_garments(&self, engine: &mut Engine, shown: &Garments) {
        let Some(scene) = engine.scene_manager.active_scene_mut() else { return };
        for (node, visible) in self.garment_nodes.iter().zip([shown.shirt, shown.shorts]) {
            if let Some(node) = node.and_then(|n| scene.get_node_mut(n)) {
                node.visible = visible;
            }
        }
    }

    pub fn update_camera(&mut self, engine: &mut Engine, dt: f32) {
        let Some(scene) = engine.scene_manager.active_scene_mut() else { return };
        if let Some(node) = scene.get_node_mut(self.camera) {
            self.controls.update(&mut node.transform, &engine.input, 30.0, dt);
        }
    }

    pub fn reset_view(&mut self, back: bool) {
        let mut pos = START;
        if back {
            pos.z = -pos.z;
        }
        self.controls.set_target(TARGET);
        self.controls.set_position(pos);
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
