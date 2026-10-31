"""Bottoms cut from the body surface: shorts, tights, pants.

The surface between two heights is copied off the body, relaxed over a
smoothed crotch (so the fabric drapes over it instead of following every
groove), pushed out by a gap and, for loose cuts, flared towards the leg
openings. Leg pieces wrap around each leg (`leg_u`): one rectangle per leg,
the outer side seam in the middle; an optional waistband is its own piece
wrapping around the waist, the back centre at both ends."""

import math

import bmesh
import bpy
from mathutils import Vector

from .body import new_object
from .geom import smoothstep
from .mesh import set_smooth, shrinkwrap, smooth_edges

STRIPE_WIDTH = 0.06  # leg stripe (m), as wide as the tops' side panels
TUCK_GAP = 0.002  # distance (m) of the tucked part from the body


def leg_axes(body, z_lo, z_hi):
    """Centre line of each thigh, as two points (at z_lo, z_hi) per side."""
    def centroid(side, z):
        pts = [v.co for v in body.obj.data.vertices
               if abs(v.co.z - z) < 0.006 and 0.02 < v.co.x * side < 0.28]
        return sum(pts, Vector()) / len(pts)
    return {side: (centroid(side, z_lo), centroid(side, z_hi)) for side in (1, -1)}


def thigh_axes(body, hem_z):
    """The thigh axes `leg_tube` uses for a bottom with its hem at `hem_z`."""
    z_lo = hem_z + 0.02
    return leg_axes(body, z_lo, max(body.m.THIGH_AXIS_Z, z_lo + 0.08))


def axis_at(axis, z):
    lo, hi = axis
    t = (z - lo.z) / (hi.z - lo.z)
    return lo + (hi - lo) * t


def leg_u(axes, side, co):
    """Around-the-leg coordinate: 0 inside, 0.5 outside, front at 0.25 on the
    left leg (x > 0) and 0.75 on the right one, so print reads left to right."""
    c = axis_at(axes[side], co.z)
    outward = (co.x - c.x) * side
    a = math.atan2(-(co.y - c.y), -outward)  # inner 0, front +90 degrees
    u = (a / (2 * math.pi)) % 1.0
    return u if side > 0 else 1.0 - u


def waist_u(centre, co):
    """Around-the-waist coordinate: 0 at the back centre, over the wearer's
    right side to the front centre (0.5) and back over the left side."""
    a = math.atan2(-(co.x - centre.x), co.y - centre.y)
    return (a / (2 * math.pi)) % 1.0


