"""Finishing touches for bottoms made with `legs.leg_tube`: a shaped hem
(side splits), looser legs, a seat spanned over hollows, leg pieces unrolled
true to size at every height (for long legs, where the thigh is much wider
than the ankle) and extra panels cut out of the legs (e.g. a back panel
across the seat).

Each works on the object `leg_tube` returned, with the same `top_z`,
`hem_z` and `waistband`; call `shape_hem` first, then `add_panel`, then
`unroll_legs` (which recomputes the leg pieces' layout and outlines)."""

import math

import bmesh
import bpy
from mathutils import Vector

from .geom import smoothstep
from .legs import STRIPE_WIDTH, axis_at, leg_u, thigh_axes
from .mesh import set_smooth, shrinkwrap


def shape_hem(obj, body, top_z, hem_z, waistband, rise, gap=0.005):
    """Raises the leg openings by `rise(u)` (metres; u around the leg as in
    `leg_u`: 0 inner seam, 0.5 outer side seam), stretching each leg
    linearly between the hem and the waistband, e.g. a curved side split.
    The leg UVs are stale afterwards: call `unroll_legs`."""
    axes = thigh_axes(body, hem_z)
    leg_top = min(top_z - waistband, body.m.TOP_HEM_Z - 0.04)  # leave the tucked part
    for v in obj.data.vertices:
        z = v.co.z
        if z >= leg_top:
            continue
        side = 1 if v.co.x > 0 else -1
        r = rise(leg_u(axes, side, v.co))
        if r <= 0:
            continue
        new_hem = hem_z + r
        v.co.z = new_hem + (z - hem_z) * (leg_top - new_hem) / (leg_top - hem_z)
    obj.data.update()
    push_out(obj, body, gap)


def push_out(obj, body, gap):
    """Pushes fabric that sank into the body back out to `gap`, except
    where `legs.tuck` keeps it close under the tops (from just below
    body.m.TOP_HEM_Z up)."""
    group = obj.vertex_groups.get("free") or obj.vertex_groups.new(name="free")
    hem = body.m.TOP_HEM_Z
    for v in obj.data.vertices:
        group.add([v.index], 1.0 - smoothstep(hem - 0.04, hem + 0.01, v.co.z), "REPLACE")
    target = body.fitting_target()
    shrinkwrap(obj, target, gap, mode="OUTSIDE", group="free")
    bpy.data.objects.remove(target)
    set_smooth(obj)


def _ring_centre(bm, z, band=0.004):
    ring = [v.co for v in bm.verts if abs(v.co.z - z) < band]
    return sum(ring, Vector()) / len(ring)


def _ring_circumference(verts, centre, z, band=0.004):
    radii = [math.hypot(v.x - centre.x, v.y - centre.y) for v in verts if abs(v.z - z) < band]
    return 2 * math.pi * sum(radii) / len(radii)


def _smooth(values, passes=6):
    """Moving average (ends kept), to iron out sampling noise."""
    v = list(values)
    for _ in range(passes):
        v = [v[0]] + [(v[i - 1] + 2 * v[i] + v[i + 1]) / 4 for i in range(1, len(v) - 1)] + [v[-1]]
    return v


def add_panel(obj, sheet, piece, top_z, bottom_z, half_width, back=True):
    """Re-assigns the faces between `bottom_z` and `top_z` within
    `half_width` (a fraction of the circumference, around the back centre,
    or the front centre with back=False) to `piece`, cut along exact
    boundaries, laid out unrolled around the hips (y down from top_z)."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    centre = _ring_centre(bm, (top_z + bottom_z) / 2)
    # cut along the boundaries: the bottom edge and the two sides
    geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
    bmesh.ops.bisect_plane(bm, geom=geom, plane_co=Vector((0, 0, bottom_z)), plane_no=Vector((0, 0, 1)))
    for s in (1, -1):
        a = 2 * math.pi * half_width * s
        # waist angle a (0 = back centre) lies along (-sin a, cos a); the
        # plane's normal is perpendicular to that, horizontal
        normal = Vector((math.cos(a), math.sin(a), 0)) if back else Vector((math.cos(a), -math.sin(a), 0))
        geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
        bmesh.ops.bisect_plane(bm, geom=geom, plane_co=Vector((centre.x, centre.y, 0)), plane_no=normal)

    def u_of(co):
        """-0.5..0.5 around the hips, 0 at the centre of the panel's side,
        increasing to the wearer's left as seen from that side."""
        c = co - centre
        a = math.atan2(-c.x, c.y) if back else math.atan2(-c.x, -c.y)
        return a / (2 * math.pi)

    piece_layer = bm.faces.layers.int.get("piece")
    uv_layer = bm.loops.layers.uv["UVMap"]
    index = sheet.index(piece)
    verts = [v.co.copy() for v in bm.verts]
    zs = [bottom_z + (top_z - bottom_z) * i / 20 for i in range(21)]
    raw = [_ring_circumference(verts, centre, z) for z in zs]
    circ = dict(zip(zs, _smooth(raw)))

    def c_at(z):
        t = min(max((z - bottom_z) / (top_z - bottom_z), 0.0), 1.0) * 20
        i = min(int(t), 19)
        return circ[zs[i]] + (circ[zs[i + 1]] - circ[zs[i]]) * (t - i)

    width = max(circ.values()) * 2 * half_width
    eps = 1e-5
    for f in bm.faces:
        c = f.calc_center_median()
        if not (bottom_z - eps < c.z < top_z + eps) or abs(u_of(c)) > half_width:
            continue
        f[piece_layer] = index
        for loop in f.loops:
            co = loop.vert.co
            # seen from the back, the wearer's left is to the left
            x = (-u_of(co) if back else u_of(co)) * c_at(co.z)
            loop[uv_layer].uv = (width / 2 + x, top_z - co.z)
    # outline: unrolled width at each height
    left = [(width / 2 - c_at(z) * half_width, top_z - z) for z in reversed(zs)]
    right = [(width / 2 + c_at(z) * half_width, top_z - z) for z in zs]
    sheet.add_outline(piece, left + right)
    bm.to_mesh(obj.data)
    bm.free()
    set_smooth(obj)


