#!/usr/bin/env python3
"""Builds the example designs through OpenJersey's MCP interface, the same
way an AI assistant would: it starts `openjersey mcp` (which opens the app),
sets garments and colours, adds everything in `batch` calls, and saves each
design (`docs/examples/<name>.json`) with a preview render
(`docs/images/<name>.png`) and the print sheet (`docs/images/<name>-sheet.png`).

    python3 docs/examples/build_examples.py [design ...]

Set OPENJERSEY to the binary (default: target/release/openjersey). To keep a
running OpenJersey window of your own untouched, give this script its own
XDG_CACHE_HOME: the bridge talks to the app that owns that session file.
"""

import math
import os
import sys
from pathlib import Path

from mcp_client import Mcp

HERE = Path(__file__).resolve().parent
IMAGES = HERE.parent / "images"
EXE = os.environ.get("OPENJERSEY") or str(HERE.parents[1] / "target" / "release" / "openjersey")


class Design:
    """One design being built: garments, colours, and a list of operations
    that are sent as a single `batch` (one undo step)."""

    def __init__(self, mcp: Mcp, top: str, bottom: str):
        self.m = mcp
        mcp.call("new_design")
        mcp.call("set_garment", garment=top)
        mcp.call("set_garment", garment=bottom)
        mcp.call("set_garments", shirt=True, shorts=True)
        overview, _ = mcp.call("get_overview")
        self.pieces = {p["id"]: (p["x_mm"], p["y_mm"], p["width_mm"], p["height_mm"]) for p in overview["pieces"]}
        self.ops: list[dict] = []
        self.layers: list[str | None] = []  # the group each operation ends up in
        self._layer: str | None = None

    # ---- geometry: positions inside a piece, as fractions of its box ----
    def at(self, piece: str, u: float, v: float) -> tuple[float, float]:
        x, y, w, h = self.pieces[piece]
        return x + u * w, y + v * h

    def size(self, piece: str) -> tuple[float, float]:
        return self.pieces[piece][2:]

    # ---- colours ----
    def colours(self, **targets: str):
        for target, colour in targets.items():
            self.m.call("set_piece_color", target=target, color=colour)

    # ---- operations ----
    def layer(self, name: str):
        """`with d.layer("Mountains"):` collects what is added inside into one group."""
        design = self

        class Scope:
            def __enter__(self):
                design._layer = name

            def __exit__(self, *exc):
                design._layer = None

        return Scope()

    def add(self, tool: str, **args):
        self.ops.append({"tool": tool, "args": args})
        self.layers.append(self._layer)

    def polygon(self, points, fill=None, stroke=None, width=2.0, clip=None, name=None, opacity=None):
        args = {"points_mm": [[round(x, 1), round(y, 1)] for x, y in points], "closed": True}
        if fill:
            args["fill"] = fill
        if stroke:
            args.update(stroke=stroke, stroke_width_mm=width)
        if clip:
            args["clip"] = clip
        if name:
            args["name"] = name
        self.add("add_shape", **args)

    def line(self, points, colour, width=2.0, clip=None):
        args = {"points_mm": [[round(x, 1), round(y, 1)] for x, y in points], "closed": False, "stroke": colour, "stroke_width_mm": width}
        if clip:
            args["clip"] = clip
        self.add("add_shape", **args)

    def rect(self, piece, u, v, w_frac, h_frac, fill, name=None):
        """A rectangle placed and sized as fractions of a piece, kept on that piece."""
        x, y, w, h = self.pieces[piece]
        args = {"x_mm": round(x + u * w, 1), "y_mm": round(y + v * h, 1), "width_mm": round(w_frac * w, 1), "height_mm": round(h_frac * h, 1), "fill": fill, "clip": [piece]}
        self.add("add_rectangle", **args)

    def finish(self, name: str):
        """Sends the operations, saves the design and the renders."""
        result, _ = self.m.call("batch", operations=self.ops)
        # one group per layer, in the order the layers first appear
        members: dict[str, list[int]] = {}
        for layer, outcome in zip(self.layers, result["results"]):
            if layer and isinstance(outcome, dict) and "id" in outcome:
                members.setdefault(layer, []).append(outcome["id"])
        for layer, ids in members.items():
            if len(ids) > 1:
                self.m.call("group_objects", ids=ids, name=layer)
        IMAGES.mkdir(exist_ok=True)
        _, images = self.m.call("render_preview", view="garment")
        (IMAGES / f"{name}.png").write_bytes(images[0])
        _, images = self.m.call("render_preview", view="sheet")
        (IMAGES / f"{name}-sheet.png").write_bytes(images[0])
        self.m.call("save_design", path=str(HERE / f"{name}.json"))
        print(f"{name}: {len(self.ops)} objects")