def crotch_target(body, name="crotch_target"):
    """The body with the crotch smoothed into a soft mound."""
    bm = bmesh.new()
    bm.from_mesh(body.obj.data)
    crotch_z = body.m.CROTCH_Z
    crotch = [v for v in bm.verts if abs(v.co.x) < 0.11 and crotch_z - 0.08 < v.co.z < crotch_z + 0.18 and v.co.y < 0.03]
    for _ in range(80):
        bmesh.ops.smooth_vert(bm, verts=crotch, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
    return new_object(name, bm, hidden=True)


def tuck(obj, body):
    """Pulls the part a top covers (from just below the shortest top's hem,
    body.m.TOP_HEM_Z, up) close to the body, so it can't show through a
    top that bridges a hollow there."""
    group = obj.vertex_groups.new(name="tuck")
    hem = body.m.TOP_HEM_Z
    for v in obj.data.vertices:
        w = smoothstep(hem - 0.04, hem + 0.01, v.co.z)
        if w > 0:
            group.add([v.index], w, "REPLACE")
    target = body.fitting_target()
    shrinkwrap(obj, target, TUCK_GAP, mode="ON_SURFACE", group="tuck")
    bpy.data.objects.remove(target)


def ring_circumference(bm, axes, z, side=1):
    """Mean around-the-leg length at height z."""
    c = axis_at(axes[side], z)
    radii = [math.hypot(v.co.x - c.x, v.co.y - c.y) for v in bm.verts
             if abs(v.co.z - z) < 0.004 and v.co.x * side > 0]
    return 2 * math.pi * sum(radii) / len(radii)


def relaxed_skin(body, z0, z1, gap, rounds, passes, name="skin"):
    """The body's skin between heights z0 and z1 as a new object, pushed
    out by `gap` and relaxed over a smoothed crotch: `rounds` x `passes`
    smoothing rounds iron out the crotch and muscle detail; the shrinkwrap
    only pushes fabric that sank into the body back out. The boundary rings
    stay at z0 and z1."""
    bm = body.surface_band(z0, z1)
    bm.normal_update()
    for v in bm.verts:
        v.co += v.normal * gap
    obj = new_object(name, bm)
    target = crotch_target(body)
    mid = (z0 + z1) / 2
    for _ in range(rounds):
        bm = bmesh.new()
        bm.from_mesh(obj.data)
        inner = [v for v in bm.verts if not v.is_boundary]
        for _ in range(passes):
            bmesh.ops.smooth_vert(bm, verts=inner, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
        smooth_edges(bm)
        for v in bm.verts:
            if v.is_boundary:
                v.co.z = z1 if v.co.z > mid else z0
        bm.to_mesh(obj.data)
        bm.free()
        shrinkwrap(obj, target, gap, mode="OUTSIDE")
    bpy.data.objects.remove(target)
    return obj


def leg_tube(body, sheet, name, top_z, hem_z, gap=0.005, flare=0.0, flare_start_z=None,
             rounds=5, passes=12, waistband=0.0, stripe=STRIPE_WIDTH):
    """Shorts or tights from the waist (top_z) down to the leg openings
    (hem_z, below the crotch). `flare` adds room towards the openings from
    `flare_start_z` down; `rounds` x `passes` smoothing rounds relax the
    surface (fewer = closer to the body). `waistband` > 0 makes the top
    band that high its own piece "waistband". Pieces: "leg_l", "leg_r"
    (and "waistband"); sheet.legs gets the leg entries with a stripe of
    width `stripe` centred on the outer side seam."""
    obj = relaxed_skin(body, hem_z, top_z, gap, rounds, passes, name)
    tuck(obj, body)

    axes = thigh_axes(body, hem_z)
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    if flare:
        for v in bm.verts:
            side = 1 if v.co.x > 0 else -1
            c = axis_at(axes[side], v.co.z)
            radial = Vector((v.co.x - c.x, v.co.y - c.y, 0))
            if radial.length > 1e-6:
                v.co += radial.normalized() * flare * smoothstep(flare_start_z, hem_z, v.co.z)

    circumference = ring_circumference(bm, axes, min(hem_z + 0.08, (hem_z + body.m.CROTCH_Z) / 2))
    leg_top = top_z - waistband
    band_z = top_z - waistband / 2
    band_ring = [v.co for v in bm.verts if abs(v.co.z - band_z) < 0.006]
    waist_centre = sum(band_ring, Vector()) / len(band_ring) if band_ring else Vector()
    waist = 0.0  # mean circumference of the waistband
    if waistband:
        radii = [math.hypot(v.co.x - waist_centre.x, v.co.y - waist_centre.y)
                 for v in bm.verts if abs(v.co.z - band_z) < 0.004]
        waist = 2 * math.pi * sum(radii) / len(radii)

    uv_layer = bm.loops.layers.uv.new("UVMap")
    piece_layer = bm.faces.layers.int.new("piece")
    for f in bm.faces:
        centre = f.calc_center_median()
        if waistband and centre.z > leg_top:
            f[piece_layer] = sheet.index("waistband")
            us = [waist_u(waist_centre, loop.vert.co) for loop in f.loops]
            if max(us) - min(us) > 0.5:  # face straddles the back centre
                us = [u + 1 if u < 0.5 else u for u in us]
            for loop, u in zip(f.loops, us):
                loop[uv_layer].uv = (u * waist, top_z - loop.vert.co.z)
            continue
        side = 1 if centre.x > 0 else -1
        f[piece_layer] = sheet.index("leg_l" if side > 0 else "leg_r")
        us = [leg_u(axes, side, loop.vert.co) for loop in f.loops]
        if max(us) - min(us) > 0.5:  # face straddles the inseam
            us = [u + 1 if u < 0.5 else u for u in us]
        for loop, u in zip(f.loops, us):
            loop[uv_layer].uv = (u * circumference, leg_top - loop.vert.co.z)
    h = leg_top - hem_z
    rect = lambda w, hh: [(0.0, 0.0), (w, 0.0), (w, hh), (0.0, hh)]
    for piece in ("leg_l", "leg_r"):
        sheet.add_outline(piece, rect(circumference, h))
    if waistband:
        sheet.add_outline("waistband", rect(waist, waistband))
    width = round(stripe / circumference, 4)
    # leg pieces: the outer side seam is in the middle; the front half is to
    # the right on the right leg and to the left on the left leg
    sheet.legs.extend(
        {"piece": piece, "frontDir": front_dir,
         "stripe": {"topCentre": 0.5, "topWidth": width, "hemCentre": 0.5, "hemWidth": width}}
        for piece, front_dir in (("leg_r", 1), ("leg_l", -1)))
    bm.normal_update()
    bm.to_mesh(obj.data)
    bm.free()
    set_smooth(obj)
    obj["subdivide"] = False  # cut from the dense body mesh already
    return obj
