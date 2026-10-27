//! OpenJersey: volleyball jersey designer. Object tree and properties on the
//! left, the 3D model (Myth engine) in the middle, the print template editor
//! on the right; objects can be picked and moved in both views. AI
//! assistants edit the design through the same tools: in the built-in chat
//! (Claude / OpenAI) or from Codex and Claude Code via MCP (`openjersey mcp`).

mod ai;
mod document;
mod editor;
mod pick3d;
mod presets;
mod render;
mod scene;
mod splash;
mod template;
mod ui;
mod workspace;

use std::any::Any;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

use myth::egui::egui::{self, Color32, ColorImage, Key, Pos2, TextureOptions};
use myth::egui::{UiPass, UiPassNode};
use myth::math::Mat4;
use myth::prelude::*;
use winit::event::WindowEvent;

use ai::server::Request;
use document::{io, BaseColors, Document, Object};
use editor::{CanvasView, Editor, Tool};
use pick3d::GarmentMesh;
use render::Mode;
use scene::Studio;
use template::pattern::Pattern;
use ui::chat::ChatPanel;
use ui::layers::LayersPanel;
use workspace::Workspace;

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
    chat: ChatPanel,
    mesh: GarmentMesh,
    /// The print template on the model (jersey and shorts share it).
    texture: TextureHandle,
    /// The same pixels for the 2D view. egui wants gamma-space colours, but
    /// the model's texture is sRGB-decoded on sampling, which looks too dark.
    preview: egui::TextureHandle,
    last_render: Option<tiny_skia::Pixmap>,
    /// tool calls from the MCP bridge and the chat
    requests: Receiver<Request>,
    request_tx: Sender<Request>,
    clipboard: Vec<Object>,
    fps: Fps,
    vsync: bool,
    /// Where the 3D pane's centre is, in window NDC (lens shift).
    pane_centre_ndc: f32,
    inv_view_proj: Mat4,
    dragging_3d: bool,
    menu_bottom: f32,
    file: Option<std::path::PathBuf>,
    /// Frames drawn with the model; the first ones are slow (GPU setup).
    model_frames: u32,
}

fn winit_window(window: &dyn Window) -> &winit::window::Window {
    window.as_any().downcast_ref::<winit::window::Window>().expect("winit backend")
}

fn empty_document() -> Document {
    Document::new(BaseColors { primary: [255; 3], accent: [0x1f, 0x5f, 0xd1], trim: [0x2b, 0x2b, 0x2e], overrides: vec![] })
}