def mix(a: str, b: str, t: float) -> str:
    """The colour t of the way from a to b (both #rrggbb)."""
    pa, pb = (int(a[i:i + 2], 16) for i in (1, 3, 5)), (int(b[i:i + 2], 16) for i in (1, 3, 5))
    return "#" + "".join(f"{round(x + (y - x) * t):02x}" for x, y in zip(pa, pb))


def gradient(d: Design, piece: str, v0: float, v1: float, c0: str, c1: str, bands=22, power=1.0):
    """A vertical colour gradient over part of a piece, built from stacked bands."""
    with d.layer(f"Gradient · {piece}"):
        x, y, w, h = d.pieces[piece]
        for i in range(bands):
            t = ((i + 0.5) / bands) ** power
            top = y + h * (v0 + (v1 - v0) * i / bands)
            band = h * (v1 - v0) / bands
            d.add("add_rectangle", x_mm=round(x + w / 2, 1), y_mm=round(top + band / 2, 1), width_mm=round(w + 16, 1), height_mm=round(band + 1.4, 1), fill=mix(c0, c1, t), clip=[piece])


def contours(d: Design, piece: str, lines: int, colour: str, width: float, top=0.0, bottom=1.0, amplitude=14.0, wavelength=0.9, phase=0.0):
    """Wavy lines across a piece, like the contour lines of a map."""
    with d.layer(f"Contour lines · {piece}"):
        x, y, w, h = d.pieces[piece]
        steps = 48
        for i in range(lines):
            base = y + h * (top + (bottom - top) * i / max(lines - 1, 1))
            swing = amplitude * (0.6 + 0.4 * math.sin(i * 1.7))
            pts = [(x - 6 + (w + 12) * k / steps, base + swing * math.sin(2 * math.pi * (k / steps) / wavelength + phase + i * 0.55)) for k in range(steps + 1)]
            d.line(pts, colour, width, clip=[piece])


def ridge(d: Design, piece: str, profile, fill: str, edge: str | None = None, edge_width=2.2):
    """A mountain silhouette along the bottom of a piece: `profile` is a list
    of (u, v) points, drawn left to right, closed down to the bottom edge."""
    with d.layer(f"Mountains · {piece}"):
        pts = [d.at(piece, u, v) for u, v in profile]
        left, right = pts[0], pts[-1]
        bottom = d.pieces[piece][1] + d.pieces[piece][3] + 10
        d.polygon([(left[0] - 8, left[1])] + pts[1:-1] + [(right[0] + 8, right[1]), (right[0] + 8, bottom), (left[0] - 8, bottom)], fill=fill, clip=[piece])
        if edge:
            d.line(pts, edge, edge_width, clip=[piece])


def diagonal_stripes(d: Design, piece: str, colours, width: float, gap: float, slope=0.8, start=0.0, v0=0.0, v1=1.0):
    """Parallel slanted stripes across part of a piece; `colours` is cycled."""
    with d.layer(f"Stripes · {piece}"):
        x, y, w, h = d.pieces[piece]
        y, h = y + h * v0 - (10 if v0 == 0 else 0), h * (v1 - v0) + (10 if v0 == 0 else 0) + (10 if v1 == 1 else 0)
        shift = (h + 20) * slope
        step = width + gap
        count = int((w + shift) / step) + 2
        for i in range(count):
            left = x - shift + start + i * step
            d.polygon([(left, y - 10), (left + width, y - 10), (left + width + shift, y + h + 10), (left + shift, y + h + 10)], fill=colours[i % len(colours)], clip=[piece])


