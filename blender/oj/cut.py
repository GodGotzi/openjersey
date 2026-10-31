"""Garments cut straight from the (dense) body surface, for close-fitting
pieces such as bikini bottoms and sports tops: copy the skin, relax it a
little, cut it along smooth contours (`iso_cut`) and unwrap the pieces
(`conformal_unwrap` for panels, `strip_uv` for bands and bindings).

Typical use:

    bm = body.surface_band(z0, z1)
    ... offset / smooth ...
    iso_cut(bm, lambda co: outline_value(co))          # keeps value < 0
    iso_cut(bm, lambda co: band_value(co), delete=False)  # a piece border
    ... set the "piece" face layer ...
    obj = new_object(name, bm)
    conformal_unwrap(obj, {sheet.index("front"): target_front, ...})
    strip_uv(obj, sheet.index("collar"), loop)
    add_outlines(sheet, obj)
"""

import math

import bmesh
import bpy
from mathutils import Vector
from mathutils.kdtree import KDTree


def iso_cut(bm, value, delete=True):
    """Cuts `bm` along value(co) = 0 (value a smooth function of position;
    the cut is exact on each edge, interpolated linearly). With `delete`,
    removes the faces where value > 0. Returns the new vertices."""
    vals = {}
    orig = {}
    for v in bm.verts:
        x = value(v.co)
        vals[v] = x if abs(x) > 1e-9 else 1e-9
        orig[v] = v.co.copy()
        v.co = Vector((v.co.x, v.co.y, vals[v]))
    geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
    bmesh.ops.bisect_plane(bm, geom=geom, dist=1e-9, plane_co=(0, 0, 0), plane_no=(0, 0, 1))
    new = []
    for v in bm.verts:
        if v in orig:
            v.co = orig[v]
    for v in bm.verts:
        if v in orig:
            continue
        ends = [e.other_vert(v) for e in v.link_edges if e.other_vert(v) in orig]
        pair = next(((a, b) for a in ends for b in ends if vals[a] < 0 < vals[b]), None)
        if pair:
            a, b = pair
            t = vals[a] / (vals[a] - vals[b])
            v.co = orig[a].lerp(orig[b], t)
        new.append(v)
    if delete:
        newset = set(new)
        doomed = [f for f in bm.faces if sum(0.0 if v in newset else vals[v] for v in f.verts) > 0]
        bmesh.ops.delete(bm, geom=doomed, context="FACES")
        loose = [v for v in bm.verts if not v.link_faces]
        bmesh.ops.delete(bm, geom=loose, context="VERTS")
    return [v for v in new if v.is_valid]


def boundary_loops(bm):
    """The mesh's boundary as closed loops of vertex coordinates (ordered)."""
    edges = {e for e in bm.edges if e.is_boundary}
    loops = []
    while edges:
        e = edges.pop()
        start, v = e.verts
        loop = [start.co.copy(), v.co.copy()]
        while v is not start:
            nxt = next((f for f in v.link_edges if f in edges), None)
            if nxt is None:
                break
            edges.discard(nxt)
            v = nxt.other_vert(v)
            loop.append(v.co.copy())
        loops.append(loop[:-1] if v is start else loop)
    return loops


def strip_uv(obj, piece_index, loop, start=None, closed=True, flip=False, height=None):
    """Unwraps the faces of piece `piece_index` as a straight strip along
    `loop` (ordered 3D points of the edge the strip runs along): u = arc
    length along the loop (from the point nearest `start`), v = distance
    from it (or `height` minus that, so the loop's edge is at the bottom
    of the layout). `flip` reverses the direction along the loop."""
    if flip:
        loop = loop[::-1]
    if start is not None:
        i0 = min(range(len(loop)), key=lambda i: (loop[i] - Vector(start)).length)
        loop = loop[i0:] + loop[:i0]
    # dense samples with their arc length
    pts = loop + ([loop[0]] if closed else [])
    samples, s = [], 0.0
    for a, b in zip(pts, pts[1:]):
        n = max(1, int((b - a).length / 0.001))
        for k in range(n):
            samples.append((a.lerp(b, k / n), s + (b - a).length * k / n))
        s += (b - a).length
    total = s
    tree = KDTree(len(samples))
    for i, (p, _) in enumerate(samples):
        tree.insert(p, i)
    tree.balance()

    mesh = obj.data
    pieces = mesh.attributes["piece"].data
    uvs = mesh.uv_layers["UVMap"].data
    cache = {}
    for poly in mesh.polygons:
        if pieces[poly.index].value != piece_index:
            continue
        coords = []
        for vi in poly.vertices:
            if vi not in cache:
                _, i, d = tree.find(mesh.vertices[vi].co)
                cache[vi] = (samples[i][1], d)
            coords.append(cache[vi])
        if closed and max(c[0] for c in coords) - min(c[0] for c in coords) > total / 2:
            coords = [(u + total if u < total / 2 else u, d) for u, d in coords]
        for li, (u, d) in zip(poly.loop_indices, coords):
            uvs[li].uv = (u, d if height is None else height - d)
    return total


