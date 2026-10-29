"""Cut Kusanagi and Muramasa out of the illustrations.

Writes, per sword: <name>-cut.png (RGBA, as painted, cropped) and <name>-flat.png (RGBA, rotated
so the hilt is left, the tip right and the cutting edge - the convex side of the curve - below),
plus <name>-flat.json with the pommel, guard and tip positions in the flat image.
"""
import json
import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

sys.path.insert(0, str(Path(__file__).parent))
from mura_outline import outline as mura_outline, tassel as mura_tassel  # noqa: E402

out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)


def band_mask(size, axis, half):
    """Pixels within `half[i]` (interpolated) of the axis polyline."""
    m = Image.new("L", size, 0)
    d = ImageDraw.Draw(m)
    for (a, b), (ha, hb) in zip(zip(axis, axis[1:]), zip(half, half[1:])):
        steps = max(2, int(math.dist(a, b) / 10))
        for i in range(steps + 1):
            t = i / steps
            x, y = a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t
            r = ha + (hb - ha) * t
            d.ellipse((x - r, y - r, x + r, y + r), fill=255)
    return np.asarray(m) > 0


def fill_holes(mask):
    """Close interior gaps: anything not reachable from the border through background."""
    # copy(): an image wrapping a NumPy array is not filled in place.
    inv = Image.fromarray(((~mask) * 255).astype(np.uint8)).copy()
    ImageDraw.floodfill(inv, (0, 0), 128)
    return np.asarray(inv) != 128


swords = {
    "kusanagi": dict(
        src="artzip/Kusanagi.png",
        axis=[(2760, 560), (2600, 950), (2420, 1330), (2395, 1450), (2370, 1600), (2220, 1850), (2060, 2150),
              (1810, 2450), (1600, 2750), (1360, 3050), (1180, 3250), (1085, 3320)],
        half=[90, 90, 90, 190, 120, 115, 110, 100, 90, 80, 60, 30],
        pommel=(2760, 560), guard=(2395, 1450), tip=(1085, 3320),
        # The handle traced by hand: the pommel cap and the shaded side of the wrap are too
        # dark for the brightness cut.
        hilt=[(2694, 563), (2722, 547), (2806, 569), (2822, 602), (2789, 702), (2756, 791), (2711, 902),
              (2667, 1002), (2628, 1102), (2589, 1202), (2556, 1291), (2544, 1360), (2361, 1360), (2383, 1291),
              (2428, 1191), (2478, 1069), (2528, 958), (2583, 847), (2633, 736), (2683, 586)],
    ),
    "muramasa": dict(
        src="artzip/Muramasa Transformed.png",
        pommel=(1070, 1110), guard=(1300, 1600), tip=(2570, 3450),
    ),
}

for name, s in swords.items():
    im = Image.open(s["src"]).convert("RGB")
    rgb = np.asarray(im)
    if name == "kusanagi":
        v = rgb.max(axis=2) / 255
        band = band_mask(im.size, s["axis"], s["half"])
        mask = band & (v > 0.72)
        hilt = Image.new("L", im.size, 0)
        ImageDraw.Draw(hilt).polygon(s["hilt"], fill=255)
        mask = fill_holes(mask | (np.asarray(hilt) > 0))
    else:
        m = Image.new("L", im.size, 0)
        draw = ImageDraw.Draw(m)
        draw.polygon(mura_outline, fill=255)
        for poly in mura_tassel:
            draw.polygon(poly, fill=255)
        mask = np.asarray(m) > 0
    # Soft 1-2 px edge so downsampling keeps a clean silhouette.
    alpha = Image.fromarray((mask * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(1.5))
    cut = im.convert("RGBA")
    cut.putalpha(alpha)
    box = alpha.point(lambda a: 255 if a > 8 else 0).getbbox()
    pad = 24
    box = (max(0, box[0] - pad), max(0, box[1] - pad), min(im.width, box[2] + pad), min(im.height, box[3] + pad))
    cut = cut.crop(box)
    cut.save(out / f"{name}-cut.png")

    # Flat: rotate so pommel -> tip points right (PIL rotates counterclockwise on screen).
    (px, py), (tx, ty) = s["pommel"], s["tip"]
    angle = math.degrees(math.atan2(ty - py, tx - px))
    flat = cut.rotate(angle, Image.Resampling.BICUBIC, expand=True)
    # Track the key points through the same rotation (about the image centre, with expansion).
    w, h = cut.size
    fw, fh = flat.size

    def move(p):
        x, y = p[0] - box[0] - w / 2, p[1] - box[1] - h / 2
        # PIL's rotate(angle) turns content counterclockwise by `angle` on screen.
        c, sn = math.cos(math.radians(angle)), math.sin(math.radians(angle))
        return (x * c + y * sn + fw / 2, -x * sn + y * c + fh / 2)

    pts = {k: move(s[k]) for k in ("pommel", "guard", "tip")}
    # Which way does the blade bow? Compare the blade's centre line with the guard->tip chord.
    a = np.asarray(flat.getchannel("A")) > 128
    gx, tx2 = int(pts["guard"][0]), int(pts["tip"][0])
    offsets = []
    for x in range(gx + (tx2 - gx) // 4, gx + 3 * (tx2 - gx) // 4, 5):
        ys = np.nonzero(a[:, x])[0]
        if len(ys):
            chord = pts["guard"][1] + (pts["tip"][1] - pts["guard"][1]) * (x - pts["guard"][0]) / (pts["tip"][0] - pts["guard"][0])
            offsets.append((ys.min() + ys.max()) / 2 - chord)
    bow = float(np.mean(offsets))
    if bow < 0:
        # Bowing upward means the convex (cutting) edge is on top: flip so it faces down.
        flat = flat.transpose(Image.Transpose.FLIP_TOP_BOTTOM)
        pts = {k: (x, fh - y) for k, (x, y) in pts.items()}
    fbox = flat.getchannel("A").point(lambda v: 255 if v > 8 else 0).getbbox()
    flat = flat.crop(fbox)
    pts = {k: (round(x - fbox[0], 1), round(y - fbox[1], 1)) for k, (x, y) in pts.items()}
    flat.save(out / f"{name}-flat.png")
    json.dump(dict(points=pts, bow_before_flip=round(bow, 1), size=flat.size), open(out / f"{name}-flat.json", "w"), indent=1)
    print(name, "cut", cut.size, "flat", flat.size, "bow", round(bow, 1), pts)