def chevrons(d: Design, piece: str, colours, first=0.15, spacing=0.07, depth=0.16, width=4.0, flip=False):
    """Nested V lines across a piece, pointing down (or up when `flip`)."""
    with d.layer(f"Chevrons · {piece}"):
        x, y, w, h = d.pieces[piece]
        sign = -1 if flip else 1
        for i, colour in enumerate(colours):
            apex = y + h * (first + spacing * i)
            d.line([(x - 6, apex - sign * depth * h), (x + w / 2, apex), (x + w + 6, apex - sign * depth * h)], colour, width, clip=[piece])


def honeycomb(d: Design, piece: str, radius: float, colour: str, width: float, top=0.0, bottom=1.0, fade_to: str | None = None):
    """A grid of hexagon outlines across part of a piece, optionally fading to another colour towards the bottom."""
    with d.layer(f"Honeycomb · {piece}"):
        x, y, w, h = d.pieces[piece]
        cell_w, row_h = math.sqrt(3) * radius, 1.5 * radius
        row = 0
        v = y + h * top
        while v < y + h * bottom + radius:
            offset = cell_w / 2 if row % 2 else 0.0
            u = x - cell_w / 2 + offset
            while u < x + w + cell_w:
                t = (v - y - h * top) / max(h * (bottom - top), 1)
                tint = mix(colour, fade_to, min(max(t, 0), 1)) if fade_to else colour
                d.add("add_preset_shape", shape="hexagon", x_mm=round(u, 1), y_mm=round(v, 1), width_mm=round(cell_w * 0.93, 1), height_mm=round(radius * 1.86, 1), stroke=tint, stroke_width_mm=width, clip=[piece])
                u += cell_w
            v += row_h
            row += 1


def scatter(d: Design, piece: str, shape: str, count: int, colour: str, smallest: float, biggest: float, seed=7, top=0.02, bottom=0.98):
    """Shapes strewn over a piece at repeatable pseudo-random places and sizes."""
    with d.layer(f"Scattered {shape}s · {piece}"):
        x, y, w, h = d.pieces[piece]
        state = seed
        def rnd():
            nonlocal state
            state = (state * 1103515245 + 12345) % 2**31
            return state / 2**31
        for _ in range(count):
            size = smallest + (biggest - smallest) * rnd() ** 2
            d.add("add_preset_shape", shape=shape, x_mm=round(x + w * rnd(), 1), y_mm=round(y + h * (top + (bottom - top) * rnd()), 1), width_mm=round(size, 1), height_mm=round(size, 1), fill=colour, rotation_deg=round(rnd() * 70 - 35), clip=[piece])


def back_print(d: Design, club: str, number: str, number_colour: str, accent: str, number_cap=190, above=340):
    """The back: PLAYER on top, the number big, the club's name small below it."""
    d.add("add_text", text="PLAYER", panel="back", across_mm=0, above_hem_mm=above + 165, cap_height_mm=36, color=accent, max_width_mm=240, clip=["back"])
    d.add("add_text", text=number, panel="back", across_mm=0, above_hem_mm=above, cap_height_mm=number_cap, color=number_colour, clip=["back"])
    d.add("add_text", text=club, panel="back", across_mm=0, above_hem_mm=above - 0.5 * number_cap - 38, cap_height_mm=17, color=accent, max_width_mm=190, clip=["back"])


def leg_number(d: Design, number: str, colour: str, cap=62):
    x, y, w, h = d.pieces["leg_r"]
    d.add("add_text", text=number, x_mm=round(x + 0.80 * w, 1), y_mm=round(y + 0.74 * h, 1), cap_height_mm=cap, color=colour, clip=["leg_r"])


