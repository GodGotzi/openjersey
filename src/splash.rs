//! The loading window. Setting up the 3D view (shaders, environment light)
//! blocks the app's own window for several seconds on slow GPUs, so the
//! splash is a separate process (`openjersey --splash`) drawn on the CPU: it
//! shows up at once and keeps animating. The app sends status lines on its
//! stdin and closes it when the model is on screen; the splash also quits if
//! the app dies.

use std::io::{BufRead, Write};
use std::num::NonZeroU32;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, PixmapPaint, Point, Rect, Shader,
    SpreadMode, Transform,
};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::{Window, WindowId};

use crate::document::Asset;
use crate::render::text::Fonts;

const SIZE: (f32, f32) = (440.0, 300.0);
/// Give up if the app never reports back.
const TIMEOUT: Duration = Duration::from_secs(90);

/// The app's handle on the running splash.
pub struct Splash {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Splash {
    pub fn spawn() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        let mut child = Command::new(exe).arg("--splash").stdin(Stdio::piped()).spawn().ok()?;
        let stdin = child.stdin.take();
        Some(Self { child, stdin })
    }

    /// The line under the title.
    pub fn status(&mut self, text: &str) {
        if let Some(stdin) = &mut self.stdin
            && writeln!(stdin, "{text}").and_then(|_| stdin.flush()).is_err()
        {
            self.stdin = None;
        }
    }

    /// The app is ready: closing stdin ends the splash.
    pub fn finish(mut self) {
        self.stdin = None;
        let _ = self.child.wait();
    }
}

enum Message {
    Status(String),
    Done,
}

/// Entry point of the splash process.
pub fn run() {
    let Ok(event_loop) = EventLoop::<Message>::with_user_event().build() else { return };
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || read_stdin(proxy));
    let mut app = SplashApp {
        window: None,
        surface: None,
        art: None,
        status: "Wird gestartet …".into(),
        start: Instant::now(),
    };
    let _ = event_loop.run_app(&mut app);
}

fn read_stdin(proxy: EventLoopProxy<Message>) {
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if proxy.send_event(Message::Status(line)).is_err() {
            return;
        }
    }
    let _ = proxy.send_event(Message::Done);
}

struct SplashApp {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    art: Option<Art>,
    status: String,
    start: Instant,
}

impl ApplicationHandler<Message> for SplashApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let size = winit::dpi::LogicalSize::new(SIZE.0, SIZE.1);
        // a fixed size makes tiling compositors float and centre it
        let attrs = Window::default_attributes()
            .with_title("OpenJersey")
            .with_name("openjersey-splash", "openjersey-splash")
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_max_inner_size(size)
            .with_resizable(false)
            .with_decorations(false);
        let Ok(window) = event_loop.create_window(attrs) else {
            event_loop.exit();
            return;
        };
        let window = Arc::new(window);
        let Ok(context) = softbuffer::Context::new(window.clone()) else { return };
        self.surface = softbuffer::Surface::new(&context, window.clone()).ok();
        self.window = Some(window);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, msg: Message) {
        match msg {
            Message::Status(text) => self.status = text,
            Message::Done => event_loop.exit(),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.start.elapsed() > TIMEOUT {
            event_loop.exit();
            return;
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(33)));
    }
}

impl SplashApp {
    fn draw(&mut self) {
        let (Some(window), Some(surface)) = (&self.window, &mut self.surface) else { return };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if surface.resize(w, h).is_err() {
            return;
        }
        let scale = size.width as f32 / SIZE.0;
        if self.art.as_ref().is_none_or(|a| a.scale != scale) {
            self.art = Art::new(scale);
        }
        let Some(art) = &self.art else { return };
        let frame = art.frame(&self.status, self.start.elapsed().as_secs_f32(), size.width, size.height);
        let Ok(mut buffer) = surface.buffer_mut() else { return };
        for (dst, px) in buffer.iter_mut().zip(frame.pixels()) {
            *dst = (px.red() as u32) << 16 | (px.green() as u32) << 8 | px.blue() as u32;
        }
        let _ = buffer.present();
    }
}

/// Everything that doesn't change between frames, at the window's scale.
struct Art {
    scale: f32,
    background: Pixmap,
    fonts: Fonts,
}

/// The icon's accent gradient.
const ACCENT: [[u8; 3]; 2] = [[0x5b, 0x9b, 0xff], [0x2f, 0x6e, 0xe6]];
/// The progress bar, logical pixels: x, y, width, height.
const BAR: [f32; 4] = [110.0, 246.0, 220.0, 4.0];

