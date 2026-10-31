# Examples

Eleven designs, each a saved `.json` file you can open in OpenJersey (File → Open), with the renders the README shows. Nine were designed through the [MCP interface](../docs/ai-assistants.md); two are a club's own designs, prepared for publishing.

| Design | Garments | Idea |
| --- | --- | --- |
| [`alpine-night`](alpine-night.json) | Raglan T-shirt, game shorts (men) | Layered mountains and contour lines on a night-blue gradient |
| [`summit-teal`](summit-teal.json) | Zip jacket, track pants (men) | Teal gradient with a fading stack of lime chevrons |
| [`neon-court`](neon-court.json) | Sleeveless shirt, beach shorts (men) | Black with slanted neon stripes |
| [`coral-wave`](coral-wave.json) | Sports top, bikini bottom (women) | Coral-to-pink gradient with wave lines and stars |
| [`mono-chevron`](mono-chevron.json) | Crew neck shirt, game shorts (men) | Black and white, nested chevrons |
| [`lagoon`](lagoon.json) | Tank top, bikini bottom / slip (women) | Deep-water gradient, layered waves and a sun setting behind them |
| [`rose-quartz`](rose-quartz.json) | Raglan T-shirt, fitted shorts (women) | Pastel honeycomb |
| [`glacier-hood`](glacier-hood.json) | Hooded jacket, track pants (women) | Deep navy with a scatter of snow stars |
| [`sunset-retro`](sunset-retro.json) | Polo shirt, board shorts (men) | Retro sunset stripes |
| [`vbc-stainach`](vbc-stainach.json) | Raglan T-shirt (men), shirt only | VBC Stainach's warm-up shirt: mountain panel with the club's watermark |
| [`vbc-contrast-lines`](vbc-contrast-lines.json) | Raglan T-shirt, game shorts (men) | VBC Stainach's black-and-white contrast kit with marble lines |

Each design has `docs/images/<name>.png` (front and back) and `docs/images/<name>-sheet.png` (the print sheet).

## Themes of your own

[`themes/`](themes) has three themes to copy as a starting point. They are not bundled with the app: they are what you would put in your own `themes/` folder.

| Theme | Look |
| --- | --- |
| [`ocean`](themes/ocean.toml) | Dark sea blue-green with a coral accent |
| [`dusk`](themes/dusk.toml) | Dark violet with a warm amber accent |
| [`rose`](themes/rose.toml) | Light and soft, a rose accent on near-white |

```sh
mkdir -p ~/.config/openjersey/themes
cp examples/themes/ocean.toml ~/.config/openjersey/themes/
```

Restart OpenJersey and the theme shows up under **Settings → Theme → Your themes**. How the files work is in [Themes](../docs/themes.md).

## Rebuild them

```sh
cargo build --release
python3 examples/build_examples.py            # all nine, or name some: ... alpine-night neon-court
python3 docs/generate-mcp-reference.py             # the tool reference
```

The script starts `openjersey mcp`, which opens the app. To keep an OpenJersey window of your own untouched, give it its own session directory, e.g. `XDG_CACHE_HOME=/tmp/oj-cache XDG_CONFIG_HOME=/tmp/oj-config python3 examples/build_examples.py`.

## The club designs

`vbc-stainach` and `vbc-contrast-lines` are VBC Stainach's own designs, loaded and prepared with [`adapt_club_design.py`](adapt_club_design.py): the supplier's logo is taken out, other companies' logos are replaced by neutral "SPONSOR" and "LOGO" blocks, and the player's name by PLAYER. The club's crest and its line artwork stay as designed.

```sh
python3 examples/adapt_club_design.py SOURCE.json vbc-stainach --block light --patch "#7ccdf2"
```

The names in the other eight designs ("SUMMIT", "ALPINE VBC") are made up.
