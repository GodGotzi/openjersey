# Architecture

OpenJersey is one Rust binary. The window is [egui](https://github.com/emilk/egui) on top of the [Myth](https://crates.io/crates/myth-engine) engine (wgpu); the print sheet is drawn on the CPU with [tiny-skia](https://crates.io/crates/tiny-skia).

## Modules

| Path | What it holds |
| --- | --- |
| `src/main.rs` | The app: window, menus, the panels' layout, the main loop, command-line modes |
| `src/document/` | The design as data: `Document`, `Object` and its kinds (text, graphic, shape, pattern, placeholder, group), embedded graphics, save/load, undo history, moving a design between sheets, the old save format |
| `src/template/` | The print sheet: `Pattern` (the top's atlas with the bottom's below, from the garments' `pattern.json`) and the colour regions |
| `src/render/` | The CPU rasteriser that turns a document into the sheet image, hit testing and bounds, text layout |
| `src/workspace.rs` | A document with its history, selection, snapping and drag logic, independent of any UI. Both the UI and the MCP tools drive this |
| `src/editor.rs`, `src/ui/` | The 2D canvas and its tools, and the panels: layers, properties, garment browser, tool panel, colour picker, 3D toolbar, and the shared widgets (segmented control, section titles) |
| `src/scene.rs` | The 3D scene: lights, camera, the two garment models wearing the sheet as texture |
| `src/pick3d.rs` | The garment mesh on the CPU: which sheet point is under the pointer in the 3D view, and the software preview renderer |
| `src/garments.rs`, `build.rs` | The garment library, embedded at compile time; `build.rs` also compiles the translations |
| `src/shapes.rs`, `src/presets.rs` | The preset shapes and ready-made designs |
| `src/i18n.rs`, `src/theme.rs`, `src/config.rs` | Translations (see [Translations](translations.md)), colour themes, bundled and the user's own (see [Themes](themes.md)) and the little the app remembers between runs (`~/.config/openjersey/`) |
| `src/ai/` | The MCP interface: `commands.rs` (the tools), `server.rs` (the local socket the app serves them on), `mcp.rs` (the stdio bridge) |

## How a change travels

1. Something changes the design: a tool in the 2D editor, a drag in the 3D view, a menu command, or an MCP call. All of them go through `Workspace`, which takes a snapshot for undo (`checkpoint`), edits the `Document` and calls `changed()`.
2. `changed()` marks the sheet dirty. The main loop re-renders it with the rasteriser.
3. The sheet image is uploaded as the 2D editor's texture and cut into one texture per garment for the 3D models.
4. The 3D view and the 2D editor draw the new state. Picking in 3D works on the CPU mesh in `pick3d.rs`, which maps a ray to a point on the sheet, so a drag in the 3D view edits the same document.

## MCP

`openjersey mcp` runs the stdio bridge (`ai/mcp.rs`). It forwards tool calls over a local socket to the running app. The socket thread (`ai/server.rs`) hands each call to the UI thread through a channel, where `ai/commands.rs` runs it against the `Workspace`, so MCP edits and manual edits share undo and can't race.

## Garments

A garment is a folder with a mesh, a print pattern and metadata. `build.rs` embeds every folder under `garments/` and generates the table the app reads; nothing about a particular garment is in the Rust code. The Blender scripts that produce the folders are in `blender/`.

## Tests

`cargo test --release` covers the document model, undo, snapping and picking, the MCP tools (including `batch` and its rollback), the colour picker, shapes and their triangulation, the software preview's lighting and the translation files.
