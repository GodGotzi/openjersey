"""
Builds every garment into garments/<id>/ (garment.json,
model.glb, pattern.json; see GARMENTS.md).

    blender --background --python blender/build.py -- --garment <id|prefix|all>
        [--render <dir>] [--views front,back,...] [--preview-only]
    blender --background --python blender/build.py -- --combo <top id>,<bottom id> --render <dir>
    blender --background --python blender/build.py -- --list

--garment takes an id, "all", or an id prefix (e.g. hooded-jacket-).
--render also writes preview renders of each garment; --preview-only skips
writing the assets. --combo renders an already built top and bottom
together on their body.

Needs assets-src/mannequin-<body>.blend (see blender/prepare_mannequin.py).
"""

import glob
import importlib
import os
import pkgutil
import sys
import traceback

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)
# Blender's glTF exporter needs numpy; fall back to a user-site install if
# Blender's own Python doesn't bundle it.
for _site in sorted(glob.glob(os.path.expanduser("~/.local/lib/python3*/site-packages"))):
    if _site not in sys.path:
        sys.path.append(_site)

import bpy  # noqa: E402

import garments  # noqa: E402
from oj import preview  # noqa: E402
from oj.body import REPO_ROOT, Body  # noqa: E402
from oj.finish import export_glb, finish  # noqa: E402
from oj.registry import GARMENTS  # noqa: E402
from oj.sheet import write_garment_json  # noqa: E402

GARMENTS_DIR = os.path.join(REPO_ROOT, "garments")

for module in pkgutil.iter_modules(garments.__path__):
    importlib.import_module(f"garments.{module.name}")


def arg(argv: list, name: str, default=None):
    """The value after `name` in `argv`, or `default`."""
    return argv[argv.index(name) + 1] if name in argv else default


def build(info: dict, render_dir, views, preview_only: bool) -> None:
    """Builds one registered garment and writes its three files."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    body = Body(info["body"])
    sheet, objects = info["build"](body, info)
    for i, obj in enumerate(objects):
        obj.name = obj.data.name = info["kind"] if len(objects) == 1 else f"{info['kind']}.{i}"
    sheet.assign_atlas(objects)
    if not preview_only:
        for obj in objects:
            finish(obj, sheet, body, subdivide=obj.get("subdivide", True))
        out = os.path.join(GARMENTS_DIR, info["id"])
        os.makedirs(out, exist_ok=True)
        sheet.write(os.path.join(out, "pattern.json"))
        write_garment_json(os.path.join(out, "garment.json"), info)
        export_glb(objects, os.path.join(out, "model.glb"))
        print(f"Wrote {out}")
    if render_dir:
        preview.render(objects, body, render_dir, info["id"], views)


def combo(top, bottom, render_dir, views):
    """Renders two built garments together on their (posed) body."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    assert GARMENTS[top]["body"] == GARMENTS[bottom]["body"], "top and bottom are for different bodies"
    body = Body(GARMENTS[top]["body"])
    body.lower_arms()
    objects = []
    for gid in (top, bottom):
        bpy.ops.import_scene.gltf(filepath=os.path.join(GARMENTS_DIR, gid, "model.glb"))
        objects += [o for o in bpy.context.selected_objects if o.type == "MESH"]
    colours = [(0.85, 0.85, 0.87)] * (len(objects) - 1) + [(0.25, 0.35, 0.7)]
    preview.render(objects, body, render_dir, f"combo_{top}+{bottom}", views, colours)


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    render_dir = arg(argv, "--render")
    views = arg(argv, "--views")
    views = views.split(",") if views else None
    if "--list" in argv:
        for gid, info in sorted(GARMENTS.items(), key=lambda kv: kv[1]["order"]):
            print(f"{gid:48} {info['kind']:6} {info['name']}")
        return
    if "--combo" in argv:
        top, bottom = arg(argv, "--combo").split(",")
        combo(top, bottom, render_dir, views)
        return
    wanted = arg(argv, "--garment", "all")
    ids = [g for g in GARMENTS if wanted == "all" or g.startswith(wanted)]
    if not ids:
        sys.exit(f"no garment {wanted!r}; see --list")
    failed = []
    for gid in sorted(ids, key=lambda g: GARMENTS[g]["order"]):
        print(f"--- {gid}")
        try:
            build(GARMENTS[gid], render_dir, views, "--preview-only" in argv)
        except Exception:
            traceback.print_exc()
            failed.append(gid)
    if failed:
        sys.exit(f"failed: {', '.join(failed)}")


if __name__ == "__main__":
    main()
