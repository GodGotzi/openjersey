//! The tools AI assistants use to edit the open design, served over MCP
//! (Codex, Claude Code, …). Commands run on the UI thread against the workspace; each
//! change is one undo step.

use crate::i18n::tr;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::document::{Asset, Document, Object, ObjectId, ObjectKind, Rgb, StrokeStyle, Xf, map};
use crate::pick3d::GarmentMesh;
use crate::render::{Mode, geom};
use crate::shapes::ShapeKind;
use crate::template::pattern::{Panel, Pattern, PieceId, Pt};
use crate::template::regions::{RegionId, Side};
use crate::workspace::{Workspace, with_world_pos};

pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

/// Tools that don't change the design (MCP `readOnlyHint`).
pub fn is_read_only(tool: &str) -> bool {
    matches!(tool, "get_overview" | "list_objects" | "get_object" | "list_garments" | "list_presets" | "render_preview")
}

/// Tools that delete objects or may overwrite a file (MCP `destructiveHint`).
pub fn is_destructive(tool: &str) -> bool {
    matches!(tool, "delete_object" | "save_design" | "export_print_template")
}

/// A ready-made starting request an MCP client can offer to the user.
pub struct PromptSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// (name, description, required)
    pub arguments: &'static [(&'static str, &'static str, bool)],
    pub render: fn(&Value) -> String,
}

fn arg_or<'a>(args: &'a Value, key: &str, default: &'a str) -> &'a str {
    args.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).unwrap_or(default)
}

pub fn prompts() -> Vec<PromptSpec> {
    vec![
        PromptSpec {
            name: "design_jersey",
            description: "Design a complete team jersey from scratch.",
            arguments: &[
                ("team", "team or club name", true),
                ("colors", "colour scheme, e.g. \"navy blue with white and gold\"", false),
                ("number", "player number", false),
                ("player", "player name for the back", false),
                ("sponsors", "sponsor names or logo file paths", false),
            ],
            render: |a| {
                format!(
                    "Design a jersey in OpenJersey for the team \"{}\". Colours: {}. Player number: {}. Player name: {}. Sponsors: {}.\n\
                     Start with get_overview and list_garments, pick fitting garments with set_garment, set the base colours, then add the \
                     number, name and sponsor logos (placeholders when no file is given) in one batch. Finish with render_preview and fix \
                     anything that looks off.",
                    arg_or(a, "team", "my team"),
                    arg_or(a, "colors", "your suggestion"),
                    arg_or(a, "number", "none"),
                    arg_or(a, "player", "none"),
                    arg_or(a, "sponsors", "none"),
                )
            },
        },
        PromptSpec {
            name: "review_design",
            description: "Check the open design for problems and fix them.",
            arguments: &[],
            render: |_| {
                "Review the open OpenJersey design: call get_overview and list_objects, render_preview of the sheet and the garment, and look \
                 for text that is cut off or crosses a seam, objects that sit on the wrong piece, poor contrast between text and background, \
                 and uneven placement between front and back. Fix what you find and summarise the changes."
                    .to_string()
            },
        },
    ]
}

#[derive(Debug)]
pub struct Output {
    pub text: String,
    /// PNG images (previews)
    pub images: Vec<Vec<u8>>,
}

const HEM_Z: f32 = 0.86;

pub const INSTRUCTIONS: &str = "You are editing a sports jersey design in OpenJersey (a volleyball jersey designer). \
A design is one top and one bottom on one body (see get_overview, list_garments, set_garment) and a print \
sheet with all of their pieces laid out side by side. Objects (text, graphics, shapes, watermark patterns, \
logo placeholders, groups) sit on top of the pieces' base colours (set_piece_color). \
Units are millimetres on the sheet (x right, y down) unless you give a garment position: \
{\"panel\": \"front\"|\"back\", \"across_mm\": <from the centre line, positive = wearer's left>, \
\"above_hem_mm\": <height above the jersey hem>}. \
Workflow: call get_overview, then list_objects; make several related edits in one `batch` call (it undoes in \
one step); check the result with render_preview (use `piece` to zoom into one piece). Prefer editing existing \
objects over adding duplicates, keep sponsor placeholders where they are unless asked, and never overwrite \
files the user did not name.";

fn pos_props() -> Value {
    json!({
        "x_mm": {"type": "number", "description": "sheet x in mm"},
        "y_mm": {"type": "number", "description": "sheet y in mm"},
        "panel": {"type": "string", "enum": ["front", "back"], "description": "garment position instead of x/y"},
        "across_mm": {"type": "number", "description": "with panel: from the centre line, + = wearer's left"},
        "above_hem_mm": {"type": "number", "description": "with panel: height above the jersey hem"}
    })
}

fn with(mut base: Value, extra: Value) -> Value {
    if let (Some(b), Some(e)) = (base.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            b.insert(k.clone(), v.clone());
        }
    }
    base
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

