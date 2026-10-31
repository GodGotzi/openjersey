"""Crew neck shirt (unisex): a regular-fit long-sleeve crew neck for
training. Front, back, set-in long sleeves with a
printed cuff band, and a ribbed crew collar; no side or shoulder panels.
Built for both bodies (unisex) with the set-in cut (`SetInTop`)."""

from oj.registry import register

from garments.set_in_top import SetInTop


class CrewShirt(SetInTop):
    SLEEVE = "long"
    SIDE_PANELS = False
    SHOULDER_PANELS = False
    COLLAR_WIDTH = 0.02  # rib collar, wider than the game shirts' band
    FIT_GAP = 0.014  # regular fit: a little more room than the slim game shirts


def build(body, info):
    return CrewShirt(body).build()


for body in ("man", "women"):
    register(build, id=f"crew-shirt-{body}", name="Crew Neck Shirt",
             kind="top", body=body, unisex=True, order=300)
