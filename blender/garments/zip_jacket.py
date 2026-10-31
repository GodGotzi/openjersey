"""Zip jacket (unisex): a full-zip track jacket. Raglan cut like the raglan
T-shirt (front, back, side panels, raglan sleeves), but with long sleeves down to the wrist (printed cuffs), a lower
hem, a looser fit, a stand-up collar (one strip with the neck band, open at
the zip) and the zip itself as a narrow strip along the centre front.

The sheet keeps one "front" piece (both halves of the zip as one print).

`ZipJacket` is the cut; `hooded_jacket` adds a hood to it, and
`neckline` / `new_piece_object` help building pieces on the fitted top."""

from types import SimpleNamespace

import bmesh
import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from garments.raglan_tshirt import ROWS as TEE_ROWS, RaglanTee
from oj.body import new_object
from oj.geom import with_mirrored
from oj.mesh import fit_to_body, hem_weight, lower_arms, pillow_mesh, set_smooth, shrinkwrap, smooth_inner
from oj.registry import register
from oj.sheet import Sheet

ROWS = [row if row != ["collar"] else ["collar", "zipper"] for row in TEE_ROWS]
PIECES = {
    "collar": ("COLLAR", "Collar", "accent"),
    "zipper": ("ZIP", "Zip", "trim"),
}


def arm_axis(body):
    """The raised right arm (x > 0) as (pivot, direction, centre(s)): the
    axis from the shoulder pivot to the fingertips, and the arm's centre
    at distance s along it (from cross sections of the mesh)."""
    piv = Vector(body.pivot)
    verts = [v.co.copy() for v in body.obj.data.vertices if v.co.x > 0 and body.arm[v.index] > 0.5]
    tip = max(verts, key=lambda c: c.x)
    d = (tip - piv).normalized()
    centres = []
    for k in range(4, 60):
        s = k / 100
        sec = [c for c in verts if abs((c - piv).dot(d) - s) < 0.008]
        if sec:
            centres.append((s, sum(sec, Vector()) / len(sec)))

    def centre(s):
        if s <= centres[0][0]:
            return centres[0][1]
        for (s0, c0), (s1, c1) in zip(centres, centres[1:]):
            if s0 <= s <= s1:
                return c0.lerp(c1, (s - s0) / (s1 - s0))
        return centres[-1][1]
    return piv, d, centre


def _arc_lengths(points):
    """Cumulative 3D length along a polyline of Vectors."""
    out = [0.0]
    for a, b in zip(points, points[1:]):
        out.append(out[-1] + (b - a).length)
    return out