pub fn specs() -> Vec<ToolSpec> {
    let color = json!({"type": "string", "description": "#rrggbb"});
    let clip = json!({"type": "array", "items": {"type": "string"},
        "description": "restrict to pieces: front, back, side_l, side_r, sleeve_l, sleeve_r, collar, leg_l, leg_r"});
    vec![
        ToolSpec {
            name: "get_overview",
            description: "Sheet size, every garment piece with its position and size in mm, the garment coordinate convention, base colours and a short object summary.",
            schema: schema(json!({}), &[]),
        },
        ToolSpec {
            name: "list_objects",
            description: "The object tree (top of the stack first) with id, name, type, position (mm), rotation, scale, size (mm), visibility and kind-specific fields.",
            schema: schema(json!({}), &[]),
        },
        ToolSpec {
            name: "add_text",
            description: "Add a line of text.",
            schema: schema(
                with(
                    pos_props(),
                    json!({
                        "text": {"type": "string"},
                        "cap_height_mm": {"type": "number", "description": "height of capital letters"},
                        "max_width_mm": {"type": "number", "description": "condense wider text to this width (long names)"},
                        "color": color, "rotation_deg": {"type": "number"}, "name": {"type": "string"}, "clip": clip
                    }),
                ),
                &["text", "cap_height_mm"],
            ),
        },
        ToolSpec {
            name: "add_placeholder",
            description: "Add a logo placeholder box (shown in the editor, left out of the print file).",
            schema: schema(
                with(
                    pos_props(),
                    json!({
                        "label": {"type": "string"}, "width_mm": {"type": "number"}, "height_mm": {"type": "number"},
                        "rotation_deg": {"type": "number"}
                    }),
                ),
                &["label", "width_mm", "height_mm"],
            ),
        },
        ToolSpec {
            name: "add_shape",
            description: "Add a polygon (closed) or polyline (open) from sheet points in mm.",
            schema: schema(
                json!({
                    "points_mm": {"type": "array", "items": {"type": "array", "items": {"type": "number"}}},
                    "closed": {"type": "boolean"}, "fill": color, "stroke": color,
                    "stroke_width_mm": {"type": "number"}, "name": {"type": "string"}, "clip": clip
                }),
                &["points_mm"],
            ),
        },
        ToolSpec {
            name: "import_image",
            description: "Place an image or SVG file from disk (e.g. a sponsor logo). Optionally replace a placeholder: the graphic is fitted into its box.",
            schema: schema(
                with(
                    pos_props(),
                    json!({
                        "path": {"type": "string"}, "width_mm": {"type": "number"}, "tint": color,
                        "replace_placeholder_id": {"type": "integer"}
                    }),
                ),
                &["path"],
            ),
        },
        ToolSpec {
            name: "update_object",
            description: "Change an object: any of name, position (x_mm/y_mm or panel/across_mm/above_hem_mm), dx_mm/dy_mm, rotation_deg, scale, opacity, visible, locked, text, cap_height_mm, max_width_mm (0 = off), color, fill, stroke, tint, label, width_mm, height_mm, clip.",
            schema: schema(
                with(
                    pos_props(),
                    json!({
                        "id": {"type": "integer"}, "name": {"type": "string"},
                        "dx_mm": {"type": "number"}, "dy_mm": {"type": "number"},
                        "rotation_deg": {"type": "number"}, "scale": {"type": "number"}, "opacity": {"type": "number"},
                        "visible": {"type": "boolean"}, "locked": {"type": "boolean"},
                        "text": {"type": "string"}, "cap_height_mm": {"type": "number"}, "max_width_mm": {"type": "number"},
                        "color": color, "fill": color, "stroke": color, "tint": color,
                        "label": {"type": "string"}, "width_mm": {"type": "number"}, "height_mm": {"type": "number"},
                        "clip": clip
                    }),
                ),
                &["id"],
            ),
        },
        ToolSpec {
            name: "delete_object",
            description: "Delete an object (and its children).",
            schema: schema(json!({"id": {"type": "integer"}}), &["id"]),
        },
        ToolSpec {
            name: "duplicate_object",
            description: "Copy an object, offset by dx_mm/dy_mm.",
            schema: schema(json!({"id": {"type": "integer"}, "dx_mm": {"type": "number"}, "dy_mm": {"type": "number"}}), &["id"]),
        },
        ToolSpec {
            name: "reorder_object",
            description: "Move an object up or down the stack: to = up | down | top | bottom.",
            schema: schema(
                json!({"id": {"type": "integer"}, "to": {"type": "string", "enum": ["up", "down", "top", "bottom"]}}),
                &["id", "to"],
            ),
        },
        ToolSpec {
            name: "group_objects",
            description: "Group sibling objects.",
            schema: schema(json!({"ids": {"type": "array", "items": {"type": "integer"}}, "name": {"type": "string"}}), &["ids"]),
        },
        ToolSpec {
            name: "ungroup",
            description: "Dissolve a group, keeping its children.",
            schema: schema(json!({"id": {"type": "integer"}}), &["id"]),
        },
        ToolSpec {
            name: "set_piece_color",
            description: "Set a base colour: target = primary | accent | trim (roles) or a region: a piece id from get_overview (front, back, side_l, side_r, sleeve_l, sleeve_r, collar, armhole_l, armhole_r, leg_l, leg_r — depending on the model), cuff_l, cuff_r, leg_stripe_l, leg_stripe_r, leg_hem_l, leg_hem_r.",
            schema: schema(json!({"target": {"type": "string"}, "color": color}), &["target", "color"]),
        },
        ToolSpec {
            name: "set_garments",
            description: "Choose which garments the design includes and the plain colour of their inside on the model. A left-out garment is hidden on the model and left off the print sheet (e.g. a warm-up shirt without shorts).",
            schema: schema(json!({"shirt": {"type": "boolean"}, "shorts": {"type": "boolean"}, "inside_color": color}), &[]),
        },
        ToolSpec {
            name: "load_preset",
            description: "Replace the design with a preset for the current model: 'example'.",
            schema: schema(json!({"name": {"type": "string"}}), &["name"]),
        },
        ToolSpec {
            name: "set_garment",
            description: "Put the design on another top or bottom from the library (get_overview lists them by id), or switch the body (man / women: tops and bottoms follow to the same products where they exist). Objects keep their place on the body; objects on pieces the new garment lacks (e.g. sleeves on a tank top) are hidden.",
            schema: schema(
                json!({
                    "garment": {"type": "string", "enum": crate::garments::GARMENTS.iter().map(|g| g.id).collect::<Vec<_>>()},
                    "body": {"type": "string", "enum": ["man", "women"]}
                }),
                &[],
            ),
        },
        ToolSpec {
            name: "get_object",
            description: "One object by id, with its kind-specific fields (and children for groups).",
            schema: schema(json!({"id": {"type": "integer"}}), &["id"]),
        },
        ToolSpec {
            name: "list_garments",
            description: "The garment library: id, name, kind (top | bottom), body and whether it is unisex. Filter by kind and/or body. Use the id with set_garment.",
            schema: schema(
                json!({"kind": {"type": "string", "enum": ["top", "bottom"]}, "body": {"type": "string", "enum": ["man", "women"]}}),
                &[],
            ),
        },
        ToolSpec {
            name: "list_presets",
            description: "Names of the ready-made designs load_preset accepts.",
            schema: schema(json!({}), &[]),
        },
        ToolSpec {
            name: "add_rectangle",
            description: "Add a rectangle centred on the position (x_mm/y_mm or panel/across_mm/above_hem_mm). Good for stripes, bands and blocks; use clip to keep it on certain pieces.",
            schema: schema(
                with(
                    pos_props(),
                    json!({
                        "width_mm": {"type": "number"}, "height_mm": {"type": "number"}, "rotation_deg": {"type": "number"},
                        "fill": color, "stroke": color, "stroke_width_mm": {"type": "number"}, "name": {"type": "string"}, "clip": clip
                    }),
                ),
                &["width_mm", "height_mm"],
            ),
        },
        ToolSpec {
            name: "add_ellipse",
            description: "Add an ellipse (or circle) centred on the position (x_mm/y_mm or panel/across_mm/above_hem_mm).",
            schema: schema(
                with(
                    pos_props(),
                    json!({
                        "width_mm": {"type": "number"}, "height_mm": {"type": "number"}, "rotation_deg": {"type": "number"},
                        "fill": color, "stroke": color, "stroke_width_mm": {"type": "number"}, "name": {"type": "string"}, "clip": clip
                    }),
                ),
                &["width_mm", "height_mm"],
            ),
        },
        ToolSpec {
            name: "add_preset_shape",
            description: "Add a preset shape centred on the position: rounded_rect, triangle, diamond, pentagon, hexagon, star, arrow (points right), chevron, cross, or line (width_mm is its length, rotation_deg turns it; a line needs a stroke colour). Styled like add_rectangle: fill, stroke, stroke_width_mm, name, clip.",
            schema: schema(
                with(
                    pos_props(),
                    json!({
                        "shape": {"type": "string", "enum": ShapeKind::ALL.iter().map(|k| k.id()).collect::<Vec<_>>()},
                        "width_mm": {"type": "number"}, "height_mm": {"type": "number", "description": "not needed for a line"},
                        "rotation_deg": {"type": "number"},
                        "fill": color, "stroke": color, "stroke_width_mm": {"type": "number"}, "name": {"type": "string"}, "clip": clip
                    }),
                ),
                &["shape", "width_mm"],
            ),
        },
        ToolSpec {
            name: "add_pattern",
            description: "Add a watermark: text repeated in staggered rows over the pieces in `clip` (all pieces when omitted).",
            schema: schema(
                json!({
                    "text": {"type": "string"}, "cap_height_mm": {"type": "number"}, "line_spacing_mm": {"type": "number"},
                    "angle_deg": {"type": "number"}, "color": color, "name": {"type": "string"}, "clip": clip
                }),
                &["text"],
            ),
        },
        ToolSpec {
            name: "centre_on_piece",
            description: "Centre objects horizontally on the piece they are on (on the front or back: on the centre line).",
            schema: schema(json!({"ids": {"type": "array", "items": {"type": "integer"}}}), &["ids"]),
        },
        ToolSpec {
            name: "mirror_to_other_side",
            description: "Copy objects from the front to the same place on the back, or the other way round (e.g. a number on the chest and the back).",
            schema: schema(json!({"ids": {"type": "array", "items": {"type": "integer"}}}), &["ids"]),
        },
        ToolSpec {
            name: "batch",
            description: "Run several tools in order as ONE undo step. operations = [{\"tool\": name, \"args\": {...}}, ...]. Stops at the first error; with atomic = true the whole batch is undone then. Returns every result. render_preview and the file tools are not allowed inside.",
            schema: schema(
                json!({
                    "operations": {"type": "array", "items": {"type": "object", "properties": {
                        "tool": {"type": "string"}, "args": {"type": "object"}}, "required": ["tool"]}},
                    "atomic": {"type": "boolean"}
                }),
                &["operations"],
            ),
        },
        ToolSpec {
            name: "new_design",
            description: "Start an empty design on the current garments (the previous one stays available through undo).",
            schema: schema(json!({}), &[]),
        },
        ToolSpec {
            name: "save_design",
            description: "Save the design as an OpenJersey .json file. Without `path`, saves to the file it was opened from or last saved to.",
            schema: schema(json!({"path": {"type": "string"}}), &[]),
        },
        ToolSpec {
            name: "open_design",
            description: "Open a saved .json design, replacing the current one (undo brings the previous one back).",
            schema: schema(json!({"path": {"type": "string"}}), &["path"]),
        },
        ToolSpec {
            name: "export_print_template",
            description: "Write the print sheet (placeholders left out) as a PNG file for the printer.",
            schema: schema(json!({"path": {"type": "string"}}), &["path"]),
        },
        ToolSpec { name: "undo", description: "Undo the last change.", schema: schema(json!({}), &[]) },
        ToolSpec { name: "redo", description: "Redo.", schema: schema(json!({}), &[]) },
        ToolSpec {
            name: "select",
            description: "Select objects in the editor (so the user sees what you mean).",
            schema: schema(json!({"ids": {"type": "array", "items": {"type": "integer"}}}), &["ids"]),
        },
        ToolSpec {
            name: "render_preview",
            description: "PNG previews: the print sheet and/or the garment from front and back. view = sheet | garment | both (default both). With `piece` (an id from get_overview) the sheet preview is cropped to that piece, to check details.",
            schema: schema(json!({"view": {"type": "string", "enum": ["sheet", "garment", "both"]}, "piece": {"type": "string"}}), &[]),
        },
    ]
}

