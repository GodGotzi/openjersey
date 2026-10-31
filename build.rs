//! Finds the garments in `garments/<id>/` (see blender/GARMENTS.md)
//! and writes the library `garments.rs`, embedding each garment's model and
//! pattern.

use std::fmt::Write;
use std::path::Path;

fn main() {
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
