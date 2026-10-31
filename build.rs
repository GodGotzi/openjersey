//! Finds the garments in `garments/<id>/` (see blender/GARMENTS.md)
//! and writes the library `garments.rs`, embedding each garment's model and
//! pattern.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

fn main() {
    themes();
    translations();
    garments();
}

fn garments() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("garments");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut garments = Vec::new();
    for entry in std::fs::read_dir(&root).expect("the garments/ folder exists") {
        let dir = entry.expect("readable").path();
        let meta = dir.join("garment.json");
        if !meta.exists() {
            continue;
        }
        println!("cargo:rerun-if-changed={}", meta.display());
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&meta).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", meta.display()));
        garments.push((json, dir));
    }
    garments.sort_by_key(|(g, _)| (g["order"].as_i64().unwrap_or(0), g["id"].as_str().unwrap_or("").to_string()));

    let mut out = String::from("pub static GARMENTS: &[Garment] = &[\n");
    for (g, dir) in &garments {
        let s = |key: &str| format!("{:?}", g[key].as_str().unwrap_or(""));
        let kind = match g["kind"].as_str() {
            Some("top") => "Kind::Top",
            Some("bottom") => "Kind::Bottom",
            other => panic!("{}: kind {other:?}", dir.display()),
        };
        let body = match g["body"].as_str() {
            Some("man") => "Body::Man",
            Some("women") => "Body::Women",
            other => panic!("{}: body {other:?}", dir.display()),
        };
        let path = |f: &str| format!("{:?}", dir.join(f).display().to_string());
        writeln!(
            out,
            "    Garment {{ id: {}, name: {}, kind: {kind}, body: {body}, unisex: {}, \
             glb_path: {}, glb: include_bytes!({}), pattern_json: include_str!({}) }},",
            s("id"),
            s("name"),
            g["unisex"].as_bool().unwrap_or(false),
            path("model.glb"),
            path("model.glb"),
            path("pattern.json"),
        )
        .unwrap();
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("garments.rs");
    std::fs::write(dest, out).unwrap();
}

/// The UI languages: (name of the generated table, locale file).
const LANGUAGES: [(&str, &str); 2] = [("EN", "en"), ("DE", "de")];

/// Reads `locales/<code>.toml` and writes `translations.rs`, one sorted
/// `(key, text)` table per language. Every language must have the same keys
/// and the same number of `{}` placeholders in each text.
fn translations() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    let mut tables: Vec<(&str, &str, BTreeMap<String, String>)> = Vec::new();
    for (name, code) in LANGUAGES {
        let path = dir.join(format!("{code}.toml"));
        println!("cargo:rerun-if-changed={}", path.display());
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let table: toml::Table = text.parse().unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut flat = BTreeMap::new();
        flatten("", &table, &mut flat, &path);
        tables.push((name, code, flat));
    }
    let (_, base_code, base) = &tables[0];
    for (_, code, other) in &tables[1..] {
        for (key, text) in base {
            let Some(translated) = other.get(key) else { panic!("locales/{code}.toml has no `{key}` (it is in {base_code}.toml)") };
            let holes = |t: &str| t.matches("{}").count();
            assert_eq!(holes(text), holes(translated), "`{key}` has a different number of {{}} in {base_code}.toml and {code}.toml");
        }
        for key in other.keys() {
            assert!(base.contains_key(key), "locales/{code}.toml has `{key}`, which {base_code}.toml lacks");
        }
    }
    let mut out = String::new();
    for (name, _, table) in &tables {
        writeln!(out, "pub static {name}: &[(&str, &str)] = &[").unwrap();
        for (key, text) in table {
            writeln!(out, "    ({key:?}, {text:?}),").unwrap();
        }
        out.push_str("];\n");
    }
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("translations.rs"), out).unwrap();
}

/// Nested tables become dotted keys: `[menu.file] save = "…"` is `menu.file.save`.
fn flatten(prefix: &str, table: &toml::Table, out: &mut BTreeMap<String, String>, path: &Path) {
    for (key, value) in table {
        let full = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
        match value {
            toml::Value::String(text) => {
                out.insert(full, text.clone());
            }
            toml::Value::Table(inner) => flatten(&full, inner, out, path),
            other => panic!("{}: `{full}` must be a string or a table, not {}", path.display(), other.type_str()),
        }
    }
}

/// Embeds every `themes/*.toml` (checked to be valid TOML here; `src/theme.rs`
/// reads their contents) as `(id, text)`, the id being the file name.
fn themes() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("the themes/ folder exists")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    let mut out = String::from("pub static THEME_FILES: &[(&str, &str)] = &[\n");
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        text.parse::<toml::Table>().unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let id = path.file_stem().and_then(|s| s.to_str()).expect("a theme file name");
        writeln!(out, "    ({id:?}, include_str!({:?})),", path.display().to_string()).unwrap();
    }
    out.push_str("];\n");
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("themes.rs"), out).unwrap();
}
