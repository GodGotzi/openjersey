//! UI themes: presets that are read from `themes/*.toml` (embedded at build
//! time, see `build.rs`) and applied to egui, to the parts the app paints
//! itself (the 2D canvas, the rulers, the selection) and to the 3D view's
//! background. The user picks one in Settings; the choice is remembered.
//!
//! Besides the bundled presets, the app reads `*.toml` files in the same format
//! from `~/.config/openjersey/themes/` (created at start) when it starts; they appear under "Your themes".
//!
//! The theme comes from `OJ_THEME` (a file name without `.toml`), else from
//! the saved choice (`theme` in `~/.config/openjersey/settings.toml`), else it is Graphite.

use std::ops::Deref;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use myth::egui::egui::style::ScrollStyle;
use myth::egui::egui::{self, Color32, CornerRadius, FontId, Margin, Shadow, Stroke, TextStyle, Vec2, Visuals};
use serde::{Deserialize, Deserializer};

include!(concat!(env!("OUT_DIR"), "/themes.rs"));

/// The theme used when nothing else is chosen.
pub const DEFAULT: &str = "graphite";

/// A colour written as `#rrggbb` in a theme file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hex(Color32);

impl Deref for Hex {
    type Target = Color32;
    fn deref(&self) -> &Color32 {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Hex {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let digits = text
            .strip_prefix('#')
            .filter(|d| d.len() == 6)
            .ok_or_else(|| serde::de::Error::custom(format!("`{text}` is not a #rrggbb colour")))?;
        let n = u32::from_str_radix(digits, 16).map_err(|_| serde::de::Error::custom(format!("`{text}` is not a #rrggbb colour")))?;
        Ok(Hex(Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)))
    }
}

/// What the colours are used for is noted in each theme file.
#[derive(Debug, Deserialize)]
pub struct Colors {
    pub window: Hex,
    pub panel: Hex,
    pub surface: Hex,
    pub surface_hover: Hex,
    pub surface_active: Hex,
    pub border: Hex,
    pub text: Hex,
    pub text_weak: Hex,
    pub accent: Hex,
    pub accent_text: Hex,
    pub input: Hex,
    pub canvas: Hex,
    pub viewport: Hex,
    pub ruler: Hex,
    pub warn: Hex,
    pub error: Hex,
}

#[derive(Debug, Deserialize)]
pub struct Shape {
    pub rounding: f32,
}

#[derive(Debug, Deserialize)]
pub struct Theme {
    pub name: String,
    /// The position among the bundled themes; your own themes may leave it out.
    #[serde(default)]
    pub order: i32,
    pub dark: bool,
    pub colors: Colors,
    pub shape: Shape,
}

/// A theme with its id (the file name).
pub struct Entry {
    pub id: &'static str,
    pub theme: Theme,
    /// Bundled with the app, not read from the user's theme folder.
    pub builtin: bool,
}

/// The themes and what went wrong reading the user's own.
struct Loaded {
    themes: Vec<Entry>,
    problems: Vec<String>,
}

fn loaded() -> &'static Loaded {
    static LOADED: OnceLock<Loaded> = OnceLock::new();
    LOADED.get_or_init(|| {
        let mut themes: Vec<Entry> = THEME_FILES
            .iter()
            .map(|&(id, text)| Entry { id, theme: toml::from_str(text).unwrap_or_else(|e| panic!("themes/{id}.toml: {e}")), builtin: true })
            .collect();
        themes.sort_by_key(|e| e.theme.order);
        let taken: Vec<&str> = themes.iter().map(|e| e.id).collect();
        let (own, problems) = crate::config::themes_dir().map_or_else(Default::default, |dir| load_user_themes(&dir, &taken));
        themes.extend(own);
        Loaded { themes, problems }
    })
}

/// The bundled themes in the order of their `order` keys, then the user's own
/// in the order of their file names.
pub fn all() -> &'static [Entry] {
    &loaded().themes
}

/// Messages about the user's theme files that could not be used.
pub fn problems() -> &'static [String] {
    &loaded().problems
}

/// Reads the `*.toml` files in `dir` as themes. A file whose name is in
/// `taken`, or that does not parse, is reported instead of used.
fn load_user_themes(dir: &Path, taken: &[&str]) -> (Vec<Entry>, Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return Default::default() };
    let mut files: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect();
    files.sort();
    let (mut themes, mut problems) = (Vec::new(), Vec::new());
    for path in files {
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let id = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if taken.contains(&id.as_str()) {
            problems.push(format!("{file}: a bundled theme has this name"));
            continue;
        }
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| toml::from_str::<Theme>(&text).map_err(|e| e.message().to_string()));
        match parsed {
            // there are few of these and they live as long as the app
            Ok(theme) => themes.push(Entry { id: Box::leak(id.into_boxed_str()), theme, builtin: false }),
            Err(message) => problems.push(format!("{file}: {message}")),
        }
    }
    (themes, problems)
}

