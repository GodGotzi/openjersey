//! OpenJersey: volleyball jersey designer. Object tree and properties on the
//! left, the 3D model (Myth engine) in the middle, the print template editor
//! on the right; objects can be picked and moved in both views. AI
//! assistants (Codex, Claude Code, …) edit the open design through MCP
//! (`openjersey mcp`).

mod ai;
mod document;
mod editor;
mod garments;
mod i18n;
mod pick3d;
mod presets;
mod render;
mod scene;
mod shapes;
mod splash;
mod template;
mod ui;
mod workspace;

use std::any::Any;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use myth::egui::egui::{self, Color32, ColorImage, Key, Pos2, TextureOptions};
use myth::egui::{UiPass, UiPassNode};
use myth::math::Mat4;
use myth::prelude::*;
use winit::event::WindowEvent;

use ai::server::Request;
use document::{BaseColors, Document, Object, io};
use editor::{CanvasView, Editor, Tool};
use garments::Kind;
use i18n::tr;
use pick3d::GarmentMesh;
use render::Mode;
use scene::Studio;
use template::pattern::Pattern;
use ui::browser::GarmentBrowser;
use ui::layers::LayersPanel;
use workspace::Workspace;

/// How close (in mm) a dragged object must come to a line to snap to it in the 3D view.
const SNAP_3D_MM: f32 = 6.0;

/// Seconds the pointer rests on a control before its tooltip shows (egui's default is 0.5).
const TOOLTIP_DELAY_SECS: f32 = 0.25;

struct Fps {
    frames: u32,
    since: Instant,
    fps: f32,
}

impl Fps {
    fn tick(&mut self) {
        self.frames += 1;
        let elapsed = self.since.elapsed().as_secs_f32();
        if elapsed >= 0.5 {
            self.fps = self.frames as f32 / elapsed;
            if std::env::var_os("OJ_LOG_FPS").is_some() {
                eprintln!("fps {:.1}", self.fps);
            }
            self.frames = 0;
            self.since = Instant::now();
        }
    }
}

/// The loading window, until the 3D view is up.
static SPLASH: Mutex<Option<splash::Splash>> = Mutex::new(None);

fn splash_status(text: &str) {
    if let Some(s) = SPLASH.lock().ok().as_mut().and_then(|s| s.as_mut()) {
        s.status(text);
    }
}

fn close_splash() {
    if let Some(s) = SPLASH.lock().ok().and_then(|mut s| s.take()) {
        std::thread::spawn(move || s.finish());
    }
}

