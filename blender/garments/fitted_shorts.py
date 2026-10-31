"""Fitted shorts (women): tight game shorts with a supportive waistband and a 7 cm inseam (a little shorter than the
women's game shorts, with a lower, narrower waistband)."""

from oj.legs import leg_tube
from oj.registry import register
from oj.sheet import Sheet

INSEAM = 0.07
WAISTBAND = 0.055


def build(body, info):
    m = body.m
    sheet = Sheet([["waistband"], ["leg_r"], ["leg_l"]])
    shorts = leg_tube(body, sheet, "bottom", top_z=m.WAIST_Z + 0.01, hem_z=m.CROTCH_Z - INSEAM,
                      gap=0.003, rounds=3, passes=6, waistband=WAISTBAND)
    return sheet, [shorts]


register(build, id="fitted-shorts-women", name="Fitted Shorts",
         kind="bottom", body="women", order=180)