static ACTIVE: AtomicUsize = AtomicUsize::new(0);

fn index_of(id: &str) -> Option<usize> {
    all().iter().position(|e| e.id == id)
}

/// Picks the theme to start with (see the module docs).
pub fn init() {
    let wanted = std::env::var("OJ_THEME").ok().or_else(|| crate::config::get_str("theme"));
    let index = wanted.as_deref().and_then(index_of).or_else(|| index_of(DEFAULT)).unwrap_or(0);
    ACTIVE.store(index, Ordering::Relaxed);
}

pub fn current_index() -> usize {
    ACTIVE.load(Ordering::Relaxed).min(all().len() - 1)
}

pub fn current() -> &'static Theme {
    &all()[current_index()].theme
}

/// Switches the theme and remembers the choice. Call `apply` afterwards.
pub fn set(index: usize) {
    ACTIVE.store(index, Ordering::Relaxed);
    crate::config::set("theme", all()[current_index()].id);
}

// ---- colours for what the app paints itself ----

pub fn colors() -> &'static Colors {
    &current().colors
}

pub fn accent() -> Color32 {
    *colors().accent
}

/// Text or an icon on the accent colour.
pub fn on_accent() -> Color32 {
    *colors().accent_text
}

/// The colour to give the 3D engine as its background so that the screen
/// shows `color`. The engine treats it as a scene colour: it goes through the
/// Neutral tone mapping and sRGB encoding like everything else, so the
/// inverse of that is applied here.
pub fn engine_background(color: Color32) -> [f32; 3] {
    let wanted = [color.r(), color.g(), color.b()].map(srgb_to_linear);
    let mut scene = wanted;
    for _ in 0..40 {
        let shown = neutral_tone_map(scene);
        for i in 0..3 {
            scene[i] = (scene[i] + wanted[i] - shown[i]).clamp(0.0, 4.0);
        }
    }
    scene
}

/// The background of the 3D view for the current theme.
pub fn viewport_background() -> [f32; 3] {
    engine_background(*colors().viewport)
}

fn srgb_to_linear(value: u8) -> f32 {
    let s = value as f32 / 255.0;
    if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
}

/// Khronos PBR Neutral tone mapping, which the 3D view uses.
fn neutral_tone_map([r, g, b]: [f32; 3]) -> [f32; 3] {
    const START_COMPRESSION: f32 = 0.8 - 0.04;
    const DESATURATION: f32 = 0.15;
    let lowest = r.min(g).min(b);
    let offset = if lowest < 0.08 { lowest - 6.25 * lowest * lowest } else { 0.04 };
    let [r, g, b] = [r - offset, g - offset, b - offset];
    let peak = r.max(g).max(b);
    if peak < START_COMPRESSION {
        return [r, g, b];
    }
    let d = 1.0 - START_COMPRESSION;
    let new_peak = 1.0 - d * d / (peak + d - START_COMPRESSION);
    let mix_to_white = 1.0 - 1.0 / (DESATURATION * (peak - new_peak) + 1.0);
    [r, g, b].map(|c| {
        let c = c * new_peak / peak;
        c + (new_peak - c) * mix_to_white
    })
}

/// The colour `t` (0..1) of the way from `a` to `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}

