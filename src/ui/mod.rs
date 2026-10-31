pub mod browser;
pub mod canvas_panel;
pub mod color_picker;
pub mod layers;
pub mod properties;
pub mod viewport;
pub mod widgets;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use myth::egui::egui;

    /// Symbol characters (icons, arrows, punctuation) used in string literals of the
    /// sources and in the locale files, with where they appear. Comments are skipped.
    fn symbols_used() -> BTreeMap<char, String> {
        fn visit(dir: &Path, found: &mut BTreeMap<char, String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    visit(&path, found);
                } else if path.extension().is_some_and(|e| e == "rs" || e == "toml") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for (number, line) in text.lines().enumerate() {
                        if line.trim_start().starts_with("//") || line.trim_start().starts_with('#') {
                            continue;
                        }
                        for c in line.chars().filter(|c| !c.is_ascii() && !c.is_alphanumeric() && !c.is_whitespace()) {
                            found.entry(c).or_insert_with(|| format!("{}:{}", path.file_name().unwrap().to_string_lossy(), number + 1));
                        }
                    }
                }
            }
        }
        let mut found = BTreeMap::new();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        visit(&root.join("src"), &mut found);
        visit(&root.join("locales"), &mut found);
        found
    }

    /// A symbol the interface font lacks is drawn as an empty box.
    #[test]
    #[allow(deprecated)]
    fn every_symbol_the_interface_draws_exists_in_its_font() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let font = egui::FontId::proportional(14.0);
        let missing: Vec<String> = symbols_used()
            .into_iter()
            .filter(|(c, _)| !ctx.fonts_mut(|fonts| fonts.has_glyph(&font, *c)))
            .map(|(c, at)| format!("{c} (U+{:04X}, {at})", c as u32))
            .collect();
        assert!(missing.is_empty(), "symbols the font can't draw (they show as boxes): {missing:?}");
    }
}
