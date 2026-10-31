"""Sports top (women): a beach volleyball top. A cropped racerback: front and back panels, a wide underbust
band, and narrow bindings around the neckline and the armholes; the back
armholes sweep in to a narrow centre column between the shoulder blades.

Cut from the body surface (see oj/cut.py) with the arms already lowered:
the skin of the chest, relaxed so the fabric bridges the cleavage, then
cut along the outline drawn in front-view pattern coordinates (x, z), on
the front for points in front of the body's mid-plane, on the back
behind it."""

import math

import bmesh
import bpy
from mathutils.kdtree import KDTree

from oj.body import largest_island, new_object
from oj.cut import add_outlines, boundary_loops, conformal_unwrap, iso_cut, mark_seams, strip_uv
from oj.geom import cubic, smoothstep
from oj.mesh import set_smooth, shrinkwrap
from oj.registry import register
from oj.sheet import Sheet

PIECES = {
    "underbust_band": ("UNDERBUST BAND", "Underbust Band", "trim"),
    "collar": ("NECK BINDING", "Neck Binding", "trim"),
}
ROWS = [["front", "back"], ["underbust_band"], ["collar"], ["armhole_r", "armhole_l"]]

GAP = 0.004  # fabric to skin (m)
HEM_Z = 1.045  # bottom of the underbust band (m above the floor)
BAND = 0.055  # underbust band height (m)
BINDING = 0.012  # width of the neck and armhole bindings (m)
SIDE_BLEND = 0.04  # front and back outlines blend over this distance either side of the mid-plane
NECK_X = 0.07  # strap on the shoulder seam: neck edge
STRAP_X = 0.112  # strap on the shoulder seam: armhole edge
ARM_SIDE_Z = 1.125  # armhole bottom at the side
FRONT_NECK_Z = 1.275  # front neckline at the centre (a deep scoop)
BACK_NECK_Z = 1.30  # back neckline at the centre (above the racer column)
# armhole curves from the side up to the strap: inner control points (x, z)
ARMHOLE = {"front": ((0.15, None), (0.105, 1.30)),
           "back": ((0.035, None), (0.03, 1.27))}  # racerback: in to the spine, then up
NECK = {"front": ((0.055, None), (0.068, 1.33)),
        "back": ((0.03, None), (0.055, 1.36))}


def shoulder_z(m, x):
    t = (x - m.NECK_SIDE[0]) / (m.SHOULDER[0] - m.NECK_SIDE[0])
    return m.NECK_SIDE[1] + (m.SHOULDER[1] - m.NECK_SIDE[1]) * t


def table(points, key, value):
    """Linear interpolation table value(key) from curve points, clamped."""
    pts = sorted((key(p), value(p)) for p in points)
    keys = [k for k, _ in pts]

    def f(k):
        if k <= keys[0]:
            return pts[0][1]
        if k >= keys[-1]:
            return pts[-1][1]
        lo, hi = 0, len(keys) - 1
        while hi - lo > 1:
            mid = (lo + hi) // 2
            if keys[mid] <= k:
                lo = mid
            else:
                hi = mid
        (k0, v0), (k1, v1) = pts[lo], pts[hi]
        return v0 + (v1 - v0) * (k - k0) / max(k1 - k0, 1e-12)
    return f


def dense(points, step=0.0008):
    """Points along a polyline, at most `step` apart."""
    out = []
    for (x0, z0), (x1, z1) in zip(points, points[1:]):
        n = max(1, int(math.hypot(x1 - x0, z1 - z0) / step))
        out += [(x0 + (x1 - x0) * k / n, z0 + (z1 - z0) * k / n) for k in range(n)]
    return out + [points[-1]]


def tree_of(points):
    tree = KDTree(len(points))
    for i, (x, z) in enumerate(points):
        tree.insert((x, z, 0.0), i)
    tree.balance()
    return tree


class Panel:
    """The outline of the front or back panel as region functions of (x, z):
    the signed distance to the neck cut-out and to the arm cut-out
    (positive inside the cut-out)."""

    def __init__(self, m, panel, x_side):
        self.top_z = shoulder_z(m, STRAP_X)
        neck_z = shoulder_z(m, NECK_X)
        bottom = FRONT_NECK_Z if panel == "front" else BACK_NECK_Z
        (x1, z1), c2 = NECK[panel]
        neck = cubic((0.0, bottom), (x1, bottom if z1 is None else z1), c2, (NECK_X, neck_z), 301)
        self.neck = table(neck, lambda p: p[0], lambda p: p[1])
        self.neck_tree = tree_of(dense(neck + [(NECK_X, 2.0)]))
        (x1, z1), c2 = ARMHOLE[panel]
        arm = cubic((x_side, ARM_SIDE_Z), (x1, ARM_SIDE_Z if z1 is None else z1), c2, (STRAP_X, self.top_z), 301)
        self.arm = table(arm, lambda p: p[1], lambda p: p[0])
        self.arm_tree = tree_of(dense([(0.6, ARM_SIDE_Z)] + arm + [(STRAP_X, 2.0)]))

    def neck_value(self, x, z):
        x = abs(x)
        d = self.neck_tree.find((x, z, 0.0))[2]
        return d if x < NECK_X and z > self.neck(x) else -d

    def arm_value(self, x, z):
        x = abs(x)
        d = self.arm_tree.find((x, z, 0.0))[2]
        return d if z > ARM_SIDE_Z and x > self.arm(z) else -d


