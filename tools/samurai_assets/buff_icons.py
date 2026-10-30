"""Samurai buff (status) icons in the manner of the original client's status icons.

Each icon is a set of one-pixel bright lines: the frame ring and the glyph's outlines.
Every other pixel is coloured by its distance from the nearest line (glow, mid-tone,
translucent fill), the way the stock 32x32 status TGAs are built. Kusanagi (Divinity)
buffs use the classic blue theme; Muramasa (Cursed) buffs a crimson one.

CLI: py buff_icons.py --output <overlay root> [--preview preview.png]
Writes data/texture/effect/efst_sam_<skill>.tga (32x32 RGBA).
"""
from __future__ import annotations

import argparse
import math
from pathlib import Path

from PIL import Image, ImageDraw

# (distance, RGBA) anchors from the line outward; beyond the last is the fill.
THEMES = {
    "blue": [(0, (197, 249, 255, 255)), (1, (79, 170, 255, 201)), (1.5, (77, 168, 252, 199)),
             (2, (31, 81, 150, 145)), (3, (0, 24, 83, 105))],
    "crimson": [(0, (255, 226, 214, 255)), (1, (235, 46, 62, 205)), (1.5, (232, 44, 60, 202)),
                (2, (150, 24, 40, 150)), (3, (64, 6, 16, 110))],
}


def colour(theme, d):
    ramp = THEMES[theme]
    if d >= ramp[-1][0]:
        return ramp[-1][1]
    for (d0, c0), (d1, c1) in zip(ramp, ramp[1:]):
        if d0 <= d <= d1:
            t = (d - d0) / (d1 - d0)
            return tuple(round(a + (b - a) * t) for a, b in zip(c0, c1))
    return ramp[0][1]


def blade(draw, tip, base, half, point=3):
    """Outline of a straight blade from base to tip, `half` pixels each side, pointed."""
    (tx, ty), (bx, by) = tip, base
    length = math.hypot(tx - bx, ty - by)
    ux, uy = (tx - bx) / length, (ty - by) / length
    px, py = -uy * half, ux * half
    shoulder = (tx - ux * point, ty - uy * point)
    poly = [(bx + px, by + py), (shoulder[0] + px, shoulder[1] + py), (tx, ty),
            (shoulder[0] - px, shoulder[1] - py), (bx - px, by - py)]
    draw.polygon([(round(x), round(y)) for x, y in poly], outline=1)


def guard_and_hilt(draw, base, toward, guard=4, hilt=5):
    """A guard across the blade at `base` and a hilt running away from `toward` (the tip)."""
    (bx, by), (tx, ty) = base, toward
    length = math.hypot(tx - bx, ty - by)
    ux, uy = (tx - bx) / length, (ty - by) / length
    px, py = -uy, ux
    draw.line([(round(bx + px * guard), round(by + py * guard)), (round(bx - px * guard), round(by - py * guard))], fill=1)
    end = (bx - ux * hilt, by - uy * hilt)
    for s in (-1, 1):
        draw.line([(round(bx + px * s - ux), round(by + py * s - uy)), (round(end[0] + px * s), round(end[1] + py * s))], fill=1)
    draw.line([(round(end[0] + px), round(end[1] + py)), (round(end[0] - px), round(end[1] - py))], fill=1)


def drop(draw, x, y, h=11, r=3):
    """A teardrop outline with its point at (x, y)."""
    cy = y + h - r
    draw.ellipse([x - r, cy - r, x + r, cy + r], outline=1)
    draw.line([(x, y), (x - r, cy - 1)], fill=1)
    draw.line([(x, y), (x + r, cy - 1)], fill=1)


def kusanagi_guard(d):
    blade(d, (26, 5), (11, 20), 2)
    guard_and_hilt(d, (11, 20), (26, 5))
    d.arc([3, 3, 29, 29], 190, 260, fill=1)  # the parry: a blow turned aside
    d.arc([7, 7, 25, 25], 200, 250, fill=1)


