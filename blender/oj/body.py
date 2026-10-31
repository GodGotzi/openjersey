"""The posed mannequins (assets-src/mannequin-<body>.blend, see
prepare_mannequin.py), their measurements and the arm pose.

The arms are raised while fitting, so sleeves have room under them; tops
lower body and garment together afterwards (`lower_arms`). Bottoms never
touch the arms. Every garment of one body is fitted on the same posed
mannequin, so any top goes with any bottom."""

import math
import os
from types import SimpleNamespace

import bmesh
import bpy
from mathutils import Matrix, Vector

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# Pattern landmarks per body, in front-view metres (x right = wearer's left,
# z up), right half; measured on the posed mannequins (see GARMENTS.md,
# "Measurements"). The tops' side seam runs HEM_SIDE -> WAIST_SIDE ->
# UNDERARM, the shoulder seam NECK_SIDE -> SHOULDER; the sleeve follows the
# raised upper arm, CUFF_BOTTOM/CUFF_TOP its opening at full sleeve length.
MEASUREMENTS = {
    "man": SimpleNamespace(
        HEM_SIDE=(0.245, 0.86),
        WAIST_SIDE=(0.232, 1.03),
        UNDERARM=(0.255, 1.17),
        # the upper-arm axis runs ~42 degrees down through (0.30, 1.22);
        # cuff half-width ~0.085 = biceps + ~10% ease
        CUFF_BOTTOM=(0.313, 1.091),
        CUFF_TOP=(0.427, 1.217),
        SHOULDER=(0.215, 1.405),
        NECK_SIDE=(0.052, 1.465),
        FRONT_NECK_BOTTOM=1.405,
        BACK_NECK_BOTTOM=1.45,
        # half-depth of the starting pillow; must clear the body everywhere
        RHO_FRONT=0.175,
        RHO_BACK=0.14,
        RHO_SLEEVE=0.095,
        # the fitting target loses head and neck (a slightly flared cylinder
        # around the neck, the chin and the throat), so necklines settle
        # around the neck base instead of climbing the neck
        NECK_CUT_Z=1.50,
        NECK_BASE_Z=1.39,
        NECK_CENTER_Y=-0.02,
        NECK_RADIUS=0.056,
        NECK_FLARE=0.1,  # neck radius growth per metre of height
        CHIN_Z=1.44,
        CHIN_Y=-0.075,
        THROAT_Z=1.40,
        THROAT_Y=-0.055,
        THROAT_HALF_WIDTH=0.06,
        # bottoms: waist (hidden under any top's hem), thigh axes, crotch;
        # TOP_HEM_Z is the shortest top's hem (bottoms keep close above it)
        WAIST_Z=0.95,
        TOP_HEM_Z=0.86,
        CROTCH_Z=0.74,
        THIGH_AXIS_Z=0.74,
        ANKLE_Z=0.11,  # narrowest above the foot (full-length bottoms' hem)
    ),
    "women": SimpleNamespace(
        # shorter torso, fitted waist, hem flaring over the hips
        HEM_SIDE=(0.233, 0.85),
        WAIST_SIDE=(0.19, 1.03),
        UNDERARM=(0.226, 1.145),
        CUFF_BOTTOM=(0.27, 1.083),
        CUFF_TOP=(0.367, 1.17),
        SHOULDER=(0.183, 1.38),
        NECK_SIDE=(0.044, 1.425),
        FRONT_NECK_BOTTOM=1.365,
        BACK_NECK_BOTTOM=1.41,
        RHO_FRONT=0.185,
        RHO_BACK=0.13,
        RHO_SLEEVE=0.08,
        NECK_CUT_Z=1.46,
        NECK_BASE_Z=1.35,
        NECK_CENTER_Y=-0.02,
        NECK_RADIUS=0.047,
        NECK_FLARE=0.1,
        CHIN_Z=1.41,
        CHIN_Y=-0.075,
        THROAT_Z=1.36,
        THROAT_Y=-0.05,
        THROAT_HALF_WIDTH=0.05,
        WAIST_Z=0.95,
        TOP_HEM_Z=0.85,
        CROTCH_Z=0.76,
        THIGH_AXIS_Z=0.76,
        ANKLE_Z=0.11,
    ),
}

FIT_GAP = 0.009  # fabric to skin (m)
ARM_DROP_DEGREES = 16  # how far tops lower the raised arms after fitting
ARMLESS_WEIGHT = 0.3  # arm weight above which a sleeveless fit ignores the body


