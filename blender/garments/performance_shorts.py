"""Performance shorts (man) and back-panel shorts (man): game shorts,
mid-thigh with an elastic waistband and a little room at the leg openings.
The back-panel variant adds a panel across the seat, from side seam to side
seam, for a full-width print. Pieces: waistband, leg_r, leg_l (and back_panel)."""

from oj.bottoms import add_panel, bridge_seat, unroll_legs
from oj.legs import leg_tube
from oj.registry import register
from oj.sheet import Sheet

WAISTBAND = 0.04
HEM_Z = 0.585  # mid-thigh, a little shorter than the game shorts
PIECES = {"back_panel": ("BACK PANEL", "Back Panel", "accent")}


def shorts(body, sheet):
    m = body.m
    top_z = m.WAIST_Z
    obj = leg_tube(body, sheet, "bottom", top_z=top_z, hem_z=HEM_Z, gap=0.005, flare=0.025,
                   flare_start_z=0.84, waistband=WAISTBAND)
    bridge_seat(obj, body)
    return obj, top_z


def build_q2(body, info):
    sheet = Sheet([["waistband"], ["leg_r", "leg_l"]])
    obj, top_z = shorts(body, sheet)
    unroll_legs(obj, sheet, body, top_z, HEM_Z, WAISTBAND, taper=False)
    return sheet, [obj]


def build_pro(body, info):
    sheet = Sheet([["waistband"], ["back_panel"], ["leg_r", "leg_l"]], PIECES)
    obj, top_z = shorts(body, sheet)
    # from under the waistband down over the seat to just above the crotch
    # (most of it shows below a top's hem), between the side seams
    add_panel(obj, sheet, "back_panel", top_z - WAISTBAND, body.m.CROTCH_Z + 0.01, 0.22)
    unroll_legs(obj, sheet, body, top_z, HEM_Z, WAISTBAND, taper=False)
    return sheet, [obj]


register(build_q2, id="performance-shorts-man", name="Performance Shorts",
         kind="bottom", body="man", order=150)
register(build_pro, id="back-panel-shorts-man", name="Back-Panel Shorts",
         kind="bottom", body="man", order=160)