def mark_seams(obj, seam=None):
    """Marks seams between faces of different pieces, plus the edges where
    `seam(face_a_centre, face_b_centre)` is true (to open closed rings)."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    layer = bm.faces.layers.int["piece"]
    for e in bm.edges:
        fs = e.link_faces
        e.seam = False
        if len(fs) == 2:
            a, b = fs
            if a[layer] != b[layer] or (seam and seam(a.calc_center_median(), b.calc_center_median())):
                e.seam = True
    bm.to_mesh(obj.data)
    bm.free()


def conformal_unwrap(obj, targets):
    """Unwraps the pieces in `targets` {piece index: target(co) -> (lx, ly)}
    conformally (seams between pieces must be marked, see mark_seams), at
    true size, then turns and moves each piece to best match its target
    layout (e.g. (x, -z) for a front)."""
    mesh = obj.data
    pieces = mesh.attributes["piece"].data
    for p in mesh.polygons:
        p.select = pieces[p.index].value in targets
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_mode(type="FACE")
    bpy.ops.uv.unwrap(method="CONFORMAL", margin=0.0)
    bpy.ops.object.mode_set(mode="OBJECT")
    mesh = obj.data
    pieces = mesh.attributes["piece"].data
    uvs = mesh.uv_layers["UVMap"].data

    for index, target in targets.items():
        polys = [p for p in mesh.polygons if pieces[p.index].value == index]
        loops = [li for p in polys for li in p.loop_indices]
        # true size: scale by the 3D / UV area ratio
        area3 = sum(p.area for p in polys)
        area2 = 0.0
        for p in polys:
            q = [uvs[li].uv for li in p.loop_indices]
            area2 += 0.5 * sum(a.x * b.y - b.x * a.y for a, b in zip(q, q[1:] + q[:1]))
        k = math.sqrt(area3 / max(abs(area2), 1e-12))
        dst = [Vector(target(mesh.vertices[mesh.loops[li].vertex_index].co)) for li in loops]
        cd = sum(dst, Vector((0, 0))) / len(dst)
        best = None
        for sign in (1, -1):  # as unwrapped, or mirrored: whichever fits the target
            src = [Vector((uvs[li].uv.x * k * sign, uvs[li].uv.y * k)) for li in loops]
            cs = sum(src, Vector((0, 0))) / len(src)
            sxx = sxy = 0.0
            for a, b in zip(src, dst):
                a, b = a - cs, b - cd
                sxx += a.x * b.x + a.y * b.y
                sxy += a.x * b.y - a.y * b.x
            ang = math.atan2(sxy, sxx)
            c, s = math.cos(ang), math.sin(ang)
            out = [Vector((cd.x + c * (a - cs).x - s * (a - cs).y, cd.y + s * (a - cs).x + c * (a - cs).y)) for a in src]
            err = sum((o - b).length_squared for o, b in zip(out, dst))
            if best is None or err < best[0]:
                best = (err, out)
        for li, o in zip(loops, best[1]):
            uvs[li].uv = o


def add_outlines(sheet, obj, step=0.004):
    """Records each piece's outline(s) on the sheet from its faces' layout
    coordinates (the "UVMap" in piece-local metres), simplified to points
    at least `step` apart."""
    mesh = obj.data
    pieces = mesh.attributes["piece"].data
    uvs = mesh.uv_layers["UVMap"].data
    key = lambda uv: (round(uv[0], 6), round(uv[1], 6))
    for index, piece in enumerate(sheet.ids):
        directed = {}
        for p in mesh.polygons:
            if pieces[p.index].value != index:
                continue
            q = [key(uvs[li].uv) for li in p.loop_indices]
            for a, b in zip(q, q[1:] + q[:1]):
                directed[(a, b)] = True
        boundary = {}
        for (a, b) in directed:
            if (b, a) not in directed:
                boundary[a] = b
        while boundary:
            a, b = boundary.popitem()
            poly = [a]
            while b in boundary:
                poly.append(b)
                b = boundary.pop(b)
            if len(poly) < 3:
                continue
            simple = [poly[0]]
            for p in poly[1:]:
                if math.dist(p, simple[-1]) >= step:
                    simple.append(p)
            sheet.add_outline(piece, simple)