class Body:
    """A posed mannequin in the scene: `obj`, the measurements `m`, the
    shoulder pivot and the arm weights."""

    def __init__(self, name):
        self.name = name
        self.m = MEASUREMENTS[name]
        path = os.path.join(REPO_ROOT, "assets-src", f"mannequin-{name}.blend")
        with bpy.data.libraries.load(path) as (_, dst):
            dst.objects = ["mannequin"]
        self.obj = dst.objects[0]
        bpy.context.collection.objects.link(self.obj)
        self.pivot = tuple(self.obj["shoulder_pivot"])
        self.arms_lowered = False
        group = self.obj.vertex_groups["arm"].index
        self.arm = [0.0] * len(self.obj.data.vertices)
        for v in self.obj.data.vertices:
            for g in v.groups:
                if g.group == group:
                    self.arm[v.index] = g.weight

    def rotate_arms(self, mesh, weights, degrees):
        """Turns weighted vertices of `mesh` about the shoulder pivots
        (positive = down)."""
        for v, w in zip(mesh.vertices, weights):
            if w <= 0:
                continue
            side = 1 if v.co.x > 0 else -1
            pivot = Vector((self.pivot[0] * side, self.pivot[1], self.pivot[2]))
            v.co = pivot + Matrix.Rotation(side * math.radians(degrees) * w, 3, "Y") @ (v.co - pivot)
        mesh.update()

    def lower_arms(self):
        """Lowers the body's arms to the mockups' pose (once)."""
        if not self.arms_lowered:
            self.rotate_arms(self.obj.data, self.arm, ARM_DROP_DEGREES)
            self.arms_lowered = True

    def fitting_target(self, drop_arms=False, name="fit_target"):
        """The body without neck and head (and, with drop_arms, without the
        arms, so armholes of sleeveless tops don't cling to them), as a new
        hidden object. Remove it with bpy.data.objects.remove when done.

        The nearest body point for the fabric around a neckline is otherwise
        the neck, and pulling it there turns the neckline into a stand-up
        collar."""
        m = self.m
        bm = bmesh.new()
        bm.from_mesh(self.obj.data)
        bm.verts.ensure_lookup_table()
        cut = [
            v for v in bm.verts
            if (drop_arms and self.arm[v.index] > ARMLESS_WEIGHT)
            or v.co.z > m.NECK_CUT_Z
            or (v.co.z > m.CHIN_Z and v.co.y < m.CHIN_Y)
            or (v.co.z > m.THROAT_Z and v.co.y < m.THROAT_Y and abs(v.co.x) < m.THROAT_HALF_WIDTH)
            or (v.co.z > m.NECK_BASE_Z
                and math.hypot(v.co.x, v.co.y - m.NECK_CENTER_Y) < m.NECK_RADIUS + m.NECK_FLARE * (v.co.z - m.NECK_BASE_Z))
        ]
        bmesh.ops.delete(bm, geom=cut, context="VERTS")
        return new_object(name, bm, hidden=True)

    def surface_band(self, z0, z1, max_x=0.28):
        """A copy of the body surface between heights z0 and z1 (|x| <
        max_x, largest connected part), without the body's groups and UVs,
        as a bmesh — the start of bottoms cut from the body."""
        bm = bmesh.new()
        bm.from_mesh(self.obj.data)
        for layer in list(bm.verts.layers.deform):
            bm.verts.layers.deform.remove(layer)
        for layer in list(bm.loops.layers.uv):
            bm.loops.layers.uv.remove(layer)
        cut = [v for v in bm.verts if not (z0 <= v.co.z <= z1) or abs(v.co.x) > max_x]
        bmesh.ops.delete(bm, geom=cut, context="VERTS")
        largest_island(bm)
        return bm


def new_object(name, bm, hidden=False):
    """A new scene object holding (and freeing) `bm`."""
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    obj.hide_render = hidden
    return obj


def largest_island(bm):
    """Deletes everything but the largest connected part of `bm`."""
    seen, best = set(), []
    for start in bm.verts:
        if start in seen:
            continue
        island, stack = [], [start]
        seen.add(start)
        while stack:
            v = stack.pop()
            island.append(v)
            for e in v.link_edges:
                o = e.other_vert(v)
                if o not in seen:
                    seen.add(o)
                    stack.append(o)
        if len(island) > len(best):
            best = island
    keep = set(best)
    bmesh.ops.delete(bm, geom=[v for v in bm.verts if v not in keep], context="VERTS")
