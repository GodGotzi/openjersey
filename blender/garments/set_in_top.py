"""Set-in-sleeve game shirts: T-shirt, sleeveless shirt and long-sleeve shirt
(man and women). Slim fit, set-in sleeves, a narrow crew-neck collar band;
ventilated side panels (a strip across each side seam, front and back half)
and shoulder panels (a crescent each side along the armhole, front and back
half joined at the shoulder seam). Sleeves end in a printed cuff band.

`SetInTop` is the cut: short, long or no sleeves, with or without side and
shoulder panels; other set-in tops (e.g. the crew shirt) reuse it."""

from oj.geom import (arc_lengths, band, coons, cubic, lerp2, quadratic, resample, sample_along, sub2, unit2,
                     with_mirrored)
from oj.mesh import Grids, collar_halves, fit_to_body, hem_weight, lower_arms, pillow_mesh
from oj.registry import register
from oj.sheet import Sheet

from garments.raglan_tshirt import RaglanTee

PIECES = {
    "shoulder_l": ("LEFT SHOULDER", "Left Shoulder", "accent"),
    "shoulder_r": ("RIGHT SHOULDER", "Right Shoulder", "accent"),
}

# Long sleeve openings at the wrist (front view, raised arm, right half):
# the arm runs straight from the short sleeve's cuff on, so the sleeve
# continues along the same axis; measured on the posed mannequins (wrist
# about 71% of the way from the shoulder pivot to the fingertips, radius
# ~2.7 cm, plus ease).
WRIST = {
    "man": {"LONG_CUFF_BOTTOM": (0.492, 0.963), "LONG_CUFF_TOP": (0.560, 1.031)},
    "women": {"LONG_CUFF_BOTTOM": (0.468, 0.935), "LONG_CUFF_TOP": (0.526, 1.001)},
}


def seam_frame(a, b):
    """Layout along a seam: x along a -> b, y across (perpendicular)."""
    e = unit2(sub2(b, a))
    return lambda p, s=1: ((p[0] - a[0]) * e[0] + (p[1] - a[1]) * e[1],
                           s * ((p[0] - a[0]) * e[1] - (p[1] - a[1]) * e[0]))


