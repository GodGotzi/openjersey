"""The garment list. Builder modules in blender/garments/ register
themselves with `register` (see GARMENTS.md, "Building")."""

GARMENTS = {}  # id -> info dict (with "build")


def register(build, *, id, name, kind, body, order, unisex=False):
    """Registers `build(body, info) -> (sheet, objects)` as garment `id`."""
    assert kind in ("top", "bottom") and body in ("man", "women"), id
    assert id not in GARMENTS, f"garment {id} registered twice"
    GARMENTS[id] = {"id": id, "name": name, "kind": kind,
                    "body": body, "unisex": unisex, "order": order, "build": build}


def garment(**info):
    """Decorator form of `register`."""
    def wrap(build):
        register(build, **info)
        return build
    return wrap
