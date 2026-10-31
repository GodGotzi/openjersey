"""Bikini bottom (women): a beach volleyball bikini bottom, low rise with
narrow sides high on the hips, high-cut leg openings and a cheeky back. Two
pieces, front and back, joined at the crotch and at the hips.

Cut from the body surface (see oj/cut.py): the skin around the pelvis,
relaxed over a smoothed crotch, then cut along the waistline and the leg
openings, each a smooth height curve around the waist or the thigh."""

import math

import bmesh
import bpy
from mathutils import Vector

from oj.body import largest_island, new_object
from oj.cut import add_outlines, conformal_unwrap, iso_cut, mark_seams
from oj.legs import leg_axes, leg_u, relaxed_skin, waist_u
from oj.mesh import set_smooth
from oj.registry import register
from oj.sheet import Sheet

PIECES = {
    "bottom_front": ("BOTTOM FRONT", "Bottom Front", "primary"),
    "bottom_back": ("BOTTOM BACK", "Bottom Back", "primary"),
}
GAP = 0.003  # fabric to skin (m)

# Outline heights (m) above the crotch (the lowest point between the legs).
# Waistline at the back centre, at the sides, at the front centre:
TOP = {"back": 0.115, "side": 0.14, "front": 0.10}
# leg opening around the thigh: inside (crotch), front, outside (hip), back
LEG = {"inside": -0.006, "front": 0.065, "outside": 0.112, "back": 0.045}


def periodic(v0, v25, v50, v75):
    """A smooth periodic curve through four values at u = 0, .25, .5, .75."""
    a1, b1 = (v0 - v50) / 2, (v25 - v75) / 2
    s, d = (v0 + v50) / 2, (v25 + v75) / 2
    a0, a2 = (s + d) / 2, (s - d) / 2
    return lambda u: (a0 + a1 * math.cos(2 * math.pi * u) + b1 * math.sin(2 * math.pi * u)
                      + a2 * math.cos(4 * math.pi * u))


def build(body, info):
    m = body.m
    sheet = Sheet([["bottom_front", "bottom_back"]], PIECES)
    band = relaxed_skin(body, m.CROTCH_Z - 0.09, m.CROTCH_Z + 0.2, GAP, rounds=2, passes=5)
    bm = bmesh.new()
    bm.from_mesh(band.data)
    bpy.data.objects.remove(band)

    apex_v = min((v for v in bm.verts if abs(v.co.x) < 0.006), key=lambda v: v.co.z)
    apex, split_y = apex_v.co.z, apex_v.co.y
    axes = leg_axes(body, m.CROTCH_Z - 0.14, m.CROTCH_Z - 0.03)
    ring = [v.co for v in body.obj.data.vertices if abs(v.co.z - (apex + 0.12)) < 0.004 and abs(v.co.x) < 0.25]
    centre = sum(ring, Vector()) / len(ring)

    top = periodic(TOP["back"], TOP["side"], TOP["front"], TOP["side"])
    leg = periodic(LEG["inside"], LEG["front"], LEG["outside"], LEG["back"])

    def outline(co):
        side = 1 if co.x > 0 else -1
        u = leg_u(axes, side, co)
        u = u if side > 0 else 1.0 - u  # inside 0, front .25, outside .5, back .75
        above_top = co.z - (apex + top(waist_u(centre, co)))
        below_leg = (apex + leg(u)) - co.z
        return max(above_top, below_leg)

    iso_cut(bm, outline)
    largest_island(bm)
    iso_cut(bm, lambda co: co.y - split_y, delete=False)

    bm.loops.layers.uv.new("UVMap")
    piece = bm.faces.layers.int.new("piece")
    for f in bm.faces:
        f[piece] = sheet.index("bottom_front" if f.calc_center_median().y < split_y else "bottom_back")
    obj = new_object("bottom", bm)
    mark_seams(obj)
    conformal_unwrap(obj, {sheet.index("bottom_front"): lambda co: (co.x, -co.z),
                           sheet.index("bottom_back"): lambda co: (-co.x, -co.z)})
    add_outlines(sheet, obj)
    set_smooth(obj)
    obj["subdivide"] = False
    return sheet, [obj]


register(build, id="bikini-bottom-women", name="Bikini Bottom",
         kind="bottom", body="women", order=350)