fn piece_name(id: PieceId) -> &'static str {
    id.as_str()
}

/// A piece of the current top or bottom.
fn piece_by_name(pattern: &Pattern, name: &str) -> Option<PieceId> {
    pattern.pieces.iter().map(|p| p.id).find(|p| p.as_str() == name)
}

fn hex([r, g, b]: Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn parse_color(v: Option<&Value>) -> Result<Option<Rgb>, String> {
    let Some(s) = v.and_then(Value::as_str) else { return Ok(None) };
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return Err(format!("bad colour '{s}', use #rrggbb"));
    }
    let n = u32::from_str_radix(s, 16).map_err(|_| format!("bad colour '{s}'"))?;
    Ok(Some([(n >> 16) as u8, (n >> 8) as u8, n as u8]))
}

fn num(args: &Value, key: &str) -> Option<f32> {
    args.get(key).and_then(Value::as_f64).map(|v| v as f32)
}

fn id_arg(args: &Value) -> Result<ObjectId, String> {
    args.get("id").and_then(Value::as_u64).ok_or_else(|| "missing id".to_string())
}

fn clip_arg(args: &Value) -> Option<Vec<PieceId>> {
    args.get("clip")?.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(PieceId::new)).collect())
}

const POSITION_HINT: &str = "give x_mm/y_mm or panel/across_mm/above_hem_mm";

fn ids_arg(ws: &Workspace, args: &Value) -> Result<Vec<ObjectId>, String> {
    let ids: Vec<ObjectId> = args.get("ids").and_then(Value::as_array).ok_or("missing ids")?.iter().filter_map(Value::as_u64).collect();
    match ids.iter().find(|&&id| ws.doc.find(id).is_none()) {
        Some(id) => Err(format!("no object {id}")),
        None => Ok(ids),
    }
}

/// The corners of a rectangle or the points of an ellipse centred on the
/// position in `args`, in sheet pixels.
fn outline(ws: &Workspace, args: &Value, ellipse: bool) -> Result<Vec<Pt>, String> {
    let mm = ws.px_per_mm();
    let centre = position(ws, args).ok_or(POSITION_HINT)?;
    let half_w = num(args, "width_mm").ok_or("missing width_mm")? * mm / 2.0;
    let half_h = num(args, "height_mm").ok_or("missing height_mm")? * mm / 2.0;
    let (sin, cos) = num(args, "rotation_deg").unwrap_or(0.0).to_radians().sin_cos();
    let local: Vec<Pt> = if ellipse {
        const STEPS: usize = 64;
        (0..STEPS)
            .map(|i| {
                let t = i as f32 / STEPS as f32 * std::f32::consts::TAU;
                [half_w * t.cos(), half_h * t.sin()]
            })
            .collect()
    } else {
        vec![[-half_w, -half_h], [half_w, -half_h], [half_w, half_h], [-half_w, half_h]]
    };
    Ok(local.into_iter().map(|[x, y]| [centre[0] + x * cos - y * sin, centre[1] + x * sin + y * cos]).collect())
}

/// The outline of a preset shape centred on the position in `args`, rotated
/// by `rotation_deg`, in sheet pixels. A line is `width_mm` long.
fn preset_outline(ws: &Workspace, args: &Value, kind: ShapeKind) -> Result<Vec<Pt>, String> {
    let mm = ws.px_per_mm();
    let centre = position(ws, args).ok_or(POSITION_HINT)?;
    let half_w = num(args, "width_mm").ok_or("missing width_mm")? * mm / 2.0;
    let half_h = if kind.closed() { num(args, "height_mm").ok_or("missing height_mm")? * mm / 2.0 } else { 0.0 };
    let (sin, cos) = num(args, "rotation_deg").unwrap_or(0.0).to_radians().sin_cos();
    let from = [centre[0] - half_w, centre[1] - half_h];
    let to = [centre[0] + half_w, centre[1] + half_h];
    Ok(kind
        .outline(from, to)
        .into_iter()
        .map(|[x, y]| {
            let (dx, dy) = (x - centre[0], y - centre[1]);
            [centre[0] + dx * cos - dy * sin, centre[1] + dx * sin + dy * cos]
        })
        .collect())
}

/// Adds a polygon or polyline styled by fill / stroke / stroke_width_mm / name / clip.
fn add_styled_shape(ws: &mut Workspace, args: &Value, points: Vec<Pt>, closed: bool) -> Result<Output, String> {
    let mm = ws.px_per_mm();
    let fill = parse_color(args.get("fill"))?;
    let stroke =
        parse_color(args.get("stroke"))?.map(|color| StrokeStyle { color, width: num(args, "stroke_width_mm").unwrap_or(3.0) * mm });
    let name = args.get("name").and_then(Value::as_str).unwrap_or(tr("object.shape"));
    // a shape with neither fill nor stroke would be invisible
    let fill = fill.or(if stroke.is_none() { Some([255, 255, 255]) } else { None });
    let obj = Object::shape(name, points, closed, fill, stroke).with_clip(&clip_arg(args).unwrap_or_default());
    let id = ws.add(None, obj);
    Ok(Output { text: json!({"id": id}).to_string(), images: vec![] })
}

