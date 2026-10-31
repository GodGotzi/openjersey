# Translations

Every text in the interface is a key such as `menu.file.save` in a file per language, `locales/<language>.toml`. Nested tables become dotted keys:

```toml
[menu.file]
save = "Save (Ctrl+S)"
```

Code asks for a text with `tr("menu.file.save")`, or `tr_args("error.unreadable_image", &[&name])` for texts with `{}` holes that are filled in order.

## What is checked

- `build.rs` reads all language files and **refuses to build** when they differ in keys or in the number of `{}` placeholders, then compiles them into sorted lookup tables (no parsing at run time).
- A test scans the source for every key it uses and fails when a key has no text, when a text is unused, or when a garment piece has no name.

Garment piece names come from the garments' `pattern.json`; their texts are keyed by the name (`Left Sleeve` is `piece.left_sleeve`), so a new piece needs an entry in every language file.

## Which language is used

`OJ_LANG` (`en` or `de`), otherwise the choice saved from the Settings menu (`language` in `~/.config/openjersey/settings.toml`), otherwise the system locale, otherwise English.

## Add a language

1. Copy `locales/en.toml` to `locales/<code>.toml` and translate the values.
2. List the code in `LANGUAGES` in `build.rs`.
3. Add it to `Language` in `src/i18n.rs` (its name, code and table).

`cargo test` then tells you if anything is missing.
