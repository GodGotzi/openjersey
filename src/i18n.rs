//! UI language. Every user-visible text is a key such as `menu.file.save` in
//! `locales/<language>.toml` (one file per language, nested tables become
//! dotted keys). Code asks for it with `tr("menu.file.save")`; `build.rs`
//! checks the files agree and compiles them into lookup tables. To add a
//! language, add a file, list it in `build.rs` and in `Language`.
//!
//! The language comes from `OJ_LANG` (`en`/`de`), else from the saved choice
//! (`~/.config/openjersey/language`), else from the system locale (`LANG`).

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    German,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::German];

    /// The language's own name, as shown in the language menu.
    pub fn name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::German => "Deutsch",
        }
    }

    fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::German => "de",
        }
    }

    fn from_code(code: &str) -> Option<Language> {
        match code.trim().to_lowercase().get(..2)? {
            "en" => Some(Language::English),
            "de" => Some(Language::German),
            _ => None,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Picks the startup language (see the module docs).
pub fn init() {
    let from_env = std::env::var("OJ_LANG").ok().and_then(|v| Language::from_code(&v));
    let saved = crate::config::read("language").and_then(|v| Language::from_code(&v));
    let system = std::env::var("LANG").ok().and_then(|v| Language::from_code(&v));
    set_runtime(from_env.or(saved).or(system).unwrap_or(Language::English));
}

fn set_runtime(lang: Language) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Language {
    if CURRENT.load(Ordering::Relaxed) == Language::German as u8 { Language::German } else { Language::English }
}

/// Switches the language and remembers the choice for the next start.
pub fn set(lang: Language) {
    set_runtime(lang);
    crate::config::write("language", lang.code());
}

include!(concat!(env!("OUT_DIR"), "/translations.rs"));

fn table(language: Language) -> &'static [(&'static str, &'static str)] {
    match language {
        Language::English => EN,
        Language::German => DE,
    }
}

/// The text for a key (`menu.file.save`) in the current language.
pub fn lookup(key: &str) -> Option<&'static str> {
    let table = table(current());
    table.binary_search_by_key(&key, |&(k, _)| k).ok().map(|i| table[i].1)
}

/// The text for a key in the current language. The keys live in
/// `locales/<language>.toml`; a key without a text shows as itself, and a test
/// makes sure every key used in the code has one.
pub fn tr(key: &'static str) -> &'static str {
    lookup(key).unwrap_or(key)
}

/// Like `tr`, for texts with `{}` holes that are filled in order:
/// `tr_args("error.unreadable_image", &[&name])`.
pub fn tr_args(key: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let mut args = args.iter();
    let mut out = String::new();
    let mut rest = tr(key);
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        if let Some(arg) = args.next() {
            out.push_str(&arg.to_string());
        }
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys written as `tr("...")` / `tr_args("...")` in the code.
    fn keys_used_in_code() -> std::collections::BTreeSet<String> {
        fn visit(dir: &std::path::Path, keys: &mut std::collections::BTreeSet<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    visit(&path, keys);
                } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("i18n.rs") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    for call in ["tr(\"", "tr_args(\""] {
                        for (at, _) in source.match_indices(call) {
                            let before = source[..at].chars().next_back();
                            if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                                continue;
                            }
                            let start = at + call.len();
                            let end = start + source[start..].find('"').unwrap();
                            keys.insert(source[start..end].to_string());
                        }
                    }
                }
            }
        }
        let mut keys = std::collections::BTreeSet::new();
        visit(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut keys);
        keys
    }

    #[test]
    fn every_key_used_in_the_code_has_a_text_and_no_text_is_unused() {
        let used = keys_used_in_code();
        assert!(used.len() > 100, "found only {} keys; did the scan break?", used.len());
        let known = |key: &str| EN.binary_search_by_key(&key, |&(k, _)| k).is_ok();
        let missing: Vec<_> = used.iter().filter(|k| !known(k)).collect();
        assert!(missing.is_empty(), "keys used in the code but not in locales/: {missing:?}");
        // piece names are looked up by the names in the garments' pattern.json
        let pieces: std::collections::BTreeSet<String> = crate::garments::GARMENTS
            .iter()
            .flat_map(|g| {
                let pattern: serde_json::Value = serde_json::from_str(g.pattern_json).unwrap();
                let names: Vec<String> =
                    pattern["pieces"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect();
                names
            })
            .map(|name| crate::template::pattern::piece_key(&name))
            .collect();
        let unused: Vec<_> = EN
            .iter()
            .map(|&(k, _)| k)
            .filter(|k| !used.contains(*k) && !k.strip_prefix("piece.").is_some_and(|p| pieces.contains(p)))
            .collect();
        assert!(unused.is_empty(), "texts in locales/ that nothing uses: {unused:?}");
        let unnamed: Vec<_> = pieces.iter().filter(|p| !known(&format!("piece.{p}"))).collect();
        assert!(unnamed.is_empty(), "garment pieces without a name in locales/: {unnamed:?}");
    }

    #[test]
    fn holes_are_filled_in_order_and_missing_keys_show_themselves() {
        assert_eq!(tr_args("error.unreadable_image", &[&"logo.png"]), "logo.png could not be read");
        assert_eq!(tr("no.such.key"), "no.such.key");
    }

    #[test]
    fn language_codes_parse() {
        assert_eq!(Language::from_code("de_AT.UTF-8"), Some(Language::German));
        assert_eq!(Language::from_code("en"), Some(Language::English));
        assert_eq!(Language::from_code("fr"), None);
        assert_eq!(Language::from_code(""), None);
    }
}
