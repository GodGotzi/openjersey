//! The tools AI assistants use to edit the open design. One definition for
//! both the MCP server (Codex, Claude Code) and the in-app chat (Claude and
//! OpenAI APIs). Commands run on the UI thread against the workspace; each
//! change is one undo step.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::document::{map, Asset, Object, ObjectId, ObjectKind, Rgb, StrokeStyle, Xf};
use crate::pick3d::GarmentMesh;
use crate::render::{geom, Mode};
use crate::template::pattern::{Panel, PieceId, Pt};
use crate::template::regions::{RegionId, Side};
use crate::workspace::{with_world_pos, Workspace};

pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

pub struct Output {
    pub text: String,
    /// PNG images (previews)
    pub images: Vec<Vec<u8>>,
}

const HEM_Z: f32 = 0.86;

pub const INSTRUCTIONS: &str = "You are editing a volleyball jersey + shorts design in OpenJersey. \
The design is a print template (sheet) with all garment pieces laid out side by side; objects \
(text, graphics, shapes, watermark patterns, logo placeholders, groups) sit on top of the pieces' \
base colours. Coordinates are millimetres on the sheet (x right, y down) unless you give a garment \
position: {\"panel\": \"front\"|\"back\", \"across_mm\": <from the centre line, positive = wearer's left>, \
\"above_hem_mm\": <height above the jersey hem>}. Call get_overview first, then list_objects. \
Prefer editing existing objects over adding duplicates, keep sponsor placeholders where they are \
unless asked, and call render_preview to check your result visually.";

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
                with(pos_props(), json!({
                    "text": {"type": "string"},
                    "cap_height_mm": {"type": "number", "description": "height of capital letters"},
                    "max_width_mm": {"type": "number", "description": "condense wider text to this width (long names)"},
                    "color": color, "rotation_deg": {"type": "number"}, "name": {"type": "string"}, "clip": clip
                })),
                &["text", "cap_height_mm"],
            ),
        },
        ToolSpec {
            name: "add_placeholder",
            description: "Add a logo placeholder box (shown in the editor, left out of the print file).",
            schema: schema(
                with(pos_props(), json!({
                    "label": {"type": "string"}, "width_mm": {"type": "number"}, "height_mm": {"type": "number"},
                    "rotation_deg": {"type": "number"}
                })),
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
                with(pos_props(), json!({
                    "path": {"type": "string"}, "width_mm": {"type": "number"}, "tint": color,
                    "replace_placeholder_id": {"type": "integer"}
                })),
                &["path"],
            ),
        },
        ToolSpec {
            name: "update_object",
            description: "Change an object: any of name, position (x_mm/y_mm or panel/across_mm/above_hem_mm), dx_mm/dy_mm, rotation_deg, scale, opacity, visible, locked, text, cap_height_mm, max_width_mm (0 = off), color, fill, stroke, tint, label, width_mm, height_mm, clip.",
            schema: schema(
                with(pos_props(), json!({
                    "id": {"type": "integer"}, "name": {"type": "string"},
                    "dx_mm": {"type": "number"}, "dy_mm": {"type": "number"},
                    "rotation_deg": {"type": "number"}, "scale": {"type": "number"}, "opacity": {"type": "number"},
                    "visible": {"type": "boolean"}, "locked": {"type": "boolean"},
                    "text": {"type": "string"}, "cap_height_mm": {"type": "number"}, "max_width_mm": {"type": "number"},
                    "color": color, "fill": color, "stroke": color, "tint": color,
                    "label": {"type": "string"}, "width_mm": {"type": "number"}, "height_mm": {"type": "number"},
                    "clip": clip
                })),
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
            schema: schema(json!({"id": {"type": "integer"}, "to": {"type": "string", "enum": ["up", "down", "top", "bottom"]}}), &["id", "to"]),
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
            description: "Set a base colour: target = primary | accent | trim (roles) or a region: front, back, side_l, side_r, sleeve_l, sleeve_r, collar, leg_l, leg_r, cuff_l, cuff_r, leg_stripe_l, leg_stripe_r, leg_hem_l, leg_hem_r.",
            schema: schema(json!({"target": {"type": "string"}, "color": color}), &["target", "color"]),
        },
        ToolSpec {
            name: "set_garments",
            description: "Choose which garments the design includes and the plain colour of their inside on the model. A left-out garment is hidden on the model and left off the print sheet (e.g. a warm-up shirt without shorts).",
            schema: schema(json!({"shirt": {"type": "boolean"}, "shorts": {"type": "boolean"}, "inside_color": color}), &[]),
        },
        ToolSpec {
            name: "load_preset",
            description: "Replace the design with a preset: 'ninesquared' (example) or 'vbc' (VBC Stainach warm-up).",
            schema: schema(json!({"name": {"type": "string"}}), &["name"]),
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
            description: "PNG previews: the print sheet and/or the garment from front and back. view = sheet | garment | both (default both).",
            schema: schema(json!({"view": {"type": "string", "enum": ["sheet", "garment", "both"]}}), &[]),
        },
    ]
}