def try_colours(d: Design, **targets: str):
    """Sets colours, skipping regions this garment does not have."""
    for target, colour in targets.items():
        try:
            d.m.call("set_piece_color", target=target, color=colour)
        except RuntimeError:
            pass


def alpine_night(m: Mcp):
    d = Design(m, "raglan-tshirt-man", "game-shorts-man")
    ice, ink, slate, glow = "#7fd4ff", "#0d0f14", "#141a26", "#17405f"
    d.colours(primary=ink, accent=slate, trim=ice)
    for piece in ("front", "back"):
        gradient(d, piece, 0.45, 1.0, ink, glow, power=1.6)
    for piece, phase in (("front", 0.0), ("back", 1.3)):
        contours(d, piece, 14, "#2c3a52", 1.5, top=0.04, bottom=0.8, amplitude=16, phase=phase)
    contours(d, "sleeve_l", 8, "#2c3a52", 1.4, amplitude=10)
    contours(d, "sleeve_r", 8, "#2c3a52", 1.4, amplitude=10, phase=2.0)
    ridge(d, "front", [(0, 0.76), (0.12, 0.69), (0.22, 0.74), (0.40, 0.55), (0.52, 0.68), (0.66, 0.60), (0.84, 0.75), (1, 0.68)], "#102a40")
    ridge(d, "front", [(0, 0.84), (0.16, 0.74), (0.28, 0.82), (0.44, 0.64), (0.58, 0.78), (0.74, 0.70), (0.9, 0.82), (1, 0.78)], "#0e2236", ice, 2.0)
    ridge(d, "front", [(0, 0.95), (0.22, 0.88), (0.4, 0.93), (0.62, 0.86), (0.82, 0.92), (1, 0.89)], ink)
    ridge(d, "back", [(0, 0.86), (0.2, 0.78), (0.36, 0.84), (0.55, 0.68), (0.7, 0.82), (0.86, 0.76), (1, 0.83)], "#102a40")
    ridge(d, "back", [(0, 0.93), (0.25, 0.86), (0.45, 0.92), (0.68, 0.82), (0.85, 0.9), (1, 0.87)], "#0e2236", ice, 1.8)
    ridge(d, "back", [(0, 0.99), (0.3, 0.95), (0.55, 0.98), (0.8, 0.94), (1, 0.97)], ink)
    d.add("add_ellipse", panel="front", across_mm=-100, above_hem_mm=455, width_mm=58, height_mm=58, stroke=ice, stroke_width_mm=3.2, clip=["front"])
    d.add("add_preset_shape", shape="triangle", panel="front", across_mm=-100, above_hem_mm=452, width_mm=30, height_mm=24, fill=ice, clip=["front"])
    d.add("add_rectangle", panel="front", across_mm=0, above_hem_mm=300, width_mm=330, height_mm=64, fill=ice, clip=["front"])
    d.add("add_text", text="SPONSOR", panel="front", across_mm=0, above_hem_mm=300, cap_height_mm=30, color=ink, clip=["front"])
    d.add("add_text", text="7", panel="front", across_mm=105, above_hem_mm=455, cap_height_mm=70, color="#e8eef5", clip=["front"])
    back_print(d, "ALPINE VBC", "7", "#e8eef5", ice)
    for piece in ("leg_r", "leg_l"):
        gradient(d, piece, 0.55, 1.0, ink, glow, power=1.6)
        contours(d, piece, 12, "#2c3a52", 1.5, top=0.05, bottom=0.8, amplitude=12, phase=0.8)
    leg_number(d, "7", "#e8eef5")
    d.finish("alpine-night")



