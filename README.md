# OpenJersey

Volleyball jersey designer in Rust: an object tree on the left, the 3D model
in the middle (Myth engine, wgpu), the print template on the right (egui).
The template shows every garment piece laid out like a sublimation print
sheet; objects can be picked and moved on the template and
directly on the 3D model.

    cargo run --release            # or: openjersey --top "tank top" --bottom "bikini bottom" / openjersey design.json

## Install (Linux)
    ./install/install-linux.sh            # build, add to the app launcher with icon
    ./install/install-linux.sh --remove   # remove the launcher entry and icons

The launcher runs `target/release/openjersey` in this checkout, so after
`git pull && cargo build --release` it starts the new version.

## Editor
- **Layers** (left): every object (text, graphic/logo, shape, line, watermark
  pattern, logo placeholder, group) with visibility, lock, rename
  (double-click), drag-and-drop to reorder or move into groups, and a context
  menu (duplicate, delete, group, ungroup, restack, centre on piece, mirror
  front ↔ back). **Pattern pieces** at the bottom holds the pieces' base colours.
- **Properties**: position in mm, rotation, scale, opacity, "only on"
  pieces, and the settings of the object's kind (text, size, colour, tint,
  placeholder size, "Insert logo…" to turn a placeholder into a logo).
- **Editor** (right): a toolbox on the left (grouped tools; hover for names
  and shortcuts), an options bar on top (the colour picker, quick swatches and the options
  of the current tool; the picker has an HSV square, a hex field, the design's
  own colours, recent colours, a palette and a shortcut to the eyedropper), rulers in millimetres with a cursor marker, and a status
  bar with the cursor position, the selection's size and zoom controls (scroll
  to zoom, middle/right drag or Space to pan). Tools: Select/transform (V; corner handles
  scale, the round handle rotates, Shift = free/15°), Hand (H), Brush (B),
  Eraser (E), Rectangle (R), Ellipse (O), Polygon (P), Text (T), Image/SVG (I),
  Logo placeholder (L), Fill (F, pieces or objects), Eyedropper (Y).
- 3D view: click an object on the model to select it, drag to move it;
  dragging elsewhere orbits.
- Ctrl+Z/Y undo/redo, Ctrl+D duplicate, Ctrl+G / Ctrl+Shift+G group/ungroup,
  Ctrl+C/V, Ctrl+A, Del, arrow keys nudge (Shift = 10 mm), snapping of a
  dragged object's edges and centre to the centre lines, pieces and the edges
  and centres of all other objects, images and logos included, in both views
  (the Snap toggle in the 3D toolbar). Clicking a group selects it; once a
  member is selected, clicks pick its siblings, so group members can be moved
  one by one with the mouse (Ctrl+click or double-click selects a member
  directly).
- **Garment browser** (the Top and Bottom buttons in the sidebar's Model block, which also switches Men / Women): a searchable grid of
  garments with an outline of each print template, filtered by body (Men /
  Women) and tops / bottoms. A design wears one top and one bottom on one
  body; the sheet holds the top's pieces with the bottom's below. Switching
  keeps the design where it is on the body; objects on pieces the new garment
  doesn't have (sleeves on a tank top) are hidden. Top and bottom are saved
  with the design.
- **3D toolbar** (floating at the top of the 3D view): camera views (front,
  left, back, right), show/hide the top and the bottom (a hidden garment is
  also left off the print sheet), and snapping.
- **Settings** (menu bar): language and VSync. Language is English or German; it follows the system
  locale at first; `OJ_LANG=en|de` overrides it.
- **File**: presets (example), open/save
  (`.json`, graphics embedded), export the print PNG (placeholders left out).
  Without a window: `openjersey --export-template out.png [design.json |
  --preset <name>] [--top <name>] [--bottom <name>] [--garment preview.png]`.

## AI assistants (MCP)
`openjersey mcp` is a stdio [MCP](https://modelcontextprotocol.io) server that
forwards to the running app (and starts it if needed), so Codex, Claude Code
or any other MCP client edits the design you see. Each change is one undo
step in the editor.

    claude mcp add openjersey -- /path/to/openjersey mcp

    # ~/.codex/config.toml
    [mcp_servers.openjersey]
    command = "/path/to/openjersey"
    args = ["mcp"]

Tools:

| Area | Tools |
| --- | --- |
| Look | `get_overview`, `list_objects`, `get_object`, `list_garments`, `list_presets`, `render_preview` (sheet, garment, or one piece zoomed) |
| Add | `add_text`, `add_placeholder`, `add_rectangle`, `add_ellipse`, `add_shape`, `add_pattern` (watermark), `import_image` (also fills a logo placeholder) |
| Change | `update_object`, `delete_object`, `duplicate_object`, `reorder_object`, `group_objects`, `ungroup`, `centre_on_piece`, `mirror_to_other_side`, `select` |
| Colours and garments | `set_piece_color`, `set_garments`, `set_garment`, `load_preset` |
| Files | `new_design`, `open_design`, `save_design`, `export_print_template` (only `.json` / `.png` paths are written) |
| Workflow | `batch` (several tools, one undo step, optional all-or-nothing), `undo`, `redo` |

Two prompts, `design_jersey` and `review_design`, are offered to clients that
list MCP prompts. The app listens on 127.0.0.1 only; the bridge authenticates
with a token from `~/.cache/openjersey/session.json` (mode 600).

## Assets
Every garment lives in `garments/<id>/`
(`garment.json`, `model.glb`, `pattern.json`); `build.rs` picks the folders
up at compile time. They are generated by the Blender scripts in
`blender/` (see [blender/GARMENTS.md](blender/GARMENTS.md)) and only need
rebuilding when a garment changes.

1. Download Blender Studio's CC0 *Human Base Meshes* bundle from
   https://www.blender.org/download/demo-files/ and unzip it to
   `assets-src/human-base-meshes/` (gitignored).
2. Pose the mannequins:

       for body in man women; do
           blender --background assets-src/human-base-meshes/human-base-meshes-bundle-v1.4.1/human_base_meshes_bundle.blend \
               --python blender/prepare_mannequin.py -- --body $body
       done

3. Build the garments:

       blender --background --python blender/build.py -- --garment all [--render <dir>]

The glTF exporter needs numpy; the script adds `~/.local/lib/python3.14/site-packages`
to Blender's path. `studio.hdr` is Poly Haven's CC0 "Studio Small 09" (1k),
the font is DejaVu Sans Condensed Bold.

## Benchmarking
Environment variables: `OJ_VSYNC=1`, `OJ_PATH=basic` (instead of the HDR
pipeline), `OJ_AA=none|fxaa|msaa|msaa-fxaa`, `OJ_SHADOWS=0`, `OJ_VIEW=front|left|back|right` (start view), `OJ_LOG_FPS=1`
(prints the frame rate to stderr).

## License and notices
OpenJersey is free software under the [GNU GPL v3](LICENSE) (or later).

The garments are generic cuts of common sports apparel, built by the Blender
scripts in `blender/`.
Bundled third-party assets: Blender Studio *Human Base Meshes* (CC0,
mannequins used only to fit garments), Poly Haven "Studio Small 09" HDRI
(CC0), DejaVu Sans (free license).