struct JerseyApp {
    ui_pass: UiPass,
    studio: Studio,
    ws: Workspace,
    editor: Editor,
    layers: LayersPanel,
    browser: GarmentBrowser,
    mesh: GarmentMesh,
    /// ids of the top and bottom the 3D view and `mesh` show
    shown: [&'static str; 2],
    /// The print sheet's part for the top and for the bottom, on the models.
    textures: [TextureHandle; 2],
    /// The same pixels for the 2D view. egui wants gamma-space colours, but
    /// the model's texture is sRGB-decoded on sampling, which looks too dark.
    preview: egui::TextureHandle,
    last_render: Option<tiny_skia::Pixmap>,
    /// tool calls from the MCP bridge
    requests: Receiver<Request>,
    clipboard: Vec<Object>,
    fps: Fps,
    vsync: bool,
    /// Where the 3D pane's centre is, in window NDC (lens shift).
    pane_centre_ndc: f32,
    inv_view_proj: Mat4,
    dragging_3d: bool,
    menu_bottom: f32,
    /// Frames drawn with the model; the first ones are slow (GPU setup).
    model_frames: u32,
}

fn winit_window(window: &dyn Window) -> &winit::window::Window {
    window.as_any().downcast_ref::<winit::window::Window>().expect("winit backend")
}

fn empty_document(pattern: &Pattern) -> Document {
    Document::for_sheet(pattern, BaseColors { primary: [255; 3], accent: [0x1f, 0x5f, 0xd1], trim: [0x2b, 0x2b, 0x2e], overrides: vec![] })
}

impl AppHandler for JerseyApp {
    fn init(engine: &mut Engine, window: &dyn Window) -> Self {
        splash_status(tr("status.loading_design"));
        let wgpu_ctx = engine.renderer.wgpu_ctx().expect("renderer initialised");
        let ui_pass = UiPass::new(&wgpu_ctx.device, wgpu_ctx.surface_view_format, winit_window(window));
        ui_pass.context().global_style_mut(|style| style.interaction.tooltip_delay = TOOLTIP_DELAY_SECS);
        let mut ws = Workspace::new(initial_document());
        let pm = ws.raster.render(&ws.pattern, &ws.regions, &ws.doc, Mode::Display);
        ws.dirty = false;
        let textures = part_textures(engine, &ws.pattern, &pm);
        let preview = ui_pass.context().load_texture("print-preview", color_image(&pm), TextureOptions::LINEAR);
        let requests = ai::server::start();
        let mesh = GarmentMesh::load(&ws.pattern);
        let studio = Studio::new(engine, &ws.pattern);
        let shown = ws.pattern.parts.clone().map(|p| p.garment.id);
        splash_status(tr("status.preparing_3d"));
        Self {
            ui_pass,
            studio,
            ws,
            editor: Editor::new(),
            layers: LayersPanel::new(),
            browser: GarmentBrowser::new(),
            mesh,
            shown,
            textures,
            preview,
            last_render: Some(pm),
            requests,
            clipboard: Vec::new(),
            fps: Fps { frames: 0, since: Instant::now(), fps: 0.0 },
            vsync: engine.renderer.settings().vsync,
            pane_centre_ndc: 0.0,
            inv_view_proj: Mat4::IDENTITY,
            dragging_3d: false,
            menu_bottom: 0.0,
            model_frames: 0,
        }
    }

    fn on_event(&mut self, _engine: &mut Engine, window: &dyn Window, event: &dyn Any) -> bool {
        let Some(event) = event.downcast_ref::<WindowEvent>() else { return false };
        match event {
            WindowEvent::Resized(size) => self.ui_pass.resize(size.width, size.height, window.scale_factor()),
            WindowEvent::CloseRequested => {
                close_splash();
                ai::server::stop();
            }
            _ => {}
        }
        // panels get their events; only the rest reaches the orbit camera
        self.ui_pass.handle_input(winit_window(window), event)
    }

    fn update(&mut self, engine: &mut Engine, window: &dyn Window, frame: &FrameState) {
        self.fps.tick();
        self.studio.try_instantiate(engine, self.textures);
        self.studio.show_garments(engine, &self.ws.doc.garments);

        // tool calls from AI assistants
        while let Ok(req) = self.requests.try_recv() {
            let result = ai::commands::execute(&mut self.ws, &self.mesh, &req.tool, &req.args);
            let _ = req.reply.send(result);
            self.sync_garments(engine);
        }

        self.ui_pass.begin_frame(winit_window(window));
        let ctx = self.ui_pass.context().clone();
        self.shortcuts(&ctx);
        self.editor.shortcuts(&ctx);
        self.top_bar(&ctx, engine);
        let screen = ctx.content_rect();

        #[allow(deprecated)] // the top-level `show` is what fits UiPass's begin/end frame
        let left = egui::Panel::left("layers").resizable(true).default_size(300.0).size_range(220.0..=520.0).show(&ctx, |ui| {
            self.browser.sidebar(ui, &mut self.ws);
            ui.separator();
            self.layers.ui(ui, &mut self.ws);
            ui.separator();
            ui::properties::ui(ui, &mut self.ws);
        });
        #[allow(deprecated)]
        let right = egui::Panel::right("editor")
            .resizable(true)
            .default_size(760.0)
            .size_range(360.0..=(screen.width() - 600.0).max(400.0))
            .show(&ctx, |ui| self.editor_panel(ui));
        // the 3D view is what the panels leave free
        let pane = egui::Rect::from_min_max(
            egui::pos2(left.response.rect.right(), self.menu_bottom),
            egui::pos2(right.response.rect.left(), screen.max.y),
        );
        self.pane_centre_ndc = 2.0 * (pane.center().x - screen.min.x) / screen.width() - 1.0;
        self.browser.show(&ctx, &mut self.ws);
        ui::viewport::toolbar(&ctx, pane, &mut self.ws, &mut self.studio);

        self.interact_3d(&ctx, pane, screen);
        if !self.dragging_3d {
            self.studio.update_camera(engine, frame.dt);
        }
        if !self.ws.status.is_empty() {
            egui::Area::new(egui::Id::new("status")).fixed_pos(pane.left_bottom() + egui::vec2(10.0, -28.0)).show(&ctx, |ui| {
                ui.label(egui::RichText::new(&self.ws.status).color(Color32::from_gray(80)));
            });
        }
        self.ui_pass.end_frame(winit_window(window));
        self.sync_garments(engine);
        self.upload(engine);
    }