impl Art {
    fn new(scale: f32) -> Option<Self> {
        let (w, h) = ((SIZE.0 * scale) as u32, (SIZE.1 * scale) as u32);
        let s = scale;
        let mut bg = Pixmap::new(w, h)?;
        let gradient = LinearGradient::new(
            Point::from_xy(0.0, 0.0),
            Point::from_xy(0.0, h as f32),
            vec![GradientStop::new(0.0, rgb([0x2a, 0x2d, 0x34])), GradientStop::new(1.0, rgb([0x11, 0x12, 0x16]))],
            SpreadMode::Pad,
            Transform::identity(),
        )?;
        bg.fill_rect(Rect::from_xywh(0.0, 0.0, w as f32, h as f32)?, &Paint { shader: gradient, ..Paint::default() }, Transform::identity(), None);

        // icon and title
        let icon_size = 88.0 * s;
        if let Ok(icon) = Asset::new("icon.svg", include_bytes!("../assets/icon.svg").to_vec())
            && let Some(px) = icon.raster(icon_size as u32, icon_size as u32, None)
        {
            let x = (w as f32 - icon_size) / 2.0;
            bg.draw_pixmap(x as i32, (38.0 * s) as i32, px.as_ref().as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
        let fonts = Fonts::new();
        text(&mut bg, &fonts, "OpenJersey", 30.0 * s, [w as f32 / 2.0, 156.0 * s], [0xf3, 0xf4, 0xf6]);
        text(&mut bg, &fonts, "TRIKOT-DESIGNER", 10.5 * s, [w as f32 / 2.0, 182.0 * s], [0x6b, 0x72, 0x7e]);

        // the bar's track
        let [x, y, bw, bh] = BAR.map(|v| v * s);
        if let Some(path) = rounded_bar(x, y, bw, bh) {
            bg.fill_path(&path, &solid([0x30, 0x34, 0x3c], 1.0), FillRule::Winding, Transform::identity(), None);
        }
        Some(Self { scale, background: bg, fonts })
    }

    fn frame(&self, status: &str, t: f32, w: u32, h: u32) -> Pixmap {
        let s = self.scale;
        let mut pm = self.background.clone();
        if pm.width() != w || pm.height() != h {
            pm = Pixmap::new(w, h).unwrap_or(pm);
            pm.draw_pixmap(0, 0, self.background.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
        text(&mut pm, &self.fonts, status, 12.5 * s, [w as f32 / 2.0, 222.0 * s], [0x9a, 0xa0, 0xa8]);

        // indeterminate progress: a segment glides across the track, easing
        // in and out, clipped to the track
        let [x, y, bw, bh] = BAR.map(|v| v * s);
        let phase = (t / 1.6).fract();
        let eased = 0.5 - 0.5 * (std::f32::consts::PI * phase).cos();
        let seg = bw * 0.35;
        let start = x - seg + (bw + seg) * eased;
        let (x0, x1) = (start.max(x), (start + seg).min(x + bw));
        if x1 > x0
            && let Some(path) = rounded_bar(x0, y, x1 - x0, bh)
            && let Some(shader) = LinearGradient::new(
                Point::from_xy(start, 0.0),
                Point::from_xy(start + seg, 0.0),
                vec![GradientStop::new(0.0, rgb(ACCENT[0])), GradientStop::new(1.0, rgb(ACCENT[1]))],
                SpreadMode::Pad,
                Transform::identity(),
            )
        {
            pm.fill_path(&path, &Paint { shader, anti_alias: true, ..Paint::default() }, FillRule::Winding, Transform::identity(), None);
        }
        pm
    }
}

fn rounded_bar(x: f32, y: f32, w: f32, h: f32) -> Option<tiny_skia::Path> {
    let r = (h / 2.0).min(w / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

fn text(pm: &mut Pixmap, fonts: &Fonts, s: &str, size: f32, [x, y]: [f32; 2], color: [u8; 3]) {
    if let Some(path) = fonts.text_path(s, size) {
        pm.fill_path(&path, &solid(color, 1.0), FillRule::Winding, Transform::from_translate(x, y), None);
    }
}

fn rgb([r, g, b]: [u8; 3]) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

fn solid([r, g, b]: [u8; 3], alpha: f32) -> Paint<'static> {
    let color = Color::from_rgba8(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0) as u8);
    Paint { shader: Shader::SolidColor(color), anti_alias: true, ..Paint::default() }
}
