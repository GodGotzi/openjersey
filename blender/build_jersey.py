"""
Builds assets/jersey.glb by fitting a jersey and shorts onto a mannequin,
ghost-mannequin style (the body shapes the fabric but is not exported).

    blender --background --python blender/build_jersey.py \
        [-- --render <dir>] [--views front,back,...] [--preview-only]

Needs assets-src/mannequin.blend (see blender/prepare_mannequin.py).

The jersey is made of the pieces of the ninesquared print template: front,
back, a side panel per side, a raglan sleeve per side and the collar band;
the shorts are one piece per leg. All pieces share one texture atlas laid out
like that template (see ATLAS_ROWS), so the app's 2D view is the print sheet.

How it works: the front and back pattern pieces (world meters in the front
view: x right, z up) are joined into one mesh along the side, sleeve and
shoulder seams and inflated into a loose "pillow" that clears the body.
Alternating shrinkwrap and smoothing passes then pull it onto the body with
a small gap, like a fitted volleyball jersey: chest and shoulders show
through, the fabric bridges small hollows. The hem is held slightly looser.
(A cloth simulation was tried before, but left the jersey baggy and boxy.)
Then the arms of body and jersey are lowered to the mockups' pose together.

The shorts are cut from the body surface between waist and mid-thigh,
relaxed over a smoothed crotch and flared towards the leg openings. Their
UVs wrap around each leg (see shorts_uv).

The atlas layout (piece outlines, cuff bands, print placement) is written to
assets/pattern.json for the app.
"""

import bpy
import bmesh
import json
import math
import os
import sys
from mathutils import Matrix, Vector
from mathutils.kdtree import KDTree

_user_site = os.path.expanduser("~/.local/lib/python3.14/site-packages")
if _user_site not in sys.path:
    sys.path.append(_user_site)

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MANNEQUIN_PATH = os.path.join(REPO_ROOT, "assets-src", "mannequin.blend")
GLB_PATH = os.path.join(REPO_ROOT, "assets", "jersey.glb")
PATTERN_JSON_PATH = os.path.join(REPO_ROOT, "assets", "pattern.json")

# ---------------------------------------------------------------------------
# pattern, right half (left is mirrored), sized to the mannequin
# ---------------------------------------------------------------------------

HEM_SIDE = (0.245, 0.86)
WAIST_SIDE = (0.232, 1.03)
UNDERARM = (0.255, 1.17)
# the sleeve follows the mannequin's measured upper-arm axis (~42 degrees
# down through (0.30, 1.22)); cuff half-width ~0.085 = biceps + ~10% ease.
# The arms are lowered to the mockups' pose only after fitting, see lower_arms.
CUFF_BOTTOM = (0.313, 1.091)
CUFF_TOP = (0.427, 1.217)
SHOULDER = (0.215, 1.405)
NECK_SIDE = (0.052, 1.465)
FRONT_NECK_BOTTOM = 1.405
BACK_NECK_BOTTOM = 1.45

# The pieces, as in the ninesquared print template: front, back, a side
# panel strip per side (front and back half), a raglan sleeve per side (front
# and back half, joined along the shoulder/top seam) and a collar band.
SIDE_HALF_WIDTH = 0.03  # side panel width on the front and on the back
COLLAR_WIDTH = 0.014
COLLAR_END = 0.016  # where the collar band's end meets the shoulder seam
RAGLAN_FROM_NECK = 0.03  # raglan seam start on the collar, from its end
# the raglan seam bows out towards the shoulder point (0 = straight), so
# front and back reach far onto the shoulder and the sleeve only runs up to
# the collar as a narrow strip, as on the ninesquared template
RAGLAN_BULGE = 0.6
RAGLAN_JOIN = 0.12  # side panel top, as a fraction up the raglan seam

# half-depth of the starting pillow; must clear the body everywhere
RHO_FRONT = 0.175
RHO_BACK = 0.14
RHO_SLEEVE = 0.095

# samples
NB = 16  # across half the front/back
NR_SIDE = 26  # up the side panel
NR_RAGLAN = 14  # along the raglan seam, side panel top to collar
NC_SIDE = 4  # across the side panel half
NR_UNDER = 8  # along the underarm seam
NC_SLEEVE = 12  # across the sleeve half
NECK_DENSE = 201

# print template layout (meters on the garment; also the atlas scale)
ATLAS_PX_PER_M = 1200
# portrait, so the sheet uses the app's tall 2D pane: the shirt pieces on
# top (front and back, then side panels around the sleeves, then the
# collar), the shorts below, where they can be left out
ATLAS_ROWS = [
    ["front", "back"],
    ["side_r", "sleeve_r", "sleeve_l", "side_l"],
    ["collar"],
    ["leg_r"],
    ["leg_l"],
]
ATLAS_MARGIN = 0.04
ATLAS_GAP = 0.05
ATLAS_LABEL = 0.06  # room for the piece name above each row
PIECE_LABELS = {
    "front": "FRONT", "back": "BACK", "side_r": "RIGHT SIDE", "side_l": "LEFT SIDE",
    "sleeve_r": "RIGHT SLEEVE", "sleeve_l": "LEFT SLEEVE", "collar": "COLLAR",
    "leg_r": "RIGHT LEG", "leg_l": "LEFT LEG",
}
CUFF_WIDTH = 0.016  # printed cuff band at the sleeve end
# the fabric's inside is mapped to this small square in the bottom right
# corner of the atlas, which the app fills with a plain colour
INSIDE_SWATCH = 0.03
PIECES = [p for row in ATLAS_ROWS for p in row]

