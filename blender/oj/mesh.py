"""Garment meshes from pattern grids and fitting them onto the body.

How a top is made: its front and back pattern pieces (front-view metres)
are joined into one mesh along the seams and inflated into a loose
"pillow" that clears the body (`pillow_mesh`). Alternating shrinkwrap and
smoothing passes then pull it onto the body with a small gap, like a fitted
volleyball jersey: chest and shoulders show through, the fabric bridges
small hollows (`fit_to_body`). (A cloth simulation was tried before, but
left the jersey baggy and boxy.) Then the arms of body and garment are
lowered to the mockups' pose together (`lower_arms`).

Every face carries its piece in the int face attribute "piece" (an index
into the sheet's pieces) and its piece-local layout coordinates (metres,
ly down) in the UV map "UVMap"; `Sheet.assign_atlas` turns those into
atlas UVs."""

import math

import bmesh
import bpy
from mathutils import Vector
from mathutils.kdtree import KDTree

from .body import ARM_DROP_DEGREES, FIT_GAP
from .geom import grid_outline, mirror, seam_distance, smoothstep

FIT_ITERATIONS = 6


class Grid:
    """One grid of a piece, in front-view pattern coordinates (x, z), with
    piece-local layout coordinates (lx, ly: metres, ly down) per point."""

    def __init__(self, piece, panel, pts, local):
        self.piece, self.panel, self.pts, self.local = piece, panel, pts, local


class Grids:
    """Collects the grids of one panel ("front" or "back") and records each
    grid's outline on the sheet."""

    def __init__(self, sheet, panel):
        self.sheet, self.panel, self.items = sheet, panel, []

    def add(self, piece, pts, local_fn, mirrored=False):
        """`local_fn(row, col, point)` gives the piece-local layout point;
        `mirrored` puts the grid on the other half (x -> -x) after the
        layout is taken."""
        local = [[local_fn(j, i, p) for i, p in enumerate(row)] for j, row in enumerate(pts)]
        self.sheet.add_outline(piece, grid_outline(local))
        if mirrored:
            pts = [[mirror(p) for p in row] for row in pts]
        self.items.append(Grid(piece, self.panel, pts, local))


def collar_halves(front, front_length, back_length):
    """(x offset, direction, mirrored) of the two halves of the collar strip
    as seen from one panel. The strip runs front (x > 0) -> back (x > 0) ->
    back (x < 0) -> front (x < 0), joined at the shoulder seams; the lengths
    are those of one half's front and back neck edge."""
    if front:
        return ((0.0, 1, False), (2 * front_length + 2 * back_length, -1, True))
    return ((front_length + back_length, -1, False), (front_length + back_length, 1, True))


def pillow_depth(p, rho, seams):
    """Half-depth of the pillow at pattern point p: 0 on the seams, rising
    like a circle of radius rho."""
    d = min(seam_distance(p, seams), rho)
    return math.sqrt(max(rho * rho - (rho - d) ** 2, 0.0))


def hem_weight(hem_z):
    """Fit weight: the starting fit pulls the hem a bit less (looser)."""
    return lambda p: 0.82 + 0.18 * smoothstep(hem_z, 1.12, p[1])


def pillow_mesh(name, sheet, panels, seams, depth, weight):
    """One mesh from the grids of front and back. `panels` is a list of
    (panel, grids) with panel "front" (placed at -y) or "back" (+y);
    points on `seams` are shared between front and back, everything else
    is per panel. `depth(p, panel)` is the pillow half-depth, `weight(p)`
    the fit weight (vertex group "fit")."""
    bm = bmesh.new()
    # layers first: adding one later invalidates the element references
    deform = bm.verts.layers.deform.verify()
    uv_layer = bm.loops.layers.uv.new("UVMap")
    piece_layer = bm.faces.layers.int.new("piece")
    verts = {}
    pending = []  # (face, {vert: local}, piece)
    for panel, grids in panels:
        sign = -1 if panel == "front" else 1
        for grid in grids:
            vgrid = []
            for row in grid.pts:
                vrow = []
                for p in row:
                    on_seam = seam_distance(p, seams) < 1e-7
                    key = (round(p[0], 6), round(p[1], 6)) + (() if on_seam else (panel,))
                    if key not in verts:
                        v = bm.verts.new(Vector((p[0], sign * depth(p, panel), p[1])))
                        v[deform][0] = weight(p)
                        verts[key] = v
                    vrow.append(verts[key])
                vgrid.append(vrow)
            for j in range(len(vgrid) - 1):
                for i in range(len(vgrid[0]) - 1):
                    idx = [(j, i), (j, i + 1), (j + 1, i + 1), (j + 1, i)]
                    quad = [vgrid[a][b] for a, b in idx]
                    if len(set(quad)) < 4:
                        continue  # degenerate corner cell
                    face = bm.faces.new(quad)
                    pending.append((face, {v: grid.local[a][b] for v, (a, b) in zip(quad, idx)}, grid.piece))

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    for face, local, piece in pending:
        face[piece_layer] = sheet.index(piece)
        for loop in face.loops:  # by vertex: recalc may have flipped the loop order
            loop[uv_layer].uv = local[loop.vert]
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(name, mesh)
    obj.vertex_groups.new(name="fit")
    bpy.context.collection.objects.link(obj)
    return obj