    fn render(&mut self, engine: &mut Engine, _window: &dyn Window) {
        use myth::renderer::graph::core::{GraphBlackboard, HookStage};
        let Some(composer) = scene::compose_frame_shifted(engine, self.pane_centre_ndc, &mut self.inv_view_proj) else { return };
        self.ui_pass.resolve_textures(composer.device(), composer.resource_manager());
        let ui_pass = &mut self.ui_pass;
        composer
            .add_custom_pass(HookStage::AfterPostProcess, move |rdg, bb| {
                let surface = rdg.add_pass("UI_Pass", |builder| {
                    let out = builder.mutate_texture(bb.surface_out, "Surface_With_UI");
                    (UiPassNode { pass: ui_pass, target_tex: out }, out)
                });
                GraphBlackboard { surface_out: surface, ..bb }
            })
            .render();
        if self.studio.loaded() {
            self.model_frames += 1;
            if self.model_frames == 3 {
                close_splash();
            }
        }
    }
}

impl JerseyApp {
    /// After the design switched garments (menu, AI, undo, opening a
    /// file): loads the new models, and print textures of the new size.
    fn sync_garments(&mut self, engine: &mut Engine) {
        let now = self.ws.pattern.parts.clone().map(|p| p.garment.id);
        if now == self.shown {
            return;
        }
        let pm = self.ws.raster.render(&self.ws.pattern, &self.ws.regions, &self.ws.doc, Mode::Display);
        self.textures = part_textures(engine, &self.ws.pattern, &pm);
        self.mesh = GarmentMesh::load(&self.ws.pattern);
        self.studio.set_garments(engine, &self.ws.pattern);
        self.shown = now;
        self.ws.dirty = true;
    }

    /// Re-renders the template when the design changed; the 3D texture also
    /// gets the selection outline.
    fn upload(&mut self, engine: &mut Engine) {
        if !std::mem::take(&mut self.ws.dirty) {
            return;
        }
        let ws = &self.ws;
        let pm = ws.raster.frame(&ws.pattern, &ws.regions, &ws.doc, Mode::Display, ws.moving);
        let mut on_model = pm.clone();
        ws.raster.draw_selection(&mut on_model, &ws.doc, &ws.selection);
        for (part, &texture) in ws.pattern.parts.iter().zip(&self.textures) {
            let Some(cut) = part_pixels(&on_model, part.rect) else { continue };
            if let Err(err) = engine.assets.update_dynamic_texture(texture, cut.data()) {
                eprintln!("print texture upload failed: {err}");
            }
        }
        self.preview.set(color_image(&pm), TextureOptions::LINEAR);
        self.last_render = Some(pm);
    }