impl AppHandler for JerseyApp {
    fn init(engine: &mut Engine, window: &dyn Window) -> Self {
        splash_status("Design wird geladen …");
        let wgpu_ctx = engine.renderer.wgpu_ctx().expect("renderer initialised");
        let ui_pass = UiPass::new(&wgpu_ctx.device, wgpu_ctx.surface_view_format, winit_window(window));
        let pattern = Pattern::load();
        let doc = initial_document(&pattern);
        let mut ws = Workspace::new(doc);
        let pm = ws.raster.render(&ws.pattern, &ws.regions, &ws.doc, Mode::Display);
        ws.dirty = false;
        let (w, h) = ws.pattern.size();
        let texture = engine
            .assets
            .create_dynamic_texture("print-template", w, h, pm.data().to_vec(), ColorSpace::Srgb, true)
            .expect("print texture");
        let preview = ui_pass.context().load_texture("print-preview", color_image(&pm), TextureOptions::LINEAR);
        let (request_tx, requests) = ai::server::start();
        let mesh = GarmentMesh::load(ws.pattern.size());
        let studio = Studio::new(engine);
        splash_status("3D-Ansicht wird vorbereitet …");
        Self {
            ui_pass,
            studio,
            ws,
            editor: Editor::new(),
            layers: LayersPanel::new(),
            chat: ChatPanel::new(),
            mesh,
            texture,
            preview,
            last_render: Some(pm),
            requests,
            request_tx,
            clipboard: Vec::new(),
            fps: Fps { frames: 0, since: Instant::now(), fps: 0.0 },
            vsync: engine.renderer.settings().vsync,
            pane_centre_ndc: 0.0,
            inv_view_proj: Mat4::IDENTITY,
            dragging_3d: false,
            menu_bottom: 0.0,
            file: None,
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
        self.studio.try_instantiate(engine, self.texture);
        self.studio.show_garments(engine, &self.ws.doc.garments);

        // tool calls from AI assistants
        while let Ok(req) = self.requests.try_recv() {
            let result = ai::commands::execute(&mut self.ws, &self.mesh, &req.tool, &req.args);
            let _ = req.reply.send(result);
        }

        self.ui_pass.begin_frame(winit_window(window));
        let ctx = self.ui_pass.context().clone();
        self.shortcuts(&ctx);
        self.editor.shortcuts(&ctx);
        self.top_bar(&ctx, engine);
        let screen = ctx.content_rect();

        #[allow(deprecated)] // the top-level `show` is what fits UiPass's begin/end frame
        let left = egui::Panel::left("layers")
            .resizable(true)
            .default_size(300.0)
            .size_range(220.0..=520.0)
            .show(&ctx, |ui| {
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
        self.chat.ui(&ctx, &self.ws, &self.request_tx);

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
        if self.studio.loaded {
            self.model_frames += 1;
            if self.model_frames == 3 {
                close_splash();
            }
        }
    }
}

impl JerseyApp {
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
        if let Err(err) = engine.assets.update_dynamic_texture(self.texture, on_model.data()) {
            eprintln!("print texture upload failed: {err}");
        }
        self.preview.set(color_image(&pm), TextureOptions::LINEAR);
        self.last_render = Some(pm);
    }

    /// Picking and dragging objects directly on the model.
    fn interact_3d(&mut self, ctx: &egui::Context, pane: egui::Rect, screen: egui::Rect) {
        let (pos, pressed, down, released) = ctx.input(|i| {
            (i.pointer.hover_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released())
        });
        let over_ui = ctx.is_pointer_over_egui();
        let hit = |this: &Self, p: Pos2| {
            let ndc = [2.0 * (p.x - screen.min.x) / screen.width() - 1.0, 1.0 - 2.0 * (p.y - screen.min.y) / screen.height()];
            let (origin, dir) = GarmentMesh::ray(&this.inv_view_proj, ndc);
            this.mesh.pick(origin, dir, &this.ws.doc.garments).map(|(_, at)| at)
        };
        if self.dragging_3d {
            if down && let Some(at) = pos.and_then(|p| hit(self, p)) {
                self.ws.drag_move(at, 0.0);
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
                ui.menu_button("Datei", |ui| {
                    if ui.button("Neu").clicked() {
                        self.ws.replace(empty_document());
                        self.file = None;
                        ui.close();
                    }
                    ui.menu_button("Vorlage", |ui| {
                        for (name, build) in presets::PRESETS {
                            if ui.button(name).clicked() {
                                let doc = build(&self.ws.pattern);
                                self.ws.replace(doc);
                                self.file = None;
                                ui.close();
                            }
                        }
                    });
                    ui.separator();
                    if ui.button("Öffnen… (Strg+O)").clicked() {
                        self.open();
                        ui.close();
                    }
                    if ui.button("Speichern (Strg+S)").clicked() {
                        self.save(false);
                        ui.close();
                    }
                    if ui.button("Speichern unter…").clicked() {
                        self.save(true);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Druckvorlage exportieren…").clicked() {
                        self.export();
                        ui.close();
                    }
                });
                ui.menu_button("Bearbeiten", |ui| {
                    if ui.button("Rückgängig (Strg+Z)").clicked() {
                        self.ws.undo();
                        ui.close();
                    }
                    if ui.button("Wiederholen (Strg+Y)").clicked() {
                        self.ws.redo();
                        ui.close();
                    }
                    ui.separator();
                    ui::layers::context_menu(ui, &mut self.ws);
                });
                ui.menu_button("Ansicht", |ui| {
                    if ui.button("Vorne").clicked() {
                        self.studio.reset_view(false);
                        ui.close();
                    }
                    if ui.button("Hinten").clicked() {
                        self.studio.reset_view(true);
                        ui.close();
                    }
                    ui.checkbox(&mut self.ws.snap, "Einrasten (Mitte, Teile, Objekte)");
                    if ui.checkbox(&mut self.vsync, "VSync").changed() {
                        let settings = RendererSettings { vsync: self.vsync, ..engine.renderer.settings().clone() };
                        engine.renderer.update_settings(settings);
                    }
                });
                ui.menu_button("KI", |ui| {
                    if ui.button("Assistent öffnen").clicked() {
                        self.chat.open = true;
                        ui.close();
                    }
                    if ui.button("Codex / Claude Code verbinden…").clicked() {
                        self.chat.connect_open = true;
                        ui.close();
                    }
                });
                if ui.selectable_label(self.chat.open, "✨ KI-Assistent").clicked() {
                    self.chat.open = !self.chat.open;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let fps = self.fps.fps.max(0.001);
                    ui.monospace(format!("{fps:>4.0} fps"));
                    if let Some(f) = &self.file {
                        ui.weak(f.file_name().and_then(|n| n.to_str()).unwrap_or_default());
                    }
                });
            });
        });
        self.menu_bottom = bar.response.rect.bottom();
    }

    fn editor_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for tool in Tool::ALL {
                let text = egui::RichText::new(tool.icon()).size(16.0);
                if ui.add(egui::Button::selectable(self.editor.tool == tool, text)).on_hover_text(tool.label()).clicked() {
                    self.editor.set_tool(tool);
                }
            }
        });
        self.tool_options(ui);
        ui.separator();
        let view = CanvasView { texture: self.preview.id(), pixels: self.last_render.as_ref() };
        let sel_before = self.ws.selection.clone();
        self.editor.canvas(ui, &mut self.ws, view);
        if self.ws.selection != sel_before {
            self.ws.dirty = true;
        }
    }

    fn tool_options(&mut self, ui: &mut egui::Ui) {
        let e = &mut self.editor;
        ui.horizontal_wrapped(|ui| {
            ui.label("Farbe");
            ui.color_edit_button_srgb(&mut e.color);
            for c in PALETTE {
                if swatch(ui, c, e.color == c).clicked() {
                    e.color = c;
                }
            }
            match e.tool {
                Tool::Brush | Tool::Eraser => {
                    ui.separator();
                    ui.add(egui::Slider::new(&mut e.brush_mm, 1.0..=80.0).text("mm"));
                }
                Tool::Rect | Tool::Ellipse | Tool::Polygon => {
                    ui.separator();
                    ui.checkbox(&mut e.fill_shapes, "Füllung");
                    ui.checkbox(&mut e.stroke_shapes, "Kontur");
                    if e.stroke_shapes {
                        ui.color_edit_button_srgb(&mut e.stroke_color);
                        ui.add(egui::Slider::new(&mut e.brush_mm, 1.0..=40.0).text("mm"));
                    }
                }
                Tool::Text => {
                    ui.separator();
                    ui.add(egui::TextEdit::singleline(&mut e.text_input).desired_width(120.0));
                    ui.add(egui::Slider::new(&mut e.text_mm, 5.0..=250.0).text("mm Versalhöhe"));
                }
                Tool::Placeholder => {
                    ui.separator();
                    ui.label("Beschriftung");
                    ui.add(egui::TextEdit::singleline(&mut e.placeholder_label).desired_width(80.0));
                }
                Tool::Image => {
                    ui.separator();
                    if ui.button("Datei wählen…").clicked() {
                        e.choose_image();
                    }
                    if let Some(a) = &e.pending_image {
                        ui.weak(format!("{} – klicken zum Platzieren", a.name));
                    }
                }
                _ => {}
            }
        });
    }

    fn save(&mut self, choose: bool) {
        let path = match (&self.file, choose) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new().add_filter("OpenJersey-Design", &["json"]).set_file_name("trikot.json").save_file(),
        };
        let Some(path) = path else { return };
        self.ws.status = match io::to_json(&self.ws.doc).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => {
                self.file = Some(path.clone());
                format!("Gespeichert: {}", path.display())
            }
            Err(e) => format!("Speichern fehlgeschlagen: {e}"),
        };
    }

    fn open(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("OpenJersey-Design", &["json"]).pick_file() else { return };
        match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| io::from_json(&b)) {
            Ok(doc) => {
                self.ws.replace(doc);
                self.ws.status = format!("Geöffnet: {}", path.display());
                self.file = Some(path);
            }
            Err(e) => self.ws.status = format!("Öffnen fehlgeschlagen: {e}"),
        }
    }

    fn export(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("druckvorlage.png").save_file() else {
            return;
        };
        let pm = self.ws.raster.render(&self.ws.pattern, &self.ws.regions, &self.ws.doc, Mode::Print);
        self.ws.status = match pm.save_png(&path) {
            Ok(()) => format!("Exportiert: {}", path.display()),
            Err(e) => format!("Export fehlgeschlagen: {e}"),
        };
    }
}

