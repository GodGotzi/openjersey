# Getting started

## What you need

- **Rust** (edition 2024, a recent stable toolchain) from [rustup.rs](https://rustup.rs).
- A GPU with a Vulkan, Metal or DX12 driver (the 3D view uses wgpu).
- On Linux, the usual windowing libraries. The CI workflow lists the Ubuntu packages (`libwayland-dev`, `libxkbcommon-dev`, `libx11-dev`, `libxcursor-dev`, `libxi-dev`, `libxrandr-dev`, `libgl1-mesa-dev`, `libasound2-dev`, `pkg-config`).
- **Blender** is only needed to rebuild the garment models. The repository ships them ready-made (see [Garment library](garments.md)).

## Build and run

```sh
git clone https://github.com/GodGotzi/openjersey
cd openjersey
cargo run --release
```

The first build compiles the 3D engine and takes a few minutes. To start on particular garments or a saved design:

```sh
cargo run --release -- --top "tank top" --bottom "bikini bottom"
cargo run --release -- examples/alpine-night.json
```

`--top` and `--bottom` take a garment's id or any words of its name (`polo` is enough for the polo shirt; the garment for the other body is picked to match).

## Install

An install script builds the app, adds it to your launcher with its icon and sets up the [settings folder](#settings-files). It needs [Rust](https://rustup.rs).

| System | Run | Adds |
| --- | --- | --- |
| Linux | `./install/install-linux.sh` | An entry in the app launcher |
| macOS | `./install/install-macos.sh` | `OpenJersey.app` in `~/Applications` |
| Windows | `.\install\install-windows.ps1` | A Start menu shortcut |

What every script does:

- builds `target/release/openjersey`, which the launcher entry starts, so after `git pull && cargo build --release` it starts the new version;
- creates the settings folder with `settings.toml` if you have none;
- copies the three [example themes](../examples/themes) (Ocean, Dusk, Rose) into its `themes/` folder, so they show up under *Settings → Theme*. A file that is already there is kept, and `--no-themes` (`-NoThemes` on Windows) skips this;
- can be run again to update, and `--remove` (`-Remove`) takes the launcher entry away again. Your settings and themes are never touched.

If PowerShell refuses to run the Windows script, start it with `powershell -ExecutionPolicy Bypass -File .\install\install-windows.ps1`. The macOS and Windows scripts are newer and have had less testing than the Linux one. On Windows the program opens no console window; started from a terminal it prints to that terminal, though PowerShell doesn't wait for it to finish.

## Without a window

The same binary can render without opening a window:

```sh
openjersey --export-template sheet.png design.json --garment preview.png
openjersey --export-template sheet.png --preset example --top "polo shirt" --bottom "board shorts"
```

`--export-template` writes the print sheet, and `--garment` adds a front-and-back preview of the 3D model. `OJ_PREVIEW_HEIGHT` sets the preview's height in pixels (default 900).

## Environment variables

| Variable | Effect |
| --- | --- |
| `OJ_LANG=en\|de` | The interface language (otherwise the saved choice, then the system locale) |
| `OJ_THEME=graphite\|midnight\|nord\|forest\|daylight\|paper` | The colour theme (otherwise the saved choice; see [Themes](themes.md)) |
| `OJ_VSYNC=1\|0` | VSync on or off at start (also in Settings) |
| `OJ_PATH=basic` | Use the basic render path instead of the HDR pipeline |
| `OJ_AA=none\|fxaa\|msaa\|msaa-fxaa` | Anti-aliasing mode |
| `OJ_SHADOWS=0` | Turn shadows off |
| `OJ_VIEW=front\|left\|back\|right` | The 3D view's starting side |
| `OJ_LOG_FPS=1` | Print the frame rate to stderr |
| `OJ_PREVIEW_HEIGHT`, `OJ_PREVIEW_YAW` | Height and camera angle (degrees) of the software preview |

## Settings files

The app keeps what it remembers in `~/.config/openjersey/` (or `$XDG_CONFIG_HOME/openjersey/`; on Windows `%APPDATA%\openjersey\`), created at the first start. `openjersey --config-dir` prints where it is:

```
settings.toml     language, theme, VSync and Agent mode, changed in the Settings menu
themes/           your own colour themes (see Themes)
```

`settings.toml` is plain TOML and fine to edit by hand:

```toml
language = "de"        # "en" or "de"; without it the system language is used
theme = "graphite"     # a bundled theme, or the name of a file in themes/
vsync = false
agent_mode = false     # show the agent log panel
```

The environment variables below win over it for one run.

## Tests

```sh
cargo test --release
cargo clippy --release --all-targets
```

The tests cover the document model, picking and snapping, the MCP tools, the translation files and the software renderer.