def summit_teal(m: Mcp):
    d = Design(m, "zip-jacket-man", "track-pants-man")
    lime, deep, teal, dark = "#c6f432", "#082a30", "#11818a", "#05161a"
    d.colours(primary=deep, accent=dark, trim=lime)
    try_colours(d, leg_stripe_l=lime, leg_stripe_r=lime)
    for piece in ("front", "back"):
        gradient(d, piece, 0.0, 1.0, deep, teal, power=1.3)
    fade = [mix(lime, teal, i / 7) for i in range(7)]
    chevrons(d, "front", fade, first=0.34, spacing=0.062, depth=0.15, width=7)
    chevrons(d, "back", fade, first=0.52, spacing=0.062, depth=0.15, width=7)
    for piece in ("sleeve_l", "sleeve_r"):
        for u in (0.40, 0.56):
            d.rect(piece, u, 0.5, 0.05, 1.1, lime)
    d.add("add_text", text="SUMMIT", panel="front", across_mm=-105, above_hem_mm=430, cap_height_mm=26, color=lime, clip=["front"])
    d.add("add_text", text="SPONSOR", panel="back", across_mm=0, above_hem_mm=110, cap_height_mm=24, color=mix(lime, teal, 0.35), clip=["back"])
    back_print(d, "SUMMIT TC", "12", lime, "#e9fff2", number_cap=170, above=330)
    for piece in ("leg_r", "leg_l"):
        gradient(d, piece, 0.0, 1.0, deep, teal, power=1.3)
        chevrons(d, piece, fade[:5], first=0.50, spacing=0.07, depth=0.12, width=6, flip=True)
    d.finish("summit-teal")


def neon_court(m: Mcp):
    d = Design(m, "sleeveless-shirt-man", "beach-shorts-man")
    volt, ink, white = "#e4ff3d", "#0a0a0c", "#f5f5f5"
    d.colours(primary=ink, accent=volt, trim=white)
    for piece in ("front", "back"):
        diagonal_stripes(d, piece, [volt, "#1c1d12"], 16, 34, slope=0.9, v0=0.42, v1=1.0)
        diagonal_stripes(d, piece, [white], 5, 150, slope=0.9, start=24, v0=0.42, v1=1.0)
    d.add("add_rectangle", panel="front", across_mm=20, above_hem_mm=470, width_mm=240, height_mm=58, fill=white, clip=["front"])
    d.add("add_text", text="SPONSOR", panel="front", across_mm=20, above_hem_mm=470, cap_height_mm=26, color=ink, clip=["front"])
    d.add("add_preset_shape", shape="hexagon", panel="front", across_mm=-105, above_hem_mm=395, width_mm=50, height_mm=54, stroke=volt, stroke_width_mm=3.5, clip=["front"])
    d.add("add_text", text="9", panel="front", across_mm=-105, above_hem_mm=395, cap_height_mm=24, color=volt, clip=["front"])
    back_print(d, "NEON COURT", "9", volt, white, number_cap=200, above=330)
    for piece in ("leg_r", "leg_l"):
        diagonal_stripes(d, piece, [volt, "#1c1d12"], 16, 34, slope=0.9, v0=0.0, v1=1.0)
    leg_number(d, "9", white)
    d.finish("neon-court")


def coral_wave(m: Mcp):
    d = Design(m, "sports-top-women", "bikini-bottom-women")
    coral, pink, teal, cream, navy = "#ff7a59", "#ff3d8b", "#00b8b0", "#fff1e6", "#14263d"
    d.colours(primary=coral, accent=teal, trim=cream)
    for piece in ("front", "back", "bottom_front", "bottom_back"):
        gradient(d, piece, 0.0, 1.0, coral, pink, power=1.1)
        contours(d, piece, 6, cream, 4.4, top=0.10, bottom=0.92, amplitude=26, wavelength=0.5)
    for piece in ("front", "back"):
        scatter(d, piece, "star", 8, cream, 18, 38, seed=11, top=0.05, bottom=0.55)
    for piece in ("bottom_front", "bottom_back"):
        scatter(d, piece, "star", 3, cream, 10, 18, seed=5)
    d.add("add_text", text="BEACH CLUB", panel="front", across_mm=0, above_hem_mm=300, cap_height_mm=18, color=cream, max_width_mm=190, clip=["front"])
    # the back is short on a sports top: smaller lettering than on a shirt
    d.add("add_text", text="PLAYER", panel="back", across_mm=0, above_hem_mm=395, cap_height_mm=20, color=cream, max_width_mm=150, clip=["back"])
    d.add("add_text", text="23", panel="back", across_mm=0, above_hem_mm=325, cap_height_mm=62, color=cream, clip=["back"])
    d.add("add_text", text="BEACH CLUB", panel="back", across_mm=0, above_hem_mm=270, cap_height_mm=11, color=cream, max_width_mm=130, clip=["back"])
    d.finish("coral-wave")