    /// Picking and dragging objects directly on the model.
    fn interact_3d(&mut self, ctx: &egui::Context, pane: egui::Rect, screen: egui::Rect) {
        let (pos, pressed, down, released) =
            ctx.input(|i| (i.pointer.hover_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released()));
        let over_ui = ctx.is_pointer_over_egui();
        let hit = |this: &Self, p: Pos2| {
            let ndc = [2.0 * (p.x - screen.min.x) / screen.width() - 1.0, 1.0 - 2.0 * (p.y - screen.min.y) / screen.height()];
            let (origin, dir) = GarmentMesh::ray(&this.inv_view_proj, ndc);
            this.mesh.pick(origin, dir, &this.ws.doc.garments).map(|(_, at)| at)
        };
        if self.dragging_3d {
            if down && let Some(at) = pos.and_then(|p| hit(self, p)) {
                self.ws.drag_move(at, SNAP_3D_MM * self.ws.px_per_mm());
            }
            if released || !down {
                self.ws.end_drag();
                self.dragging_3d = false;
            }
            return;
        }
        let Some(p) = pos.filter(|p| pane.contains(*p) && !over_ui) else { return };
        let Some(at) = hit(self, p) else { return };
        let picked = self.ws.pick(at, 3.0, ctx.input(|i| i.modifiers.command));
        if pressed && self.editor.tool == Tool::Select {
            match picked {
                Some(id) => {
                    if !self.ws.selection.contains(&id) {
                        self.ws.selection = vec![id];
                        self.ws.dirty = true;
                    }
                    self.ws.begin_move(at);
                    self.dragging_3d = true;
                }
                None => {
                    if !self.ws.selection.is_empty() {
                        self.ws.selection.clear();
                        self.ws.dirty = true;
                    }
                }
            }
        }
        if let Some(id) = picked {
            let name = self.ws.object_name(id);
            egui::Area::new(egui::Id::new("pick3d")).order(egui::Order::Tooltip).fixed_pos(p + egui::vec2(14.0, 14.0)).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(name));
            });
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let before = self.ws.selection.clone();
        if cmd && pressed(Key::Z) && !shift {
            self.ws.undo();
        } else if cmd && (pressed(Key::Y) || (shift && pressed(Key::Z))) {
            self.ws.redo();
        } else if cmd && pressed(Key::D) {
            self.ws.duplicate_selection();
        } else if cmd && pressed(Key::G) && shift {
            self.ws.ungroup_selection();
        } else if cmd && pressed(Key::G) {
            self.ws.group_selection();
        } else if cmd && pressed(Key::A) {
            self.ws.selection = self.ws.doc.objects.iter().map(|o| o.id).collect();
        } else if cmd && pressed(Key::C) {
            self.clipboard = self.ws.selection.iter().filter_map(|&id| self.ws.doc.find(id).cloned()).collect();
        } else if cmd && pressed(Key::V) && !self.clipboard.is_empty() {
            self.ws.checkpoint();
            let offset = 5.0 * self.ws.px_per_mm();
            let mut new = Vec::new();
            for mut obj in self.clipboard.clone() {
                obj.xf.pos = [obj.xf.pos[0] + offset, obj.xf.pos[1] + offset];
                new.push(self.ws.doc.add(None, obj));
            }
            self.ws.selection = new;
            self.ws.changed();
        } else if cmd && pressed(Key::S) {
            self.save(false);
        } else if cmd && pressed(Key::O) {
            self.open();
        } else if pressed(Key::Delete) || pressed(Key::Backspace) {
            self.ws.delete_selection();
        } else if pressed(Key::Escape) {
            self.ws.selection.clear();
        }
        if self.ws.selection != before {
            self.ws.dirty = true; // the selection outline is drawn on the model
        }
    }

    fn top_bar(&mut self, ctx: &egui::Context, engine: &mut Engine) {
        #[allow(deprecated)]
        let bar = egui::Panel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(tr("menu.file.title"), |ui| {
                    if ui.button(tr("menu.file.new")).clicked() {
                        self.ws.replace(empty_document(&self.ws.pattern));
                        self.ws.file = None;
                        ui.close();
                    }
                    ui.menu_button(tr("menu.file.template"), |ui| {
                        for (name, build) in presets::PRESETS {
                            if ui.button(name).clicked() {
                                let doc = build(&self.ws.pattern);
                                self.ws.replace(doc);
                                self.ws.file = None;
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if ui.button(tr("menu.file.open")).clicked() {
                        self.open();
                        ui.close();
                    }
                    if ui.button(tr("menu.file.save")).clicked() {
                        self.save(false);
                        ui.close();
                    }
                    if ui.button(tr("menu.file.save_as")).clicked() {
                        self.save(true);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(tr("menu.file.export")).clicked() {
                        self.export();
                        ui.close();
                    }
                });
                ui.menu_button(tr("menu.edit.title"), |ui| {
                    if ui.button(tr("menu.edit.undo")).clicked() {
                        self.ws.undo();
                        ui.close();
                    }
                    if ui.button(tr("menu.edit.redo")).clicked() {
                        self.ws.redo();
                        ui.close();
                    }
                    ui.separator();
                    ui::layers::context_menu(ui, &mut self.ws);
                });
                ui.menu_button(tr("menu.settings.title"), |ui| {
                    ui.weak(tr("menu.settings.language"));
                    for lang in i18n::Language::ALL {
                        if ui.radio(i18n::current() == lang, lang.name()).clicked() {
                            i18n::set(lang);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.checkbox(&mut self.vsync, "VSync").changed() {
                        let settings = RendererSettings { vsync: self.vsync, ..engine.renderer.settings().clone() };
                        engine.renderer.update_settings(settings);
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let fps = self.fps.fps.max(0.001);
                    ui.monospace(format!("{fps:>4.0} fps"));
                    if let Some(f) = &self.ws.file {
                        ui.weak(f.file_name().and_then(|n| n.to_str()).unwrap_or_default());
                    }
                });
            });
        });
        self.menu_bottom = bar.response.rect.bottom();
    }

    fn editor_panel(&mut self, ui: &mut egui::Ui) {
        let view = CanvasView { texture: self.preview.id(), pixels: self.last_render.as_ref() };
        let sel_before = self.ws.selection.clone();
        ui::canvas_panel::show(ui, &mut self.editor, &mut self.ws, view);
        if self.ws.selection != sel_before {
            self.ws.dirty = true;
        }
    }

    fn save(&mut self, choose: bool) {
        let path = match (&self.ws.file, choose) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new().add_filter(tr("dialog.filter.design"), &["json"]).set_file_name("jersey.json").save_file(),
        };
        let Some(path) = path else { return };
        self.ws.status = match io::to_json(&self.ws.doc).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => {
                self.ws.file = Some(path.clone());
                format!("{}: {}", tr("status.saved"), path.display())
            }
            Err(e) => format!("{}: {e}", tr("status.save_failed")),
        };
    }

    fn open(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter(tr("dialog.filter.design"), &["json"]).pick_file() else {
            return;
        };
        match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| io::from_json(&b)) {
            Ok(doc) => {
                self.ws.replace(doc);
                self.ws.status = format!("{}: {}", tr("status.opened"), path.display());
                self.ws.file = Some(path);
            }
            Err(e) => self.ws.status = format!("{}: {e}", tr("status.open_failed")),
        }
    }

    fn export(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("print-template.png").save_file() else {
            return;
        };
        let pm = self.ws.raster.render(&self.ws.pattern, &self.ws.regions, &self.ws.doc, Mode::Print);
        self.ws.status = match pm.save_png(&path) {
            Ok(()) => format!("{}: {}", tr("status.exported"), path.display()),
            Err(e) => format!("{}: {e}", tr("status.export_failed")),
        };
    }
}