FIT_ITERATIONS = 6
# the fitting target loses head and neck (a slightly flared cylinder around
# the neck, plus the chin), so the neckline settles snugly around the neck
# base instead of climbing the neck
NECK_CUT_Z = 1.50
NECK_BASE_Z = 1.39
NECK_CENTER_Y = -0.02
NECK_RADIUS = 0.056
NECK_FLARE = 0.1  # radius growth per meter of height
CHIN_Z = 1.44
CHIN_Y = -0.075  # everything in front of this above CHIN_Z is chin
THROAT_Z = 1.40
THROAT_Y = -0.055  # the throat in front of the neck, which the V-neck would cling to
THROAT_HALF_WIDTH = 0.06
FIT_GAP = 0.009  # fabric to skin

# the mannequin's arms are raised (see prepare_mannequin.py) so the sleeves
# can be fitted; afterwards body and jersey are lowered by this much
SHOULDER_PIVOT = (0.175, -0.01, 1.335)  # right side; left is mirrored
ARM_DROP_DEGREES = 16

# shorts: cut from the body between waist and mid-thigh, then relaxed
SHORTS_TOP_Z = 0.95  # hidden under the jersey
SHORTS_HEM_Z = 0.56
SHORTS_GAP = 0.005
SHORTS_FLARE = 0.035  # extra room towards the leg openings
SHORTS_FLARE_START_Z = 0.86


def mirror(p):
    return (-p[0], p[1])


def lerp2(a, b, t):
    return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)


def sub2(a, b):
    return (a[0] - b[0], a[1] - b[1])


def unit2(v):
    n = math.hypot(*v)
    return (v[0] / n, v[1] / n)


def resample(points, n):
    lengths = [0.0]
    for a, b in zip(points, points[1:]):
        lengths.append(lengths[-1] + math.dist(a, b))
    total = lengths[-1]
    out, seg = [], 0
    for i in range(n):
        target = total * i / (n - 1)
        while seg < len(points) - 2 and lengths[seg + 1] < target:
            seg += 1
        span = lengths[seg + 1] - lengths[seg]
        t = 0.0 if span == 0 else (target - lengths[seg]) / span
        out.append(lerp2(points[seg], points[seg + 1], min(max(t, 0.0), 1.0)))
    out[0], out[-1] = points[0], points[-1]
    return out


def arc_lengths(points):
    out = [0.0]
    for a, b in zip(points, points[1:]):
        out.append(out[-1] + math.dist(a, b))
    return out


def coons(bottom, top, left, right):
    rows, cols = len(left), len(bottom)
    p00, p10, p01, p11 = bottom[0], bottom[-1], top[0], top[-1]
    grid = []
    for j in range(rows):
        t = j / (rows - 1)
        row = []
        for i in range(cols):
            s = i / (cols - 1)
            row.append(tuple(
                (1 - t) * bottom[i][c] + t * top[i][c]
                + (1 - s) * left[j][c] + s * right[j][c]
                - ((1 - s) * (1 - t) * p00[c] + s * (1 - t) * p10[c]
                   + (1 - s) * t * p01[c] + s * t * p11[c])
                for c in (0, 1)
            ))
        grid.append(row)
    return grid


SEAMS = [
    (HEM_SIDE, WAIST_SIDE),
    (WAIST_SIDE, UNDERARM),
    (UNDERARM, CUFF_BOTTOM),
    (CUFF_TOP, SHOULDER),
    (SHOULDER, NECK_SIDE),
]
SEAMS = SEAMS + [(mirror(a), mirror(b)) for a, b in SEAMS]


def seam_distance(p):
    best = math.inf
    for a, b in SEAMS:
        dx, dz = b[0] - a[0], b[1] - a[1]
        t = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / (dx * dx + dz * dz)
        t = min(max(t, 0.0), 1.0)
        best = min(best, math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dz)))
    return best


def smoothstep(e0, e1, x):
    t = min(max((x - e0) / (e1 - e0), 0.0), 1.0)
    return t * t * (3 - 2 * t)


def sleeve_factor(p):
    """0 on the torso, rising to 1 within 14 cm beyond the armhole line."""
    n = unit2((SHOULDER[1] - UNDERARM[1], UNDERARM[0] - SHOULDER[0]))  # points outwards
    q = (abs(p[0]), p[1])
    d = (q[0] - UNDERARM[0]) * n[0] + (q[1] - UNDERARM[1]) * n[1]
    return smoothstep(0.0, 0.14, d)


def pillow_depth(p, rho_torso):
    rho = rho_torso + (RHO_SLEEVE - rho_torso) * sleeve_factor(p)
    d = min(seam_distance(p), rho)
    return math.sqrt(max(rho * rho - (rho - d) ** 2, 0.0))


def fit_weight(p):
    """How strongly the starting fit pulls this point onto the body (the hem stays a bit looser)."""
    return 0.82 + 0.18 * smoothstep(P_HEM_Z, 1.12, p[1])


P_HEM_Z = 0.86

# ---------------------------------------------------------------------------
# neckline and collar
# ---------------------------------------------------------------------------


def neck_dense(bottom_z, v_shape):
    """The neck edge from the centre (x = 0) to the neck side, densely sampled."""
    pts = []
    for i in range(NECK_DENSE):
        t = i / (NECK_DENSE - 1)
        w = t ** 1.1 if v_shape else t * t
        pts.append((NECK_SIDE[0] * t, bottom_z + (NECK_SIDE[1] - bottom_z) * w))
    return pts