def neckline(obj, sheet):
    """The fitted top's neck edge as a loop of (position, along): starting
    at the centre front, round the wearer's left (x > 0), the back and the
    right to the centre front again (that point twice), with `along` the
    neck band's layout x there (0 .. band length)."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    uv = bm.loops.layers.uv["UVMap"]
    piece = bm.faces.layers.int["piece"]
    collar = sheet.index("collar")
    edges = [e for e in bm.edges if e.is_boundary]
    loops, seen = [], set()
    for e in edges:  # group the boundary into loops
        if e in seen:
            continue
        loop, stack = set(), [e]
        while stack:
            f = stack.pop()
            if f in seen:
                continue
            seen.add(f)
            loop.update(f.verts)
            stack += [g for v in f.verts for g in v.link_edges if g.is_boundary and g not in seen]
        loops.append(loop)
    verts = max(loops, key=lambda vs: sum(v.co.z for v in vs) / len(vs))
    start = min((v for v in verts if v.co.y < 0), key=lambda v: abs(v.co.x))
    order, prev, v = [start], None, start
    nxt = [e.other_vert(v) for e in v.link_edges if e.is_boundary]
    v, prev = max(nxt, key=lambda o: o.co.x), start
    while v is not start:
        order.append(v)
        v, prev = next(e.other_vert(v) for e in v.link_edges if e.is_boundary and e.other_vert(v) is not prev), v
    order.append(start)

    def alongs(v):
        return [lp[uv].uv.x for lp in v.link_loops if lp.face[piece] == collar]
    out = []
    for k, v in enumerate(order):
        xs = alongs(v)
        a = min(xs) if k == 0 else max(xs) if k == len(order) - 1 else sum(xs) / len(xs)
        out.append((v.co.copy(), a))
    bm.free()
    return out


def new_piece_object(name, sheet, grid, pieces, local):
    """A mesh object from a grid of 3D points (rows of Vectors), with the
    face attribute "piece" (`pieces(j, i)`: piece id of cell j, i) and
    UVMap = `local(j, i)` (piece-local metres) at each grid point."""
    bm = bmesh.new()
    uvl = bm.loops.layers.uv.new("UVMap")
    pl = bm.faces.layers.int.new("piece")
    vs = [[bm.verts.new(p) for p in row] for row in grid]
    for j in range(len(grid) - 1):
        for i in range(len(grid[0]) - 1):
            idx = [(j, i), (j, i + 1), (j + 1, i + 1), (j + 1, i)]
            f = bm.faces.new([vs[a][b] for a, b in idx])
            f[pl] = sheet.index(pieces(j, i))
            for lp, (a, b) in zip(f.loops, idx):
                lp[uvl].uv = local(a, b)
    obj = new_object(name, bm)
    set_smooth(obj)
    return obj


class ZipJacket(RaglanTee):
    V_NECK = False
    FIT_GAP = 0.014  # looser than the game shirts
    DRAPE_ROUNDS = 3  # smoothing + push-out rounds after the fit (bridges the body's detail)
    HEM_DROP = 0.06  # below the shirts' hem
    HEM_EASE = 0.012
    NECK_RAISE = {"front": 0.035, "back": 0.008}  # a jacket closes higher at the neck
    WRIST_S = 0.49  # the cuff, along the arm axis from the shoulder pivot
    CUFF_HALF = 0.05  # sleeve half-width (front view) at the cuff
    NR_UNDER = 26
    STAND_HEIGHT = 0.045  # the stand-up collar above the neck band
    STAND_IN = 0.01  # it leans in towards the neck by this much at the top
    STAND_ROWS = 5
    ZIP_HALF = 0.005
    ZIP_PROUD = 0.0035  # above the subdivided surface (the fabric is 1.5 mm thick)

    def __init__(self, body):
        super().__init__(body)
        m = SimpleNamespace(**vars(body.m))
        m.HEM_SIDE = (m.HEM_SIDE[0] + self.HEM_EASE, m.HEM_SIDE[1] - self.HEM_DROP)
        m.FRONT_NECK_BOTTOM += self.NECK_RAISE["front"]
        m.BACK_NECK_BOTTOM += self.NECK_RAISE["back"]
        # the sleeve runs on to the wrist, along the arm axis in the front view
        piv, d, centre = arm_axis(body)
        w = centre(self.WRIST_S)
        a = Vector((d.x, d.z)).normalized()
        n = Vector((-a.y, a.x))  # perpendicular, pointing up
        m.CUFF_TOP = (w.x + n.x * self.CUFF_HALF, w.z + n.y * self.CUFF_HALF)
        m.CUFF_BOTTOM = (w.x - n.x * self.CUFF_HALF, w.z - n.y * self.CUFF_HALF)
        self.m = m
        self.arm = (piv, d, centre)
        self.seams = with_mirrored([
            (m.HEM_SIDE, m.WAIST_SIDE),
            (m.WAIST_SIDE, m.UNDERARM),
            (m.UNDERARM, m.CUFF_BOTTOM),
            (m.CUFF_TOP, m.SHOULDER),
            (m.SHOULDER, m.NECK_SIDE),
        ])

    def follow_arm(self, obj):
        """Moves the sleeves' pillow onto the arm's centre line (the forearm
        reaches forward), fading in along the arm."""
        piv, d, centre = self.arm
        for v in obj.data.vertices:
            side = 1 if v.co.x > 0 else -1
            p = Vector((v.co.x * side, v.co.y, v.co.z))
            s = (p - piv).dot(d)
            if s > 0.15:
                v.co.y += centre(s).y * min((s - 0.15) / 0.15, 1.0)
        obj.data.update()

    def drape(self, obj):
        """Smooths the fitted fabric and only pushes back out what sank into
        the body: a looser garment bridging chest and back detail."""
        target = self.body.fitting_target()
        for _ in range(self.DRAPE_ROUNDS):
            smooth_inner(obj, 8)
            shrinkwrap(obj, target, self.FIT_GAP, mode="OUTSIDE")
        bpy.data.objects.remove(target)

    def build_body(self, sheet):
        collar_lengths = self.collar_lengths()
        panels = [(panel, self.panel_grids(sheet, panel, collar_lengths)) for panel in ("front", "back")]
        obj = pillow_mesh("top", sheet, panels, self.seams, self.depth, hem_weight(self.m.HEM_SIDE[1]))
        self.follow_arm(obj)
        fit_to_body(obj, self.body, gap=self.FIT_GAP)
        self.drape(obj)
        lower_arms(obj, self.body, gap=self.FIT_GAP)
        return obj

    def stand_collar(self, sheet, neck):
        """The stand-up collar on the neck edge, leaning in a little; its
        layout continues the neck band upwards (ly < 0), open at the zip."""
        centre = sum((p for p, _ in neck[:-1]), Vector()) / (len(neck) - 1)
        rows = []
        for j in range(self.STAND_ROWS):
            t = j / (self.STAND_ROWS - 1)
            row = []
            for p, _ in neck:
                inward = Vector((centre.x - p.x, centre.y - p.y, 0)).normalized()
                row.append(p + Vector((0, 0, self.STAND_HEIGHT * t)) + inward * self.STAND_IN * t * t)
            rows.append(row)
        local = lambda j, i: (neck[i][1], -self.STAND_HEIGHT * j / (self.STAND_ROWS - 1))
        length = neck[-1][1]
        sheet.add_outline("collar", [(0, 0), (length, 0), (length, -self.STAND_HEIGHT), (0, -self.STAND_HEIGHT)])
        return new_piece_object("collar", sheet, rows, lambda j, i: "collar", local), rows

    def centre_front(self, obj, step=0.005):
        """The top's centre front line from the hem up to the neck, as
        (position, outward normal) from the bottom up, on the surface as
        `finish` will subdivide it (else hollows cover the zip): rays at
        x = 0 every `step`."""
        sub = obj.modifiers.new("Subsurf", "SUBSURF")
        sub.levels = 1
        tree = BVHTree.FromObject(obj, bpy.context.evaluated_depsgraph_get())
        obj.modifiers.remove(sub)
        out, z = [], self.m.HEM_SIDE[1] - 0.05
        while z < self.m.NECK_SIDE[1]:
            hit, normal, _, _ = tree.ray_cast(Vector((0, -2, z)), Vector((0, 1, 0)))
            if hit is not None and hit.y < 0:
                out.append((hit, normal if normal.y < 0 else -normal))
            elif out:
                break  # above the neck edge
            z += step
        return out

    def zipper(self, sheet, obj, collar_rows):
        """The zip: a narrow strip along the centre front, standing proud of
        the fabric, up the collar's front edge to its top."""
        line = self.centre_front(obj)
        for row in collar_rows[1:]:
            p = row[0]
            line.append((p, Vector((0, -1, 0))))
        pts, ns = [p for p, _ in line], [n for _, n in line]
        along = _arc_lengths(pts)
        rows = []
        for p, n in zip(pts, ns):
            side = n.cross(Vector((0, 0, 1))).normalized()  # across the zip
            if side.x < 0:
                side = -side
            base = p + n * self.ZIP_PROUD
            rows.append([base - side * self.ZIP_HALF, base, base + side * self.ZIP_HALF])
        total = along[-1]
        local = lambda j, i: ((i - 1) * self.ZIP_HALF, total - along[j])
        sheet.add_outline("zipper", [(-self.ZIP_HALF, 0), (self.ZIP_HALF, 0),
                                     (self.ZIP_HALF, total), (-self.ZIP_HALF, total)])
        zip_obj = new_piece_object("zipper", sheet, rows, lambda j, i: "zipper", local)
        zip_obj["subdivide"] = False
        return zip_obj

    def build(self, rows=ROWS, pieces=PIECES):
        sheet = Sheet(rows, pieces)
        obj = self.build_body(sheet)
        neck = neckline(obj, sheet)
        collar, collar_rows = self.stand_collar(sheet, neck)
        return sheet, [obj, collar, self.zipper(sheet, obj, collar_rows)]


def build(body, info):
    return ZipJacket(body).build()


for _body, _order in (("man", 240), ("women", 241)):
    register(build, id=f"zip-jacket-{_body}", name="Zip Jacket",
             kind="top", body=_body, order=_order, unisex=True)