/// Applies the current theme to egui: colours, shapes, spacing and type sizes.
pub fn apply(ctx: &egui::Context) {
    let theme = current();
    let c = &theme.colors;
    let rounding = CornerRadius::same(theme.shape.rounding.round().clamp(0.0, 20.0) as u8);
    ctx.global_style_mut(|style| {
        let mut v = if theme.dark { Visuals::dark() } else { Visuals::light() };
        v.panel_fill = *c.panel;
        v.window_fill = *c.window;
        v.window_stroke = Stroke::new(1.0, *c.border);
        v.window_corner_radius = CornerRadius::same(rounding.nw.saturating_add(3));
        v.menu_corner_radius = rounding;
        v.extreme_bg_color = *c.input;
        v.text_edit_bg_color = Some(*c.input);
        v.faint_bg_color = mix(*c.panel, *c.surface, 0.5);
        v.code_bg_color = *c.input;
        v.weak_text_color = Some(*c.text_weak);
        v.hyperlink_color = *c.accent;
        v.warn_fg_color = *c.warn;
        v.error_fg_color = *c.error;
        v.selection.bg_fill = *c.accent;
        v.selection.stroke = Stroke::new(1.0, *c.accent_text);
        let shadow_alpha = if theme.dark { 90 } else { 40 };
        v.window_shadow = Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(shadow_alpha) };
        v.popup_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(shadow_alpha) };

        let border = Stroke::new(1.0, *c.border);
        let text = Stroke::new(1.0, *c.text);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = *c.panel;
        w.noninteractive.weak_bg_fill = *c.panel;
        w.noninteractive.bg_stroke = border;
        w.noninteractive.fg_stroke = text;
        w.inactive.bg_fill = *c.surface;
        w.inactive.weak_bg_fill = *c.surface;
        w.inactive.bg_stroke = border;
        w.inactive.fg_stroke = text;
        w.hovered.bg_fill = *c.surface_hover;
        w.hovered.weak_bg_fill = *c.surface_hover;
        w.hovered.bg_stroke = Stroke::new(1.0, mix(*c.border, *c.accent, 0.5));
        w.hovered.fg_stroke = text;
        w.active.bg_fill = *c.surface_active;
        w.active.weak_bg_fill = *c.surface_active;
        w.active.bg_stroke = Stroke::new(1.0, *c.accent);
        w.active.fg_stroke = text;
        w.open.bg_fill = *c.surface_active;
        w.open.weak_bg_fill = *c.surface_active;
        w.open.bg_stroke = border;
        w.open.fg_stroke = text;
        for widget in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            widget.corner_radius = rounding;
            widget.expansion = 0.0;
        }
        style.visuals = v;

        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.interact_size.y = 24.0;
        style.spacing.window_margin = Margin::same(12);
        style.spacing.menu_margin = Margin::same(6);
        style.spacing.indent = 16.0;
        style.spacing.scroll = ScrollStyle::floating();
        style.text_styles.insert(TextStyle::Heading, FontId::proportional(17.0));
        style.text_styles.insert(TextStyle::Body, FontId::proportional(13.5));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(13.5));
        style.text_styles.insert(TextStyle::Small, FontId::proportional(11.5));
        style.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.5));
    });
}

