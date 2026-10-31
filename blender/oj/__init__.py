"""
Shared code of the garment builds (see blender/build.py and GARMENTS.md).

    geom     2D pattern geometry (curves, Coons grids, offsets)
    body     the posed mannequins, their measurements and arm pose
    mesh     pillow meshes from pattern grids, fitting onto the body
    sheet    the print sheet: pieces, atlas layout, pattern.json
    legs     bottoms cut from the body surface (shorts, tights, pants)
    bottoms  shaped hems, back panels, unrolled legs for those bottoms
    cut      garments cut along contours of the body surface (iso_cut, unwraps)
    finish   thickness, inside swatch, GLB export
    preview  Cycles preview renders
    registry the garment list
"""