def unroll_legs(obj, sheet, body, top_z, hem_z, waistband, rise=None, taper=True, stripe=STRIPE_WIDTH):
    """Recomputes the leg pieces' layout from the (possibly reshaped)
    geometry: around the leg as in `leg_tube`, but with taper=True scaled to
    the leg's own circumference at each height (true to size from thigh to
    ankle); the outlines follow the unrolled shape and the hem `rise`. The
    pieces stay centred on the outer side seam; leg stripe widths are
    updated."""
    axes = thigh_axes(body, hem_z)
    leg_top = top_z - waistband
    rise = rise or (lambda u: 0.0)
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    piece_layer = bm.faces.layers.int.get("piece")
    uv_layer = bm.loops.layers.uv["UVMap"]
    legs = {sheet.index("leg_l"): 1, sheet.index("leg_r"): -1}

    # circumference of each leg (both legs are alike; take the left one)
    # at heights from the hem to the top
    n = 40
    zs = [hem_z + (leg_top - hem_z) * i / n for i in range(n + 1)]
    leg_verts = [v.co.copy() for v in bm.verts if v.co.x > 0 and v.co.z <= leg_top + 0.004]

    def circumference(z):
        c = axis_at(axes[1], z)
        radii = [math.hypot(v.x - c.x, v.y - c.y) for v in leg_verts if abs(v.z - z) < 0.004]
        return 2 * math.pi * sum(radii) / len(radii) if radii else None

    raw = [circumference(z) for z in zs]
    known = [(z, c) for z, c in zip(zs, raw) if c]
    circ = []
    for z in zs:  # fill heights the split left empty from the nearest known
        circ.append(min(known, key=lambda k: abs(k[0] - z))[1])
    circ = _smooth(circ)
    if not taper:
        mean = sum(circ[: n // 4 + 1]) / (n // 4 + 1)  # like leg_tube: near the hem
        circ = [mean] * len(circ)

    def c_at(z):
        t = min(max((z - hem_z) / (leg_top - hem_z), 0.0), 1.0) * n
        i = min(int(t), n - 1)
        return circ[i] + (circ[i + 1] - circ[i]) * (t - i)

    width = max(circ)
    for f in bm.faces:
        side = legs.get(f[piece_layer])
        if side is None:
            continue
        us = [leg_u(axes, side, loop.vert.co) for loop in f.loops]
        if max(us) - min(us) > 0.5:  # face straddles the inseam
            us = [u + 1 if u < 0.5 else u for u in us]
        for loop, u in zip(f.loops, us):
            z = loop.vert.co.z
            loop[uv_layer].uv = (width / 2 + (u - 0.5) * c_at(z), leg_top - z)
    bm.to_mesh(obj.data)
    bm.free()

    # outline: down the inner seam at u = 0 (left edge), along the hem,
    # up the inner seam at u = 1 (right edge)
    steps = 40
    outline = [(width / 2 - c_at(z) / 2, leg_top - z) for z in reversed(zs)]
    outline += [(width / 2 + (u - 0.5) * c_at(hem_z + rise(u)), leg_top - hem_z - rise(u))
                for u in (i / steps for i in range(1, steps))]
    outline += [(width / 2 + c_at(z) / 2, leg_top - z) for z in zs]
    for piece in ("leg_l", "leg_r"):
        sheet.outlines[piece] = [outline]
    for leg in sheet.legs:
        leg["stripe"]["topWidth"] = leg["stripe"]["hemWidth"] = round(stripe / width, 4)


def side_split(height, half_width):
    """A `rise` for `shape_hem`/`unroll_legs`: a rounded split on the outer
    side seam, `height` high, running into the hem `half_width` (fraction
    of the circumference) to either side."""
    def rise(u):
        d = abs((u % 1.0) - 0.5)
        return height * math.cos(math.pi / 2 * d / half_width) ** 1.5 if d < half_width else 0.0
    return rise


def ease_legs(obj, body, hem_z, ease, passes=8, gap=0.005, midline=0.004):
    """Loosens the legs: pushes every point `ease(z)` metres out, from its
    thigh axis below the crotch and from the hips' centre above it (fading
    out below the part the tops cover), keeping the legs apart (|x| >=
    `midline`), then relaxes the surface and pushes anything that sank into
    the body back out."""
    axes = thigh_axes(body, hem_z)
    crotch = body.m.CROTCH_Z
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    centres = {}
    for v in bm.verts:
        side = 1 if v.co.x > 0 else -1
        leg = 1.0 - smoothstep(crotch - 0.02, crotch + 0.06, v.co.z)
        amount = ease(min(v.co.z, crotch)) * (1.0 - smoothstep(crotch + 0.02, body.m.TOP_HEM_Z - 0.04, v.co.z))
        if amount <= 0:
            continue
        c_leg = axis_at(axes[side], v.co.z)
        key = round(v.co.z, 2)
        if key not in centres:
            centres[key] = _ring_centre(bm, key, 0.006)
        c = c_leg * leg + centres[key] * (1 - leg)
        radial = Vector((v.co.x - c.x, v.co.y - c.y, 0))
        if radial.length < 1e-6:
            continue
        v.co += radial.normalized() * amount
        if v.co.x * side < midline:
            v.co.x = midline * side
    inner = [v for v in bm.verts if not v.is_boundary and v.co.z < body.m.TOP_HEM_Z - 0.04]
    for _ in range(passes):
        bmesh.ops.smooth_vert(bm, verts=inner, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=False)
    bm.to_mesh(obj.data)
    bm.free()
    push_out(obj, body, gap)


def _hull(points):
    """Convex hull (monotone chain), counter-clockwise."""
    pts = sorted(set(points))
    if len(pts) < 3:
        return pts
    cross = lambda o, a, b: (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    lower, upper = [], []
    for p in pts:
        while len(lower) >= 2 and cross(lower[-2], lower[-1], p) <= 0:
            lower.pop()
        lower.append(p)
    for p in reversed(pts):
        while len(upper) >= 2 and cross(upper[-2], upper[-1], p) <= 0:
            upper.pop()
        upper.append(p)
    return lower[:-1] + upper[:-1]


def _ray_to_hull(hull, c, d):
    """Distance from c along unit direction d to the hull boundary."""
    best = None
    n = len(hull)
    for i in range(n):
        a, b = hull[i], hull[(i + 1) % n]
        ex, ey = b[0] - a[0], b[1] - a[1]
        den = d[0] * ey - d[1] * ex
        if abs(den) < 1e-12:
            continue
        t = ((a[0] - c[0]) * ey - (a[1] - c[1]) * ex) / den
        u = ((a[0] - c[0]) * d[1] - (a[1] - c[1]) * d[0]) / den
        if t > 0 and -1e-9 <= u <= 1 + 1e-9:
            best = t if best is None else min(best, t)
    return best


def bridge_seat(obj, body):
    """`bridge_hollows` over the seat: from just above the crotch up to just
    below the part the tops cover (see `legs.tuck`)."""
    bridge_hollows(obj, body.m.CROTCH_Z + 0.04, body.m.TOP_HEM_Z - 0.03)


def bridge_hollows(obj, z0, z1, passes=4, band=0.003):
    """Spans the fabric across hollows between z0 and z1 (where the
    garment is one tube around the hips, above the crotch): each point
    moves out to the convex hull of its cross-section, like fabric under a
    little tension (bridges the seat's cleft and the small of the back)."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    rows = {}
    for v in bm.verts:
        if z0 <= v.co.z <= z1:
            rows.setdefault(round(v.co.z / band), []).append(v)
    moved = []
    for key, verts in rows.items():
        ring = [v for k in (key - 1, key, key + 1) for v in rows.get(k, [])]
        hull = _hull([(round(v.co.x, 5), round(v.co.y, 5)) for v in ring])
        if len(hull) < 3:
            continue
        c = (sum(p[0] for p in hull) / len(hull), sum(p[1] for p in hull) / len(hull))
        for v in verts:
            dx, dy = v.co.x - c[0], v.co.y - c[1]
            r = math.hypot(dx, dy)
            if r < 1e-6:
                continue
            reach = _ray_to_hull(hull, c, (dx / r, dy / r))
            if reach and reach > r + 1e-4:
                v.co.x, v.co.y = c[0] + dx / r * reach, c[1] + dy / r * reach
                moved.append(v)
    inner = [v for v in bm.verts if not v.is_boundary and z0 - 0.02 <= v.co.z <= z1 + 0.02]
    for _ in range(passes):
        bmesh.ops.smooth_vert(bm, verts=inner, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
    bm.to_mesh(obj.data)
    bm.free()
    set_smooth(obj)