/// How well `a` can be read on `b` (WCAG contrast ratio, 1 to 21).
#[cfg(test)]
fn contrast(a: Color32, b: Color32) -> f32 {
    let luminance = |c: Color32| {
        let channel = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    };
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundled themes only: what is in the tester's own theme folder must not matter.
    fn bundled() -> impl Iterator<Item = &'static Entry> {
        all().iter().filter(|e| e.builtin)
    }

    #[test]
    fn every_theme_file_parses_and_the_default_exists() {
        assert!(bundled().count() >= 4, "only {} themes", bundled().count());
        assert!(THEME_FILES.iter().any(|(id, _)| *id == DEFAULT), "the default theme `{DEFAULT}` is missing");
        let mut orders: Vec<_> = bundled().map(|e| e.theme.order).collect();
        orders.sort_unstable();
        orders.dedup();
        assert_eq!(orders.len(), bundled().count(), "two bundled themes share an order");
    }

    #[test]
    fn a_users_theme_files_are_loaded_and_the_bad_ones_reported() {
        let dir = std::env::temp_dir().join(format!("openjersey-themes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let graphite = THEME_FILES.iter().find(|(id, _)| *id == "graphite").unwrap().1;
        // a copy with another name and no `order`
        let mine: String = graphite
            .lines()
            .filter(|l| !l.starts_with("order"))
            .map(|l| if l.starts_with("name") { "name = \"Mine\"".to_string() } else { l.to_string() })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(dir.join("mine.toml"), mine).unwrap();
        std::fs::write(dir.join("broken.toml"), graphite.replace("#1c1d21", "not a colour")).unwrap();
        std::fs::write(dir.join("graphite.toml"), graphite).unwrap();
        std::fs::write(dir.join("notes.txt"), "not a theme").unwrap();

        let (themes, problems) = load_user_themes(&dir, &["graphite"]);
        assert_eq!(themes.len(), 1);
        assert_eq!((themes[0].id, themes[0].theme.name.as_str(), themes[0].builtin), ("mine", "Mine", false));
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|p| p.starts_with("broken.toml") && p.contains("#rrggbb")), "{problems:?}");
        assert!(problems.iter().any(|p| p.starts_with("graphite.toml")), "{problems:?}");
        // a folder that doesn't exist is fine
        assert!(load_user_themes(&dir.join("nowhere"), &[]).0.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_example_themes_load_and_are_readable() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/themes");
        let taken: Vec<&str> = bundled().map(|e| e.id).collect();
        let (themes, problems) = load_user_themes(&dir, &taken);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(themes.len() >= 3);
        for entry in &themes {
            let c = &entry.theme.colors;
            assert!(contrast(*c.text, *c.panel) >= 7.0, "{}: text on the panel", entry.id);
            assert!(contrast(*c.text_weak, *c.panel) >= 4.5, "{}: weak text on the panel", entry.id);
            assert!(contrast(*c.accent_text, *c.accent) >= 4.5, "{}: text on the accent", entry.id);
            assert_eq!(entry.theme.dark, contrast(*c.text, *c.panel) > contrast(Color32::BLACK, *c.panel), "{}: dark flag", entry.id);
        }
    }

    #[test]
    fn every_theme_is_readable() {
        for entry in bundled() {
            let (c, name) = (&entry.theme.colors, entry.id);
            for (what, background) in [("panel", *c.panel), ("window", *c.window), ("surface", *c.surface), ("input", *c.input)] {
                assert!(
                    contrast(*c.text, background) >= 4.5,
                    "{name}: text on {what} is hard to read ({:.1})",
                    contrast(*c.text, background)
                );
            }
            assert!(contrast(*c.text_weak, *c.panel) >= 3.0, "{name}: weak text on the panel ({:.1})", contrast(*c.text_weak, *c.panel));
            assert!(contrast(*c.accent_text, *c.accent) >= 3.0, "{name}: text on the accent ({:.1})", contrast(*c.accent_text, *c.accent));
            assert!(
                contrast(*c.accent, *c.panel) >= 2.0,
                "{name}: the accent barely shows on the panel ({:.1})",
                contrast(*c.accent, *c.panel)
            );
            assert!(
                entry.theme.dark == (contrast(*c.text, *c.panel) > contrast(Color32::BLACK, *c.panel)),
                "{name}: `dark` doesn't match its colours"
            );
        }
    }

    #[test]
    #[allow(deprecated)]
    fn applying_a_theme_gives_egui_its_colours() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        for index in (0..all().len()).filter(|&i| all()[i].builtin) {
            ACTIVE.store(index, Ordering::Relaxed);
            apply(&ctx);
            let theme = &all()[index].theme;
            let style = ctx.global_style();
            assert_eq!(style.visuals.panel_fill, *theme.colors.panel, "{}", all()[index].id);
            assert_eq!(style.visuals.dark_mode, theme.dark, "{}", all()[index].id);
            assert_eq!(style.visuals.selection.bg_fill, *theme.colors.accent, "{}", all()[index].id);
            assert_eq!(style.visuals.widgets.inactive.corner_radius, CornerRadius::same(theme.shape.rounding as u8));
        }
        ACTIVE.store(index_of(DEFAULT).unwrap(), Ordering::Relaxed);
    }

    #[test]
    fn bad_colours_are_rejected() {
        assert!(toml::from_str::<Colors>("window = \"red\"").is_err());
        let hex: Result<Hex, _> =
            serde::Deserialize::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new("#12345"));
        assert!(hex.is_err());
    }

    /// What the screen shows for a scene colour: tone mapping, then sRGB encoding.
    fn shown(scene: [f32; 3]) -> [u8; 3] {
        neutral_tone_map(scene).map(|c| {
            let c = c.clamp(0.0, 1.0);
            let encoded = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
            (encoded * 255.0).round() as u8
        })
    }

    #[test]
    fn the_viewport_shows_the_colour_the_theme_asks_for() {
        for entry in bundled() {
            let wanted = *entry.theme.colors.viewport;
            let got = shown(engine_background(wanted));
            for (g, w) in got.iter().zip([wanted.r(), wanted.g(), wanted.b()]) {
                assert!(g.abs_diff(w) <= 2, "{}: the viewport shows {got:?} instead of {wanted:?}", entry.id);
            }
        }
        // and the tone mapping is what was measured in the app: #3b3e47 as a scene colour shows as about #797c86
        let measured = shown([59.0 / 255.0, 62.0 / 255.0, 71.0 / 255.0]);
        assert!(measured[0].abs_diff(0x79) <= 3 && measured[2].abs_diff(0x86) <= 3, "{measured:?}");
    }

    #[test]
    fn mixing_colours_goes_from_one_to_the_other() {
        let (black, white) = (Color32::BLACK, Color32::WHITE);
        assert_eq!(mix(black, white, 0.0), black);
        assert_eq!(mix(black, white, 1.0), white);
        assert_eq!(mix(black, white, 0.5), Color32::from_rgb(128, 128, 128));
    }
}