def moonlit_reflection(d):
    d.arc([3, 5, 21, 27], 95, 265, fill=1)  # crescent: outer edge
    d.arc([8, 8, 20, 24], 110, 250, fill=1)  # inner edge
    d.line([(11, 6), (13, 9)], fill=1)
    d.line([(11, 26), (13, 23)], fill=1)
    d.line([(28, 8), (19, 16), (28, 24)], fill=1)  # a blow glancing back
    d.line([(28, 24), (24, 24)], fill=1)
    d.line([(28, 24), (28, 20)], fill=1)


def blade_harmony(d):
    blade(d, (22, 4), (11, 15), 1.5)
    guard_and_hilt(d, (11, 15), (22, 4), guard=3, hilt=4)
    blade(d, (28, 11), (17, 22), 1.5)
    guard_and_hilt(d, (17, 22), (28, 11), guard=3, hilt=4)
    for y, x0 in ((21, 3), (25, 5), (29 - 1, 9)):
        d.line([(x0, y), (x0 + 5, y)], fill=1)


def blood_covenant(d):
    d.polygon([(6, 5), (23, 5), (27, 7), (23, 9), (6, 9)], outline=1)  # blade, edge down
    d.line([(6, 3), (6, 11)], fill=1)  # guard
    d.line([(3, 6), (5, 6)], fill=1)
    d.line([(3, 8), (5, 8)], fill=1)
    for x in (9, 16, 23):
        drop(d, x, 13)


def muramasas_curse(d):
    flame = [(16, 2), (19, 7), (22, 4), (23, 10), (26, 8), (26, 16), (24, 22), (20, 26),
             (12, 26), (8, 22), (6, 16), (6, 8), (9, 10), (10, 4), (13, 7)]
    d.polygon(flame, outline=1)
    blade(d, (16, 6), (16, 19), 1.5, point=2)
    d.line([(12, 20), (20, 20)], fill=1)
    d.line([(15, 21), (15, 24)], fill=1)
    d.line([(17, 21), (17, 24)], fill=1)


ICONS = {
    "efst_sam_kusanagi_guard": ("blue", kusanagi_guard),
    "efst_sam_moonlit_reflection": ("blue", moonlit_reflection),
    "efst_sam_blade_harmony": ("blue", blade_harmony),
    "efst_sam_blood_covenant": ("crimson", blood_covenant),
    "efst_sam_muramasas_curse": ("crimson", muramasas_curse),
}


def render(name):
    theme, glyph = ICONS[name]
    mask = Image.new("1", (32, 32))
    draw = ImageDraw.Draw(mask)
    draw.rectangle([0, 0, 31, 31], outline=1)
    for corner in ((0, 0), (31, 0), (0, 31), (31, 31)):
        mask.putpixel(corner, 0)
    glyph(draw)
    lines = [(x, y) for y in range(32) for x in range(32) if mask.getpixel((x, y))]
    image = Image.new("RGBA", (32, 32))
    for y in range(32):
        for x in range(32):
            image.putpixel((x, y), colour(theme, min(math.hypot(x - a, y - b) for a, b in lines)))
    return image


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="overlay root (contains data/)")
    parser.add_argument("--preview", type=Path, help="write an 8x contact sheet on grey here")
    args = parser.parse_args()
    folder = args.output / "data/texture/effect"
    folder.mkdir(parents=True, exist_ok=True)
    images = {name: render(name) for name in ICONS}
    for name, image in images.items():
        image.save(folder / f"{name}.tga", format="TGA")
    if args.preview:
        scale, gap = 8, 8
        sheet = Image.new("RGBA", (len(images) * (32 * scale + gap) + gap, 32 * scale + 2 * gap), (128, 128, 128, 255))
        for i, image in enumerate(images.values()):
            tile = Image.new("RGBA", (32, 32), (128, 128, 128, 255))
            tile.alpha_composite(image)
            sheet.paste(tile.resize((32 * scale, 32 * scale), Image.Resampling.NEAREST), (gap + i * (32 * scale + gap), gap))
        sheet.convert("RGB").save(args.preview)
    print("\n".join(f"{name}.tga" for name in images))


if __name__ == "__main__":
    main()