def mono_chevron(m: Mcp):
    d = Design(m, "crew-shirt-man", "game-shorts-man")
    ink, white, grey = "#101012", "#f2f2f2", "#8d8d94"
    d.colours(primary=ink, accent=white, trim=ink)
    steps = [mix(white, "#2a2a2f", i / 9) for i in range(10)]
    chevrons(d, "front", steps, first=0.34, spacing=0.05, depth=0.16, width=6)
    chevrons(d, "back", steps, first=0.50, spacing=0.05, depth=0.16, width=6)
    for piece in ("sleeve_l", "sleeve_r"):
        chevrons(d, piece, [ink] * 6, first=0.15, spacing=0.14, depth=0.10, width=6, flip=True)
    d.add("add_text", text="5", panel="front", across_mm=105, above_hem_mm=470, cap_height_mm=66, color=white, clip=["front"])
    d.add("add_rectangle", panel="front", across_mm=0, above_hem_mm=215, width_mm=300, height_mm=50, fill=white, clip=["front"])
    d.add("add_text", text="SPONSOR", panel="front", across_mm=0, above_hem_mm=215, cap_height_mm=24, color=ink, clip=["front"])
    back_print(d, "MONO VBC", "5", white, white, number_cap=170, above=250)
    for piece in ("leg_r", "leg_l"):
        chevrons(d, piece, steps[:7], first=0.30, spacing=0.07, depth=0.16, width=6, flip=True)
    leg_number(d, "5", white)
    d.finish("mono-chevron")


def rose_quartz(m: Mcp):
    d = Design(m, "raglan-tshirt-women", "fitted-shorts-women")
    blush, rose, plum, cream = "#f6dfe4", "#e3a3b8", "#5b2a43", "#fff7f2"
    d.colours(primary=blush, accent=rose, trim=plum)
    for piece in ("front", "back"):
        gradient(d, piece, 0.0, 1.0, "#fbe9ec", "#efc3d0", power=1.2)
        honeycomb(d, piece, 31, "#e7a8bb", 2.2, top=0.0, bottom=1.0, fade_to="#f6d3dc")
    for piece in ("sleeve_l", "sleeve_r"):
        honeycomb(d, piece, 26, "#f6dfe4", 2.0, top=0.0, bottom=1.0, fade_to="#e3a3b8")
    d.add("add_rectangle", panel="front", across_mm=0, above_hem_mm=290, width_mm=300, height_mm=56, fill=plum, clip=["front"])
    d.add("add_text", text="SPONSOR", panel="front", across_mm=0, above_hem_mm=290, cap_height_mm=26, color=cream, clip=["front"])
    d.add("add_text", text="3", panel="front", across_mm=100, above_hem_mm=440, cap_height_mm=64, color=plum, clip=["front"])
    back_print(d, "ROSE VBC", "3", plum, "#a1587a", number_cap=190, above=320)
    for piece in ("leg_r", "leg_l"):
        gradient(d, piece, 0.0, 1.0, "#f6dfe4", "#e3a3b8", power=1.0)
        honeycomb(d, piece, 28, "#e0a0b5", 2.0, top=0.0, bottom=1.0)
    leg_number(d, "3", plum)
    d.finish("rose-quartz")


