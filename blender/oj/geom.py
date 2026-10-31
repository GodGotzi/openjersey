"""2D pattern geometry. Points are (x, z) tuples in front-view metres
(x right = the wearer's left, z up from the floor) unless said otherwise."""

import math


def mirror(p):
    return (-p[0], p[1])


def lerp2(a, b, t):
    return (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)


def sub2(a, b):
    return (a[0] - b[0], a[1] - b[1])


def unit2(v):
    n = math.hypot(*v)
    return (v[0] / n, v[1] / n)


def toward(a, b, d):
    """The point d metres from a towards b."""
    u = unit2(sub2(b, a))
    return (a[0] + u[0] * d, a[1] + u[1] * d)


def smoothstep(e0, e1, x):
    t = min(max((x - e0) / (e1 - e0), 0.0), 1.0)
    return t * t * (3 - 2 * t)


def arc_lengths(points):
    out = [0.0]
    for a, b in zip(points, points[1:]):
        out.append(out[-1] + math.dist(a, b))
    return out


def resample(points, n):
    """n points evenly spaced along the polyline `points` (ends kept)."""
    lengths = arc_lengths(points)
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


def quadratic(a, control, b, n=201):
    """A quadratic Bezier curve, densely sampled."""
    out = []
    for i in range(n):
        t = i / (n - 1)
        out.append(lerp2(lerp2(a, control, t), lerp2(control, b, t), t))
    return out


def cubic(p0, c1, c2, p3, n=201):
    """A cubic Bezier curve, densely sampled."""
    out = []
    for i in range(n):
        t = i / (n - 1)
        a, b, c, d = (1 - t) ** 3, 3 * (1 - t) ** 2 * t, 3 * (1 - t) * t * t, t ** 3
        out.append(tuple(a * p0[k] + b * c1[k] + c * c2[k] + d * p3[k] for k in (0, 1)))
    return out


def coons(bottom, top, left, right):
    """A grid (rows bottom -> top, columns left -> right) filling four
    boundary curves; bottom/top have the column count, left/right the row
    count, and the corners must agree."""
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


def relax(grid, passes):
    """Laplacian smoothing of a grid's interior (evens out Coons patches with
    strongly curved sides)."""
    rows, cols = len(grid), len(grid[0])
    for _ in range(passes):
        for j in range(1, rows - 1):
            for i in range(1, cols - 1):
                ns = (grid[j - 1][i], grid[j + 1][i], grid[j][i - 1], grid[j][i + 1])
                grid[j][i] = (sum(p[0] for p in ns) / 4, sum(p[1] for p in ns) / 4)
    return grid


def with_mirrored(seams):
    """Seam segments of the right half plus their mirror images."""
    return seams + [(mirror(a), mirror(b)) for a, b in seams]


def seam_distance(p, seams):
    """Distance from p to the nearest seam segment."""
    best = math.inf
    for a, b in seams:
        dx, dz = b[0] - a[0], b[1] - a[1]
        t = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / (dx * dx + dz * dz)
        t = min(max(t, 0.0), 1.0)
        best = min(best, math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dz)))
    return best


def polyline_x(points, z):
    """x of a polyline running upwards (z increasing) at height z, clamped."""
    if z <= points[0][1]:
        return points[0][0]
    for a, b in zip(points, points[1:]):
        if a[1] <= z <= b[1]:
            t = (z - a[1]) / (b[1] - a[1]) if b[1] != a[1] else 0.0
            return a[0] + (b[0] - a[0]) * t
    return points[-1][0]


def sample_along(inner, outer, i0, i1, n):
    """n points spaced evenly along `inner` between dense indices i0..i1, with
    the matching points of `outer` (the two curves sampled index by index)."""
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


def offset_curve(outer, width, turn, start=None, end=None, ramp=12):
    """`outer` offset by `width` (mitred) to the left of its direction of
    travel for turn = 1, to the right for turn = -1 — the inner edge of a
    binding or band. The ends are pulled onto `start` / `end` (None keeps
    them) over `ramp` points, so the band meets a seam exactly there."""
    def normal(a, b):
        t = unit2(sub2(b, a))
        return (-turn * t[1], turn * t[0])

    inner = []
    for k in range(len(outer)):
        n1 = normal(outer[max(k - 1, 0)], outer[max(k, 1)])
        n2 = normal(outer[min(k, len(outer) - 2)], outer[min(k + 1, len(outer) - 1)])
        m = (n1[0] + n2[0], n1[1] + n2[1])
        f = width / (1 + n1[0] * n2[0] + n1[1] * n2[1])
        inner.append((outer[k][0] + m[0] * f, outer[k][1] + m[1] * f))
    for target, idx in ((start, range(ramp)), (end, range(len(inner) - 1, len(inner) - 1 - ramp, -1))):
        if target is None:
            continue
        idx = list(idx)
        shift = sub2(target, inner[idx[0]])
        for k, i in enumerate(idx):
            w = 1 - smoothstep(0, 1, k / ramp)
            inner[i] = (inner[i][0] + shift[0] * w, inner[i][1] + shift[1] * w)
        inner[idx[0]] = target
    return inner


def band(inner, outer):
    """A three-row grid between two matching curves (outer edge row first),
    for bindings and collar bands."""
    return [outer, [lerp2(a, b, 0.5) for a, b in zip(inner, outer)], inner]


def grid_outline(local):
    """Outline polygon of a grid of piece-local points."""
    rows = len(local)
    return (local[0] + [local[j][-1] for j in range(1, rows)]
            + list(reversed(local[-1]))[1:] + [local[j][0] for j in range(rows - 2, 0, -1)])
