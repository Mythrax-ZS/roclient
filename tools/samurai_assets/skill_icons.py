"""Samurai skill icons: 24x24 pixel art in the manner of the Guillotine Cross icons.

The frame and every motif are drawn here pixel by pixel; nothing is copied from game
data. Divinity (Kusanagi) skills use pale blades with gold light; Cursed (Muramasa)
skills use crimson blades with wine-dark auras.

CLI: py skill_icons.py --output <overlay root> [--preview preview.png]
Writes data/texture/유저인터페이스/item/<skill>.bmp (24-bit, magenta = transparent),
the names the client looks up before falling back to a stock icon.
"""
from __future__ import annotations

import argparse
from pathlib import Path

from PIL import Image

PALETTE = {
    "#": (255, 0, 255),  # transparent key
    ".": (0x2A, 0x27, 0x3B), ",": (0x2F, 0x1F, 0x4C), "x": (0x16, 0x16, 0x30),
    "1": (0x4C, 0x3A, 0x6A), "2": (0x68, 0x55, 0x87), "3": (0x81, 0x70, 0xA2), "4": (0x9B, 0x8B, 0xBA),
    "5": (0xB5, 0xA6, 0xD2), "6": (0xCB, 0xC0, 0xE9), "7": (0xED, 0xED, 0xFF),
    "w": (0xFF, 0xF3, 0xEB), "c": (0xEE, 0xDF, 0xD7), "C": (0xDC, 0xCB, 0xC3), "k": (0xCA, 0xB7, 0xAF),
    "s": (0xB8, 0xA3, 0x9B), "g": (0xA9, 0x8F, 0x8B), "h": (0x99, 0x7B, 0x7B), "G": (0x89, 0x67, 0x6B),
    "b": (0x76, 0x3A, 0x32), "B": (0x4F, 0x26, 0x1C),
    "m": (0x5C, 0x25, 0x3B), "n": (0x80, 0x41, 0x51), "N": (0x9E, 0x5C, 0x66),
    "r": (0x8F, 0x22, 0x18), "R": (0xBD, 0x64, 0x55), "o": (0xED, 0x64, 0x47),
    "A": (0xEA, 0xA1, 0x71), "a": (0xF7, 0xA5, 0x72), "f": (0xFF, 0xC0, 0x8C),
    "e": (0xFE, 0xDE, 0xAC), "y": (0xFF, 0xE5, 0xA3), "Y": (0xFF, 0xFF, 0xCB),
}

# Interior: 18 columns x 16 rows, placed at (3, 4). The first character of each icon's
# grid sets its field colour.
ICONS = {
    # Divinity: a katana falls from a burst of heavenly light (the critical strike).
    "sam_heavens_severance": [
        ",,,,,,,,,,,,,,o,,,",
        ",,3,,,,,,,,,,,a,,,",
        ",,,,,,,,,,,,,aya,,",
        ",,,,,,,,,,,oaywyao",
        ",,,,,,,,,,,,,aya,,",
        ",,,,,,,,,,,,w,o,,,",
        ",,,,,,,,,,,wck,,,,",
        ",,,,,,,,,,wcCk,,,,",
        ",,,,,,,,,wcCk,,,1,",
        ",,,,,,,,wcCk,,,,,,",
        ",,,,,,,wcCk,,,,,,,",
        ",,,,,,wcCk,,,,,,,,",
        ",,,,,wcCk,,,,,,,,,",
        ",,,ayea,,,,,,,,,,,",
        ",,,Rb,,,,,,,,,,,,,",
        ",,bR,,,,,,,,,,,,,,",
    ],
    # Divinity: the upright katana turns blows aside (a spark and a parry arc).
    "sam_kusanagi_guard": [
        ".........w........",
        "........wck.4.....",
        "....o...wck..5....",
        "...aya..wck..56..3",
        "..oywyo.wck...6...",
        "...aya..wck...6...",
        "....o...wck..56..3",
        "........wck..5....",
        "........wck.4.....",
        "........wck.......",
        "......AayyyaA.....",
        "........bRb.......",
        "........RbR.......",
        "........bRb.......",
        "........RbR.......",
        ".........B........",
    ],
    # Divinity: a crescent moon; a blow glances off the blade and is sent back.
    "sam_moonlit_reflection": [
        ",,,,,,,,,,,,,,,,,,",
        ",,,,5677,,,,,,,,w,",
        ",,,567,,,,,,,,,wc,",
        ",,567,,,,,,,,,wck,",
        ",,67,,,,,,,,,wck,,",
        ",567,,,,,,,,wck,,,",
        ",567,,,,,,,wck,,,,",
        ",567,,,,,,wck,,,6,",
        ",,67,,,,,wckya4567",
        ",,567,,ayea,a,,,6,",
        ",,,567Rb,,,,,4,,,,",
        ",,,,5Rb7,,,,,,3,,,",
        ",,,,Bb,,,,,,,,,2,,",
        ",,,,,,,,,,,,,,,,1,",
        ",,,,,,,,,,,,,,,,,,",
        ",,,,,,,,,,,,,,,,,,",
    ],
    # Divinity: two blades moving as one, trailing speed lines.
    "sam_blade_harmony": [
        "............w..a..",
        "...........wc.aya.",
        "..........wck..a..",
        "..11234..wck......",
        ".........wck....w.",
        ".......wck.....wc.",
        "1234..wck.....wck.",
        "....ayea.....wck..",
        "....Rb......wck...",
        "...bR......wck....",
        "..Bb......wck.....",
        ".1234....ayea.....",
        "........Rb........",
        ".......bR.........",
        ".123..Bb..........",
        "..................",
    ],
    # Cursed: a crimson blade sheds three drops of its wielder's blood.
    "sam_blood_covenant": [
        "...............o..",
        "..............oR..",
        ".............oRr..",
        "............oRr...",
        "...........oRr....",
        "..........oRr.....",
        ".........oRr....r.",
        "........oRr....rRr",
        ".......oRr..r..roR",
        ".....GssG..rRr..r.",
        ".....xr....roR....",
        "....rx..r...r.....",
        "...xr..rRr........",
        ".......roR........",
        "........r.........",
        "..................",
    ],
    # Cursed: the blade burns with a dark flame under watching red eyes.
    "sam_muramasas_curse": [
        ",,,,,,,,,,,,,,,,,,",
        ",,ro,,,,,,,,,,or,,",
        ",,,ryo,,,,,,oyr,,,",
        ",,,,,,,,,o,,,,,,,,",
        ",,,,,n,,oRr,,n,,,,",
        ",,,,,mn,oRr,nm,,,,",
        ",,,n,mr,oRr,rm,n,,",
        ",,,mnrr,oRr,rrnm,,",
        ",,,,mrr,oRr,rrm,,,",
        ",,,nmrr,oRr,rrmn,,",
        ",,,,mmr,oRr,rmm,,,",
        ",,,,,mm,oRr,mm,,,,",
        ",,,,,,xGsksGx,,,,,",
        ",,,,,,,,xrx,,,,,,,",
        ",,,,,,,,rxr,,,,,,,",
        ",,,,,,,,,x,,,,,,,,",
    ],
    # Cursed: a whirl of ash-grey cuts, flecked with blood.
    "sam_ashen_wind_slash": [
        "..................",
        "..r....GGghh...R..",
        "....ww.....ssk..r.",
        "...cwC......gkC...",
        "...ckC......hhc...",
        "..CCs...R....scc..",
        "..kh..........kw..",
        "..kg..........Cw..",
        "..s.........r.....",
        "..h...............",
        "..gg..........GG..",
        "...g..........g...",
        "...G.........hg...",
        ".r....wkkshssh....",
        "..R...wwcCCk......",
        "..................",
    ],
    # Cursed: the chains break and the crimson blade comes down in a burst.
    "sam_wrath_unbound": [
        ",,,,a,,,xrx,,,a,,,",
        ",,,o,,,,rxr,,,,o,,",
        ",,or,,,,xrx,,,,ro,",
        ",,,,o,xGsksGx,o,,,",
        ",,,r,,,,oRr,,,,r,,",
        ",,or,,,,oRr,,,,ro,",
        ",,,,,,,,oRr,,,,,,,",
        ",,,,,,,,oRr,,,,,,,",
        ",,,,,,,,oRr,,,,,,,",
        ",,,,,,,,oRr,,,,,,,",
        ",,,,,,,,oRr,,,,,,,",
        ",,,,,o,,,o,,,o,,,,",
        ",,,,,,ra,y,ar,,,,,",
        ",,,ro,aeyYyea,or,,",
        ",,oafyeYYwYYeyfao,",
        ",oaeyYwwwwwwwYyeao",
    ],
}


