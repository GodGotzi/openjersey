"""Cycles preview renders of garments, with or without the body."""

import os

import bpy
from mathutils import Vector

VIEWS = {
    # name: (camera position, hide body, look-at height)
    "front": ((0, -3.0, 1.05), True, 1.0),
    "neck": ((0, -1.1, 1.45), True, 1.38),
    "three_quarter": ((1.8, -2.3, 1.2), True, 1.0),
    "back": ((0, 3.0, 1.05), True, 1.0),
    "side": ((3.0, 0, 1.05), True, 1.0),
    "with_body": ((0.9, -2.8, 1.2), False, 1.0),
    "with_body_back": ((-0.9, 2.8, 1.2), False, 1.0),
    # whole figure, for full-length bottoms
    "full_front": ((0, -4.6, 0.8), True, 0.75),
    "full_back": ((0, 4.6, 0.8), True, 0.75),
    "full_side": ((4.6, 0, 0.8), True, 0.75),
    "full_with_body": ((1.4, -4.4, 1.0), False, 0.8),
}


def render(objects, body, out_dir, prefix, views=None, colours=None):
    """Renders `objects` (grey, or the RGB in `colours[i]`) from each view
    to <out_dir>/<prefix>_<view>.png."""
    for i, obj in enumerate(objects):
        mat = bpy.data.materials.new(f"preview{i}")
        bsdf = mat.node_tree.nodes.get("Principled BSDF")
        rgb = colours[i] if colours else (0.85, 0.85, 0.87)
        bsdf.inputs["Base Color"].default_value = (*rgb, 1)
        bsdf.inputs["Roughness"].default_value = 0.8
        obj.data.materials.clear()
        obj.data.materials.append(mat)

    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = 24
    scene.render.resolution_x = 700
    scene.render.resolution_y = 700
    world = bpy.data.worlds.new("world")
    scene.world = world
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.8, 0.8, 0.82, 1)
    bg.inputs[1].default_value = 0.6
    key = bpy.data.objects.new("key", bpy.data.lights.new("key", "AREA"))
    key.data.energy = 300
    key.data.size = 2
    key.location = (1.2, -1.8, 2.2)
    key.rotation_euler = (Vector((0, 0, 1.15)) - key.location).to_track_quat("-Z", "Y").to_euler()
    bpy.context.collection.objects.link(key)

    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    cam.data.lens = 70
    bpy.context.collection.objects.link(cam)
    scene.camera = cam
    os.makedirs(out_dir, exist_ok=True)
    for name in views or VIEWS:
        pos, ghost, target_z = VIEWS[name]
        body.obj.hide_render = ghost
        cam.location = pos
        cam.rotation_euler = (Vector((0, 0, target_z)) - Vector(pos)).to_track_quat("-Z", "Y").to_euler()
        scene.render.filepath = os.path.join(out_dir, f"{prefix}_{name}.png")
        bpy.ops.render.render(write_still=True)
