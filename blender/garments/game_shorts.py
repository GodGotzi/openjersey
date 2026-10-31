"""Game shorts: loose game shorts (man) and tight short volleyball shorts
with a high waistband and a 9.5 cm inseam (women, "F5")."""

from oj.legs import leg_tube
from oj.registry import register
from oj.sheet import Sheet


def build_man(body, info):
    sheet = Sheet([["leg_r"], ["leg_l"]])
    # from the waist (hidden under the top's hem) to mid-thigh, relaxed and
    # flared towards the leg openings
    shorts = leg_tube(body, sheet, "bottom", top_z=body.m.WAIST_Z, hem_z=0.56, gap=0.005,
                      flare=0.035, flare_start_z=0.86)
    return sheet, [shorts]


F5_INSEAM = 0.095


def build_f5(body, info):
    sheet = Sheet([["waistband"], ["leg_r"], ["leg_l"]])
    # close to the body (little smoothing, no flare), a high waistband
    shorts = leg_tube(body, sheet, "bottom", top_z=body.m.WAIST_Z + 0.03, hem_z=body.m.CROTCH_Z - F5_INSEAM,
                      gap=0.003, rounds=3, passes=6, waistband=0.07)
    return sheet, [shorts]


register(build_man, id="game-shorts-man", name="Game Shorts",
         kind="bottom", body="man", order=120)
register(build_f5, id="game-shorts-f5-women", name="Game Shorts",
         kind="bottom", body="women", order=130)
