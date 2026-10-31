"""Fabric thickness, the inside swatch and the GLB export."""

import os

import bpy
from mathutils.kdtree import KDTree

from .mesh import bake_modifiers

THICKNESS = 0.0015  # fabric thickness (m)


def finish(garment, sheet, body, subdivide=True):
    """Subdivides (once, for meshes built from coarse pattern grids) and
    gives the fabric its thickness, then maps the inner surface to the
    sheet's plain inside swatch (the rims keep the print's edge)."""
    if subdivide:
        sub = garment.modifiers.new("Subsurf", "SUBSURF")
        sub.levels = sub.render_levels = 1
    garment.vertex_groups.new(name="shell")
    solid = garment.modifiers.new("Thickness", "SOLIDIFY")
    solid.thickness = THICKNESS
    solid.offset = 1
    solid.shell_vertex_group = "shell"
    mesh = bake_modifiers(garment)

    group = garment.vertex_groups["shell"].index
    in_shell = [any(g.group == group and g.weight > 0.5 for g in v.groups) for v in mesh.vertices]
    shell = [p.index for p in mesh.polygons if all(in_shell[v] for v in p.vertices)]
    original = [p.index for p in mesh.polygons if not any(in_shell[v] for v in p.vertices)]

    # the inner surface is the one facing the body. Solidify flips the
    # original faces (so the solid's normals point out of the fabric): if the
    # baked original faces point towards the body (checked on a sample of
    # faces), the original surface is the inner one, else the shell is
    tree = KDTree(len(body.obj.data.vertices))
    for bv in body.obj.data.vertices:
        tree.insert(bv.co, bv.index)
    tree.balance()
    outward = 0
    for f in original[::max(len(original) // 400, 1)]:
        poly = mesh.polygons[f]
        near, _, _ = tree.find(poly.center)
        outward += 1 if poly.normal.dot(poly.center - near) > 0 else -1
    inner = original if outward < 0 else shell
    u, v = sheet.inside_uv()
    uvs = mesh.uv_layers["UVMap"].data
    for f in inner:
        for li in mesh.polygons[f].loop_indices:
            uvs[li].uv = (u, v)


def export_glb(objects, path):
    """Exports just `objects` (modifiers applied) as a GLB."""
    for o in bpy.context.scene.objects:
        o.select_set(False)
    for o in objects:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    os.makedirs(os.path.dirname(path), exist_ok=True)
    bpy.ops.export_scene.gltf(filepath=path, export_format="GLB", use_selection=True, export_apply=True)
