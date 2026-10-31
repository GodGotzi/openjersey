"""Tank top (man and women). Front and
back are one piece each, joined at the side and shoulder seams, with a
round neck and deep armholes; neckline and armholes are finished with
narrow bindings. The man's back armholes are cut a little further in, the
women's far in (racerback), so the back straps narrow towards the neck.

The top is fitted on the body without arms (so the armholes don't cling to
them); then only the body's arms are lowered.

`TankTop` is the cut, with the per-body shape given as a namespace like
`MAN` / `WOMEN`; other sleeveless tops can reuse it."""

from types import SimpleNamespace

from oj.geom import (arc_lengths, band, coons, cubic, lerp2, offset_curve, polyline_x, relax, resample,
                     sample_along, toward, with_mirrored)
from oj.mesh import Grids, collar_halves, fit_to_body, hem_weight, pillow_depth, pillow_mesh
from oj.registry import register
from oj.sheet import Sheet

ROWS = [
    ["front", "back"],
    ["armhole_r"],
    ["armhole_l"],
    ["collar"],
]
PIECES = {"collar": ("NECK BINDING", "Neck Binding", "trim")}

MAN = SimpleNamespace(
    ARM_SIDE_DROP=0.06,  # armhole bottom on the side seam, below the underarm
    NECK_X=0.075,  # neckline on the shoulder seam
    STRAP_X=0.128,  # armhole on the shoulder seam
    NECK_BOTTOM={"front": 1.33, "back": 1.40},
    # inner control points of the armhole curve (a cubic from the side
    # seam, leaving it horizontally, up to the strap), as (x, z)
    ARMHOLE={"front": ((0.16, None), (0.115, 1.31)), "back": ((0.14, None), (0.085, 1.29))},
)
WOMEN = SimpleNamespace(
    ARM_SIDE_DROP=0.055,
    NECK_X=0.066,
    STRAP_X=0.108,
    NECK_BOTTOM={"front": 1.31, "back": 1.385},
    # racerback: the back armhole sweeps in towards the spine
    ARMHOLE={"front": ((0.14, None), (0.098, 1.29)), "back": ((0.02, None), (0.03, 1.27))},
)


