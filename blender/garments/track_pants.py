"""Track pants (unisex): regular fit, from an elastic waistband to the ankle,
tapering towards the ankle zips (not modelled). The leg pieces are laid out
true to size from thigh to ankle. Pieces: waistband, leg_r, leg_l."""

from oj.bottoms import bridge_seat, ease_legs, unroll_legs
from oj.geom import smoothstep
from oj.legs import leg_tube
from oj.registry import register
from oj.sheet import Sheet

WAISTBAND = 0.045


def ease(m):
    """Room around the leg: regular through thigh and knee, tapering to
    the ankle cuff."""
    knee = m.CROTCH_Z - 0.26
    return lambda z: 0.004 + 0.014 * smoothstep(m.ANKLE_Z + 0.02, knee, z)


def build(body, info):
    m = body.m
    top_z, hem_z = m.WAIST_Z, m.ANKLE_Z + 0.01
    sheet = Sheet([["waistband"], ["leg_r", "leg_l"]])
    obj = leg_tube(body, sheet, "bottom", top_z=top_z, hem_z=hem_z, gap=0.010, rounds=6, passes=30,
                   waistband=WAISTBAND)
    ease_legs(obj, body, hem_z, ease(m), passes=12, gap=0.010)
    bridge_seat(obj, body)
    unroll_legs(obj, sheet, body, top_z, hem_z, WAISTBAND)
    return sheet, [obj]


for body in ("man", "women"):
    register(build, id=f"track-pants-{body}", name="Track Pants",
             kind="bottom", body=body, order=250, unisex=True)
