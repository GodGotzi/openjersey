"""Board shorts (man and women): loose beach volleyball board shorts with a
flat waistband (drawstring and side pockets are not modelled). The man's end
above the knee with a straight hem, the women's are short with small side
notches. Pieces: waistband, leg_r, leg_l."""

from types import SimpleNamespace

from oj.bottoms import bridge_seat, shape_hem, side_split, unroll_legs
from oj.legs import leg_tube
from oj.registry import register
from oj.sheet import Sheet

MAN = SimpleNamespace(WAISTBAND=0.045, HEM=lambda m: 0.545, FLARE=0.05, GAP=0.011, SPLIT=None)
WOMEN = SimpleNamespace(WAISTBAND=0.04, HEM=lambda m: m.CROTCH_Z - 0.065, FLARE=0.035, GAP=0.009,
                        SPLIT=(0.025, 0.07))


def build(shape):
    def build_body(body, info):
        m = body.m
        top_z, hem_z, band = m.WAIST_Z, shape.HEM(m), shape.WAISTBAND
        sheet = Sheet([["waistband"], ["leg_r", "leg_l"]])
        # loose: heavy smoothing bridges the seat and the crotch
        obj = leg_tube(body, sheet, "bottom", top_z=top_z, hem_z=hem_z, gap=shape.GAP, flare=shape.FLARE,
                       flare_start_z=m.CROTCH_Z + 0.1, rounds=6, passes=30, waistband=band)
        bridge_seat(obj, body)
        rise = side_split(*shape.SPLIT) if shape.SPLIT else None
        if rise:
            shape_hem(obj, body, top_z, hem_z, band, rise, gap=shape.GAP)
        unroll_legs(obj, sheet, body, top_z, hem_z, band, rise=rise, taper=False)
        return sheet, [obj]
    return build_body


register(build(MAN), id="board-shorts-man", name="Board Shorts",
         kind="bottom", body="man", order=310)
register(build(WOMEN), id="board-shorts-women", name="Board Shorts",
         kind="bottom", body="women", order=320)