def collar_inner(outer):
    """The collar's inner edge: the neck edge offset into the shirt by
    COLLAR_WIDTH (mitred), ending on the shoulder seam at a point shared by
    front and back so the seam lines up."""
    def normal(a, b):
        t = unit2(sub2(b, a))
        return (t[1], -t[0])  # left-to-right tangent -> pointing down into the shirt

    # mirror the first segment so the centre gets a symmetric mitre
    pts = [mirror(outer[1])] + outer
    inner = []
    for k in range(1, len(pts)):
        n2 = normal(pts[k], pts[k + 1]) if k + 1 < len(pts) else normal(pts[k - 1], pts[k])
        n1 = normal(pts[k - 1], pts[k])
        m = (n1[0] + n2[0], n1[1] + n2[1])
        f = COLLAR_WIDTH / (1 + n1[0] * n2[0] + n1[1] * n2[1])
        inner.append((pts[k][0] + m[0] * f, pts[k][1] + m[1] * f))
    inner[0] = (0.0, inner[0][1])
    # pull the end onto the shared shoulder-seam point
    end = collar_end_point()
    shift = sub2(end, inner[-1])
    ramp = 12
    for k in range(ramp):
        w = smoothstep(0, 1, (k + 1) / ramp)
        i = len(inner) - ramp + k
        inner[i] = (inner[i][0] + shift[0] * w, inner[i][1] + shift[1] * w)
    inner[-1] = end
    return inner


def collar_end_point():
    d = unit2(sub2(SHOULDER, NECK_SIDE))
    return (NECK_SIDE[0] + d[0] * COLLAR_END, NECK_SIDE[1] + d[1] * COLLAR_END)


def sample_along(inner, outer, i0, i1, n):
    """n points spaced evenly along `inner` between dense indices i0..i1, with
    the matching points of `outer`."""
    seg_in = inner[i0:i1 + 1]
    lengths = arc_lengths(seg_in)
    total = lengths[-1]
    ins, outs, k = [], [], 0
    for j in range(n):
        target = total * j / (n - 1)
        while k < len(seg_in) - 2 and lengths[k + 1] < target:
            k += 1
        span = lengths[k + 1] - lengths[k]
        t = 0.0 if span == 0 else min(max((target - lengths[k]) / span, 0.0), 1.0)
        ins.append(lerp2(seg_in[k], seg_in[k + 1], t))
        outs.append(lerp2(outer[i0 + k], outer[i0 + k + 1], t))
    ins[0], ins[-1] = inner[i0], inner[i1]
    outs[0], outs[-1] = outer[i0], outer[i1]
    return ins, outs


# ---------------------------------------------------------------------------
# pieces
# ---------------------------------------------------------------------------


class Grid:
    """One Coons grid of a piece, in front-view pattern coordinates (x, z),
    with piece-local layout coordinates (lx, ly: meters, ly down) per point."""

    def __init__(self, piece, panel, pts, local):
        self.piece, self.panel, self.pts, self.local = piece, panel, pts, local


# piece -> outline polygons in piece-local coordinates (one per grid),
# sleeve -> (cuff edge ly, half width) for the printed cuff band, and the
# leg measurements for the leg stripes (see build_shorts)
OUTLINES = {}
CUFFS = {}
LEGS = {}


def grid_outline(local):
    rows = len(local)
    return (local[0] + [local[j][-1] for j in range(1, rows)]
            + list(reversed(local[-1]))[1:] + [local[j][0] for j in range(rows - 2, 0, -1)])


def neck_edges(panel):
    outer = neck_dense(FRONT_NECK_BOTTOM if panel == "front" else BACK_NECK_BOTTOM, panel == "front")
    return outer, collar_inner(outer)


