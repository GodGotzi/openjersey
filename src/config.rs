//! Where the app keeps the little it remembers: one file per setting in
//! `~/.config/openjersey/` (or `$XDG_CONFIG_HOME/openjersey/`).

use std::path::PathBuf;

/// The path of a setting's file, e.g. `file("language")`.
pub fn file(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("openjersey").join(name))
}

/// The saved value of a setting, trimmed, if there is one.
pub fn read(name: &str) -> Option<String> {
    let text = std::fs::read_to_string(file(name)?).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Remembers a setting for the next start (best effort).
pub fn write(name: &str, value: &str) {
    if let Some(file) = file(name) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, value);
    }
}