fn piece_name(id: PieceId) -> &'static str {
    match id {
        PieceId::Front => "front",
        PieceId::Back => "back",
        PieceId::SideR => "side_r",
        PieceId::SideL => "side_l",
        PieceId::SleeveR => "sleeve_r",
        PieceId::SleeveL => "sleeve_l",
        PieceId::Collar => "collar",
        PieceId::LegR => "leg_r",
        PieceId::LegL => "leg_l",
    }
}

fn piece_by_name(name: &str) -> Option<PieceId> {
    [
        PieceId::Front,
        PieceId::Back,
        PieceId::SideR,
        PieceId::SideL,
        PieceId::SleeveR,
        PieceId::SleeveL,
        PieceId::Collar,
        PieceId::LegR,
        PieceId::LegL,
    ]
    .into_iter()
    .find(|p| piece_name(*p) == name)
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
    args.get("clip")?.as_array().map(|a| a.iter().filter_map(|v| v.as_str().and_then(piece_by_name)).collect())
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
                    json!({"id": piece_name(p.id), "label": p.label,
                        "x_mm": p.rect[0] / mm, "y_mm": p.rect[1] / mm, "width_mm": p.rect[2] / mm, "height_mm": p.rect[3] / mm})
                })
                .collect();
            let f = ws.pattern.place(Panel::Front, [0.0, HEM_Z]);
            let b = ws.pattern.place(Panel::Back, [0.0, HEM_Z]);
            text(json!({
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
            let list: Vec<Value> = ws.doc.objects.iter().rev().map(|o| describe(ws, o)).collect();
            text(json!({"objects": list}))
        }
        "add_text" => {
            let content = args.get("text").and_then(Value::as_str).ok_or("missing text")?.to_string();
            let cap = num(args, "cap_height_mm").ok_or("missing cap_height_mm")?;
            let at = position(ws, args).ok_or("give x_mm/y_mm or panel/across_mm/above_hem_mm")?;
            let rot = num(args, "rotation_deg").unwrap_or(0.0).to_radians();
            let name = args.get("name").and_then(Value::as_str).unwrap_or(&content).to_string();
            let mut obj = Object::new(name, Xf { pos: at, rotation: rot, scale: [1.0, 1.0] }, ObjectKind::Text {
                text: content,
                size: cap * mm / 0.73,
                color: parse_color(args.get("color"))?,
                fatten: 0.04,
                italic: 0.0,
                max_width: num(args, "max_width_mm").map(|w| w * mm),
            });
            obj.clip = clip_arg(args).unwrap_or_default();
            let id = ws.add(None, obj);
            text(json!({"id": id}))
        }
        "add_placeholder" => {
            let label = args.get("label").and_then(Value::as_str).ok_or("missing label")?.to_string();
            let (w, h) = (num(args, "width_mm").ok_or("missing width_mm")?, num(args, "height_mm").ok_or("missing height_mm")?);
            let at = position(ws, args).ok_or("give x_mm/y_mm or panel/across_mm/above_hem_mm")?;
            let rot = num(args, "rotation_deg").unwrap_or(0.0).to_radians();
            let id = ws.add(None, Object::new(format!("Sponsor {label}"), Xf { pos: at, rotation: rot, scale: [1.0, 1.0] }, ObjectKind::Placeholder {
                label,
                size: [w * mm, h * mm],
            }));
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
            let fill = parse_color(args.get("fill"))?;
            let stroke = parse_color(args.get("stroke"))?
                .map(|color| StrokeStyle { color, width: num(args, "stroke_width_mm").unwrap_or(3.0) * mm });
            let name = args.get("name").and_then(Value::as_str).unwrap_or("Form");
            let obj = Object::shape(name, pts, closed, fill.or(if stroke.is_none() { Some([255, 255, 255]) } else { None }), stroke)
                .with_clip(&clip_arg(args).unwrap_or_default());
            let id = ws.add(None, obj);
            text(json!({"id": id}))
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
            let ids: Vec<ObjectId> = args.get("ids").and_then(Value::as_array).ok_or("missing ids")?.iter().filter_map(Value::as_u64).collect();
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
                        piece => RegionId::Piece(piece_by_name(piece).ok_or(format!("unknown target {piece}"))?),
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
            let n = args.get("name").and_then(Value::as_str).unwrap_or("vbc");
            let doc = crate::presets::by_name(&ws.pattern, n).ok_or(format!("no preset '{n}'"))?;
            ws.replace(doc);
            text(json!({"ok": true}))
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
            ws.selection = args.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
            text(json!({"ok": true}))
        }
        "render_preview" => {
            let view = args.get("view").and_then(Value::as_str).unwrap_or("both");
            let sheet = ws.raster.render(&ws.pattern, &ws.regions, &ws.doc, Mode::Display);
            let mut images = Vec::new();
            if view != "garment" {
                images.push(downscale(&sheet, 1400).encode_png().map_err(|e| e.to_string())?);
            }
            if view != "sheet" {
                images.push(mesh.preview(&sheet, 800, &ws.doc.garments).encode_png().map_err(|e| e.to_string())?);
            }
            Ok(Output { text: json!({"images": images.len()}).to_string(), images })
        }
        other => Err(format!("unknown tool {other}")),
    }
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
        let mesh = GarmentMesh::load(pattern.size());
        let mut ws = Workspace::new(crate::presets::ninesquared_example(&pattern));
        let before = ws.doc.objects.len();
        let out = execute(&mut ws, &mesh, "add_text", &json!({"text": "HI", "cap_height_mm": 30, "panel": "front", "above_hem_mm": 300})).unwrap();
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

    #[test]
    fn left_out_shorts_vanish_from_sheet_and_model() {
        use crate::template::pattern::PieceId;
        let pattern = crate::template::pattern::Pattern::load();
        let mesh = GarmentMesh::load(pattern.size());
        let mut ws = Workspace::new(crate::presets::ninesquared_example(&pattern));
        let leg = ws.pattern.piece(PieceId::LegL);
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
        let mesh = GarmentMesh::load(pattern.size());
        let mut ws = Workspace::new(crate::presets::vbc_warmup(&pattern));
        let list: Value = serde_json::from_str(&execute(&mut ws, &mesh, "list_objects", &json!({})).unwrap().text).unwrap();
        let typo = list["objects"].as_array().unwrap().iter().find(|o| o["name"] == "Rücken-Typo").unwrap();
        let name = typo["children"].as_array().unwrap().iter().find(|o| o["name"] == "Spielername").unwrap();
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
