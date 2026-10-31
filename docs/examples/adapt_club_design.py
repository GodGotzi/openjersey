#!/usr/bin/env python3
"""Turns one of a club's own saved designs into a public example, through MCP
like the other examples: load it, take out the supplier's logo, swap other
companies' logos for neutral blocks and the player's surname for PLAYER, save
it as `docs/examples/<name>.json` and render `docs/images/<name>.png`.

    python3 docs/examples/adapt_club_design.py SOURCE.json NAME [--block light|dark] [--patch #rrggbb]

`--block` is the colour of the neutral sponsor block (light on a dark shirt,
dark on a light area); `--patch` the colour of the small sleeve patches. The
source file is only read. The club's crest (vbc-logo.svg) and its own artwork
stay as designed.
"""

import argparse
import os
from pathlib import Path

from mcp_client import Mcp

HERE = Path(__file__).resolve().parent
IMAGES = HERE.parent / "images"
EXE = os.environ.get("OPENJERSEY") or str(HERE.parents[1] / "target" / "release" / "openjersey")

SUPPLIER_LOGO = "supplier-logo.svg"
OWN_ARTWORK = ("vbc-logo.svg", "linien-front.png", "linien-back.png")


def walk(objects):
    """Every object, groups included, with their children after them."""
    for obj in objects:
        yield obj
        yield from walk(obj.get("children", []))


def neutral_for(logo, is_main, block, patch):
    """What replaces another company's logo: a block with a name (the main
    sponsor), a line of text (other wide logos, which sit on the dark back),
    or a patch (roughly square logos, on the sleeves)."""
    w, h = logo["size_mm"]
    x, y = logo["x_mm"], logo["y_mm"]
    if w / h < 1.6:
        return [{"tool": "add_preset_shape", "args": {"shape": "rounded_rect", "x_mm": x, "y_mm": y, "width_mm": w, "height_mm": h, "fill": patch}}]
    if is_main:
        fill, ink = ("#f2f2f2", "#1a1a1a") if block == "light" else ("#1a1a1a", "#f2f2f2")
        return [
            {"tool": "add_rectangle", "args": {"x_mm": x, "y_mm": y, "width_mm": w, "height_mm": h, "fill": fill}},
            {"tool": "add_text", "args": {"text": "SPONSOR", "x_mm": x, "y_mm": y, "cap_height_mm": h * 0.34, "color": ink}},
        ]
    return [{"tool": "add_text", "args": {"text": "YOUR LOGO", "x_mm": x, "y_mm": y, "cap_height_mm": h * 0.3, "color": "#bcbcc2"}}]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("source")
    parser.add_argument("name")
    parser.add_argument("--block", choices=("light", "dark"), default="light")
    parser.add_argument("--patch", default="#f2f2f2")
    args = parser.parse_args()

    m = Mcp(EXE, dict(os.environ, OJ_LANG="en"))
    try:
        m.call("open_design", path=os.path.expanduser(args.source))
        listing, _ = m.call("list_objects")
        everything = list(walk(listing["objects"]))
        foreign = [o for o in everything if o["type"] == "Graphic" and o["file"] not in (SUPPLIER_LOGO, *OWN_ARTWORK)]
        wide_logos = sorted((o for o in foreign if o["size_mm"][0] / o["size_mm"][1] >= 1.6), key=lambda o: -o["size_mm"][0] * o["size_mm"][1])
        main_sponsor = wide_logos[0]["id"] if wide_logos else None
        ops = []
        for obj in everything:
            if obj["type"] == "Graphic" and obj["file"] == SUPPLIER_LOGO:
                ops.append({"tool": "delete_object", "args": {"id": obj["id"]}})
            elif obj["type"] == "Graphic" and obj["file"] not in OWN_ARTWORK:
                ops.append({"tool": "delete_object", "args": {"id": obj["id"]}})
                ops.extend(neutral_for(obj, obj["id"] == main_sponsor, args.block, args.patch))
            elif obj["type"] == "Logo placeholder":
                ops.append({"tool": "update_object", "args": {"id": obj["id"], "label": "LOGO"}})
            elif obj["type"] == "Text" and obj["name"] == "Spielername":
                ops.append({"tool": "update_object", "args": {"id": obj["id"], "text": "PLAYER", "cap_height_mm": 42, "max_width_mm": 300}})
        m.call("batch", operations=ops)
        IMAGES.mkdir(exist_ok=True)
        _, images = m.call("render_preview", view="garment")
        (IMAGES / f"{args.name}.png").write_bytes(images[0])
        _, images = m.call("render_preview", view="sheet")
        (IMAGES / f"{args.name}-sheet.png").write_bytes(images[0])
        m.call("save_design", path=str(HERE / f"{args.name}.json"))
        print(f"{args.name}: {len(ops)} changes")
    finally:
        m.close()


if __name__ == "__main__":
    main()