class TankTop:
    BINDING_WIDTH = 0.014
    SIDE_HALF_WIDTH = 0.03  # where the shorts' stripe would line up (hem sampling)
    NECK_DENSE = 201
    NB = 16  # across half the front/back
    NT_NECK = 15  # along the neck binding, centre to shoulder
    NT_SHOULDER = 4  # along the strap's shoulder seam between the bindings
    NT_ARMHOLE = 20  # along the armhole binding
    NT_SIDE = 16  # up the side seam
    RELAX = 60  # smoothing passes over the body grid's interior

    def __init__(self, body, shape):
        self.body, self.m, self.s = body, body.m, shape
        m = self.m
        self.arm_side_z = m.UNDERARM[1] - shape.ARM_SIDE_DROP
        self.neck = self.shoulder_point(shape.NECK_X)
        self.strap = self.shoulder_point(shape.STRAP_X)
        self.arm_side = (self.side_seam_x(self.arm_side_z), self.arm_side_z)
        self.seams = with_mirrored([(m.HEM_SIDE, m.WAIST_SIDE), (m.WAIST_SIDE, self.arm_side), (self.neck, self.strap)])

    def side_seam_x(self, z):
        m = self.m
        return polyline_x([m.HEM_SIDE, m.WAIST_SIDE, m.UNDERARM], z)

    def shoulder_point(self, x):
        """The point of the shoulder seam (NECK_SIDE -> SHOULDER) at x."""
        m = self.m
        t = (x - m.NECK_SIDE[0]) / (m.SHOULDER[0] - m.NECK_SIDE[0])
        return lerp2(m.NECK_SIDE, m.SHOULDER, t)

    def neck_curve(self, panel):
        """The neck edge from the centre to the shoulder seam."""
        bottom = self.s.NECK_BOTTOM[panel]
        return [(self.neck[0] * t, bottom + (self.neck[1] - bottom) * t * t)
                for t in (i / (self.NECK_DENSE - 1) for i in range(self.NECK_DENSE))]

    def armhole_curve(self, panel):
        """The armhole edge from the side seam up to the shoulder seam."""
        (x1, z1), c2 = self.s.ARMHOLE[panel]
        return cubic(self.arm_side, (x1, z1 if z1 is not None else self.arm_side_z), c2, self.strap, self.NECK_DENSE)

    def edges(self, panel):
        """(outer, inner) of the neck binding and of the armhole binding."""
        w = self.BINDING_WIDTH
        neck = self.neck_curve(panel)
        neck_in = offset_curve(neck, w, -1, (0.0, neck[0][1] - w), toward(self.neck, self.strap, w))
        arm = self.armhole_curve(panel)
        side_z = self.arm_side_z - w
        arm_in = offset_curve(arm, w, 1, (self.side_seam_x(side_z), side_z), toward(self.strap, self.neck, w))
        return (neck, neck_in), (arm, arm_in)

    def panel_grids(self, sheet, panel, lengths):
        m = self.m
        (neck, neck_in), (arm, arm_in) = self.edges(panel)
        n_in, n_out = sample_along(neck_in, neck, 0, len(neck) - 1, self.NT_NECK)
        a_in, a_out = sample_along(arm_in, arm, 0, len(arm) - 1, self.NT_ARMHOLE)
        edge_x = m.HEM_SIDE[0] - self.SIDE_HALF_WIDTH
        bottom = (resample([(0.0, m.HEM_SIDE[1]), (edge_x, m.HEM_SIDE[1])], self.NB)
                  + resample([(edge_x, m.HEM_SIDE[1]), m.HEM_SIDE], 3)[1:])
        top = n_in + resample([n_in[-1], a_in[-1]], self.NT_SHOULDER)[1:]
        right = resample([m.HEM_SIDE, m.WAIST_SIDE, a_in[0]], self.NT_SIDE) + a_in[1:]
        left = resample([(0.0, m.HEM_SIDE[1]), n_in[0]], len(right))
        body = relax(coons(bottom, top, left, right), self.RELAX)
        neck_band = band(n_in, n_out)
        arm_band = band(a_in, a_out)

        front = panel == "front"
        grids = Grids(sheet, panel)
        flip = 1 if front else -1
        grids.add(panel, body, lambda j, i, p: (flip * p[0], -p[1]))
        grids.add(panel, body, lambda j, i, p: (-flip * p[0], -p[1]), mirrored=True)

        # neck binding: one strip like the T-shirt's collar
        w = self.BINDING_WIDTH
        along = arc_lengths(n_out)
        for offset, direction, mirrored in collar_halves(front, lengths["neck"]["front"], lengths["neck"]["back"]):
            grids.add("collar", neck_band, lambda j, i, p, o=offset, d=direction: (o + d * along[i], j * w / 2), mirrored)

        # armhole bindings: front from the side seam up to the shoulder, then
        # the back from the shoulder down to the side seam
        along = arc_lengths(a_out)
        a_f, a_b = lengths["arm"]["front"], lengths["arm"]["back"]
        offset, direction = (0.0, 1) if front else (a_f + a_b, -1)
        for piece, mirrored in (("armhole_l", False), ("armhole_r", True)):
            grids.add(piece, arm_band, lambda j, i, p: (offset + direction * along[i], j * w / 2), mirrored)
        return grids.items

    def depth(self, p, panel):
        return pillow_depth(p, self.m.RHO_FRONT if panel == "front" else self.m.RHO_BACK, self.seams)

    def build(self, rows=ROWS, pieces=PIECES):
        sheet = Sheet(rows, pieces)
        lengths = {"neck": {}, "arm": {}}
        for panel in ("front", "back"):
            (neck, _), (arm, _) = self.edges(panel)
            lengths["neck"][panel] = arc_lengths(neck)[-1]
            lengths["arm"][panel] = arc_lengths(arm)[-1]
        panels = [(panel, self.panel_grids(sheet, panel, lengths)) for panel in ("front", "back")]
        obj = pillow_mesh("top", sheet, panels, self.seams, self.depth, hem_weight(self.m.HEM_SIDE[1]))
        fit_to_body(obj, self.body, drop_arms=True)
        self.body.lower_arms()  # the fabric doesn't follow the arms
        return sheet, [obj]


register(lambda body, info: TankTop(body, MAN).build(), id="tank-top-man", name="Tank Top",
         kind="top", body="man", order=270)
register(lambda body, info: TankTop(body, WOMEN).build(), id="tank-top-women", name="Tank Top",
         kind="top", body="women", order=280)
