"""
Prepares the (invisible) mannequins the garments are fitted onto.

    blender --background assets-src/human-base-meshes/human-base-meshes-bundle-v1.4.1/human_base_meshes_bundle.blend \
        --python blender/prepare_mannequin.py [-- --body man|women] [--render <dir>]

Takes Blender Studio's CC0 "realistic male" (or "female") base mesh, raises
the arms from the bundle's A-pose to ~45 degrees (so the sleeves have room
under the arm while fitting), bakes the sculpted multires detail at level 2
so muscles read through the fabric, and saves the result as
assets-src/mannequin-<body>.blend (object "mannequin", with the arm weights
as vertex group "arm" and the shoulder pivot as custom property
"shoulder_pivot", so the garment builds can lower the arms again). Both base
meshes share their topology and face sets.
"""

import math
import os
import sys

import bpy
from mathutils import Matrix, Vector
from mathutils.kdtree import KDTree

# Face-set ids of the base mesh's sculpt face sets that make up the arms.
UPPER_ARM_SETS = {20, 21}
FOREARM_SETS = {11, 12}
HAND_SETS = {9, 10} | set(range(64, 104))
ARM_SETS = UPPER_ARM_SETS | FOREARM_SETS | HAND_SETS

# body -> (base mesh object, shoulder pivot in metres (right side; the left
# is mirrored), arm raise in degrees)
BODIES = {
    "man": ("GEO-body_male_realistic", Vector((0.175, -0.01, 1.335)), 22),
    "women": ("GEO-body_female_realistic", Vector((0.148, 0.004, 1.31)), 22),
}
SMOOTH_ITERATIONS = 10  # arm weight smoothing passes
DETAIL_LEVEL = 2  # multires level baked into the mannequin

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def out_path(body):
    return os.path.join(REPO_ROOT, "assets-src", f"mannequin-{body}.blend")


def arm_weights(me):
    """Per-vertex weight (0..1) of the arm: 1 on the arm's face sets, with a
    smoothed falloff around them."""
    face_sets = me.attributes[".sculpt_face_set"].data
    weights = [0.0] * len(me.vertices)
    for poly, fs in zip(me.polygons, face_sets):
        if fs.value in ARM_SETS:
            for v in poly.vertices:
                weights[v] = 1.0

    neighbors = [[] for _ in me.vertices]
    for e in me.edges:
        a, b = e.vertices
        neighbors[a].append(b)
        neighbors[b].append(a)

    # soften the boundary so the shoulder bends instead of shearing
    for _ in range(SMOOTH_ITERATIONS):
        weights = [
            0.5 * w + 0.5 * (sum(weights[n] for n in nbrs) / len(nbrs)) if nbrs else w
            for w, nbrs in zip(weights, neighbors)
        ]
    return weights


def raise_arms(obj, shoulder_pivot, degrees):
    """Raises the weighted arm vertices of `obj` about the shoulder pivots."""
    me = obj.data
    weights = arm_weights(me)
    obj["arm_weights"] = weights  # reused for the baked mesh, see store_arm_weights
    for v, w in zip(me.vertices, weights):
        if w <= 0:
            continue
        side = 1 if v.co.x > 0 else -1
        pivot = Vector((shoulder_pivot.x * side, shoulder_pivot.y, shoulder_pivot.z))
        angle = -side * math.radians(degrees) * w
        v.co = pivot + Matrix.Rotation(angle, 3, "Y") @ (v.co - pivot)
    me.update()


def bake_detail(obj):
    """A new object "mannequin" with the multires modifier baked at DETAIL_LEVEL."""
    obj.modifiers[0].levels = DETAIL_LEVEL
    evaluated = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
    mesh = bpy.data.meshes.new_from_object(evaluated)
    return bpy.data.objects.new("mannequin", mesh)


def store_arm_weights(src, baked):
    """Copy the arm weights to the baked (subdivided) mesh as vertex group "arm".

    The garment builds (oj/body.py) use them to lower the arms again after fitting.
    """
    base = src.data.vertices
    tree = KDTree(len(base))
    for i, v in enumerate(base):
        tree.insert(v.co, i)
    tree.balance()
    weights = src["arm_weights"]
    group = baked.vertex_groups.new(name="arm")
    for v in baked.data.vertices:
        _, i, _ = tree.find(v.co)
        if weights[i] > 0:
            group.add([v.index], weights[i], "REPLACE")


def render_preview(obj, out_dir, body):
    """Renders a front view of the mannequin to <out_dir>/mannequin_<body>_front.png."""
    scene = bpy.context.scene
    for o in scene.objects:
        o.hide_render = True
    scene.collection.objects.link(obj)
    obj.hide_render = False
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = 16
    scene.render.resolution_x = 500
    scene.render.resolution_y = 700
    cam = bpy.data.objects.new("pcam", bpy.data.cameras.new("pcam"))
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam.location = (0, -3.2, 1.1)
    cam.rotation_euler = (Vector((0, 0, 1.1)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    sun = bpy.data.objects.new("psun", bpy.data.lights.new("psun", "SUN"))
    sun.data.energy = 3
    sun.rotation_euler = (0.8, 0.1, 0.4)
    scene.collection.objects.link(sun)
    scene.render.filepath = os.path.join(out_dir, f"mannequin_{body}_front.png")
    bpy.ops.render.render(write_still=True)


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    render_dir = argv[argv.index("--render") + 1] if "--render" in argv else None
    body = argv[argv.index("--body") + 1] if "--body" in argv else "man"
    source, shoulder_pivot, raise_degrees = BODIES[body]

    src = bpy.data.objects[source]
    src.location = (0, 0, 0)
    raise_arms(src, shoulder_pivot, raise_degrees)
    mannequin = bake_detail(src)
    store_arm_weights(src, mannequin)
    mannequin["shoulder_pivot"] = list(shoulder_pivot)

    if render_dir:
        render_preview(mannequin, render_dir, body)

    path = out_path(body)
    bpy.data.libraries.write(path, {mannequin}, fake_user=True)
    print(f"Wrote {path} ({len(mannequin.data.vertices)} verts)")


if __name__ == "__main__":
    main()
