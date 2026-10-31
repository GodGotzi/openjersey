"""Beach shorts (man and women): beach volleyball shorts with a wide
waistband and rounded side splits at the leg openings (piped). The man's
reach to mid-thigh, the women's are short running-style shorts. Pieces:
waistband, leg_r, leg_l."""

from types import SimpleNamespace

from oj.bottoms import bridge_seat, shape_hem, side_split, unroll_legs
from oj.legs import leg_tube
from oj.registry import register
from oj.sheet import Sheet

MAN = SimpleNamespace(RISE=0.0, WAISTBAND=0.055, HEM=lambda m: 0.575, FLARE=0.03, SPLIT=(0.065, 0.16))
WOMEN = SimpleNamespace(RISE=0.02, WAISTBAND=0.07, HEM=lambda m: m.CROTCH_Z - 0.045, FLARE=0.012,
                        SPLIT=(0.05, 0.17))


def build(shape):
    def build_body(body, info):
        m = body.m
        top_z, hem_z, band = m.WAIST_Z + shape.RISE, shape.HEM(m), shape.WAISTBAND
        sheet = Sheet([["waistband"], ["leg_r", "leg_l"]])
        obj = leg_tube(body, sheet, "bottom", top_z=top_z, hem_z=hem_z, gap=0.008, flare=shape.FLARE,
                       flare_start_z=m.CROTCH_Z + 0.04, rounds=6, passes=30,
                       waistband=band)
        bridge_seat(obj, body)
        rise = side_split(*shape.SPLIT)
        shape_hem(obj, body, top_z, hem_z, band, rise, gap=0.008)
        unroll_legs(obj, sheet, body, top_z, hem_z, band, rise=rise, taper=False)
        return sheet, [obj]
    return build_body


register(build(MAN), id="beach-shorts-man", name="Beach Shorts",
         kind="bottom", body="man", order=330)
register(build(WOMEN), id="beach-shorts-women", name="Beach Shorts",
         kind="bottom", body="women", order=340)
