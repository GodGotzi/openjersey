//! UI language. English is the source language; every user-visible string is
//! written at its use site as `tr("English", "Deutsch")`, so a translation
//! sits right next to the text it translates.
//!
//! The language comes from `OJ_LANG` (`en`/`de`), else from the saved choice
//! (`~/.config/openjersey/language`), else from the system locale (`LANG`).

use std::path::PathBuf;
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

fn config_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("openjersey").join("language"))
}

/// Picks the startup language (see the module docs).
pub fn init() {
    let from_env = std::env::var("OJ_LANG").ok().and_then(|v| Language::from_code(&v));
    let saved = config_file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|v| Language::from_code(&v));
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
    if let Some(file) = config_file() {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, lang.code());
    }
}

/// The text in the current language.
pub fn tr(en: &'static str, de: &'static str) -> &'static str {
    match current() {
        Language::English => en,
        Language::German => de,
    }
}

/// For formatted text: build both versions with `format!` and pick one,
/// `pick(format!("{n} objects"), format!("{n} Objekte"))`.
pub fn pick(en: String, de: String) -> String {
    match current() {
        Language::English => en,
        Language::German => de,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_codes_parse() {
        assert_eq!(Language::from_code("de_AT.UTF-8"), Some(Language::German));
        assert_eq!(Language::from_code("en"), Some(Language::English));
        assert_eq!(Language::from_code("fr"), None);
        assert_eq!(Language::from_code(""), None);
    }
}