def glacier_hood(m: Mcp):
    d = Design(m, "hooded-jacket-women", "track-pants-women")
    snow, ice, blue, navy, deep = "#eef7fc", "#9fd4f2", "#2f7bb3", "#10304f", "#0a1e33"
    d.colours(primary=navy, accent=deep, trim=ice)
    try_colours(d, leg_stripe_l=ice, leg_stripe_r=ice)
    for piece in ("front", "back", "hood"):
        if piece in d.pieces:
            gradient(d, piece, 0.0, 1.0, deep, blue, power=1.5)
            scatter(d, piece, "star", 16, snow, 10, 34, seed=3)
            scatter(d, piece, "cross", 10, ice, 8, 18, seed=9)
    for piece in ("sleeve_l", "sleeve_r"):
        scatter(d, piece, "star", 7, ice, 10, 24, seed=2)
        d.rect(piece, 0.5, 0.93, 1.2, 0.06, ice)
    d.add("add_text", text="GLACIER", panel="front", across_mm=-110, above_hem_mm=430, cap_height_mm=26, color=ice, clip=["front"])
    d.add("add_rectangle", panel="front", across_mm=0, above_hem_mm=180, width_mm=270, height_mm=50, fill=ice, clip=["front"])
    d.add("add_text", text="SPONSOR", panel="front", across_mm=0, above_hem_mm=180, cap_height_mm=24, color=deep, clip=["front"])
    back_print(d, "GLACIER VBC", "1", snow, ice, number_cap=190, above=310)
    for piece in ("leg_r", "leg_l"):
        gradient(d, piece, 0.0, 1.0, deep, blue, power=1.4)
        scatter(d, piece, "star", 9, snow, 10, 26, seed=14)
    d.finish("glacier-hood")


def sunset_retro(m: Mcp):
    d = Design(m, "polo-shirt-man", "board-shorts-man")
    night, orange, pink, gold, plum = "#1a1030", "#ff8a3d", "#ff4f8b", "#ffd166", "#6b2f8a"
    d.colours(primary=night, accent=plum, trim=orange)
    bands = [gold, orange, pink, plum]
    for piece in ("front", "back"):
        x, y, w, h = d.pieces[piece]
        if piece == "front":
            d.add("add_ellipse", x_mm=round(x + w / 2, 1), y_mm=round(y + h * 0.60, 1), width_mm=round(w * 0.62, 1), height_mm=round(w * 0.62, 1), fill=mix(gold, orange, 0.5), clip=[piece])
        for i, colour in enumerate(bands):
            d.rect(piece, 0.5, 0.60 + 0.055 * i + 0.01 * i * i * 0.0, 1.2, 0.034 - 0.004 * i, colour)
        d.rect(piece, 0.5, 0.86, 1.2, 0.30, night)
        d.rect(piece, 0.5, 0.88, 1.2, 0.014, pink)
        d.rect(piece, 0.5, 0.93, 1.2, 0.014, orange)
    d.add("add_text", text="SPONSOR", panel="front", across_mm=0, above_hem_mm=150, cap_height_mm=24, color=gold, clip=["front"])
    d.add("add_text", text="SUNSET", panel="front", across_mm=-100, above_hem_mm=470, cap_height_mm=24, color=orange, clip=["front"])
    back_print(d, "SUNSET VBC", "8", "#fff0d6", orange, number_cap=170, above=330)
    for piece in ("leg_r", "leg_l"):
        for i, colour in enumerate(bands):
            d.rect(piece, 0.5, 0.60 + 0.07 * i, 1.1, 0.05, colour)
    leg_number(d, "8", gold)
    d.finish("sunset-retro")


DESIGNS = {
    "alpine-night": alpine_night,
    "summit-teal": summit_teal,
    "neon-court": neon_court,
    "coral-wave": coral_wave,
    "mono-chevron": mono_chevron,
    "rose-quartz": rose_quartz,
    "glacier-hood": glacier_hood,
    "sunset-retro": sunset_retro,
}


def main():
    wanted = sys.argv[1:] or list(DESIGNS)
    mcp = Mcp(EXE, dict(os.environ, OJ_LANG="en"))
    try:
        for name in wanted:
            DESIGNS[name](mcp)
    finally:
        mcp.close()


if __name__ == "__main__":
    main()