def relax_skin(obj, body, gap, bust, rounds=4, passes=8, bust_passes=60):
    """Smooths the cut skin `obj` so it bridges small hollows, and the
    points `bust(co)` much more (the fabric spans the cleavage), keeping
    it `gap` outside the body; boundary points stay put."""
    target = body.fitting_target(drop_arms=True)
    for _ in range(rounds):
        bm = bmesh.new()
        bm.from_mesh(obj.data)
        inner = [v for v in bm.verts if not v.is_boundary]
        for _ in range(passes):
            bmesh.ops.smooth_vert(bm, verts=inner, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
        chest = [v for v in inner if bust(v.co)]
        for _ in range(bust_passes):
            bmesh.ops.smooth_vert(bm, verts=chest, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
        bm.to_mesh(obj.data)
        bm.free()
        shrinkwrap(obj, target, gap, mode="OUTSIDE")
    bpy.data.objects.remove(target)


def span_cleavage(obj, region, half_width=0.13, step=0.003, passes=3):
    """Compression fabric runs straight from breast to breast: in each
    horizontal slice, moves the front points of `region(co)` (|x| <
    half_width) forward onto the slice's front convex hull, then smooths a
    little."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    rows = {}
    for v in bm.verts:
        if abs(v.co.x) < half_width and region(v.co):
            rows.setdefault(round(v.co.z / step), []).append(v)
    moved = []
    for verts in rows.values():
        pts = sorted((v.co.x, v.co.y) for v in verts)
        hull = []  # front (smallest y) hull, left to right
        for p in pts:
            while len(hull) >= 2 and ((hull[-1][0] - hull[-2][0]) * (p[1] - hull[-2][1])
                                      - (hull[-1][1] - hull[-2][1]) * (p[0] - hull[-2][0])) <= 0:
                hull.pop()
            hull.append(p)
        for v in verts:
            for (x0, y0), (x1, y1) in zip(hull, hull[1:]):
                if x0 <= v.co.x <= x1:
                    y = y0 + (y1 - y0) * (v.co.x - x0) / max(x1 - x0, 1e-9)
                    if y < v.co.y:
                        v.co.y = y
                        moved.append(v)
                    break
    inner = [v for v in moved if not v.is_boundary]
    for _ in range(passes):
        bmesh.ops.smooth_vert(bm, verts=inner, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
    bm.to_mesh(obj.data)
    bm.free()


def loop_distance(loops, step=0.001):
    """Distance (3D) to the nearest point of the closed `loops`."""
    pts = []
    for lp in loops:
        for a, b in zip(lp, lp[1:] + lp[:1]):
            n = max(1, int((b - a).length / step))
            pts += [a.lerp(b, k / n) for k in range(n)]
    tree = KDTree(len(pts))
    for i, p in enumerate(pts):
        tree.insert(p, i)
    tree.balance()
    return lambda co: tree.find(co)[2]


def mid_plane(body, smooth=3):
    """y of the body's front/back mid-plane by height (from centimetre
    slices, smoothed over +-`smooth` cm and interpolated)."""
    slices = {}
    for v in body.obj.data.vertices:
        if abs(v.co.x) < 0.12:
            k = round(v.co.z * 100)
            lo, hi = slices.get(k, (v.co.y, v.co.y))
            slices[k] = (min(lo, v.co.y), max(hi, v.co.y))
    raw = {k: (lo + hi) / 2 for k, (lo, hi) in slices.items()}
    ys = {k: sum(raw[j] for j in range(k - smooth, k + smooth + 1) if j in raw)
          / sum(1 for j in range(k - smooth, k + smooth + 1) if j in raw) for k in raw}

    def mid(z):
        k = math.floor(z * 100)
        t = z * 100 - k
        return ys.get(k, 0.0) * (1 - t) + ys.get(k + 1, ys.get(k, 0.0)) * t
    return mid


def oriented(loop, start, towards_negative_x=True):
    """`loop` rotated to begin nearest `start`, running towards -x first
    (the wearer's right) or +x."""
    i0 = min(range(len(loop)), key=lambda i: (loop[i].x - start[0]) ** 2 + (loop[i].y - start[1]) ** 2
             + (loop[i].z - start[2]) ** 2)
    loop = loop[i0:] + loop[:i0]
    ahead = loop[min(10, len(loop) - 1)].x - loop[0].x
    if (ahead > 0) == towards_negative_x:
        loop = loop[:1] + loop[1:][::-1]
    return loop


def build(body, info):
    m = body.m
    body.lower_arms()
    sheet = Sheet(ROWS, PIECES)
    mid = mid_plane(body)
    x_side = m.UNDERARM[0] + 0.02  # beyond the torso's side: the side stays whole below the armhole
    front, back = Panel(m, "front", x_side), Panel(m, "back", x_side)

    def blend(fn):
        """`fn(panel, x, z)` on the front, on the back, and blended across
        the sides (front and back cut differently there)."""
        def value(co):
            w = smoothstep(mid(co.z) + SIDE_BLEND, mid(co.z) - SIDE_BLEND, co.y)
            if w >= 1:
                return fn(front, co.x, co.z)
            if w <= 0:
                return fn(back, co.x, co.z)
            return w * fn(front, co.x, co.z) + (1 - w) * fn(back, co.x, co.z)
        return value

    neck_value = blend(Panel.neck_value)
    arm_value = blend(Panel.arm_value)

    def outline(co):
        return max(HEM_Z - co.z, neck_value(co), arm_value(co))

    bm = body.surface_band(HEM_Z - 0.03, m.NECK_SIDE[1] + 0.06, max_x=0.3)
    bm.normal_update()
    for v in bm.verts:
        v.co += v.normal * GAP
    iso_cut(bm, lambda co: outline(co) - 0.025)  # rough cut first: away with arms and neck
    largest_island(bm)
    obj = new_object("top", bm)
    bust = lambda co: abs(co.x) < 0.13 and HEM_Z + 0.02 < co.z < FRONT_NECK_Z + 0.04 and co.y < mid(co.z) - 0.04
    relax_skin(obj, body, GAP, bust)
    span_cleavage(obj, lambda co: HEM_Z + BAND < co.z < FRONT_NECK_Z + 0.06 and co.y < mid(co.z))
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bpy.data.objects.remove(obj)

    iso_cut(bm, outline)
    largest_island(bm)
    # the boundary loops: hem (lowest), neck (around x = 0), armholes
    loops = boundary_loops(bm)
    hem = min(loops, key=lambda lp: sum(p.z for p in lp) / len(lp))
    rest = [lp for lp in loops if lp is not hem]
    neck = min(rest, key=lambda lp: abs(sum(p.x for p in lp) / len(lp)))
    arms = [lp for lp in rest if lp is not neck]
    # bindings: everything within BINDING of the neck or an armhole edge
    neck_dist = loop_distance([neck])
    arm_dist = loop_distance(arms)
    iso_cut(bm, lambda co: co.z - (HEM_Z + BAND), delete=False)
    iso_cut(bm, lambda co: neck_dist(co) - BINDING, delete=False)
    iso_cut(bm, lambda co: arm_dist(co) - BINDING, delete=False)
    iso_cut(bm, lambda co: co.y - mid(co.z), delete=False)

    bm.loops.layers.uv.new("UVMap")
    piece = bm.faces.layers.int.new("piece")
    for f in bm.faces:
        c = f.calc_center_median()
        if c.z < HEM_Z + BAND:
            name = "underbust_band"
        elif neck_dist(c) < BINDING:
            name = "collar"
        elif arm_dist(c) < BINDING:
            name = "armhole_l" if c.x > 0 else "armhole_r"
        else:
            name = "front" if c.y < mid(c.z) else "back"
        f[piece] = sheet.index(name)
    obj = new_object("top", bm)
    set_smooth(obj)

    back_y = max(p.y for p in hem)

    mark_seams(obj)
    conformal_unwrap(obj, {sheet.index("front"): lambda co: (co.x, -co.z),
                           sheet.index("back"): lambda co: (-co.x, -co.z)})
    strip_uv(obj, sheet.index("underbust_band"), oriented(hem, (0.0, back_y, HEM_Z)), height=BAND)
    neck_back = max(neck, key=lambda p: p.y if abs(p.x) < 0.01 else -1)
    strip_uv(obj, sheet.index("collar"), oriented(neck, neck_back))
    for lp in arms:
        side = 1 if sum(p.x for p in lp) > 0 else -1
        low = min(lp, key=lambda p: p.z)
        strip_uv(obj, sheet.index("armhole_l" if side > 0 else "armhole_r"), oriented(lp, low, side > 0))
    add_outlines(sheet, obj)
    obj["subdivide"] = False
    return sheet, [obj]


register(build, id="sports-top-women", name="Sports Top",
         kind="top", body="women", order=290)