def bake_modifiers(obj):
    """Applies all modifiers; returns the new mesh."""
    depsgraph = bpy.context.evaluated_depsgraph_get()
    evaluated = obj.evaluated_get(depsgraph)
    mesh = bpy.data.meshes.new_from_object(evaluated, preserve_all_data_layers=True, depsgraph=depsgraph)
    obj.modifiers.clear()
    obj.data = mesh
    return mesh


def smooth_edges(bm, passes=3):
    """Evens out hem, cuff and neck edges along themselves (without pulling
    them into the fabric)."""
    edge = [v for v in bm.verts if v.is_boundary]
    for _ in range(passes):
        moved = {}
        for v in edge:
            if len(v.link_faces) <= 2:
                # sharp corner (where front and back collar meet): the fit
                # pushes it along the body normal every pass, so pull it fully
                # back between its neighbours
                nbrs = [e.other_vert(v).co for e in v.link_edges]
                moved[v] = sum(nbrs, Vector()) / len(nbrs)
                continue
            nbrs = [e.other_vert(v) for e in v.link_edges if e.is_boundary]
            if len(nbrs) == 2:
                moved[v] = v.co * 0.5 + (nbrs[0].co + nbrs[1].co) * 0.25
        for v, co in moved.items():
            v.co = co


def smooth_inner(obj, passes, factor=0.5, edges=True):
    """Smooths the interior (and, with `edges`, the boundary along itself)."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    inner = [v for v in bm.verts if not v.is_boundary]
    for _ in range(passes):
        bmesh.ops.smooth_vert(bm, verts=inner, factor=factor, use_axis_x=True, use_axis_y=True, use_axis_z=True)
    if edges:
        smooth_edges(bm)
    bm.to_mesh(obj.data)
    bm.free()


def shrinkwrap(obj, target, gap, mode="OUTSIDE_SURFACE", group=None):
    wrap = obj.modifiers.new("Wrap", "SHRINKWRAP")
    wrap.target = target
    wrap.wrap_method = "NEAREST_SURFACEPOINT"
    wrap.wrap_mode = mode
    wrap.offset = gap
    if group:
        wrap.vertex_group = group
    return bake_modifiers(obj)


def fit_to_body(garment, body, drop_arms=False, gap=FIT_GAP, iterations=FIT_ITERATIONS):
    """Pulls the loose pillow onto the body (keeping `gap`) and relaxes it.

    Alternating shrinkwrap and smoothing spreads the fabric evenly over the
    body instead of bunching it where many pillow points share the same
    nearest body point, and bridges small hollows (navel, armpit crease).
    """
    target = body.fitting_target(drop_arms)
    for i in range(iterations):
        shrinkwrap(garment, target, gap, group="fit")
        # the last pass only softens muscle detail a little, like fabric bridging it
        smooth_inner(garment, 4 if i == iterations - 1 else 5)
    bpy.data.objects.remove(target)
    set_smooth(garment)


def set_smooth(obj):
    obj.data.polygons.foreach_set("use_smooth", [True] * len(obj.data.polygons))


def lower_arms(garment, body, gap=FIT_GAP):
    """Lowers the arms of body and garment together, like a skinned mesh.

    Each garment vertex takes the arm weight of the nearest body vertices,
    so sleeves turn with the arm and the shoulder bends smoothly. Fabric
    pushed into the body under the arm is then pushed back out. For tops
    without sleeves call `body.lower_arms()` alone instead.
    """
    tree = KDTree(len(body.arm))
    for v in body.obj.data.vertices:
        tree.insert(v.co, v.index)
    tree.balance()
    weights = []
    for v in garment.data.vertices:
        near = tree.find_n(v.co, 8)
        weights.append(sum(body.arm[i] for _, i, _ in near) / len(near))
    body.lower_arms()
    body.rotate_arms(garment.data, weights, ARM_DROP_DEGREES)

    target = body.fitting_target()
    for i in range(3):
        shrinkwrap(garment, target, gap, mode="OUTSIDE")
        if i < 2:
            smooth_inner(garment, 1, edges=False)
    bpy.data.objects.remove(target)
    set_smooth(garment)