class SetInTop(RaglanTee):
    SLEEVE = "short"  # "short", "long" or None (sleeveless)
    SIDE_PANELS = True
    SHOULDER_PANELS = True
    V_NECK = False
    SIDE_HALF_WIDTH = 0.03
    ARMHOLE_BOW = 0.012  # set-in armhole bows in towards the body
    SIDE_JOIN = 0.1  # side panel top, as a fraction up the armhole
    SHOULDER_ON_ARMHOLE = 0.5  # shoulder panel's lower end, fraction up the armhole
    SHOULDER_ON_SEAM = 0.35  # its upper end, fraction from the collar to the shoulder point
    SLEEVELESS_DROP = 0.03  # sleeveless: armhole bottom below the underarm
    SLEEVELESS_IN = 0.006  # sleeveless: armhole top inside the shoulder point
    FIT_GAP = 0.009

    NR_ARM = 10  # along the armhole, side panel top to shoulder panel
    NS = 7  # along the shoulder panel's inner curve, per half
    NR_UNDER_LONG = 30

    def __init__(self, body):
        super().__init__(body)
        m = self.m
        if self.SLEEVE == "long":
            w = WRIST[body.name]
            self.cuff = (w["LONG_CUFF_BOTTOM"], w["LONG_CUFF_TOP"])
        else:
            self.cuff = (m.CUFF_BOTTOM, m.CUFF_TOP)
        if self.SLEEVE:
            self.arm_bottom, self.arm_top = m.UNDERARM, m.SHOULDER
        else:
            z = m.UNDERARM[1] - self.SLEEVELESS_DROP
            self.arm_bottom = (self.side_seam_x(z), z)
            self.arm_top = lerp2(m.SHOULDER, m.NECK_SIDE, self.SLEEVELESS_IN / (m.SHOULDER[0] - m.NECK_SIDE[0]))
        seams = [(m.HEM_SIDE, m.WAIST_SIDE), (m.WAIST_SIDE, self.arm_bottom), (self.arm_top, m.NECK_SIDE)]
        if self.SLEEVE:
            seams += [(m.UNDERARM, self.cuff[0]), (self.cuff[1], m.SHOULDER)]
        self.seams = with_mirrored(seams)

    # ---- crew neck ----
    # The neckline starts a little out along the shoulder seam from the
    # body's neck side, so it rests on the trapezius instead of riding up
    # the (cut away) neck, and the front is a shallow round curve.
    NECK_WIDEN = 0.012
    CREW_RAISE = 0.008

    def neck_side(self):
        m = self.m
        return lerp2(m.NECK_SIDE, m.SHOULDER, self.NECK_WIDEN / ((m.SHOULDER[0] - m.NECK_SIDE[0]) ** 2
                                                                + (m.SHOULDER[1] - m.NECK_SIDE[1]) ** 2) ** 0.5)

    def neck_dense(self, panel):
        m = self.m
        side = self.neck_side()
        bottom = m.FRONT_NECK_BOTTOM + self.CREW_RAISE if panel == "front" else m.BACK_NECK_BOTTOM
        pts = []
        for i in range(self.NECK_DENSE):
            t = i / (self.NECK_DENSE - 1)
            # rounder than a parabola: flat at the centre, rising steeply at the side
            w = 1 - (1 - t ** 2) ** 0.5 if panel == "front" else t * t
            w = 0.5 * w + 0.5 * t * t
            pts.append((side[0] * t, bottom + (side[1] - bottom) * w))
        return pts

    def collar_end_point(self):
        m = self.m
        d = unit2(sub2(m.SHOULDER, m.NECK_SIDE))
        side = self.neck_side()
        return (side[0] + d[0] * self.COLLAR_END, side[1] + d[1] * self.COLLAR_END)

    # ---- curves ----

    def armhole_dense(self):
        """The armhole from its bottom (on the side seam) to the shoulder."""
        a, b = self.arm_bottom, self.arm_top
        if self.SLEEVE:
            mid = lerp2(a, b, 0.5)
            n = unit2((b[1] - a[1], a[0] - b[0]))  # points out of the body
            return quadratic(a, (mid[0] - 2 * self.ARMHOLE_BOW * n[0], mid[1] - 2 * self.ARMHOLE_BOW * n[1]), b, 401)
        # sleeveless: leaves the side seam inwards, then up to the shoulder
        return cubic(a, (a[0] - 0.015, a[1] + 0.02), (b[0] - 0.004, b[1] - 0.1), b, 401)

    def armhole_parts(self):
        """(bottom->side join, join->shoulder panel, panel->top) samples."""
        dense = self.armhole_dense()
        lens = arc_lengths(dense)
        at = lambda f: min(range(len(dense)), key=lambda k: abs(lens[k] - f * lens[-1]))
        k_join = at(self.SIDE_JOIN) if self.SIDE_PANELS else 0
        k_sh = at(self.SHOULDER_ON_ARMHOLE) if self.SHOULDER_PANELS else len(dense) - 1
        p1 = resample(dense[:k_join + 1], self.NC_SIDE) if self.SIDE_PANELS else [dense[0]]
        p2 = resample(dense[k_join:k_sh + 1], self.NR_ARM)
        p3 = resample(dense[k_sh:], self.NS) if self.SHOULDER_PANELS else [dense[-1]]
        return p1, p2, p3

    def seam_point(self):
        """Shoulder panel's upper end on the shoulder seam."""
        return lerp2(self.collar_end_point(), self.arm_top, self.SHOULDER_ON_SEAM)

    def shoulder_curve(self, b):
        """The shoulder panel's inner edge from the armhole (b) up to the
        shoulder seam: down from the seam, then curving out to the armhole."""
        a = self.seam_point()
        control = (a[0] + 0.25 * (b[0] - a[0]), b[1] + 0.35 * (a[1] - b[1]))
        return resample(quadratic(b, control, a), 2 * self.NS - 1)

    # ---- pieces ----

    def panel_grids(self, sheet, panel, collar_lengths):
        m = self.m
        outer, inner = self.neck_edges(panel)
        n_in, n_out = sample_along(inner, outer, 0, len(inner) - 1, self.NB)
        arm1, arm2, arm3 = self.armhole_parts()
        join = arm2[0]
        top_end = self.seam_point() if self.SHOULDER_PANELS else self.arm_top
        seam_top = resample([self.collar_end_point(), top_end], 5)

        # body: centre front/back up to the collar and shoulder seam, out to
        # the side panel, the armhole and the shoulder panel
        if self.SIDE_PANELS:
            h_in = (m.HEM_SIDE[0] - self.SIDE_HALF_WIDTH, m.HEM_SIDE[1])
            w_in = (m.WAIST_SIDE[0] - self.SIDE_HALF_WIDTH, m.WAIST_SIDE[1])
            side_inner = resample([h_in, w_in, join], self.NR_SIDE)
        else:
            h_in = m.HEM_SIDE
            side_inner = resample([m.HEM_SIDE, m.WAIST_SIDE, join], self.NR_SIDE)
        right = side_inner + arm2[1:]
        if self.SHOULDER_PANELS:
            curve = self.shoulder_curve(arm2[-1])
            right += curve[1:]
        top = n_in + seam_top[1:]
        body = coons(resample([(0.0, m.HEM_SIDE[1]), h_in], len(top)), top,
                     resample([(0.0, m.HEM_SIDE[1]), n_in[0]], len(right)), right)

        front = panel == "front"
        grids = Grids(sheet, panel)
        flip = 1 if front else -1
        grids.add(panel, body, lambda j, i, p: (flip * p[0], -p[1]))
        grids.add(panel, body, lambda j, i, p: (-flip * p[0], -p[1]), mirrored=True)

        if self.SIDE_PANELS:
            side = coons(resample([h_in, m.HEM_SIDE], self.NC_SIDE), list(reversed(arm1)), side_inner,
                         resample([m.HEM_SIDE, m.WAIST_SIDE, self.arm_bottom], self.NR_SIDE))
            for piece, s, mirrored in (("side_l", -1 if front else 1, False), ("side_r", 1 if front else -1, True)):
                grids.add(piece, side, lambda j, i, p, s=s: (s * (self.side_seam_x(p[1]) - p[0]), -p[1]), mirrored)

        if self.SHOULDER_PANELS:
            # corners: armhole end b, armhole top, seam point a; the inner
            # curve is split in its middle so the patch has four sides
            k = self.NS
            shoulder = coons(arm3, curve[k - 1:], curve[:k], resample([self.arm_top, self.seam_point()], k))
            # laid out from the shoulder seam: front half below, back above
            frame = seam_frame(self.arm_top, self.seam_point())
            for piece, mirrored in (("shoulder_l", False), ("shoulder_r", True)):
                s = 1 if front else -1
                d = 1 if piece == "shoulder_l" else -1
                grids.add(piece, shoulder, lambda j, i, p, s=s, d=d: (d * frame(p)[0], s * frame(p)[1]), mirrored)

        if self.SLEEVE:
            armhole = arm1 + arm2[1:] + arm3[1:]
            n_rows = self.NR_UNDER_LONG if self.SLEEVE == "long" else self.NR_UNDER
            cuff_b, cuff_t = self.cuff
            sleeve = coons(armhole, resample([cuff_b, cuff_t], len(armhole)),
                           resample([m.UNDERARM, cuff_b], n_rows), resample([m.SHOULDER, cuff_t], n_rows))
            # the top seam is the centre line: length along it, width across
            v = arc_lengths([row[-1] for row in sleeve])
            widths = [list(reversed(arc_lengths(list(reversed(row))))) for row in sleeve]
            for piece, s, mirrored in (("sleeve_l", -1 if front else 1, False), ("sleeve_r", 1 if front else -1, True)):
                grids.add(piece, sleeve, lambda j, i, p, s=s: (s * widths[j][i], v[j]), mirrored)
                sheet.add_cuff(piece, v[-1], widths[-1][0])

        # collar band: front (x > 0) -> back -> back (x < 0) -> front
        collar = band(n_in, n_out)
        along = arc_lengths(n_out)
        for offset, direction, mirrored in collar_halves(front, collar_lengths["front"], collar_lengths["back"]):
            grids.add("collar", collar,
                      lambda j, i, p, o=offset, d=direction: (o + d * along[i], j * self.COLLAR_WIDTH / 2), mirrored)
        return grids.items

    def rows(self):
        middle = []
        if self.SIDE_PANELS:
            middle.append("side_r")
        if self.SHOULDER_PANELS:
            middle.append("shoulder_r")
        sleeves = ["sleeve_r", "sleeve_l"] if self.SLEEVE else []
        left = [p.replace("_r", "_l") for p in reversed(middle)]
        if self.SLEEVE == "long":
            return [["front", "back"], sleeves, middle + left + ["collar"]]
        return [["front", "back"], middle + sleeves + left, ["collar"]]

    def build(self):
        sheet = Sheet(self.rows(), PIECES)
        collar_lengths = self.collar_lengths()
        panels = [(panel, self.panel_grids(sheet, panel, collar_lengths)) for panel in ("front", "back")]
        obj = pillow_mesh("top", sheet, panels, self.seams, self.depth, hem_weight(self.m.HEM_SIDE[1]))
        # also sleeveless: the wide shoulders sit on the shoulder joint and
        # turn with it (without the arms the shoulder blades' fabric slips in)
        fit_to_body(obj, self.body, gap=self.FIT_GAP)
        lower_arms(obj, self.body, gap=self.FIT_GAP)
        return sheet, [obj]


class TShirtCut(SetInTop):
    pass


class SleevelessCut(SetInTop):
    SLEEVE = None


class LongSleeveCut(SetInTop):
    SLEEVE = "long"


def builder(cut, women_side=0.028):
    def build(body, info):
        shirt = cut(body)
        if body.name == "women":
            shirt.SIDE_HALF_WIDTH = women_side
        return shirt.build()
    return build


for cls, product, orders in (
    (TShirtCut, ("tshirt", "T-Shirt"), (60, 70)),
    (SleevelessCut, ("sleeveless-shirt", "Sleeveless Shirt"), (80, 90)),
    (LongSleeveCut, ("longsleeve-shirt", "Long-Sleeve Shirt"), (100, 110)),
):
    for body, order in zip(("man", "women"), orders):
        register(builder(cls), id=f"{product[0]}-{body}", name=product[1], kind="top", body=body, order=order)