def frame():
    """An original border in the Guillotine Cross manner: wine side rails, rose cord bands
    and fringe teeth at top and bottom, with magenta corners and side notches."""
    rows = [list("." * 24) for _ in range(24)]
    for y in range(24):
        for x in range(24):
            edge = x in (0, 23)
            if (x, y) in ((0, 0), (23, 0), (0, 23), (23, 23)) or (edge and 8 <= y <= 15):
                rows[y][x] = "#"
    for y, x_of in ((1, 0), (22, 0)):
        for x in range(1, 23):
            rows[y][x] = "G" if x % 5 == 0 else ("r" if x % 5 == 3 else "h")
    for y in (2, 21):
        for x in range(1, 23):
            rows[y][x] = "s" if x % 2 else "g"
    for y in (3, 20):
        for x in range(1, 23):
            rows[y][x] = "k" if x % 3 == 1 else ("G" if x % 3 == 2 else ".")
    for y in range(4, 20):
        rows[y][1], rows[y][22] = "m", "m"
        rows[y][2], rows[y][21] = ("N" if y in (4, 19) else "n"), ("N" if y in (4, 19) else "n")
    for x, y in ((1, 3), (22, 3), (1, 20), (22, 20)):
        rows[y][x] = "c"
    return rows


def render(name):
    grid = ICONS[name]
    if len(grid) != 16 or any(len(row) != 18 for row in grid):
        raise ValueError(f"{name}: the interior must be 16 rows of 18 pixels")
    rows = frame()
    for y, row in enumerate(grid):
        for x, ch in enumerate(row):
            if ch not in PALETTE:
                raise ValueError(f"{name}: unknown colour {ch!r}")
            rows[4 + y][3 + x] = ch
    image = Image.new("RGB", (24, 24))
    image.putdata([PALETTE[ch] for row in rows for ch in row])
    return image


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="overlay root (contains data/)")
    parser.add_argument("--preview", type=Path, help="write an 8x contact sheet here")
    args = parser.parse_args()
    folder = args.output / "data/texture/유저인터페이스/item"
    folder.mkdir(parents=True, exist_ok=True)
    images = {name: render(name) for name in ICONS}
    for name, image in images.items():
        image.save(folder / f"{name}.bmp", format="BMP")
    if args.preview:
        scale, gap = 8, 8
        sheet = Image.new("RGB", (len(images) * (24 * scale + gap) + gap, 24 * scale + 2 * gap), (128, 128, 128))
        for i, image in enumerate(images.values()):
            sheet.paste(image.resize((24 * scale, 24 * scale), Image.Resampling.NEAREST), (gap + i * (24 * scale + gap), gap))
        sheet.save(args.preview)
    print("\n".join(sorted(f"{name}.bmp" for name in images)))


if __name__ == "__main__":
    main()