def panel_grids(panel, collar_lengths):
    """All grids of one panel (front or back): the right half (x > 0, the
    wearer's left) is built, the other half mirrored."""
    outer, inner = neck_edges(panel)
    last = len(inner) - 1
    lens = arc_lengths(inner)
    k_r = max(k for k in range(last) if lens[last] - lens[k] >= RAGLAN_FROM_NECK)
    r_top = inner[k_r]
    join = lerp2(UNDERARM, r_top, RAGLAN_JOIN)
    h_in = (HEM_SIDE[0] - SIDE_HALF_WIDTH, HEM_SIDE[1])
    w_in = (WAIST_SIDE[0] - SIDE_HALF_WIDTH, WAIST_SIDE[1])

    # shared edges, computed once so neighbouring grids get identical points
    side_inner = resample([h_in, w_in, join], NR_SIDE)
    raglan = raglan_curve(join, r_top, NR_RAGLAN)  # side panel top to collar
    side_top = resample([join, UNDERARM], NC_SIDE)
    underarm = resample([UNDERARM, CUFF_BOTTOM], NR_UNDER)
    mid_in, mid_out = sample_along(inner, outer, 0, k_r, NB)
    end_in, end_out = sample_along(inner, outer, k_r, last, NC_SLEEVE)

    body = coons(
        resample([(0.0, HEM_SIDE[1]), h_in], NB),
        mid_in,
        resample([(0.0, HEM_SIDE[1]), mid_in[0]], NR_SIDE + NR_RAGLAN - 1),
        side_inner + raglan[1:],
    )
    side = coons(resample([h_in, HEM_SIDE], NC_SIDE), side_top, side_inner,
                 resample([HEM_SIDE, WAIST_SIDE, UNDERARM], NR_SIDE))
    sleeve_left = list(reversed(raglan)) + side_top[1:] + underarm[1:]
    sleeve = coons(end_in, resample([CUFF_BOTTOM, CUFF_TOP], NC_SLEEVE), sleeve_left,
                   resample([collar_end_point(), SHOULDER, CUFF_TOP], len(sleeve_left)))
    collar_in = mid_in + end_in[1:]
    collar_out = mid_out + end_out[1:]
    collar = [collar_out, [lerp2(a, b, 0.5) for a, b in zip(collar_in, collar_out)], collar_in]

    front = panel == "front"
    grids = []

    def add(piece, pts, local_fn, mirrored):
        local = [[local_fn(j, i, p) for i, p in enumerate(row)] for j, row in enumerate(pts)]
        OUTLINES.setdefault(piece, []).append(grid_outline(local))
        if mirrored:
            pts = [[mirror(p) for p in row] for row in pts]
        grids.append(Grid(piece, panel, pts, local))

    # front and back as seen from their own side (so the back is mirrored)
    flip = 1 if front else -1
    name = "front" if front else "back"
    add(name, body, lambda j, i, p: (flip * p[0], -p[1]), False)
    add(name, body, lambda j, i, p: (-flip * p[0], -p[1]), True)

    # side panels across the side seam; seen from the wearer's side, the
    # front half lies towards the front (left on the left side, right on the right)
    for piece, s, mirrored in (("side_l", -1 if front else 1, False), ("side_r", 1 if front else -1, True)):
        add(piece, side, lambda j, i, p, s=s: (s * (side_seam_x(p[1]) - p[0]), -p[1]), mirrored)

    # sleeves: the shoulder/top seam is the centre line; length along it
    # down to the cuff, width across each row from it
    v = arc_lengths([row[-1] for row in sleeve])
    widths = [list(reversed(arc_lengths(list(reversed(row))))) for row in sleeve]
    for piece, s, mirrored in (("sleeve_l", -1 if front else 1, False), ("sleeve_r", 1 if front else -1, True)):
        add(piece, sleeve, lambda j, i, p, s=s: (s * widths[j][i], v[j]), mirrored)
        CUFFS[piece] = (v[-1], max(CUFFS.get(piece, (0, 0))[1], widths[-1][0]))

    # collar: one strip running front (x > 0) -> back (x > 0) -> back (x < 0)
    # -> front (x < 0), joined at the shoulder seams
    along = arc_lengths(collar_out)
    t_f, t_b = collar_lengths["front"], collar_lengths["back"]
    if front:
        halves = ((0.0, 1, False), (2 * t_f + 2 * t_b, -1, True))
    else:
        halves = ((t_f + t_b, -1, False), (t_f + t_b, 1, True))
    for offset, direction, mirrored in halves:
        add("collar", collar, lambda j, i, p, o=offset, d=direction: (o + d * along[i], j * COLLAR_WIDTH / 2), mirrored)
    return grids


def raglan_curve(join, r_top, n):
    """The raglan seam from the side panel top to the collar: a quadratic
    curve whose middle lies RAGLAN_BULGE of the way out to the shoulder point."""
    mid = lerp2(join, r_top, 0.5)
    control = (mid[0] + 2 * RAGLAN_BULGE * (SHOULDER[0] - mid[0]), mid[1] + 2 * RAGLAN_BULGE * (SHOULDER[1] - mid[1]))
    dense = []
    for i in range(201):
        t = i / 200
        a, b = lerp2(join, control, t), lerp2(control, r_top, t)
        dense.append(lerp2(a, b, t))
    return resample(dense, n)


def side_seam_x(z):
    pts = [HEM_SIDE, WAIST_SIDE, UNDERARM]
    if z >= UNDERARM[1]:
        return UNDERARM[0]
    if z <= HEM_SIDE[1]:
        return HEM_SIDE[0]
    for a, b in zip(pts, pts[1:]):
        if a[1] <= z <= b[1]:
            t = (z - a[1]) / (b[1] - a[1])
            return a[0] + (b[0] - a[0]) * t
    return UNDERARM[0]


def atlas_layout(bounds):
    """Slot (top-left, meters) of each piece; each row is centred."""
    def row_width(row):
        return sum(bounds[p][2] - bounds[p][0] for p in row) + ATLAS_GAP * (len(row) - 1)

    inner = max(row_width(row) for row in ATLAS_ROWS)
    slots, y = {}, ATLAS_MARGIN
    for row in ATLAS_ROWS:
        y += ATLAS_LABEL
        x, height = ATLAS_MARGIN + (inner - row_width(row)) / 2, 0.0
        for piece in row:
            (x0, y0, x1, y1) = bounds[piece]
            slots[piece] = (x, y)
            x += (x1 - x0) + ATLAS_GAP
            height = max(height, y1 - y0)
        y += height + ATLAS_GAP
    return slots, inner + 2 * ATLAS_MARGIN, y - ATLAS_GAP + ATLAS_MARGIN


def build_garment():
    """The jersey mesh. Each face loop's UV holds its piece-local layout
    coordinates for now (assign_atlas turns them into atlas UVs), and the
    face's piece index is kept in the "piece" face attribute."""
    collar_lengths = {panel: arc_lengths(neck_edges(panel)[0])[-1] for panel in ("front", "back")}
    bm = bmesh.new()
    # layers first: adding one later invalidates the element references
    deform = bm.verts.layers.deform.verify()
    uv_layer = bm.loops.layers.uv.new("UVMap")
    piece_layer = bm.faces.layers.int.new("piece")
    verts = {}  # key -> BMVert; seam points are shared between front and back
    pending = []  # (face, {vert: local}, piece)
    for panel, sign, rho in (("front", -1, RHO_FRONT), ("back", 1, RHO_BACK)):
        for grid in panel_grids(panel, collar_lengths):
            vgrid = []
            for row in grid.pts:
                vrow = []
                for p in row:
                    on_seam = seam_distance(p) < 1e-7
                    key = (round(p[0], 6), round(p[1], 6)) + (() if on_seam else (panel,))
                    if key not in verts:
                        v = bm.verts.new(Vector((p[0], sign * pillow_depth(p, rho), p[1])))
                        v[deform][0] = fit_weight(p)
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
        face[piece_layer] = PIECES.index(piece)
        for loop in face.loops:  # by vertex: recalc may have flipped the loop order
            loop[uv_layer].uv = local[loop.vert]

    # the side panels' corners at the hem, so the shorts stripes can line up
    # with the panels (vertex indices survive fitting)
    bm.verts.index_update()
    edge_x = round(HEM_SIDE[0] - SIDE_HALF_WIDTH, 6)
    hem_panel = [
        [verts[(side * edge_x, round(HEM_SIDE[1], 6), panel)].index for panel in ("front", "back")]
        for side in (1, -1)
    ]

    mesh = bpy.data.meshes.new("jersey")
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new("jersey", mesh)
    obj["hem_panel_l"], obj["hem_panel_r"] = hem_panel
    obj.vertex_groups.new(name="fit")
    bpy.context.collection.objects.link(obj)
    return obj