fn write_target(args: &Value, ws: &Workspace, extension: &str) -> Result<std::path::PathBuf, String> {
    let path = match args.get("path").and_then(Value::as_str) {
        Some(p) => std::path::PathBuf::from(p),
        None => ws.file.clone().ok_or("no path given and the design has not been saved yet")?,
    };
    // never write other kinds of files (this tool must not clobber arbitrary files)
    if path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() != Some(extension) {
        return Err(format!("the path must end in .{extension}"));
    }
    Ok(path)
}

/// Sheet position from x_mm/y_mm or a garment position.
fn position(ws: &Workspace, args: &Value) -> Option<Pt> {
    let mm = ws.px_per_mm();
    if let (Some(x), Some(y)) = (num(args, "x_mm"), num(args, "y_mm")) {
        return Some([x * mm, y * mm]);
    }
    let panel = match args.get("panel")?.as_str()? {
        "front" => Panel::Front,
        "back" => Panel::Back,
        _ => return None,
    };
    let across = num(args, "across_mm").unwrap_or(0.0) / 1000.0;
    let up = num(args, "above_hem_mm").unwrap_or(250.0) / 1000.0;
    Some(ws.pattern.place(panel, [across, HEM_Z + up]))
}

/// One object as the assistant sees it (groups include their children).
fn describe(ws: &Workspace, o: &Object) -> Value {
    let mm = ws.px_per_mm();
    let world = map(&ws.doc.parent_matrix(o.id), o.xf.pos);
    let size = geom::world_bounds(&ws.raster.fonts, &ws.doc, &[o.id]).map(|[x0, y0, x1, y1]| [(x1 - x0) / mm, (y1 - y0) / mm]);
    let mut v = json!({
        "id": o.id, "name": o.name, "type": o.kind.type_name(),
        "x_mm": world[0] / mm, "y_mm": world[1] / mm,
        "rotation_deg": o.xf.rotation.to_degrees(), "scale": o.xf.scale,
        "size_mm": size, "visible": o.visible, "locked": o.locked, "opacity": o.opacity,
        "clip": o.clip.iter().map(|p| piece_name(*p)).collect::<Vec<_>>(),
    });
    let extra = match &o.kind {
        ObjectKind::Text { text, size, color, max_width, .. } => {
            json!({"text": text, "cap_height_mm": size * 0.73 / mm, "color": color.map(hex),
                "max_width_mm": max_width.map(|w| w / mm)})
        }
        ObjectKind::Graphic { asset, tint, .. } => json!({"file": asset.name, "tint": tint.map(hex)}),
        ObjectKind::Shape { closed, fill, stroke, .. } => {
            json!({"closed": closed, "fill": fill.map(hex), "stroke": stroke.map(|s| hex(s.color))})
        }
        ObjectKind::Pattern { text, color, .. } => json!({"text": text, "color": hex(*color)}),
        ObjectKind::Placeholder { label, size } => {
            json!({"label": label, "box_mm": [size[0] / mm, size[1] / mm]})
        }
        ObjectKind::Group { children } => {
            json!({"children": children.iter().rev().map(|c| describe(ws, c)).collect::<Vec<_>>()})
        }
    };
    with(v.take(), extra)
}

