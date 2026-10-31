"""Polo shirt (man and women): a regular-fit polo with set-in short sleeves,
a front button placket and a stand collar. Pieces: front and back (one piece each, joined at the side and
shoulder seams), the placket (a strip down the front centre, below the
collar), a sleeve per side (front and back half, joined along the top seam;
printed cuff band) and the stand collar (a strip once around the neck,
open at the front centre).

`PoloShirt` is the cut: front and back are one grid each from
the hem to the neck and shoulder seam, the sleeve joins them along the
armhole curve (underarm -> shoulder point). The collar is built in 3D on
the fitted shirt's neckline, standing up around the neck."""

import bmesh
import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from garments.raglan_tshirt import RaglanTee
from oj.body import new_object
from oj.cut import boundary_loops
from oj.geom import coons, cubic, relax, resample, with_mirrored
from oj.mesh import Grids, fit_to_body, hem_weight, lower_arms, pillow_mesh, set_smooth
from oj.registry import register
from oj.sheet import Sheet

ROWS = [
    ["front", "back"],
    ["sleeve_r", "placket", "sleeve_l"],
    ["collar"],
]
PIECES = {
    "placket": ("PLACKET", "Placket", "trim"),
    "collar": ("COLLAR", "Stand Collar", "trim"),
}


class PoloShirt(RaglanTee):
    """Set-in sleeve top on a body's measurements, with an optional front
    placket (PLACKET_LENGTH > 0) and a stand collar (COLLAR_HEIGHT > 0)."""
    V_NECK = False
    FIT_GAP = 0.013  # regular fit: a little more room than the game shirts
    PLACKET_WIDTH = 0.028  # both halves
    PLACKET_LENGTH = 0.12  # below the front neck
    COLLAR_HEIGHT = 0.032
    COLLAR_GAP = 0.007  # the collar's top edge to the neck
    # armhole (underarm -> shoulder point): inner control points as offsets
    # from the underarm and the shoulder point
    ARMHOLE_C1 = (-0.02, 0.035)
    ARMHOLE_C2 = (-0.008, -0.09)

    # samples
    N_SIDE = 22  # up the side seam
    N_ARM_LO = 8  # up the armhole to the placket bottom's height
    N_ARM_HI = 10  # from there to the shoulder point
    N_NECK = {"front": 9, "back": 10}  # along the neck edge (front: from the placket)
    N_SHOULDER = 8  # along the shoulder seam
    N_PLACKET = 3  # across the placket half
    N_SLEEVE = 10  # sleeve rows, armhole to cuff
    RELAX = 30

    def __init__(self, body):
        super().__init__(body)
        m = self.m
        self.seams = with_mirrored([
            (m.HEM_SIDE, m.WAIST_SIDE),
            (m.WAIST_SIDE, m.UNDERARM),
            (m.UNDERARM, m.CUFF_BOTTOM),
            (m.CUFF_TOP, m.SHOULDER),
            (m.SHOULDER, m.NECK_SIDE),
        ])
        u, s = m.UNDERARM, m.SHOULDER
        self.armhole = cubic(u, (u[0] + self.ARMHOLE_C1[0], u[1] + self.ARMHOLE_C1[1]),
                             (s[0] + self.ARMHOLE_C2[0], s[1] + self.ARMHOLE_C2[1]), s)
        # the front and back are split at the placket bottom's height (also
        # without a placket, so the side seams line up)
        self.split_z = m.FRONT_NECK_BOTTOM - max(self.PLACKET_LENGTH, 0.12)
        assert self.split_z > u[1], "the split must cross the armhole"
        k = next(i for i, p in enumerate(self.armhole) if p[1] >= self.split_z)
        a, b = self.armhole[k - 1], self.armhole[k]
        t = (self.split_z - a[1]) / (b[1] - a[1])
        self.split = (a[0] + (b[0] - a[0]) * t, self.split_z)
        self.arm_lo = resample(self.armhole[:k] + [self.split], self.N_ARM_LO)
        self.arm_hi = resample([self.split] + self.armhole[k:], self.N_ARM_HI)

    def neck_edges(self, panel):
        """The neck edge; there is no collar band in the shirt (the stand
        collar sits on top of the edge)."""
        outer = self.neck_dense(panel)
        return outer, outer

    def panel_grids(self, sheet, panel, collar_lengths=None):
        m = self.m
        front = panel == "front"
        neck = self.neck_dense(panel)
        hw = self.PLACKET_WIDTH / 2 if front and self.PLACKET_LENGTH > 0 else 0.0
        k_p = next(i for i, p in enumerate(neck) if p[0] >= hw)
        z0 = self.split_z

        neck_top = resample(neck[k_p:], self.N_NECK[panel])
        shoulder = resample([m.NECK_SIDE, m.SHOULDER], self.N_SHOULDER)
        top = neck_top + shoulder[1:]
        cols = len(top)
        upper_bottom = resample([(hw, z0), self.split], cols)
        upper = relax(coons(upper_bottom, top, resample([(hw, z0), neck_top[0]], self.N_ARM_HI), self.arm_hi),
                      self.RELAX)

        side = resample([m.HEM_SIDE, m.WAIST_SIDE, m.UNDERARM], self.N_SIDE)
        right = side + self.arm_lo[1:]
        lower_top = upper_bottom
        if hw > 0:
            placket_bottom = resample([(0.0, z0), (hw, z0)], self.N_PLACKET)
            lower_top = placket_bottom + upper_bottom[1:]
        lower = relax(coons(resample([(0.0, m.HEM_SIDE[1]), m.HEM_SIDE], len(lower_top)), lower_top,
                                 resample([(0.0, m.HEM_SIDE[1]), (0.0, z0)], len(right)), right), self.RELAX)

        grids = Grids(sheet, panel)
        flip = 1 if front else -1
        for g in (upper, lower):
            grids.add(panel, g, lambda j, i, p: (flip * p[0], -p[1]))
            grids.add(panel, g, lambda j, i, p: (-flip * p[0], -p[1]), mirrored=True)
        if hw > 0:
            placket = coons(placket_bottom, resample(neck[:k_p] + [neck_top[0]], self.N_PLACKET),
                                 resample([(0.0, z0), neck[0]], self.N_ARM_HI),
                                 resample([(hw, z0), neck_top[0]], self.N_ARM_HI))
            grids.add("placket", placket, lambda j, i, p: (p[0], -p[1]))
            grids.add("placket", placket, lambda j, i, p: (-p[0], -p[1]), mirrored=True)

        # sleeve: rows from the armhole to the cuff, columns from the
        # underarm seam to the top seam. Laid out as seen from the front, turned
        # so the top seam runs straight down from the shoulder point
        armhole = self.arm_lo + self.arm_hi[1:]
        sleeve = coons(armhole, resample([m.CUFF_BOTTOM, m.CUFF_TOP], len(armhole)),
                            resample([m.UNDERARM, m.CUFF_BOTTOM], self.N_SLEEVE),
                            resample([m.SHOULDER, m.CUFF_TOP], self.N_SLEEVE))
        d = Vector((m.CUFF_TOP[0] - m.SHOULDER[0], m.CUFF_TOP[1] - m.SHOULDER[1])).normalized()
        n = Vector((d.y, -d.x))  # towards the underarm
        local = lambda p: (Vector((p[0] - m.SHOULDER[0], p[1] - m.SHOULDER[1])).dot(n),
                           Vector((p[0] - m.SHOULDER[0], p[1] - m.SHOULDER[1])).dot(d))
        for piece, s, mirrored in (("sleeve_l", -1 if front else 1, False), ("sleeve_r", 1 if front else -1, True)):
            grids.add(piece, sleeve, lambda j, i, p, s=s: (s * local(p)[0], local(p)[1]), mirrored)
            cuff = [local(p) for p in sleeve[-1]]
            sheet.add_cuff(piece, max(c[1] for c in cuff), max(c[0] for c in cuff))
        return grids.items

    def collar_ring(self, sheet, shirt):
        """The stand collar on the fitted shirt's neckline: a strip from the
        front centre once around the neck back to it, standing up and
        leaning in a little towards the neck."""
        bm = bmesh.new()
        bm.from_mesh(shirt.data)
        loops = boundary_loops(bm)
        neck = max(loops, key=lambda loop: sum(v.z for v in loop) / len(loop))
        bm.free()
        # start at the front centre, run towards +x (the wearer's left)
        start = min(range(len(neck)), key=lambda i: abs(neck[i].x) + (0 if neck[i].y < 0 else 1))
        neck = neck[start:] + neck[:start]
        if neck[1].x < 0:
            neck = [neck[0]] + list(reversed(neck[1:]))
        neck = neck + [neck[0]]
        along = [0.0]
        for a, b in zip(neck, neck[1:]):
            along.append(along[-1] + (b - a).length)

        out = bmesh.new()
        uv_layer = out.loops.layers.uv.new("UVMap")
        piece_layer = out.faces.layers.int.new("piece")
        h = self.COLLAR_HEIGHT
        # the top edge runs smoother than the neckline (which rises steeply at
        # the neck sides): the neckline's height averaged along the ring, with
        # the collar kept within 25% of its height
        n = len(neck) - 1
        window = max(n // 16, 1)
        top_z = [sum(neck[(i + k) % n].z for k in range(-window, window + 1)) / (2 * window + 1) + h
                 for i in range(n)] + [None]
        top_z = [min(max(z, neck[i].z + 0.75 * h), neck[i].z + 1.25 * h) if z is not None else None
                 for i, z in enumerate(top_z)]
        top_z[-1] = top_z[0]
        # the top edge closes around the neck, COLLAR_GAP off its surface
        tree = BVHTree.FromObject(self.body.obj, bpy.context.evaluated_depsgraph_get())
        top = []
        for p, z in zip(neck, top_z):
            loc, normal, _, _ = tree.find_nearest(Vector((p.x, p.y, z)))
            q = loc + normal * self.COLLAR_GAP
            top.append(Vector((q.x, q.y, z)))
        for _ in range(4):  # even out the body's surface detail
            top = [top[0]] + [(top[i - 1] + 2 * top[i] + top[i + 1]) / 4 for i in range(1, n)] + [top[0]]
        rows = []
        for r in range(4):
            f = r / 3
            rows.append([out.verts.new(p.lerp(q, f)) for p, q in zip(neck, top)])
        for r in range(3):
            for i in range(len(neck) - 1):
                quad = [rows[r][i], rows[r][i + 1], rows[r + 1][i + 1], rows[r + 1][i]]
                face = out.faces.new(quad)
                face[piece_layer] = sheet.index("collar")
                for loop, (rr, ii) in zip(face.loops, ((r, i), (r, i + 1), (r + 1, i + 1), (r + 1, i))):
                    loop[uv_layer].uv = (along[ii], h * (1 - rr / 3))
        outline = ([(a, h) for a in along] + [(a, 0.0) for a in reversed(along)])
        sheet.add_outline("collar", outline)
        bmesh.ops.recalc_face_normals(out, faces=out.faces[:])
        obj = new_object("collar", out)
        set_smooth(obj)
        return obj

    def build(self, rows=ROWS, pieces=PIECES):
        sheet = Sheet(rows, pieces)
        panels = [(panel, self.panel_grids(sheet, panel)) for panel in ("front", "back")]
        obj = pillow_mesh("top", sheet, panels, self.seams, self.depth, hem_weight(self.m.HEM_SIDE[1]))
        fit_to_body(obj, self.body, gap=self.FIT_GAP)
        lower_arms(obj, self.body, gap=self.FIT_GAP)
        objects = [obj]
        if self.COLLAR_HEIGHT > 0:
            collar = self.collar_ring(sheet, obj)
            collar["subdivide"] = False
            objects.append(collar)
        return sheet, objects


class WomenPoloShirt(PoloShirt):
    PLACKET_LENGTH = 0.11


register(lambda body, info: PoloShirt(body).build(), id="polo-shirt-man", name="Polo Shirt",
         kind="top", body="man", order=220)
register(lambda body, info: WomenPoloShirt(body).build(), id="polo-shirt-women", name="Polo Shirt",
         kind="top", body="women", order=230)
