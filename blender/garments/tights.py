"""Tights (women): full-length training tights, close to the body from a high supportive waistband down to
the ankle. Pieces: the waistband and one leg piece per leg."""

from oj.legs import leg_tube
from oj.registry import register
from oj.sheet import Sheet

WAISTBAND = 0.09  # high waistband, reaching above the waist
RISE = 0.06  # waistband top above the waist


def build(body, info):
    m = body.m
    sheet = Sheet([["waistband"], ["leg_r", "leg_l"]])
    tights = leg_tube(body, sheet, "bottom", top_z=m.WAIST_Z + RISE, hem_z=m.ANKLE_Z + 0.01,
                      gap=0.003, rounds=3, passes=6, waistband=WAISTBAND)
    return sheet, [tights]


register(build, id="tights-women", name="Tights",
         kind="bottom", body="women", order=360)
