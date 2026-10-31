# Garments: the contract between the Blender builds and the app

Every garment is one folder `garments/<id>/`, written
by `blender/build.py`; the app finds the folders at compile time (build.rs),
so adding a garment needs no Rust change.

A design is made for one **body** (`man` or `women`) and wears one **top**
and optionally one **bottom** fitted on that body. Tops and bottoms of the
same body are fitted on the same posed mannequin, so any top goes with any
bottom; the bottom's waist sits under the top's hem.

## Folder

    garments/<id>/
        garment.json   metadata
        model.glb      the garment mesh (one or more mesh nodes), fabric
                       thickness included; UVs cover this garment's own atlas
        pattern.json   the print sheet of this garment alone

`<id>` = `<product>-<body>`, lowercase, e.g. `raglan-tshirt-man`,
`bikini-bottom-women`.
Unisex products are built once per body (`…-man` and `…-women`).

## garment.json

    {
      "id": "raglan-tshirt-man",
      "name": "Raglan T-Shirt",           // display name; the man's and
                                           // women's version share it
      "kind": "top" | "bottom",
      "body": "man" | "women",
      "unisex": false,
      "order": 190                         // sort key in the garment list
    }

## pattern.json

Atlas pixels, y down, origin top left of this garment's atlas.

    {
      "atlas": {"width": W, "height": H, "pxPerM": 1200},
      "pieces": [
        {
          "id": "front",                  // unique within top+bottom, see below
          "label": "FRONT",               // printed on the sheet
          "name": "Front",                // English display name
          "role": "primary" | "accent" | "trim",   // default base colour
          "rect": [x, y, w, h],
          "polygons": [[[x, y], ...], ...]
        }
      ],
      "cuffs": [{"piece": "sleeve_l", "rect": [x, y, w, h]}],   // printed cuff bands, may be []
      "inside": [x, y, w, h],             // plain square the fabric's inside maps to
      "placement": {                      // tops: front/back garment origin, see below
        "front": {"origin": [x, y], "xSign": 1},
        "back": {"origin": [x, y], "xSign": -1}
      },
      "legs": [ ... ]                     // leg stripes of bottoms with leg pieces (below); else []
    }

* `placement` (required for tops, optional for bottoms): the atlas pixel of
  the garment point x = 0, z = 0 (front-view metres, x right = wearer's left,
  z up from the floor) on the front and on the back piece; `xSign` -1 on the
  back (seen from behind). Every top has pieces `front` and `back`.
* Piece ids: tops use `front`, `back`, `side_l/r`, `sleeve_l/r`, `collar`,
  `armhole_l/r`, `hood`, `placket_l/r`, `pocket_l/r`, `cuff_band_l/r`,
  `hem_band`, …; bottoms use ids that can't clash with a top's: `leg_l/r`,
  `waistband`, `bottom_front`, `bottom_back`, `gusset`, `side_panel_l/r`,
  `back_panel`, `drawstring`, … `_l` = the wearer's left.
* `legs[]`: `{"piece": "leg_l", "frontDir": -1, "stripe": {"topCentre", "topWidth",
  "hemCentre", "hemWidth"}}` (stripe as fractions of the piece width; centre
  it on the outer side seam if there is no side panel to continue).
* Leave `inside` free of pieces (bottom right corner); every face of the
  inner surface maps to its centre.

## The app

The app stacks the top's atlas over the bottom's (centred) into one sheet,
renders it, and puts the matching part on each GLB as its texture.

## Building

    blender --background --python blender/build.py -- --list
    blender --background --python blender/build.py -- --garment <id|prefix|all> [--render <dir>] [--views front,back,...] [--preview-only]
    blender --background --python blender/build.py -- --combo <top id>,<bottom id> --render <dir>

Needs `assets-src/mannequin-<body>.blend` (README, "Assets"). A garment
takes 3–10 s. `--render` writes Cycles previews (views: see `VIEWS` in
`oj/preview.py`, e.g. front, neck, three_quarter, back, side, with_body,
full_front for bottoms); `--combo` renders an
already built top and bottom together on their body. Check the result in
the app too:

    cargo run --release -- --top "<garment name or id>" --bottom "<…>" \
        --export-template sheet.png --garment preview.png

### Adding a garment

One module per garment type in `blender/garments/` (found automatically),
registering one builder per body:

    """<Name> (man and women): what it is, its pieces and how they're
    joined."""

    from oj.registry import register
    from oj.sheet import Sheet

    def build_man(body, info):
        sheet = Sheet([["front", "back"], ["sleeve_r", "sleeve_l"], ["collar"]],
                      {"hood": ("HOOD", "Hood", "accent")})  # rows of the atlas; extra pieces
        ...                                                     # make the mesh object(s)
        return sheet, [obj]

    register(build_man, id="<product>-man", name="<Name>",
             kind="top", body="man", order=<sort key>)