pub fn execute(ws: &mut Workspace, mesh: &GarmentMesh, name: &str, args: &Value) -> Result<Output, String> {
    let text = |v: Value| Ok(Output { text: v.to_string(), images: vec![] });
    let mm = ws.px_per_mm();
    match name {
        "get_overview" => {
            let (w, h) = ws.pattern.size();
            let pieces: Vec<Value> = ws
                .pattern
                .pieces
                .iter()
                .map(|p| {
                    json!({"id": piece_name(p.id), "label": p.label, "name": crate::template::pattern::piece_name(p),
                        "x_mm": p.rect[0] / mm, "y_mm": p.rect[1] / mm, "width_mm": p.rect[2] / mm, "height_mm": p.rect[3] / mm})
                })
                .collect();
            let f = ws.pattern.place(Panel::Front, [0.0, HEM_Z]);
            let b = ws.pattern.place(Panel::Back, [0.0, HEM_Z]);
            text(json!({
                "top": ws.pattern.top().id,
                "bottom": ws.pattern.bottom().id,
                "body": ws.body(),
                "sheet_mm": [w as f32 / mm, h as f32 / mm],
                "pieces": pieces,
                "garment": {
                    "front_centre_line_x_mm": f[0] / mm, "back_centre_line_x_mm": b[0] / mm,
                    "jersey_hem_y_mm": f[1] / mm, "jersey_neck_above_hem_mm": 600,
                    "note": "on the back the wearer's left is on the sheet's left; text is never mirrored"
                },
                "garments": {"shirt": ws.doc.garments.shirt, "shorts": ws.doc.garments.shorts, "inside_color": hex(ws.doc.inside_color())},
                "base": {"primary": hex(ws.doc.base.primary), "accent": hex(ws.doc.base.accent), "trim": hex(ws.doc.base.trim)},
                "objects": ws.doc.objects.iter().rev().map(|o| json!({"id": o.id, "name": o.name, "type": o.kind.type_name()})).collect::<Vec<_>>(),
                "selection": ws.selection,
            }))
        }
        "list_objects" => {
            let list: Vec<Value> = ws.doc.objects.iter().rev().map(|o| describe(ws, o)).collect();
            text(json!({"objects": list}))
        }
        "add_text" => {
            let content = args.get("text").and_then(Value::as_str).ok_or("missing text")?.to_string();
            let cap = num(args, "cap_height_mm").ok_or("missing cap_height_mm")?;
            let at = position(ws, args).ok_or(POSITION_HINT)?;
            let rot = num(args, "rotation_deg").unwrap_or(0.0).to_radians();
            let name = args.get("name").and_then(Value::as_str).unwrap_or(&content).to_string();
            let mut obj = Object::new(
                name,
                Xf { pos: at, rotation: rot, scale: [1.0, 1.0] },
                ObjectKind::Text {
                    text: content,
                    size: cap * mm / 0.73,
                    color: parse_color(args.get("color"))?,
                    fatten: 0.04,
                    italic: 0.0,
                    max_width: num(args, "max_width_mm").map(|w| w * mm),
                },
            );
            obj.clip = clip_arg(args).unwrap_or_default();
            let id = ws.add(None, obj);
            text(json!({"id": id}))
        }
        "add_placeholder" => {
            let label = args.get("label").and_then(Value::as_str).ok_or("missing label")?.to_string();
            let (w, h) = (num(args, "width_mm").ok_or("missing width_mm")?, num(args, "height_mm").ok_or("missing height_mm")?);
            let at = position(ws, args).ok_or(POSITION_HINT)?;
            let rot = num(args, "rotation_deg").unwrap_or(0.0).to_radians();
            let id = ws.add(
                None,
                Object::new(
                    format!("Sponsor {label}"),
                    Xf { pos: at, rotation: rot, scale: [1.0, 1.0] },
                    ObjectKind::Placeholder { label, size: [w * mm, h * mm] },
                ),
            );
            text(json!({"id": id}))
        }
        "add_shape" => {
            let pts: Vec<Pt> = args
                .get("points_mm")
                .and_then(Value::as_array)
                .ok_or("missing points_mm")?
                .iter()
                .filter_map(|p| Some([p.get(0)?.as_f64()? as f32 * mm, p.get(1)?.as_f64()? as f32 * mm]))
                .collect();
            if pts.len() < 2 {
                return Err("need at least 2 points".into());
            }
            let closed = args.get("closed").and_then(Value::as_bool).unwrap_or(true);
            add_styled_shape(ws, args, pts, closed)
        }
        "import_image" => {
            let path = args.get("path").and_then(Value::as_str).ok_or("missing path")?;
            let asset = Arc::new(Asset::from_file(std::path::Path::new(path))?);
            let [nw, nh] = asset.natural_size();
            let tint = parse_color(args.get("tint"))?;
            if let Some(pid) = args.get("replace_placeholder_id").and_then(Value::as_u64) {
                let Some(ObjectKind::Placeholder { size, .. }) = ws.doc.find(pid).map(|o| o.kind.clone()) else {
                    return Err(format!("{pid} is not a placeholder"));
                };
                let f = (size[0] / nw).min(size[1] / nh);
                ws.checkpoint();
                if let Some(obj) = ws.doc.find_mut(pid) {
                    obj.kind = ObjectKind::Graphic { asset, size: [nw * f, nh * f], tint };
                }
                ws.selection = vec![pid];
                ws.changed();
                return text(json!({"id": pid}));
            }
            let at = position(ws, args).ok_or("give x_mm/y_mm, panel/…, or replace_placeholder_id")?;
            let w = num(args, "width_mm").unwrap_or(100.0) * mm;
            let name = asset.name.clone();
            let id = ws.add(None, Object::new(name, Xf::at(at), ObjectKind::Graphic { asset, size: [w, w * nh / nw.max(1.0)], tint }));
            text(json!({"id": id}))
        }
        "update_object" => {
            let id = id_arg(args)?;
            let mut obj = ws.doc.find(id).cloned().ok_or(format!("no object {id}"))?;
            if let Some(n) = args.get("name").and_then(Value::as_str) {
                obj.name = n.into();
            }
            if let Some(at) = position(ws, args) {
                obj.xf = with_world_pos(&ws.doc, id, obj.xf, at);
            }
            if num(args, "dx_mm").is_some() || num(args, "dy_mm").is_some() {
                let world = map(&ws.doc.parent_matrix(id), obj.xf.pos);
                let target = [world[0] + num(args, "dx_mm").unwrap_or(0.0) * mm, world[1] + num(args, "dy_mm").unwrap_or(0.0) * mm];
                obj.xf = with_world_pos(&ws.doc, id, obj.xf, target);
            }
            if let Some(r) = num(args, "rotation_deg") {
                obj.xf.rotation = r.to_radians();
            }
            if let Some(s) = num(args, "scale") {
                obj.xf.scale = [s, s];
            }
            if let Some(o) = num(args, "opacity") {
                obj.opacity = o.clamp(0.0, 1.0);
            }
            if let Some(v) = args.get("visible").and_then(Value::as_bool) {
                obj.visible = v;
            }
            if let Some(v) = args.get("locked").and_then(Value::as_bool) {
                obj.locked = v;
            }
            if let Some(c) = clip_arg(args) {
                obj.clip = c;
            }
            let color = parse_color(args.get("color"))?;
            let fill = parse_color(args.get("fill"))?;
            let stroke = parse_color(args.get("stroke"))?;
            let tint = parse_color(args.get("tint"))?;
            match &mut obj.kind {
                ObjectKind::Text { text, size, color: c, max_width, .. } => {
                    if let Some(t) = args.get("text").and_then(Value::as_str) {
                        *text = t.into();
                    }
                    if let Some(w) = num(args, "max_width_mm") {
                        *max_width = (w > 0.0).then_some(w * mm);
                    }
                    if let Some(cap) = num(args, "cap_height_mm") {
                        *size = cap * mm / 0.73;
                    }
                    if color.is_some() {
                        *c = color;
                    }
                }
                ObjectKind::Graphic { size, tint: t, .. } => {
                    if let Some(w) = num(args, "width_mm") {
                        let ratio = size[1] / size[0];
                        *size = [w * mm, w * mm * ratio];
                    }
                    if tint.is_some() || color.is_some() {
                        *t = tint.or(color);
                    }
                }
                ObjectKind::Shape { fill: f, stroke: s, .. } => {
                    if let Some(c) = fill.or(color) {
                        *f = Some(c);
                    }
                    if let Some(c) = stroke {
                        *s = Some(StrokeStyle { color: c, width: s.map(|s| s.width).unwrap_or(3.0 * mm) });
                    }
                }
                ObjectKind::Pattern { text, color: c, .. } => {
                    if let Some(t) = args.get("text").and_then(Value::as_str) {
                        *text = t.into();
                    }
                    if let Some(col) = color {
                        *c = col;
                    }
                }
                ObjectKind::Placeholder { label, size } => {
                    if let Some(l) = args.get("label").and_then(Value::as_str) {
                        *label = l.into();
                    }
                    if let Some(w) = num(args, "width_mm") {
                        size[0] = w * mm;
                    }
                    if let Some(h) = num(args, "height_mm") {
                        size[1] = h * mm;
                    }
                }
                ObjectKind::Group { .. } => {}
            }
            ws.checkpoint();
            if let Some(target) = ws.doc.find_mut(id) {
                *target = obj;
            }
            ws.changed();
            text(json!({"ok": true}))
        }
        "delete_object" => {
            let id = id_arg(args)?;
            ws.checkpoint();
            ws.doc.remove(id).ok_or(format!("no object {id}"))?;
            ws.changed();
            text(json!({"ok": true}))
        }
        "duplicate_object" => {
            let id = id_arg(args)?;
            ws.checkpoint();
            let new = ws
                .doc
                .duplicate(id, [num(args, "dx_mm").unwrap_or(10.0) * mm, num(args, "dy_mm").unwrap_or(0.0) * mm])
                .ok_or(format!("no object {id}"))?;
            ws.changed();
            text(json!({"id": new}))
        }
        "reorder_object" => {
            let id = id_arg(args)?;
            let to = args.get("to").and_then(Value::as_str).unwrap_or("up");
            ws.checkpoint();
            let step = match to {
                "up" => 1,
                "down" => -1,
                "top" => 10_000,
                _ => -10_000,
            };
            ws.doc.restack(id, step);
            ws.changed();
            text(json!({"ok": true}))
        }
        "group_objects" => {
            let ids: Vec<ObjectId> =
                args.get("ids").and_then(Value::as_array).ok_or("missing ids")?.iter().filter_map(Value::as_u64).collect();
            ws.selection = ids;
            ws.group_selection();
            if let (Some(n), [id]) = (args.get("name").and_then(Value::as_str), ws.selection.as_slice())
                && let Some(g) = ws.doc.find_mut(*id)
            {
                g.name = n.into();
            }
            text(json!({"group": ws.selection.first()}))
        }
        "ungroup" => {
            ws.selection = vec![id_arg(args)?];
            ws.ungroup_selection();
            text(json!({"children": ws.selection}))
        }
        "set_piece_color" => {
            let target = args.get("target").and_then(Value::as_str).ok_or("missing target")?;
            let c = parse_color(args.get("color"))?.ok_or("missing color")?;
            ws.checkpoint();
            match target {
                "primary" => ws.doc.base.primary = c,
                "accent" => ws.doc.base.accent = c,
                "trim" => ws.doc.base.trim = c,
                other => {
                    let region = match other {
                        "cuff_l" => RegionId::Cuff(Side::Left),
                        "cuff_r" => RegionId::Cuff(Side::Right),
                        "leg_stripe_l" => RegionId::LegStripe(Side::Left),
                        "leg_stripe_r" => RegionId::LegStripe(Side::Right),
                        "leg_hem_l" => RegionId::LegHem(Side::Left),
                        "leg_hem_r" => RegionId::LegHem(Side::Right),
                        piece => RegionId::Piece(piece_by_name(&ws.pattern, piece).ok_or_else(|| unknown_piece(&ws.pattern, piece))?),
                    };
                    ws.doc.base.set_region(region, c);
                }
            }
            ws.changed();
            text(json!({"ok": true}))
        }
        "set_garments" => {
            let mut g = ws.doc.garments;
            if let Some(v) = args.get("shirt").and_then(Value::as_bool) {
                g.shirt = v;
            }
            if let Some(v) = args.get("shorts").and_then(Value::as_bool) {
                g.shorts = v;
            }
            if let Some(c) = parse_color(args.get("inside_color"))? {
                g.inside = Some(c);
            }
            ws.checkpoint();
            ws.doc.garments = g;
            ws.changed();
            text(json!({"shirt": g.shirt, "shorts": g.shorts, "inside_color": hex(ws.doc.inside_color())}))
        }
        "load_preset" => {
            let n = args.get("name").and_then(Value::as_str).unwrap_or("example");
            let doc = crate::presets::by_name(&ws.pattern, n).ok_or(format!("no preset '{n}'"))?;
            ws.replace(doc);
            text(json!({"ok": true}))
        }
        "set_garment" => {
            if let Some(id) = args.get("garment").and_then(Value::as_str) {
                ws.set_garment(crate::garments::find(id).ok_or(format!("no garment '{id}'"))?);
            }
            if let Some(body) = args.get("body") {
                ws.set_body(serde_json::from_value(body.clone()).map_err(|e| e.to_string())?);
            }
            text(json!({"top": ws.pattern.top().id, "bottom": ws.pattern.bottom().id, "body": ws.body()}))
        }
        "undo" => {
            ws.undo();
            text(json!({"ok": true}))
        }
        "redo" => {
            ws.redo();
            text(json!({"ok": true}))
        }
        "select" => {
            ws.selection =
                args.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
            text(json!({"ok": true}))
        }
        "get_object" => {
            let id = id_arg(args)?;
            let obj = ws.doc.find(id).ok_or(format!("no object {id}"))?;
            text(describe(ws, obj))
        }
        "list_garments" => {
            let kind = args.get("kind").and_then(Value::as_str);
            let body = args.get("body").and_then(Value::as_str);
            let list: Vec<Value> = crate::garments::GARMENTS
                .iter()
                .filter(|g| kind.is_none_or(|k| k == if g.kind == crate::garments::Kind::Top { "top" } else { "bottom" }))
                .filter(|g| {
                    body.is_none_or(|b| serde_json::to_value(g.body).ok().and_then(|v| v.as_str().map(|s| s == b)).unwrap_or(false))
                })
                .map(|g| {
                    json!({"id": g.id, "name": g.name, "body": g.body, "unisex": g.unisex,
                        "kind": if g.kind == crate::garments::Kind::Top { "top" } else { "bottom" }})
                })
                .collect();
            text(json!({"garments": list}))
        }
        "list_presets" => text(json!({"presets": crate::presets::PRESETS.iter().map(|(n, _)| *n).collect::<Vec<_>>()})),
        "add_rectangle" => {
            let points = outline(ws, args, false)?;
            add_styled_shape(ws, args, points, true)
        }
        "add_ellipse" => {
            let points = outline(ws, args, true)?;
            add_styled_shape(ws, args, points, true)
        }
        "add_preset_shape" => {
            let id = args.get("shape").and_then(Value::as_str).ok_or("missing shape")?;
            let kind = ShapeKind::from_id(id).ok_or_else(|| {
                let ids: Vec<&str> = ShapeKind::ALL.iter().map(|k| k.id()).collect();
                format!("unknown shape '{id}'; use one of: {}", ids.join(", "))
            })?;
            if !kind.closed() && args.get("stroke").is_none() {
                return Err("a line needs a stroke colour".into());
            }
            let points = preset_outline(ws, args, kind)?;
            add_styled_shape(ws, args, points, kind.closed())
        }
        "add_pattern" => {
            let content = args.get("text").and_then(Value::as_str).ok_or("missing text")?.to_string();
            let cap = num(args, "cap_height_mm").unwrap_or(12.0);
            let name = args.get("name").and_then(Value::as_str).unwrap_or(tr("object.pattern")).to_string();
            let mut obj = Object::new(
                name,
                Xf::at([0.0, 0.0]),
                ObjectKind::Pattern {
                    text: content,
                    size: cap * mm / 0.73,
                    line_spacing: num(args, "line_spacing_mm").unwrap_or(cap * 3.0) * mm,
                    angle: num(args, "angle_deg").unwrap_or(-20.0).to_radians(),
                    italic: 0.0,
                    color: parse_color(args.get("color"))?.unwrap_or([200, 200, 200]),
                    area: Vec::new(),
                },
            );
            obj.clip = clip_arg(args).unwrap_or_default();
            let id = ws.add(None, obj);
            text(json!({"id": id}))
        }
        "centre_on_piece" => {
            ws.selection = ids_arg(ws, args)?;
            ws.centre_selection_on_piece();
            text(json!({"ok": true}))
        }
        "mirror_to_other_side" => {
            ws.selection = ids_arg(ws, args)?;
            ws.mirror_selection_to_other_side();
            text(json!({"new_ids": ws.selection}))
        }
        "batch" => {
            let operations = args.get("operations").and_then(Value::as_array).ok_or("missing operations")?;
            let atomic = args.get("atomic").and_then(Value::as_bool).unwrap_or(false);
            let depth = ws.history.depth();
            let mut results: Vec<Value> = Vec::new();
            let mut failure: Option<(usize, String, String)> = None;
            ws.grouped(|ws| {
                for (i, op) in operations.iter().enumerate() {
                    let tool = op.get("tool").and_then(Value::as_str).unwrap_or_default();
                    let op_args = op.get("args").cloned().unwrap_or(json!({}));
                    let outcome = if matches!(tool, "batch" | "render_preview" | "save_design" | "open_design" | "export_print_template") {
                        Err(format!("{tool} is not allowed in a batch"))
                    } else {
                        execute(ws, mesh, tool, &op_args)
                    };
                    match outcome {
                        Ok(out) => results.push(serde_json::from_str(&out.text).unwrap_or(Value::String(out.text))),
                        Err(e) => {
                            failure = Some((i, tool.to_string(), e));
                            break;
                        }
                    }
                }
            });
            match failure {
                None => text(json!({"results": results})),
                Some((i, tool, e)) => {
                    let rolled_back = atomic && ws.history.depth() > depth;
                    if rolled_back {
                        ws.undo();
                    }
                    Err(format!(
                        "operation {i} ({tool}) failed: {e}. Completed before it: {}. {}",
                        Value::Array(results),
                        if atomic { "Nothing was changed (atomic)." } else { "Earlier operations were kept." }
                    ))
                }
            }
        }
        "new_design" => {
            let doc = Document::for_sheet(&ws.pattern, ws.doc.base.clone());
            ws.replace(doc);
            ws.file = None;
            text(json!({"ok": true}))
        }
        "save_design" => {
            let path = write_target(args, ws, "json")?;
            let bytes = crate::document::io::to_json(&ws.doc)?;
            std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            ws.status = format!("{}: {}", tr("status.saved"), path.file_name().unwrap_or_default().to_string_lossy());
            ws.file = Some(path.clone());
            text(json!({"saved": path}))
        }
        "open_design" => {
            let path = std::path::PathBuf::from(args.get("path").and_then(Value::as_str).ok_or("missing path")?);
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let doc = crate::document::io::from_json(&bytes)?;
            ws.replace(doc);
            ws.status = format!("{}: {}", tr("status.opened"), path.file_name().unwrap_or_default().to_string_lossy());
            ws.file = Some(path);
            text(json!({"ok": true, "top": ws.pattern.top().id, "bottom": ws.pattern.bottom().id}))
        }
        "export_print_template" => {
            let path = write_target(args, ws, "png")?;
            let pm = ws.raster.render(&ws.pattern, &ws.regions, &ws.doc, Mode::Print);
            pm.save_png(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            text(json!({"exported": path}))
        }
        "render_preview" => {
            let view = args.get("view").and_then(Value::as_str).unwrap_or("both");
            let sheet = ws.raster.render(&ws.pattern, &ws.regions, &ws.doc, Mode::Display);
            let mut images = Vec::new();
            if view != "garment" {
                let shown = match args.get("piece").and_then(Value::as_str) {
                    Some(id) => {
                        let piece = piece_by_name(&ws.pattern, id).ok_or_else(|| unknown_piece(&ws.pattern, id))?;
                        crop_to_piece(&sheet, ws.pattern.piece(piece).rect)
                    }
                    None => downscale(&sheet, 1400),
                };
                images.push(shown.encode_png().map_err(|e| e.to_string())?);
            }
            if view != "sheet" {
                images.push(mesh.preview(&sheet, 800, &ws.doc.garments).encode_png().map_err(|e| e.to_string())?);
            }
            Ok(Output { text: json!({"images": images.len()}).to_string(), images })
        }
        other => Err(format!("unknown tool {other}")),
    }
}

fn unknown_piece(pattern: &Pattern, name: &str) -> String {
    let ids: Vec<&str> = pattern.pieces.iter().map(|p| p.id.as_str()).collect();
    format!("unknown piece '{name}'; this model has: {}", ids.join(", "))
}

/// The sheet around a piece rectangle (x, y, w, h), with a little margin.
fn crop_to_piece(sheet: &tiny_skia::Pixmap, [x, y, w, h]: [f32; 4]) -> tiny_skia::Pixmap {
    const MARGIN: f32 = 24.0;
    let x0 = (x - MARGIN).max(0.0);
    let y0 = (y - MARGIN).max(0.0);
    let x1 = (x + w + MARGIN).min(sheet.width() as f32);
    let y1 = (y + h + MARGIN).min(sheet.height() as f32);
    tiny_skia::IntRect::from_xywh(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32)
        .and_then(|rect| sheet.clone_rect(rect))
        .unwrap_or_else(|| sheet.clone())
}

fn downscale(pm: &tiny_skia::Pixmap, max_w: u32) -> tiny_skia::Pixmap {
    if pm.width() <= max_w {
        return pm.clone();
    }
    let f = max_w as f32 / pm.width() as f32;
    let mut out = tiny_skia::Pixmap::new(max_w, (pm.height() as f32 * f) as u32).expect("non-zero");
    let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
    out.draw_pixmap(0, 0, pm.as_ref(), &paint, tiny_skia::Transform::from_scale(f, f), None);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_list_update_undo() {
        let pattern = crate::template::pattern::Pattern::load();
        let mesh = GarmentMesh::load(&pattern);
        let mut ws = Workspace::new(crate::presets::example(&pattern));
        let before = ws.doc.objects.len();
        let out = execute(&mut ws, &mesh, "add_text", &json!({"text": "HI", "cap_height_mm": 30, "panel": "front", "above_hem_mm": 300}))
            .unwrap();
        let id = serde_json::from_str::<Value>(&out.text).unwrap()["id"].as_u64().unwrap();
        assert_eq!(ws.doc.objects.len(), before + 1);
        execute(&mut ws, &mesh, "update_object", &json!({"id": id, "text": "HELLO", "dx_mm": 10})).unwrap();
        let listed = execute(&mut ws, &mesh, "list_objects", &json!({})).unwrap().text;
        assert!(listed.contains("HELLO"));
        execute(&mut ws, &mesh, "undo", &json!({})).unwrap();
        assert!(!execute(&mut ws, &mesh, "list_objects", &json!({})).unwrap().text.contains("HELLO"));
        let preview = execute(&mut ws, &mesh, "render_preview", &json!({"view": "sheet"})).unwrap();
        assert_eq!(preview.images.len(), 1);
        assert!(execute(&mut ws, &mesh, "set_piece_color", &json!({"target": "sleeve_l", "color": "#00ff00"})).is_ok());
    }

    fn setup() -> (Workspace, GarmentMesh) {
        let pattern = crate::template::pattern::Pattern::load();
        let mesh = GarmentMesh::load(&pattern);
        (Workspace::new(crate::presets::example(&pattern)), mesh)
    }

    #[test]
    fn a_batch_undoes_in_one_step() {
        let (mut ws, mesh) = setup();
        let before = ws.doc.objects.len();
        let ops = json!({"operations": [
            {"tool": "add_rectangle", "args": {"width_mm": 100, "height_mm": 20, "panel": "front", "above_hem_mm": 200, "fill": "#ff0000"}},
            {"tool": "add_ellipse", "args": {"width_mm": 30, "height_mm": 30, "panel": "back", "above_hem_mm": 300}},
            {"tool": "add_pattern", "args": {"text": "TEAM", "clip": ["front"]}},
        ]});
        let out = execute(&mut ws, &mesh, "batch", &ops).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&out.text).unwrap()["results"].as_array().unwrap().len(), 3);
        assert_eq!(ws.doc.objects.len(), before + 3);
        execute(&mut ws, &mesh, "undo", &json!({})).unwrap();
        assert_eq!(ws.doc.objects.len(), before);
    }

    #[test]
    fn an_atomic_batch_rolls_back_on_error() {
        let (mut ws, mesh) = setup();
        let before = ws.doc.objects.len();
        let ops = json!({"atomic": true, "operations": [
            {"tool": "add_rectangle", "args": {"width_mm": 50, "height_mm": 50, "panel": "front"}},
            {"tool": "delete_object", "args": {"id": 999_999}},
        ]});
        let err = execute(&mut ws, &mesh, "batch", &ops).unwrap_err();
        assert!(err.contains("operation 1") && err.contains("Nothing was changed"), "{err}");
        assert_eq!(ws.doc.objects.len(), before);
        // without atomic the earlier steps stay
        let ops = json!({"operations": [
            {"tool": "add_rectangle", "args": {"width_mm": 50, "height_mm": 50, "panel": "front"}},
            {"tool": "no_such_tool"},
        ]});
        assert!(execute(&mut ws, &mesh, "batch", &ops).is_err());
        assert_eq!(ws.doc.objects.len(), before + 1);
    }

    #[test]
    fn shapes_land_where_asked() {
        let (mut ws, mesh) = setup();
        let out = execute(&mut ws, &mesh, "add_rectangle", &json!({"width_mm": 100, "height_mm": 40, "x_mm": 200, "y_mm": 300})).unwrap();
        let id = serde_json::from_str::<Value>(&out.text).unwrap()["id"].as_u64().unwrap();
        let obj = execute(&mut ws, &mesh, "get_object", &json!({"id": id})).unwrap();
        let v: Value = serde_json::from_str(&obj.text).unwrap();
        assert!((v["size_mm"][0].as_f64().unwrap() - 100.0).abs() < 1.0 && (v["size_mm"][1].as_f64().unwrap() - 40.0).abs() < 1.0, "{v}");
        assert!(execute(&mut ws, &mesh, "add_rectangle", &json!({"width_mm": 10, "height_mm": 10})).is_err());
    }

    #[test]
    fn preset_shapes_are_added_in_the_right_size() {
        let (mut ws, mesh) = setup();
        let out = execute(
            &mut ws,
            &mesh,
            "add_preset_shape",
            &json!({"shape": "star", "width_mm": 80, "height_mm": 60, "x_mm": 200, "y_mm": 300}),
        )
        .unwrap();
        let id = serde_json::from_str::<Value>(&out.text).unwrap()["id"].as_u64().unwrap();
        let v: Value = serde_json::from_str(&execute(&mut ws, &mesh, "get_object", &json!({"id": id})).unwrap().text).unwrap();
        assert!((v["size_mm"][0].as_f64().unwrap() - 80.0).abs() < 1.0 && (v["size_mm"][1].as_f64().unwrap() - 60.0).abs() < 1.0, "{v}");
        assert_eq!(v["closed"], true);
        // a line is open, as long as asked, and needs a stroke
        let line = json!({"shape": "line", "width_mm": 100, "x_mm": 200, "y_mm": 300, "stroke": "#ff0000"});
        let out = execute(&mut ws, &mesh, "add_preset_shape", &line).unwrap();
        let id = serde_json::from_str::<Value>(&out.text).unwrap()["id"].as_u64().unwrap();
        let v: Value = serde_json::from_str(&execute(&mut ws, &mesh, "get_object", &json!({"id": id})).unwrap().text).unwrap();
        assert_eq!(v["closed"], false);
        assert!(execute(&mut ws, &mesh, "add_preset_shape", &json!({"shape": "line", "width_mm": 10, "x_mm": 1, "y_mm": 1})).is_err());
        let err =
            execute(&mut ws, &mesh, "add_preset_shape", &json!({"shape": "blob", "width_mm": 10, "height_mm": 10, "x_mm": 1, "y_mm": 1}))
                .unwrap_err();
        assert!(err.contains("star") && err.contains("hexagon"), "{err}");
    }

    #[test]
    fn a_piece_preview_is_cropped() {
        let (mut ws, mesh) = setup();
        let full = execute(&mut ws, &mesh, "render_preview", &json!({"view": "sheet"})).unwrap();
        let crop = execute(&mut ws, &mesh, "render_preview", &json!({"view": "sheet", "piece": "front"})).unwrap();
        assert!(crop.images[0].len() < full.images[0].len());
        let err = execute(&mut ws, &mesh, "render_preview", &json!({"piece": "nope"})).map(|_| ()).unwrap_err();
        assert!(err.contains("front"), "{err}");
    }

    #[test]
    fn files_are_only_written_with_the_right_extension() {
        let (mut ws, mesh) = setup();
        assert!(execute(&mut ws, &mesh, "save_design", &json!({"path": "/tmp/openjersey-test.txt"})).is_err());
        assert!(execute(&mut ws, &mesh, "export_print_template", &json!({"path": "/tmp/openjersey-test.json"})).is_err());
        assert!(execute(&mut ws, &mesh, "save_design", &json!({})).is_err()); // never saved, no path
    }

    #[test]
    fn garments_can_be_listed_and_filtered() {
        let (mut ws, mesh) = setup();
        let out = execute(&mut ws, &mesh, "list_garments", &json!({"kind": "bottom", "body": "women"})).unwrap();
        let v: Value = serde_json::from_str(&out.text).unwrap();
        let list = v["garments"].as_array().unwrap();
        assert!(!list.is_empty() && list.iter().all(|g| g["kind"] == "bottom" && g["body"] == "women"));
    }

    #[test]
    fn left_out_shorts_vanish_from_sheet_and_model() {
        use crate::template::pattern::PieceId;
        let pattern = crate::template::pattern::Pattern::load();
        let mesh = GarmentMesh::load(&pattern);
        let mut ws = Workspace::new(crate::presets::example(&pattern));
        let leg = ws.pattern.piece(PieceId::LEG_L);
        let mid = [leg.rect[0] + leg.rect[2] / 2.0, leg.rect[1] + leg.rect[3] / 2.0];
        let pixel = |ws: &Workspace| {
            let pm = ws.raster.render(&ws.pattern, &ws.regions, &ws.doc, Mode::Print);
            let c = pm.pixel(mid[0] as u32, mid[1] as u32).unwrap();
            [c.red(), c.green(), c.blue()]
        };
        // a ray through the left thigh
        let (origin, dir) = (myth::math::Vec3::new(0.1, 0.65, 2.0), myth::math::Vec3::new(0.0, 0.0, -1.0));
        assert_ne!(pixel(&ws), crate::render::BACKGROUND);
        assert!(mesh.pick(origin, dir, &ws.doc.garments).is_some());
        execute(&mut ws, &mesh, "set_garments", &json!({"shorts": false})).unwrap();
        assert_eq!(pixel(&ws), crate::render::BACKGROUND);
        assert!(mesh.pick(origin, dir, &ws.doc.garments).is_none());
        execute(&mut ws, &mesh, "undo", &json!({})).unwrap();
        assert!(ws.doc.garments.shorts);
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn long_names_are_condensed_to_the_maximum_width() {
        let pattern = crate::template::pattern::Pattern::load();
        let mesh = GarmentMesh::load(&pattern);
        let mut ws = Workspace::new(crate::presets::example(&pattern));
        let list: Value = serde_json::from_str(&execute(&mut ws, &mesh, "list_objects", &json!({})).unwrap().text).unwrap();
        let name = list["objects"].as_array().unwrap().iter().find(|o| o["name"] == "Name").unwrap();
        let id = name["id"].as_u64().unwrap();
        execute(&mut ws, &mesh, "update_object", &json!({"id": id, "text": "GOTTSBACHER"})).unwrap();
        let [x0, _, x1, _] = geom::world_bounds(&ws.raster.fonts, &ws.doc, &[id]).unwrap();
        let width_mm = (x1 - x0) / ws.px_per_mm();
        assert!((width_mm - 200.0).abs() < 1.0, "{width_mm} mm");
        // optional look: OJ_TEST_OUT=/path/preview.png cargo test
        if let Ok(path) = std::env::var("OJ_TEST_OUT") {
            let sheet = ws.raster.render(&ws.pattern, &ws.regions, &ws.doc, Mode::Display);
            mesh.preview(&sheet, 900, &ws.doc.garments).save_png(path).unwrap();
        }
    }
}