fn color_image(pm: &tiny_skia::Pixmap) -> ColorImage {
    // the template is opaque, so premultiplied and straight alpha agree
    ColorImage::from_rgba_unmultiplied([pm.width() as usize, pm.height() as usize], pm.data())
}

const PALETTE: [document::Rgb; 8] = [
    [0xff, 0xff, 0xff],
    [0x1a, 0x1a, 0x1a],
    [0x41, 0x41, 0x41],
    [0xb6, 0x2f, 0xc4],
    [0xd7, 0x26, 0x38],
    [0x1f, 0x5f, 0xd1],
    [0x2e, 0xa0, 0x4f],
    [0xf2, 0xc2, 0x1b],
];

fn swatch(ui: &mut egui::Ui, [r, g, b]: document::Rgb, active: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
    ui.painter().rect_filled(rect, 3.0, Color32::from_rgb(r, g, b));
    let stroke = if active {
        egui::Stroke::new(2.0_f32, Color32::from_rgb(70, 140, 255))
    } else {
        egui::Stroke::new(1.0_f32, Color32::GRAY)
    };
    ui.painter().rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Inside);
    response
}

/// `--preset <name>` or a design file given on the command line.
fn initial_document(pattern: &Pattern) -> Document {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--preset")
        && let Some(doc) = args.get(i + 1).and_then(|n| presets::by_name(pattern, n))
    {
        return doc;
    }
    if let Some(path) = args.iter().skip(1).find(|a| a.ends_with(".json"))
        && let Ok(doc) = std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| io::from_json(&b))
    {
        return doc;
    }
    presets::ninesquared_example(pattern)
}