/// One garment's part of the sheet.
fn part_pixels(sheet: &tiny_skia::Pixmap, [x, y, w, h]: [f32; 4]) -> Option<tiny_skia::Pixmap> {
    sheet.clone_rect(tiny_skia::IntRect::from_xywh(x as i32, y as i32, w as u32, h as u32)?)
}

/// A texture per garment (top, bottom) holding its part of the sheet `pm`.
fn part_textures(engine: &mut Engine, pattern: &Pattern, pm: &tiny_skia::Pixmap) -> [TextureHandle; 2] {
    pattern.parts.clone().map(|part| {
        let cut = part_pixels(pm, part.rect).expect("the part lies on the sheet");
        engine
            .assets
            .create_dynamic_texture(part.garment.id, cut.width(), cut.height(), cut.data().to_vec(), ColorSpace::Srgb, true)
            .expect("print texture")
    })
}

fn color_image(pm: &tiny_skia::Pixmap) -> ColorImage {
    // the template is opaque, so premultiplied and straight alpha agree
    ColorImage::from_rgba_unmultiplied([pm.width() as usize, pm.height() as usize], pm.data())
}

/// `--preset <name>` or a design file given on the command line, on the
/// garments given with `--top <name>` / `--bottom <name>` (a design file
/// moves onto them).
fn initial_document() -> Document {
    let args: Vec<String> = std::env::args().collect();
    let arg = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1));
    let top = arg("--top").and_then(|q| garments::find(q)).filter(|g| g.kind == Kind::Top);
    let bottom = arg("--bottom").and_then(|q| garments::find(q)).filter(|g| g.kind == Kind::Bottom);
    let mut doc = if let Some(doc) = arg("--preset").and_then(|n| presets::by_name(&Pattern::load(), n)) {
        doc
    } else if let Some(path) = args.iter().skip(1).find(|a| a.ends_with(".json"))
        && let Ok(doc) = std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| io::from_json(&b))
    {
        doc
    } else {
        presets::example(&Pattern::load())
    };
    if top.is_some() || bottom.is_some() {
        let from = doc.sheet();
        let body = top.map_or(from.top().body, |t| t.body);
        let pick = |g: Option<&'static garments::Garment>, current: &'static garments::Garment| {
            let g = g.unwrap_or(current);
            g.for_body(body).unwrap_or_else(|| garments::default(g.kind, body))
        };
        let to = Pattern::of(pick(top, from.top()), pick(bottom, from.bottom()));
        doc.move_to_sheet(&from, &to);
    }
    doc
}

