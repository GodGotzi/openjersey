"""The print sheet of one garment: its pieces laid out in rows like a
sublimation print template, the atlas UVs, pattern.json and
garment.json (see GARMENTS.md for the format)."""

import json

ATLAS_PX_PER_M = 1200  # also the print resolution
ATLAS_MARGIN = 0.04
ATLAS_GAP = 0.05
ATLAS_LABEL = 0.06  # room for the piece name above each row
CUFF_WIDTH = 0.016  # printed cuff band at a sleeve end
# the fabric's inside is mapped to this small square in the bottom right
# corner of the atlas, which the app fills with a plain colour
INSIDE_SWATCH = 0.03

# English display names and default colour roles of the usual pieces; builders
# can give others (or override these) in Sheet(..., pieces={...})
PIECES = {
    "front": ("FRONT", "Front", "primary"),
    "back": ("BACK", "Back", "primary"),
    "side_l": ("LEFT SIDE", "Left Side", "accent"),
    "side_r": ("RIGHT SIDE", "Right Side", "accent"),
    "sleeve_l": ("LEFT SLEEVE", "Left Sleeve", "accent"),
    "sleeve_r": ("RIGHT SLEEVE", "Right Sleeve", "accent"),
    "collar": ("COLLAR", "Collar", "trim"),
    "armhole_l": ("LEFT ARMHOLE", "Left Armhole", "trim"),
    "armhole_r": ("RIGHT ARMHOLE", "Right Armhole", "trim"),
    "leg_l": ("LEFT LEG", "Left Leg", "primary"),
    "leg_r": ("RIGHT LEG", "Right Leg", "primary"),
    "waistband": ("WAISTBAND", "Waistband", "trim"),
}


class Sheet:
    """`rows`: piece ids per atlas row, top to bottom (each row centred).
    `pieces`: extra or overriding {id: (label, display name, role)}."""

    def __init__(self, rows, pieces=None):
        self.rows = rows
        self.ids = [p for row in rows for p in row]
        self.meta = dict(PIECES)
        self.meta.update(pieces or {})
        missing = [p for p in self.ids if p not in self.meta]
        assert not missing, f"no label/name/role for {missing}"
        self.outlines = {}  # piece -> [polygon in piece-local metres]
        self.cuffs = {}  # sleeve piece -> (cuff edge ly, half width)
        self.legs = []  # pattern.json "legs" entries
        self.layout = None  # set by assign_atlas

    def index(self, piece):
        return self.ids.index(piece)

    def add_outline(self, piece, polygon):
        self.outlines.setdefault(piece, []).append(polygon)

    def add_cuff(self, piece, edge, half_width):
        """A printed cuff band at ly = edge on a sleeve, as wide as the
        widest of its grids."""
        self.cuffs[piece] = (edge, max(self.cuffs.get(piece, (0, 0))[1], half_width))

    def assign_atlas(self, objects):
        """Lays the pieces out and turns every loop's piece-local
        coordinates into atlas UVs."""
        bounds = {}
        for piece in self.ids:
            polys = self.outlines[piece]
            xs = [p[0] for poly in polys for p in poly]
            ys = [p[1] for poly in polys for p in poly]
            bounds[piece] = (min(xs), min(ys), max(xs), max(ys))

        def row_width(row):
            return sum(bounds[p][2] - bounds[p][0] for p in row) + ATLAS_GAP * (len(row) - 1)

        inner = max(row_width(row) for row in self.rows)
        slots, y = {}, ATLAS_MARGIN
        for row in self.rows:
            y += ATLAS_LABEL
            x, height = ATLAS_MARGIN + (inner - row_width(row)) / 2, 0.0
            for piece in row:
                (x0, y0, x1, y1) = bounds[piece]
                slots[piece] = (x, y)
                x += (x1 - x0) + ATLAS_GAP
                height = max(height, y1 - y0)
            y += height + ATLAS_GAP
        width, height = inner + 2 * ATLAS_MARGIN, y - ATLAS_GAP + ATLAS_MARGIN
        # the inside swatch needs free room in the bottom right corner
        if (inner - row_width(self.rows[-1])) / 2 < INSIDE_SWATCH + ATLAS_GAP:
            height += INSIDE_SWATCH + ATLAS_MARGIN
        self.layout = {"bounds": bounds, "slots": slots, "width": width, "height": height,
                       "inside": (width - ATLAS_MARGIN - INSIDE_SWATCH, height - ATLAS_MARGIN - INSIDE_SWATCH,
                                  INSIDE_SWATCH, INSIDE_SWATCH)}

        for obj in objects:
            mesh = obj.data
            pieces = mesh.attributes["piece"].data
            uvs = mesh.uv_layers["UVMap"].data
            for poly in mesh.polygons:
                piece = self.ids[pieces[poly.index].value]
                for li in poly.loop_indices:
                    x, y = self.to_atlas(piece, *uvs[li].uv)
                    uvs[li].uv = (x / width, 1.0 - y / height)  # glTF flips v back

    def to_atlas(self, piece, lx, ly):
        x0, y0, _, _ = self.layout["bounds"][piece]
        sx, sy = self.layout["slots"][piece]
        return sx + lx - x0, sy + ly - y0

    def inside_uv(self):
        x, y, w, h = self.layout["inside"]
        return (x + w / 2) / self.layout["width"], 1.0 - (y + h / 2) / self.layout["height"]

    def write(self, path):
        """pattern.json, in atlas pixels."""
        ppm = ATLAS_PX_PER_M
        px = lambda piece, p: [round(c * ppm, 2) for c in self.to_atlas(piece, *p)]
        pieces = []
        for piece in self.ids:
            x0, y0, x1, y1 = self.layout["bounds"][piece]
            sx, sy = self.layout["slots"][piece]
            label, name, role = self.meta[piece]
            pieces.append({
                "id": piece,
                "label": label,
                "name": name,
                "role": role,
                "rect": [round(v * ppm, 2) for v in (sx, sy, x1 - x0, y1 - y0)],
                "polygons": [[px(piece, p) for p in poly] for poly in self.outlines[piece]],
            })
        cuffs = []
        for piece, (edge, half) in self.cuffs.items():
            a, b = px(piece, (-half - 0.01, edge - CUFF_WIDTH)), px(piece, (half + 0.01, edge + 0.01))
            cuffs.append({"piece": piece, "rect": [a[0], a[1], b[0] - a[0], b[1] - a[1]]})
        data = {
            "atlas": {"width": round(self.layout["width"] * ppm), "height": round(self.layout["height"] * ppm),
                      "pxPerM": ppm},
            "pieces": pieces,
            "cuffs": cuffs,
            "inside": [round(v * ppm, 2) for v in self.layout["inside"]],
        }
        # front/back placement of garment points (x, z): front and back are
        # laid out as seen from their own side, at local (±x, -z)
        if "front" in self.ids and "back" in self.ids:
            placement = {}
            for piece, x_sign in (("front", 1), ("back", -1)):
                ox, oy = self.to_atlas(piece, 0.0, 0.0)
                placement[piece] = {"origin": [round(ox * ppm, 2), round(oy * ppm, 2)], "xSign": x_sign}
            data["placement"] = placement
        data["legs"] = self.legs
        with open(path, "w") as f:
            json.dump(data, f, indent=1, ensure_ascii=False)


def write_garment_json(path, info):
    """garment.json: the registry entry without the builder."""
    keys = ("id", "name", "kind", "body", "unisex", "order")
    with open(path, "w") as f:
        json.dump({k: info[k] for k in keys}, f, indent=1, ensure_ascii=False)
