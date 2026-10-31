//! What the app remembers between runs: one TOML file, `settings.toml`, in
//! `~/.config/openjersey/` (or `$XDG_CONFIG_HOME/openjersey/`), and next to it
//! a `themes/` folder for colour themes that are not bundled with the app.
//!
//! ```toml
//! language = "de"
//! theme = "graphite"
//! vsync = true
//! ```
//!
//! Older versions kept `language` and `theme` as plain one-line files; those
//! are read once and folded into `settings.toml` the next time something is saved.

use std::path::{Path, PathBuf};

use toml::{Table, Value};

const FILE: &str = "settings.toml";
const HEADER: &str = "# OpenJersey settings. Changed in the Settings window; editing by hand is fine.\n# Your own colour themes go in the themes/ folder next to this file.\n\n";
/// The file a first start creates: every setting, commented out (without one
/// the app uses its defaults, e.g. the system language).
const TEMPLATE: &str = "\
# language = \"en\"        # \"en\" or \"de\"; without it the system language is used
# theme = \"graphite\"     # a bundled theme, or the name of a file in the themes/ folder
# vsync = false
";
/// Settings that used to be plain files named after them.
const LEGACY_KEYS: [&str; 2] = ["language", "theme"];

/// The app's configuration folder.
pub fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("openjersey"))
}

/// Where colour themes that are not bundled go.
pub fn themes_dir() -> Option<PathBuf> {
    dir().map(|d| d.join("themes"))
}

/// Run once at start: makes sure the configuration folder, its `themes/`
/// folder and `settings.toml` exist, so there is something to find and edit.
/// Settings still in the older plain files are moved into `settings.toml`
/// (best effort).
pub fn init() {
    if let Some(dir) = dir() {
        let _ = init_in(&dir);
    }
}

fn init_in(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join("themes"))?;
    let file = dir.join(FILE);
    if file.exists() {
        return Ok(());
    }
    let table = load(dir);
    if table.is_empty() { std::fs::write(&file, format!("{HEADER}{TEMPLATE}")) } else { write_table(dir, &table) }
}

/// A saved text setting.
pub fn get_str(key: &str) -> Option<String> {
    get_in(&dir()?, key)?.as_str().map(str::to_string)
}

/// A saved on/off setting.
pub fn get_bool(key: &str) -> Option<bool> {
    get_in(&dir()?, key)?.as_bool()
}

/// Remembers a setting for the next start (best effort).
pub fn set(key: &str, value: impl Into<Value>) {
    if let Some(dir) = dir() {
        let _ = set_in(&dir, key, value.into());
    }
}

/// The settings in `dir`: the TOML file, plus values still in plain legacy files.
/// An unreadable file counts as empty here; `set_in` keeps it from being lost.
fn load(dir: &Path) -> Table {
    let mut table = std::fs::read_to_string(dir.join(FILE)).ok().and_then(|text| text.parse::<Table>().ok()).unwrap_or_default();
    for key in LEGACY_KEYS {
        if !table.contains_key(key)
            && let Ok(text) = std::fs::read_to_string(dir.join(key))
            && !text.trim().is_empty()
        {
            table.insert(key.to_string(), Value::String(text.trim().to_string()));
        }
    }
    table
}

fn get_in(dir: &Path, key: &str) -> Option<Value> {
    load(dir).remove(key)
}

fn set_in(dir: &Path, key: &str, value: Value) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let file = dir.join(FILE);
    // a settings file that doesn't parse (a hand edit gone wrong) is set aside, not overwritten
    if let Ok(text) = std::fs::read_to_string(&file)
        && text.parse::<Table>().is_err()
    {
        std::fs::rename(&file, dir.join(format!("{FILE}.broken")))?;
    }
    let mut table = load(dir);
    table.insert(key.to_string(), value);
    write_table(dir, &table)
}