fn main() -> myth::Result<()> {
    i18n::init();
    let args: Vec<String> = std::env::args().collect();
    // `openjersey mcp`: stdio MCP server for Codex / Claude Code
    if args.get(1).map(String::as_str) == Some("mcp") {
        ai::mcp::run();
        return Ok(());
    }
    // `openjersey --export-template out.png [design.json | --preset <name>]
    // [--top <name>] [--bottom <name>] [--garment out3d.png]` renders without opening a window
    if let Some(i) = args.iter().position(|a| a == "--export-template") {
        let out = args.get(i + 1).expect("--export-template needs an output path");
        let ws = Workspace::new(initial_document());
        let pm = ws.raster.render(&ws.pattern, &ws.regions, &ws.doc, Mode::Print);
        pm.save_png(out).expect("writing the PNG");
        if let Some(j) = args.iter().position(|a| a == "--garment") {
            let display = ws.raster.render(&ws.pattern, &ws.regions, &ws.doc, Mode::Display);
            let mesh = GarmentMesh::load(&ws.pattern);
            mesh.preview(&display, std::env::var("OJ_PREVIEW_HEIGHT").ok().and_then(|h| h.parse().ok()).unwrap_or(900), &ws.doc.garments)
                .save_png(&args[j + 1])
                .expect("writing the PNG");
        }
        return Ok(());
    }

    if args.get(1).map(String::as_str) == Some("--splash") {
        splash::run();
        return Ok(());
    }
    *SPLASH.lock().expect("not poisoned") = splash::Splash::spawn();

    // benchmarking knobs: OJ_VSYNC=1, OJ_PATH=basic, OJ_AA=..., OJ_SHADOWS=0, OJ_LOG_FPS=1.
    // VSync is off by default: with it on, frames that just miss the 60 Hz
    // deadline halve the rate (33 fps measured); Wayland doesn't tear anyway.
    let vsync = std::env::var("OJ_VSYNC").as_deref() == Ok("1");
    let path = if std::env::var("OJ_PATH").as_deref() == Ok("basic") { RenderPath::BasicForward } else { RenderPath::HighFidelity };
    let result = App::new()
        .with_title("OpenJersey")
        .with_inner_size(1600, 900)
        .with_settings(RendererSettings { vsync, path, anisotropy_clamp: 8, ..Default::default() })
        .run::<JerseyApp>();
    ai::server::stop();
    result
}