def bake_modifiers(obj):
    depsgraph = bpy.context.evaluated_depsgraph_get()
    evaluated = obj.evaluated_get(depsgraph)
    mesh = bpy.data.meshes.new_from_object(evaluated, preserve_all_data_layers=True, depsgraph=depsgraph)
    obj.modifiers.clear()
    obj.data = mesh
    return mesh


def fitting_target(body):
    """The body without neck and head.

    The nearest body point for the fabric around the neckline is otherwise
    the neck, and pulling it there turns the V-neck into a stand-up collar.
    """
    bm = bmesh.new()
    bm.from_mesh(body.data)
    cut = [
        v for v in bm.verts
        if v.co.z > NECK_CUT_Z
        or (v.co.z > CHIN_Z and v.co.y < CHIN_Y)
        or (v.co.z > THROAT_Z and v.co.y < THROAT_Y and abs(v.co.x) < THROAT_HALF_WIDTH)
        or (v.co.z > NECK_BASE_Z
            and math.hypot(v.co.x, v.co.y - NECK_CENTER_Y) < NECK_RADIUS + NECK_FLARE * (v.co.z - NECK_BASE_Z))
    ]
    bmesh.ops.delete(bm, geom=cut, context="VERTS")
    mesh = bpy.data.meshes.new("fit_target")
    bm.to_mesh(mesh)
    bm.free()
    target = bpy.data.objects.new("fit_target", mesh)
    bpy.context.collection.objects.link(target)
    target.hide_render = True
    return target


def smooth_edges(bm, passes=3):
    """Even out hem, cuff and neck edges along themselves (without pulling them into the fabric)."""
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