/// Writes `settings.toml`; what was in the plain files is in it now, so they go.
fn write_table(dir: &Path, table: &Table) -> std::io::Result<()> {
    let body = toml::to_string(table).map_err(std::io::Error::other)?;
    std::fs::write(dir.join(FILE), format!("{HEADER}{body}"))?;
    for legacy in LEGACY_KEYS {
        let _ = std::fs::remove_file(dir.join(legacy));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openjersey-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn settings_are_saved_as_toml_and_read_back() {
        let dir = temp_dir("roundtrip");
        assert_eq!(get_in(&dir, "theme"), None);
        set_in(&dir, "theme", "nord".into()).unwrap();
        set_in(&dir, "vsync", true.into()).unwrap();
        set_in(&dir, "language", "de".into()).unwrap();
        assert_eq!(get_in(&dir, "theme"), Some(Value::String("nord".into())));
        assert_eq!(get_in(&dir, "vsync"), Some(Value::Boolean(true)));
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(text.starts_with("# OpenJersey settings"), "{text}");
        assert!(text.contains("theme = \"nord\"") && text.contains("vsync = true"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_one_setting_keeps_the_others_and_unknown_keys() {
        let dir = temp_dir("keeps");
        std::fs::write(dir.join(FILE), "language = \"en\"\nsomething_else = 3\n").unwrap();
        set_in(&dir, "theme", "paper".into()).unwrap();
        let table = load(&dir);
        assert_eq!(table.get("language"), Some(&Value::String("en".into())));
        assert_eq!(table.get("something_else"), Some(&Value::Integer(3)));
        assert_eq!(table.get("theme"), Some(&Value::String("paper".into())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_first_start_creates_the_folders_and_a_commented_settings_file() {
        let dir = temp_dir("first");
        let fresh = dir.join("openjersey");
        init_in(&fresh).unwrap();
        assert!(fresh.join("themes").is_dir());
        let text = std::fs::read_to_string(fresh.join(FILE)).unwrap();
        assert!(text.contains("# theme = ") && text.contains("# language = "), "{text}");
        // nothing is set yet, so the defaults apply, and a later start leaves the file alone
        assert!(load(&fresh).is_empty());
        std::fs::write(fresh.join(FILE), "theme = \"paper\"\n").unwrap();
        init_in(&fresh).unwrap();
        assert_eq!(std::fs::read_to_string(fresh.join(FILE)).unwrap(), "theme = \"paper\"\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_start_moves_the_old_plain_files_into_the_settings_file_at_once() {
        let dir = temp_dir("startmove");
        std::fs::write(dir.join("language"), "de").unwrap();
        std::fs::write(dir.join("theme"), "nord").unwrap();
        init_in(&dir).unwrap();
        assert!(!dir.join("language").exists() && !dir.join("theme").exists());
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(text.contains("language = \"de\"") && text.contains("theme = \"nord\""), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_old_plain_files_are_read_and_then_folded_into_the_toml_file() {
        let dir = temp_dir("legacy");
        std::fs::write(dir.join("language"), "de\n").unwrap();
        std::fs::write(dir.join("theme"), "graphite").unwrap();
        // read without touching anything
        assert_eq!(get_in(&dir, "language"), Some(Value::String("de".into())));
        assert!(dir.join("language").exists());
        // the next save moves them
        set_in(&dir, "vsync", false.into()).unwrap();
        assert!(!dir.join("language").exists() && !dir.join("theme").exists(), "the plain files should be gone");
        let table = load(&dir);
        assert_eq!(table.get("language"), Some(&Value::String("de".into())));
        assert_eq!(table.get("theme"), Some(&Value::String("graphite".into())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_settings_file_that_does_not_parse_is_set_aside_not_overwritten() {
        let dir = temp_dir("broken");
        std::fs::write(dir.join(FILE), "theme = = oops").unwrap();
        assert_eq!(get_in(&dir, "theme"), None);
        set_in(&dir, "theme", "nord".into()).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(format!("{FILE}.broken"))).unwrap(), "theme = = oops");
        assert_eq!(get_in(&dir, "theme"), Some(Value::String("nord".into())));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