`build(body, info)` returns the `Sheet` and the mesh objects; `build.py`
then lays out the atlas, adds thickness and the inside swatch (`finish`),
and writes the three files. Each object needs a UV map "UVMap" holding
piece-local layout metres (lx, ly down) and an int face layer/attribute
"piece" (index into the sheet's pieces), as `pillow_mesh` and `leg_tube`
make them. Set `obj["subdivide"] = False` for meshes that are already
dense.

Unisex products are registered twice (…-man, …-women, `unisex=True`),
with the cut sized from `body.m`.

### Helpers (`blender/oj/`)

* `body.Body`: `m` the measurements (`MEASUREMENTS`, below), `obj` the
  posed mannequin (arms raised), `fitting_target(drop_arms)` (body without
  head/neck; with `drop_arms` also without arms, for sleeveless tops),
  `surface_band(z0, z1)` (the body's skin between two heights),
  `lower_arms()`; `new_object` and `largest_island` for bmesh work.
* `geom`: 2D pattern geometry in front-view metres: `lerp2`, `toward`,
  `resample`, `quadratic`, `cubic`, `coons` (grid between four edges),
  `relax`, `offset_curve`, `band`, `sample_along`, `polyline_x`,
  `with_mirrored`, `seam_distance`, `smoothstep`.
* `mesh`: tops are built "pillow" style: 2D grids per piece on the front
  and back panels (`Grids.add(piece, grid, local_fn, mirrored)`, which
  also records the piece outline), inflated into a pillow
  (`pillow_mesh` + `pillow_depth`; points on `seams` join front and back),
  pulled onto the body (`fit_to_body`), then the arms lowered with
  (`lower_arms(garment, body)`, sleeves) or without the fabric
  (`body.lower_arms()`, sleeveless). `collar_halves` lays the collar strip
  out; `shrinkwrap`, `smooth_inner`.
* `legs.leg_tube(body, sheet, name, top_z, hem_z, …)`: shorts, tights and
  pants from the waist to any hem height (flare, closeness, waistband,
  leg stripe); `tuck` keeps a bottom's waist inside the tops;
  `relaxed_skin` is the smoothed body skin it starts from.
* `bottoms`: finishing for `leg_tube` results: `shape_hem` / `side_split`
  (side splits), `ease_legs` (looser legs), `bridge_seat` /
  `bridge_hollows`, `add_panel` (e.g. a back panel) and `unroll_legs`
  (leg pieces true to size; call it last).
* `cut`: garments cut along contours of the body surface (`iso_cut`,
  `conformal_unwrap`, `strip_uv`, `add_outlines`), see the module doc.
* `sheet.Sheet`: atlas rows, piece labels/names/roles (`PIECES` has the
  usual ones), `add_cuff` for printed sleeve cuffs, `legs` entries.
* `preview.render`.

Reuse a cut by importing its class from another garment module (e.g.
`from garments.raglan_tshirt import RaglanTee`) and subclassing it.
Don't change the behaviour of shared code other garments use; add new
helpers as new modules (`oj/<topic>.py`) or inside your garment module,
and rebuild the garments you touched plus `--garment all` at the end to be
sure.

### Rules of thumb

* Tops: hem at about `m.HEM_SIDE[1]` (longer is fine), every top has
  pieces `front` and `back` (one piece each in the sheet even if the real
  garment is cut in two, e.g. at a zip: the app places designs by them).
* Bottoms: waist at `m.WAIST_Z` or above, so it disappears under any top's
  hem (`tuck`); ids must not clash with tops' pieces.
* Look at the renders from front, back and side; no spikes, no fabric
  through the body or through the other garment (`--combo`).

## Measurements

`oj/body.py` `MEASUREMENTS` holds per body the pattern landmarks (front
view, right half): side seam `HEM_SIDE → WAIST_SIDE → UNDERARM`, shoulder
seam `NECK_SIDE → SHOULDER`, neck depths, the raised upper arm
(`CUFF_BOTTOM`, `CUFF_TOP` at short-sleeve length), pillow depths, and for
bottoms `WAIST_Z`, `CROTCH_Z`, `THIGH_AXIS_Z`. They were measured on the
posed mannequins; add new landmarks there (for both bodies) rather than
hard-coding body positions in a garment.
