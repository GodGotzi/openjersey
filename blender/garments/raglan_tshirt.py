"""Raglan T-shirt (man and women): a raglan game shirt. The pieces are those of a typical print template: front,
back, a side panel strip per side (front and back half), a raglan sleeve per
side (front and back half, joined along the shoulder/top seam) and the
collar band.

`RaglanTee` is the cut, sized to a body's measurements; other raglan tops
can subclass it (e.g. change the neckline or the sleeve length)."""

from oj.geom import (arc_lengths, band, coons, lerp2, mirror, polyline_x, quadratic, resample, sample_along,
                     smoothstep, sub2, unit2, with_mirrored)
from oj.mesh import Grids, collar_halves, fit_to_body, hem_weight, lower_arms, pillow_depth, pillow_mesh
from oj.registry import register
from oj.sheet import Sheet

ROWS = [
    ["front", "back"],
    ["side_r", "sleeve_r", "sleeve_l", "side_l"],
    ["collar"],
]


class RaglanTee:
    SIDE_HALF_WIDTH = 0.03  # side panel width on the front and on the back
    COLLAR_WIDTH = 0.014
    COLLAR_END = 0.016  # where the collar band's end meets the shoulder seam
    RAGLAN_FROM_NECK = 0.03  # raglan seam start on the collar, from its end
    # the raglan seam bows out towards the shoulder point (0 = straight), so
    # front and back reach far onto the shoulder and the sleeve only runs up
    # to the collar as a narrow strip, as on the print template
    RAGLAN_BULGE = 0.6
    RAGLAN_JOIN = 0.12  # side panel top, as a fraction up the raglan seam
    V_NECK = True  # the front neckline runs nearly straight (a soft V)

    # samples
    NB = 16  # across half the front/back
    NR_SIDE = 26  # up the side panel
    NR_RAGLAN = 14  # along the raglan seam, side panel top to collar
    NC_SIDE = 4  # across the side panel half
    NR_UNDER = 8  # along the underarm seam
    NC_SLEEVE = 12  # across the sleeve half
    NECK_DENSE = 201

    def __init__(self, body):
        self.body = body
        m = self.m = body.m
        self.seams = with_mirrored([
            (m.HEM_SIDE, m.WAIST_SIDE),
            (m.WAIST_SIDE, m.UNDERARM),
            (m.UNDERARM, m.CUFF_BOTTOM),
            (m.CUFF_TOP, m.SHOULDER),
            (m.SHOULDER, m.NECK_SIDE),
        ])

    # ---- neckline and collar ----

    def neck_dense(self, panel):
        """The neck edge from the centre (x = 0) to the neck side."""
        m = self.m
        bottom = m.FRONT_NECK_BOTTOM if panel == "front" else m.BACK_NECK_BOTTOM
        v_shape = panel == "front" and self.V_NECK
        pts = []
        for i in range(self.NECK_DENSE):
            t = i / (self.NECK_DENSE - 1)
            w = t ** 1.1 if v_shape else t * t
            pts.append((m.NECK_SIDE[0] * t, bottom + (m.NECK_SIDE[1] - bottom) * w))
        return pts

    def collar_end_point(self):
        m = self.m
        d = unit2(sub2(m.SHOULDER, m.NECK_SIDE))
        return (m.NECK_SIDE[0] + d[0] * self.COLLAR_END, m.NECK_SIDE[1] + d[1] * self.COLLAR_END)

    def collar_inner(self, outer):
        """The collar's inner edge: the neck edge offset into the shirt by
        COLLAR_WIDTH (mitred), ending on the shoulder seam at a point shared
        by front and back so the seam lines up."""
        def normal(a, b):
            t = unit2(sub2(b, a))
            return (t[1], -t[0])  # left-to-right tangent -> pointing down into the shirt

        # mirror the first segment so the centre gets a symmetric mitre
        pts = [mirror(outer[1])] + outer
        inner = []
        for k in range(1, len(pts)):
            n2 = normal(pts[k], pts[k + 1]) if k + 1 < len(pts) else normal(pts[k - 1], pts[k])
            n1 = normal(pts[k - 1], pts[k])
            mm = (n1[0] + n2[0], n1[1] + n2[1])
            f = self.COLLAR_WIDTH / (1 + n1[0] * n2[0] + n1[1] * n2[1])
            inner.append((pts[k][0] + mm[0] * f, pts[k][1] + mm[1] * f))
        inner[0] = (0.0, inner[0][1])
        # pull the end onto the shared shoulder-seam point
        end = self.collar_end_point()
        shift = sub2(end, inner[-1])
        ramp = 12
        for k in range(ramp):
            w = smoothstep(0, 1, (k + 1) / ramp)
            i = len(inner) - ramp + k
            inner[i] = (inner[i][0] + shift[0] * w, inner[i][1] + shift[1] * w)
        inner[-1] = end
        return inner

    def neck_edges(self, panel):
        outer = self.neck_dense(panel)
        return outer, self.collar_inner(outer)

    # ---- pieces ----

    def side_seam_x(self, z):
        m = self.m
        return polyline_x([m.HEM_SIDE, m.WAIST_SIDE, m.UNDERARM], z)

    def raglan_curve(self, join, r_top, n):
        """The raglan seam from the side panel top to the collar: a quadratic
        curve whose middle lies RAGLAN_BULGE of the way out to the shoulder."""
        s = self.m.SHOULDER
        mid = lerp2(join, r_top, 0.5)
        control = (mid[0] + 2 * self.RAGLAN_BULGE * (s[0] - mid[0]), mid[1] + 2 * self.RAGLAN_BULGE * (s[1] - mid[1]))
        return resample(quadratic(join, control, r_top), n)

    def panel_grids(self, sheet, panel, collar_lengths):
        """All grids of one panel (front or back): the right half (x > 0, the
        wearer's left) is built, the other half mirrored."""
        m = self.m
        outer, inner = self.neck_edges(panel)
        last = len(inner) - 1
        lens = arc_lengths(inner)
        k_r = max(k for k in range(last) if lens[last] - lens[k] >= self.RAGLAN_FROM_NECK)
        r_top = inner[k_r]
        join = lerp2(m.UNDERARM, r_top, self.RAGLAN_JOIN)
        h_in = (m.HEM_SIDE[0] - self.SIDE_HALF_WIDTH, m.HEM_SIDE[1])
        w_in = (m.WAIST_SIDE[0] - self.SIDE_HALF_WIDTH, m.WAIST_SIDE[1])

        # shared edges, computed once so neighbouring grids get identical points
        side_inner = resample([h_in, w_in, join], self.NR_SIDE)
        raglan = self.raglan_curve(join, r_top, self.NR_RAGLAN)  # side panel top to collar
        side_top = resample([join, m.UNDERARM], self.NC_SIDE)
        underarm = resample([m.UNDERARM, m.CUFF_BOTTOM], self.NR_UNDER)
        mid_in, mid_out = sample_along(inner, outer, 0, k_r, self.NB)
        end_in, end_out = sample_along(inner, outer, k_r, last, self.NC_SLEEVE)

        body = coons(
            resample([(0.0, m.HEM_SIDE[1]), h_in], self.NB),
            mid_in,
            resample([(0.0, m.HEM_SIDE[1]), mid_in[0]], self.NR_SIDE + self.NR_RAGLAN - 1),
            side_inner + raglan[1:],
        )
        side = coons(resample([h_in, m.HEM_SIDE], self.NC_SIDE), side_top, side_inner,
                     resample([m.HEM_SIDE, m.WAIST_SIDE, m.UNDERARM], self.NR_SIDE))
        sleeve_left = list(reversed(raglan)) + side_top[1:] + underarm[1:]
        sleeve = coons(end_in, resample([m.CUFF_BOTTOM, m.CUFF_TOP], self.NC_SLEEVE), sleeve_left,
                       resample([self.collar_end_point(), m.SHOULDER, m.CUFF_TOP], len(sleeve_left)))
        collar = band(mid_in + end_in[1:], mid_out + end_out[1:])

        front = panel == "front"
        grids = Grids(sheet, panel)
        # front and back as seen from their own side (so the back is mirrored)
        flip = 1 if front else -1
        grids.add(panel, body, lambda j, i, p: (flip * p[0], -p[1]))
        grids.add(panel, body, lambda j, i, p: (-flip * p[0], -p[1]), mirrored=True)

        # side panels across the side seam; seen from the wearer's side, the
        # front half lies towards the front (left on the left side, right on the right)
        for piece, s, mirrored in (("side_l", -1 if front else 1, False), ("side_r", 1 if front else -1, True)):
            grids.add(piece, side, lambda j, i, p, s=s: (s * (self.side_seam_x(p[1]) - p[0]), -p[1]), mirrored)

        # sleeves: the shoulder/top seam is the centre line; length along it
        # down to the cuff, width across each row from it
        v = arc_lengths([row[-1] for row in sleeve])
        widths = [list(reversed(arc_lengths(list(reversed(row))))) for row in sleeve]
        for piece, s, mirrored in (("sleeve_l", -1 if front else 1, False), ("sleeve_r", 1 if front else -1, True)):
            grids.add(piece, sleeve, lambda j, i, p, s=s: (s * widths[j][i], v[j]), mirrored)
            sheet.add_cuff(piece, v[-1], widths[-1][0])

        # collar: one strip running front (x > 0) -> back (x > 0) -> back
        # (x < 0) -> front (x < 0), joined at the shoulder seams
        along = arc_lengths(collar[0])
        for offset, direction, mirrored in collar_halves(front, collar_lengths["front"], collar_lengths["back"]):
            grids.add("collar", collar,
                      lambda j, i, p, o=offset, d=direction: (o + d * along[i], j * self.COLLAR_WIDTH / 2), mirrored)
        return grids.items

    def sleeve_factor(self, p):
        """0 on the torso, rising to 1 within 14 cm beyond the armhole line."""
        m = self.m
        n = unit2((m.SHOULDER[1] - m.UNDERARM[1], m.UNDERARM[0] - m.SHOULDER[0]))  # points outwards
        d = (abs(p[0]) - m.UNDERARM[0]) * n[0] + (p[1] - m.UNDERARM[1]) * n[1]
        return smoothstep(0.0, 0.14, d)

    def depth(self, p, panel):
        m = self.m
        torso = m.RHO_FRONT if panel == "front" else m.RHO_BACK
        return pillow_depth(p, torso + (m.RHO_SLEEVE - torso) * self.sleeve_factor(p), self.seams)

    def collar_lengths(self):
        """Length of the collar's outer edge on the front and on the back
        (one half)."""
        return {panel: arc_lengths(self.neck_edges(panel)[0])[-1] for panel in ("front", "back")}

    def build(self, rows=ROWS, pieces=None):
        """The fitted shirt: (sheet, [object])."""
        sheet = Sheet(rows, pieces)
        collar_lengths = self.collar_lengths()
        panels = [(panel, self.panel_grids(sheet, panel, collar_lengths)) for panel in ("front", "back")]
        obj = pillow_mesh("top", sheet, panels, self.seams, self.depth, hem_weight(self.m.HEM_SIDE[1]))
        fit_to_body(obj, self.body)
        lower_arms(obj, self.body)
        return sheet, [obj]


class WomenRaglanTee(RaglanTee):
    """The women's cut: slightly narrower side panels; the measurements
    (bust room, fitted waist, shorter sleeves) come from the body."""
    SIDE_HALF_WIDTH = 0.028


def build_man(body, info):
    return RaglanTee(body).build()


def build_women(body, info):
    return WomenRaglanTee(body).build()


register(build_man, id="raglan-tshirt-man", name="Raglan T-Shirt",
         kind="top", body="man", order=190)
register(build_women, id="raglan-tshirt-women", name="Raglan T-Shirt",
         kind="top", body="women", order=200)
