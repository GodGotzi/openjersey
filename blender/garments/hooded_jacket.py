"""Hooded wind jacket (unisex): a lightweight full-zip hooded jacket. The zip jacket's raglan cut with long sleeves
and the zip (up to the neck), roomier, with a hood instead of the stand-up
collar: the hood rises from the neck edge (the neck band stays under it)
over the head to the face opening, shaped around an ellipsoid fitted to the
mannequin's head (the head itself isn't part of the fitting target).

Pieces: the zip jacket's plus "hood": both halves of the hood, each laid
out as seen from its side, joined along the crown seam (two mesh objects)."""

import math

from mathutils import Vector

from garments.zip_jacket import ROWS as JACKET_ROWS, ZipJacket, neckline, new_piece_object
from oj.registry import register
from oj.sheet import Sheet

ROWS = [row if row != ["collar", "zipper"] else ["hood", "collar", "zipper"] for row in JACKET_ROWS]
PIECES = {
    # the band the hood is sewn onto: it stays the jacket's own colour, so it does
    # not show as a stripe around the back of the neck below the hood
    "collar": ("NECK BAND", "Neck Band", "primary"),
    "zipper": ("ZIP", "Zip", "trim"),
    "hood": ("HOOD", "Hood", "accent"),
}


class HoodedJacket(ZipJacket):
    FIT_GAP = 0.018
    HEM_DROP = 0.07
    HEM_EASE = 0.02
    DRAPE_ROUNDS = 4
    CUFF_HALF = 0.055
    NECK_RAISE = {"front": 0.03, "back": 0.008}
    HOOD_GAP = 0.03  # room around the head
    HOOD_ROWS = 18
    HOOD_RELAX = 40
    CHIN_RISE = 0.07  # face opening's lower ends above the front neck

    def head_ellipsoid(self):
        """Centre and semi-axes of the mannequin's head (above the neck cut)."""
        cos = [v.co for v in self.body.obj.data.vertices if v.co.z > self.m.NECK_CUT_Z]
        lo = Vector((min(c.x for c in cos), min(c.y for c in cos), min(c.z for c in cos)))
        hi = Vector((max(c.x for c in cos), max(c.y for c in cos), max(c.z for c in cos)))
        # the cut leaves the jaw/neck below: extend the ellipsoid down to the neck base
        lo.z = self.m.NECK_BASE_Z
        return (lo + hi) / 2, (hi - lo) / 2

    def outside(self, q, centre, axes):
        """q pushed radially out of the head ellipsoid grown by HOOD_GAP."""
        g = self.HOOD_GAP
        d = q - centre
        r = math.sqrt((d.x / (axes.x + g)) ** 2 + (d.y / (axes.y + g)) ** 2 + (d.z / (axes.z + g)) ** 2)
        return centre + d / r if r < 1 else q

    def hood(self, sheet, neck):
        centre, axes = self.head_ellipsoid()
        g = self.HOOD_GAP
        length = neck[-1][1]
        z0 = neck[0][0].z + self.CHIN_RISE
        z1 = centre.z + axes.z + g * 0.8
        ox = axes.x + g * 0.6

        def opening(phi):
            """The face opening from the wearer's left lower end (phi = 0)
            over the forehead (0.5) to the right lower end (1)."""
            a = math.pi * phi
            return Vector((ox * math.cos(a), centre.y - (0.45 + 0.4 * math.sin(a)) * axes.y,
                           z0 + (z1 - z0) * math.sin(a)))

        n = self.HOOD_ROWS
        grid = [[None] * len(neck) for _ in range(n)]
        for i, (p, along) in enumerate(neck):
            o = opening(along / length)
            for j in range(n):
                t = j / (n - 1)
                q = p.lerp(o, t)
                # bulge out behind the head (the hood's fullness), most mid-way
                q += Vector((0, 1, 0.4)).normalized() * 0.06 * math.sin(math.pi * t) * math.sin(math.pi * along / length)
                grid[j][i] = self.outside(q, centre, axes) if 0 < j < n - 1 else q
        for _ in range(self.HOOD_RELAX):  # even the grid out, keeping it off the head
            new = [row[:] for row in grid]
            for j in range(1, n - 1):
                for i in range(1, len(neck) - 1):
                    avg = (grid[j - 1][i] + grid[j + 1][i] + grid[j][i - 1] + grid[j][i + 1]) / 4
                    new[j][i] = self.outside(avg, centre, axes)
            grid = new

        # layout: two side panels joined along the crown (the back centre
        # profile), each the side view of its half, front edge outwards
        mid = min(range(len(neck)), key=lambda i: abs(neck[i][1] - length / 2))
        crown_y = max(grid[j][mid].y for j in range(n))
        halves = []
        for cols, s in ((range(0, mid + 1), 1), (range(mid, len(neck)), -1)):
            cols = list(cols)
            half = [[grid[j][i] for i in cols] for j in range(n)]
            local = lambda j, i, s=s, half=half: (s * (crown_y - half[j][i].y), -half[j][i].z)
            # each half's outline: neck edge, front edge, opening, crown
            k = len(cols) - 1
            outline = ([local(0, i) for i in range(k + 1)] + [local(j, k) for j in range(1, n)]
                       + [local(n - 1, i) for i in range(k - 1, -1, -1)] + [local(j, 0) for j in range(n - 2, 0, -1)])
            sheet.add_outline("hood", outline)
            halves.append(new_piece_object("hood", sheet, half, lambda j, i: "hood", local))
        return halves

    def build(self, rows=ROWS, pieces=PIECES):
        sheet = Sheet(rows, pieces)
        obj = self.build_body(sheet)
        neck = neckline(obj, sheet)
        return sheet, [obj, *self.hood(sheet, neck), self.zipper(sheet, obj, [])]


def build(body, info):
    return HoodedJacket(body).build()


for _body, _order in (("man", 260), ("women", 261)):
    register(build, id=f"hooded-jacket-{_body}", name="Hooded Wind Jacket",
             kind="top", body=_body, order=_order, unisex=True)