def fit_to_body(garment, body):
    """Pull the loose pillow onto the body (keeping a gap) and relax it there.

    Alternating shrinkwrap and smoothing spreads the fabric evenly over the
    body instead of bunching it where many pillow points share the same
    nearest body point, and bridges small hollows (navel, armpit crease).
    """
    target = fitting_target(body)
    for i in range(FIT_ITERATIONS):
        wrap = garment.modifiers.new("Fit", "SHRINKWRAP")
        wrap.target = target
        wrap.wrap_method = "NEAREST_SURFACEPOINT"
        wrap.wrap_mode = "OUTSIDE_SURFACE"
        wrap.offset = FIT_GAP
        wrap.vertex_group = "fit"
        mesh = bake_modifiers(garment)

        bm = bmesh.new()
        bm.from_mesh(mesh)
        last = i == FIT_ITERATIONS - 1
        # the last pass only softens muscle detail a little, like fabric bridging it
        inner = [v for v in bm.verts if not v.is_boundary]
        for _ in range(4 if last else 5):
            bmesh.ops.smooth_vert(bm, verts=inner, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
        smooth_edges(bm)
        bm.to_mesh(mesh)
        bm.free()
    bpy.data.objects.remove(target)
    mesh.polygons.foreach_set("use_smooth", [True] * len(mesh.polygons))


# ---------------------------------------------------------------------------
# arm pose
# ---------------------------------------------------------------------------


def body_arm_weights(body):
    group = body.vertex_groups["arm"].index
    weights = [0.0] * len(body.data.vertices)
    for v in body.data.vertices:
        for g in v.groups:
            if g.group == group:
                weights[v.index] = g.weight
    return weights


def rotate_arms(mesh, weights, degrees):
    for v, w in zip(mesh.vertices, weights):
        if w <= 0:
            continue
        side = 1 if v.co.x > 0 else -1
        pivot = Vector((SHOULDER_PIVOT[0] * side, SHOULDER_PIVOT[1], SHOULDER_PIVOT[2]))
        v.co = pivot + Matrix.Rotation(side * math.radians(degrees) * w, 3, "Y") @ (v.co - pivot)
    mesh.update()


def lower_arms(garment, body):
    """Lower the arms of body and jersey together, like a skinned mesh.

    Each jersey vertex takes the arm weight of the nearest body vertices,
    so sleeves turn with the arm and the shoulder bends smoothly. Fabric
    pushed into the body under the arm is then pushed back out.
    """
    weights = body_arm_weights(body)
    tree = KDTree(len(weights))
    for v in body.data.vertices:
        tree.insert(v.co, v.index)
    tree.balance()
    garment_weights = []
    for v in garment.data.vertices:
        near = tree.find_n(v.co, 8)
        garment_weights.append(sum(weights[i] for _, i, _ in near) / len(near))

    rotate_arms(body.data, weights, ARM_DROP_DEGREES)
    rotate_arms(garment.data, garment_weights, ARM_DROP_DEGREES)

    target = fitting_target(body)
    for i in range(3):
        wrap = garment.modifiers.new("Settle", "SHRINKWRAP")
        wrap.target = target
        wrap.wrap_method = "NEAREST_SURFACEPOINT"
        wrap.wrap_mode = "OUTSIDE"
        wrap.offset = FIT_GAP
        mesh = bake_modifiers(garment)
        if i == 2:
            break
        bm = bmesh.new()
        bm.from_mesh(mesh)
        inner = [v for v in bm.verts if not v.is_boundary]
        bmesh.ops.smooth_vert(bm, verts=inner, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
        bm.to_mesh(mesh)
        bm.free()
    bpy.data.objects.remove(target)
    mesh.polygons.foreach_set("use_smooth", [True] * len(mesh.polygons))


# ---------------------------------------------------------------------------
# shorts
# ---------------------------------------------------------------------------


def leg_axes(body):
    """Centre line of each thigh, as two points (low, high) per side."""
    def centroid(side, z):
        pts = [v.co for v in body.data.vertices
               if abs(v.co.z - z) < 0.006 and 0.02 < v.co.x * side < 0.28]
        return sum(pts, Vector()) / len(pts)
    return {side: (centroid(side, SHORTS_HEM_Z + 0.02), centroid(side, 0.74)) for side in (1, -1)}


def axis_at(axis, z):
    lo, hi = axis
    t = (z - lo.z) / (hi.z - lo.z)
    return lo + (hi - lo) * t


def largest_island(bm):
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


def shorts_uv(axes, side, co):
    """Around-the-leg coordinate: 0 inside, 0.5 outside, front at 0.25 on the
    left leg (x > 0) and 0.75 on the right one, so print reads left to right."""
    c = axis_at(axes[side], co.z)
    outward = (co.x - c.x) * side
    a = math.atan2(-(co.y - c.y), -outward)  # inner 0, front +90 degrees
    u = (a / (2 * math.pi)) % 1.0
    return u if side > 0 else 1.0 - u


def shorts_target(body):
    """The body with the crotch smoothed into a soft mound, which the shorts
    drape over instead of following every groove."""
    bm = bmesh.new()
    bm.from_mesh(body.data)
    crotch = [v for v in bm.verts if abs(v.co.x) < 0.11 and 0.66 < v.co.z < 0.92 and v.co.y < 0.03]
    for _ in range(80):
        bmesh.ops.smooth_vert(bm, verts=crotch, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
    mesh = bpy.data.meshes.new("shorts_target")
    bm.to_mesh(mesh)
    bm.free()
    target = bpy.data.objects.new("shorts_target", mesh)
    bpy.context.collection.objects.link(target)
    target.hide_render = True
    return target


def build_shorts(body, jersey):
    """Cut the shorts from the body surface, relax them and give them room."""
    bm = bmesh.new()
    bm.from_mesh(body.data)
    # drop the body's own vertex groups and UVs
    for layer in list(bm.verts.layers.deform):
        bm.verts.layers.deform.remove(layer)
    for layer in list(bm.loops.layers.uv):
        bm.loops.layers.uv.remove(layer)
    cut = [v for v in bm.verts if not (SHORTS_HEM_Z <= v.co.z <= SHORTS_TOP_Z) or abs(v.co.x) > 0.28]
    bmesh.ops.delete(bm, geom=cut, context="VERTS")
    largest_island(bm)
    bm.normal_update()
    for v in bm.verts:
        v.co += v.normal * SHORTS_GAP
    mesh = bpy.data.meshes.new("shorts")
    bm.to_mesh(mesh)
    bm.free()
    shorts = bpy.data.objects.new("shorts", mesh)
    bpy.context.collection.objects.link(shorts)
    target = shorts_target(body)

    # smooth away the crotch and muscle detail; the wrap only pushes fabric
    # that sank into the body back out
    for _ in range(5):
        bm = bmesh.new()
        bm.from_mesh(shorts.data)
        inner = [v for v in bm.verts if not v.is_boundary]
        for _ in range(12):
            bmesh.ops.smooth_vert(bm, verts=inner, factor=0.5, use_axis_x=True, use_axis_y=True, use_axis_z=True)
        smooth_edges(bm)
        for v in bm.verts:
            if v.is_boundary:
                v.co.z = SHORTS_TOP_Z if v.co.z > 0.8 else SHORTS_HEM_Z
        bm.to_mesh(shorts.data)
        bm.free()
        wrap = shorts.modifiers.new("Settle", "SHRINKWRAP")
        wrap.target = target
        wrap.wrap_method = "NEAREST_SURFACEPOINT"
        wrap.wrap_mode = "OUTSIDE"
        wrap.offset = SHORTS_GAP
        bake_modifiers(shorts)
    bpy.data.objects.remove(target)

    axes = leg_axes(body)
    bm = bmesh.new()
    bm.from_mesh(shorts.data)
    for v in bm.verts:
        side = 1 if v.co.x > 0 else -1
        c = axis_at(axes[side], v.co.z)
        radial = Vector((v.co.x - c.x, v.co.y - c.y, 0))
        if radial.length > 1e-6:
            t = smoothstep(SHORTS_FLARE_START_Z, SHORTS_HEM_Z, v.co.z)
            v.co += radial.normalized() * SHORTS_FLARE * t

    circumference = leg_circumference_of(bm, axes, SHORTS_HEM_Z + 0.08)
    # the leg stripe continues the jersey's side panel: its edges run
    # straight down from the panel's corners at the jersey hem
    stripes = {}
    for side, key in ((1, "hem_panel_l"), (-1, "hem_panel_r")):
        corners = [jersey.data.vertices[i].co for i in jersey[key]]
        top = stripe_at(bm, axes, side, corners, SHORTS_TOP_Z - 0.06)
        hem = stripe_at(bm, axes, side, corners, SHORTS_HEM_Z + 0.01)
        # projected further down, the corners spread over the flared leg;
        # keep the stripe as wide as at the top, like a printed band
        stripes["leg_l" if side > 0 else "leg_r"] = {"top": top, "hem": (hem[0], top[1])}
    LEGS.update(stripes=stripes)
    # piece-local layout coordinates (meters, ly down from the waist), turned
    # into atlas UVs by assign_atlas like the jersey's
    uv_layer = bm.loops.layers.uv.new("UVMap")
    piece_layer = bm.faces.layers.int.new("piece")
    for f in bm.faces:
        side = 1 if f.calc_center_median().x > 0 else -1
        f[piece_layer] = PIECES.index("leg_l" if side > 0 else "leg_r")
        us = [shorts_uv(axes, side, loop.vert.co) for loop in f.loops]
        if max(us) - min(us) > 0.5:  # face straddles the inseam
            us = [u + 1 if u < 0.5 else u for u in us]
        for loop, u in zip(f.loops, us):
            loop[uv_layer].uv = (u * circumference, SHORTS_TOP_Z - loop.vert.co.z)
    leg = [(0.0, 0.0), (circumference, 0.0), (circumference, SHORTS_TOP_Z - SHORTS_HEM_Z),
           (0.0, SHORTS_TOP_Z - SHORTS_HEM_Z)]
    OUTLINES["leg_l"] = [leg]
    OUTLINES["leg_r"] = [leg]
    bm.normal_update()
    bm.to_mesh(shorts.data)
    bm.free()
    shorts.data.polygons.foreach_set("use_smooth", [True] * len(shorts.data.polygons))
    return shorts


def stripe_at(bm, axes, side, corners, z):
    """(centre u, width in u) of the leg stripe at height z: between the leg's
    surface points closest (in x, y) to the side panel's hem corners."""
    ring = [v.co for v in bm.verts if abs(v.co.z - z) < 0.006 and v.co.x * side > 0]
    us = []
    for c in corners:
        nearest = min(ring, key=lambda p: (p.x - c.x) ** 2 + (p.y - c.y) ** 2)
        us.append(shorts_uv(axes, side, nearest))
    return (us[0] + us[1]) / 2, abs(us[0] - us[1])


def leg_circumference_of(bm, axes, z):
    """Mean around-the-leg length at height z (halfway down the leg it's the
    width of a leg piece)."""
    c = axis_at(axes[1], z)
    radii = [math.hypot(v.co.x - c.x, v.co.y - c.y) for v in bm.verts
             if abs(v.co.z - z) < 0.004 and v.co.x > 0]
    return 2 * math.pi * sum(radii) / len(radii)


def assign_atlas(meshes):
    """Lays the pieces out like the ninesquared template and turns every
    loop's piece-local coordinates into atlas UVs. Returns the layout."""
    bounds = {}
    for piece, polys in OUTLINES.items():
        xs = [p[0] for poly in polys for p in poly]
        ys = [p[1] for poly in polys for p in poly]
        bounds[piece] = (min(xs), min(ys), max(xs), max(ys))
    slots, width, height = atlas_layout(bounds)

    def to_atlas(piece, lx, ly):
        x0, y0, _, _ = bounds[piece]
        sx, sy = slots[piece]
        return sx + lx - x0, sy + ly - y0

    for mesh in meshes:
        pieces = mesh.attributes["piece"].data
        uvs = mesh.uv_layers["UVMap"].data
        for poly in mesh.polygons:
            piece = PIECES[pieces[poly.index].value]
            for li in poly.loop_indices:
                x, y = to_atlas(piece, *uvs[li].uv)
                uvs[li].uv = (x / width, 1.0 - y / height)  # glTF flips v back
    inside = (width - ATLAS_MARGIN - INSIDE_SWATCH, height - ATLAS_MARGIN - INSIDE_SWATCH, INSIDE_SWATCH, INSIDE_SWATCH)
    return {"bounds": bounds, "slots": slots, "width": width, "height": height, "to_atlas": to_atlas, "inside": inside}


def load_mannequin():
    with bpy.data.libraries.load(MANNEQUIN_PATH) as (src, dst):
        dst.objects = ["mannequin"]
    body = dst.objects[0]
    bpy.context.collection.objects.link(body)
    return body


def finish(garment, subdivide, layout):
    """Subdivides and gives the fabric its thickness, then maps the inner
    surface to the plain inside swatch (the rims keep the print's edge)."""
    if subdivide:
        sub = garment.modifiers.new("Subsurf", "SUBSURF")
        sub.levels = sub.render_levels = 1
    garment.vertex_groups.new(name="shell")
    solid = garment.modifiers.new("Thickness", "SOLIDIFY")
    solid.thickness = 0.0015
    solid.offset = 1
    solid.shell_vertex_group = "shell"
    mesh = bake_modifiers(garment)

    group = garment.vertex_groups["shell"].index
    in_shell = [any(g.group == group and g.weight > 0.5 for g in v.groups) for v in mesh.vertices]
    shell = [p.index for p in mesh.polygons if all(in_shell[v] for v in p.vertices)]
    original = [p.index for p in mesh.polygons if not any(in_shell[v] for v in p.vertices)]

    # the inner surface is the one nearer the body: compare the mean distance
    # from the vertical axis around the waist, where every garment is one tube
    band = [v for v in mesh.vertices if 0.88 < v.co.z < 0.94]
    cx = sum(v.co.x for v in band) / len(band)
    cy = sum(v.co.y for v in band) / len(band)

    def radius(faces):
        verts = {v for f in faces for v in mesh.polygons[f].vertices if 0.88 < mesh.vertices[v].co.z < 0.94}
        return sum(math.hypot(mesh.vertices[v].co.x - cx, mesh.vertices[v].co.y - cy) for v in verts) / max(len(verts), 1)

    inner = shell if radius(shell) < radius(original) else original
    x, y, w, h = layout["inside"]
    u, v = (x + w / 2) / layout["width"], 1.0 - (y + h / 2) / layout["height"]
    uvs = mesh.uv_layers["UVMap"].data
    for f in inner:
        for li in mesh.polygons[f].loop_indices:
            uvs[li].uv = (u, v)


def write_pattern_json(layout):
    """Everything the app needs to draw the template, in atlas pixels."""
    ppm = ATLAS_PX_PER_M
    to_atlas = layout["to_atlas"]
    px = lambda piece, p: [round(c * ppm, 2) for c in to_atlas(piece, *p)]

    pieces = []
    for piece in PIECES:
        x0, y0, x1, y1 = layout["bounds"][piece]
        sx, sy = layout["slots"][piece]
        pieces.append({
            "id": piece,
            "label": PIECE_LABELS[piece],
            "rect": [round(v * ppm, 2) for v in (sx, sy, x1 - x0, y1 - y0)],
            "polygons": [[px(piece, p) for p in poly] for poly in OUTLINES[piece]],
        })
    cuffs = []
    for piece, (edge, half) in CUFFS.items():
        a, b = px(piece, (-half - 0.01, edge - CUFF_WIDTH)), px(piece, (half + 0.01, edge + 0.01))
        cuffs.append({"piece": piece, "rect": [a[0], a[1], b[0] - a[0], b[1] - a[1]]})
    # front/back placement of pattern points (x, z), for default print positions
    placement = {}
    for piece, x_sign in (("front", 1), ("back", -1)):
        ox, oy = to_atlas(piece, 0.0, 0.0)
        placement[piece] = {"origin": [round(ox * ppm, 2), round(oy * ppm, 2)], "xSign": x_sign}
    data = {
        "atlas": {"width": round(layout["width"] * ppm), "height": round(layout["height"] * ppm), "pxPerM": ppm},
        "pieces": pieces,
        "cuffs": cuffs,
        # plain square the inside of the fabric is mapped to
        "inside": [round(v * ppm, 2) for v in layout["inside"]],
        "placement": placement,
        # leg pieces: the outer side seam is in the middle; the front half is
        # to the right on the right leg and to the left on the left leg
        "legs": [
            {
                "piece": piece,
                "frontDir": front_dir,
                # centre and width as fractions of the piece width, at the
                # waist and at the hem
                "stripe": {
                    "topCentre": round(LEGS["stripes"][piece]["top"][0], 4),
                    "topWidth": round(LEGS["stripes"][piece]["top"][1], 4),
                    "hemCentre": round(LEGS["stripes"][piece]["hem"][0], 4),
                    "hemWidth": round(LEGS["stripes"][piece]["hem"][1], 4),
                },
            }
            for piece, front_dir in (("leg_r", 1), ("leg_l", -1))
        ],
    }
    with open(PATTERN_JSON_PATH, "w") as f:
        json.dump(data, f, indent=1)


# ---------------------------------------------------------------------------
# preview renders
# ---------------------------------------------------------------------------

ALL_VIEWS = {
    # name: (camera position, hide body, look-at height)
    "front": ((0, -3.0, 1.05), True, 1.0),
    "neck": ((0, -1.1, 1.45), True, 1.38),
    "three_quarter": ((1.8, -2.3, 1.2), True, 1.0),
    "back": ((0, 3.0, 1.05), True, 1.0),
    "with_body": ((0.9, -2.8, 1.2), False, 1.0),
}


def render_previews(garments, body, out_dir, views):
    mat = bpy.data.materials.new("preview")
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = (0.85, 0.85, 0.87, 1)
    bsdf.inputs["Roughness"].default_value = 0.8
    for garment in garments:
        garment.data.materials.clear()
        garment.data.materials.append(mat)

    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = 32
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
    for name in views:
        pos, ghost, target_z = ALL_VIEWS[name]
        target = Vector((0, 0, target_z))
        body.hide_render = ghost
        cam.location = pos
        cam.rotation_euler = (target - Vector(pos)).to_track_quat("-Z", "Y").to_euler()
        scene.render.filepath = os.path.join(out_dir, f"jersey_{name}.png")
        bpy.ops.render.render(write_still=True)


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    render_dir = argv[argv.index("--render") + 1] if "--render" in argv else None
    views = argv[argv.index("--views") + 1].split(",") if "--views" in argv else list(ALL_VIEWS)

    bpy.ops.wm.read_factory_settings(use_empty=True)
    body = load_mannequin()
    garment = build_garment()
    fit_to_body(garment, body)
    lower_arms(garment, body)
    shorts = build_shorts(body, garment)
    garments = [garment, shorts]
    layout = assign_atlas([garment.data, shorts.data])
    if "--preview-only" in argv:
        render_previews(garments, body, render_dir, views)
        return
    finish(garment, subdivide=True, layout=layout)
    finish(shorts, subdivide=False, layout=layout)  # cut from the dense body mesh already
    write_pattern_json(layout)

    body.hide_set(True)
    body.select_set(False)
    for g in garments:
        g.select_set(True)
    bpy.context.view_layer.objects.active = garment
    os.makedirs(os.path.dirname(GLB_PATH), exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=GLB_PATH, export_format="GLB", use_selection=True, export_apply=True,
    )
    print(f"Wrote {GLB_PATH}")

    if render_dir:
        render_previews(garments, body, render_dir, views)

main()