fn main() -> myth::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    // `openjersey mcp`: stdio MCP server for Codex / Claude Code
    if args.get(1).map(String::as_str) == Some("mcp") {
        ai::mcp::run();
        return Ok(());
    }
    // `openjersey --export-template out.png [design.json | --preset <name>]
    // [--garment out3d.png]` renders without opening a window
    if let Some(i) = args.iter().position(|a| a == "--export-template") {
        let out = args.get(i + 1).expect("--export-template needs an output path");
        let pattern = Pattern::load();
        let doc = initial_document(&pattern);
        let ws = Workspace::new(doc);
        let pm = ws.raster.render(&ws.pattern, &ws.regions, &ws.doc, Mode::Print);
        pm.save_png(out).expect("writing the PNG");
        if let Some(j) = args.iter().position(|a| a == "--garment") {
            let display = ws.raster.render(&ws.pattern, &ws.regions, &ws.doc, Mode::Display);
            let mesh = GarmentMesh::load(ws.pattern.size());
            mesh.preview(&display, std::env::var("OJ_PREVIEW_HEIGHT").ok().and_then(|h| h.parse().ok()).unwrap_or(900), &ws.doc.garments).save_png(&args[j + 1]).expect("writing the PNG");
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
    let path = if std::env::var("OJ_PATH").as_deref() == Ok("basic") {
        RenderPath::BasicForward
    } else {
        RenderPath::HighFidelity
    };
    let result = App::new()
        .with_title("OpenJersey")
        .with_inner_size(1600, 900)
        .with_settings(RendererSettings { vsync, path, anisotropy_clamp: 8, ..Default::default() })
        .run::<JerseyApp>();
    ai::server::stop();
    result
}
