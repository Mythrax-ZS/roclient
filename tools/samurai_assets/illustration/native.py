"""Game-size sprite sources from the flat cut-outs.

Each flat sword is shrunk so pommel-to-tip is LENGTH - 2 game pixels (a tassel past the pommel
widens the canvas), stretched in height by THICKEN so the slender painted blade still reads at
game size, box-filtered with premultiplied alpha, given binary alpha and a warm dark rim, then
written as <name>-v4.png upscaled by BLOCK (nearest) for compile_weapons.py to sample back.
"""
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

cut, out = Path(sys.argv[1]), Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
LENGTH, BLOCK = 60, 32
THICKEN = {"kusanagi": 1.45, "muramasa": 1.35}
ALPHA_MIN = 100

for name in ("kusanagi", "muramasa"):
    flat = Image.open(cut / f"{name}-flat.png").convert("RGBA")
    pts = json.load(open(cut / f"{name}-flat.json"))["points"]
    W, H = flat.size
    scale = (LENGTH - 2) / (pts["tip"][0] - pts["pommel"][0])
    k = THICKEN[name]
    size = (max(1, round(W * scale)), max(1, round(H * scale * k)))
    small = flat.convert("RGBa").resize(size, Image.Resampling.BOX).convert("RGBA")
    s = np.asarray(small).astype(np.float32)
    opaque = s[..., 3] >= ALPHA_MIN
    rows = np.nonzero(opaque.any(axis=1))[0]
    cols = np.nonzero(opaque.any(axis=0))[0]
    r0, c0 = rows.min(), cols.min()
    s = s[r0 : rows.max() + 1, c0 : cols.max() + 1]
    opaque = opaque[r0 : rows.max() + 1, c0 : cols.max() + 1]
    rgb = s[..., :3].copy()
    # Rim: opaque pixels with a transparent 4-neighbour, darkened toward warm brown/red so gold
    # and red keep their hue instead of going olive.
    o = np.pad(opaque, 1)
    rim = opaque & ~(o[:-2, 1:-1] & o[2:, 1:-1] & o[1:-1, :-2] & o[1:-1, 2:])
    rgb[rim] = rgb[rim] * np.array([0.62, 0.42, 0.36])
    canvas = np.zeros((s.shape[0] + 2, s.shape[1] + 2, 4), np.uint8)
    canvas[1:-1, 1:-1, :3] = rgb.clip(0, 255).astype(np.uint8)
    canvas[1:-1, 1:-1, 3] = np.where(opaque, 255, 0)
    native = Image.fromarray(canvas, "RGBA")
    # Grip: 60% of the way from the pommel to the guard, on the hilt's middle row.
    to_native = lambda x: 1 + x * scale - c0  # noqa: E731
    gx = round(to_native(pts["pommel"][0] + 0.6 * (pts["guard"][0] - pts["pommel"][0])))
    col = np.nonzero(canvas[:, gx, 3])[0]
    gy = int(round((col.min() + col.max()) / 2))
    native.resize((native.width * BLOCK, native.height * BLOCK), Image.Resampling.NEAREST).save(out / f"{name}-v4.png")
    native.resize((native.width * 10, native.height * 10), Image.Resampling.NEAREST).save(out / f"{name}-native-x10.png")
    info = dict(native_size=list(native.size), source_grip=[gx, gy], block=BLOCK, thicken=k,
                pommel_to_tip_px=LENGTH - 2, flat_size=[W, H], scale=round(scale, 5))
    json.dump(info, open(out / f"{name}-v4.json", "w"), indent=1)
    print(name, info)
