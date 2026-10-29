"""Muramasa's traced outline (illustration pixels), drawn over the art for checking."""
import json
import sys

from PIL import Image, ImageDraw

# Clockwise from the pommel's top-left corner: hilt and guard on the right, down the spine
# to the tip, back up the cutting edge, then the guard and hilt on the left.
right = [
    (1017, 1143), (1124, 1077),  # pommel
    (1315, 1480), (1322, 1540),  # hilt meets guard
    (1350, 1507), (1400, 1494), (1450, 1494), (1467, 1517), (1447, 1557), (1393, 1610), (1400, 1660), (1420, 1710),  # guard, right arm
    (1437, 1750), (1449, 1800), (1483, 1850), (1508, 1900), (1533, 1950), (1550, 2000), (1577, 2050),
    (1603, 2100), (1630, 2150), (1660, 2200), (1687, 2250), (1717, 2300), (1745, 2350), (1773, 2400),
    (1803, 2450), (1832, 2500), (1863, 2550), (1895, 2600), (1929, 2650), (1960, 2700), (1994, 2750),
    (2026, 2800), (2063, 2850), (2099, 2900), (2138, 2950), (2177, 3000), (2215, 3050), (2254, 3100),
    (2295, 3150), (2335, 3200), (2380, 3250), (2425, 3300), (2470, 3350), (2520, 3400), (2570, 3450),  # tip
]
left = [
    # The cutting edge: the thin glowing edge line along the whole blade (its outer side), and
    # near the tip the bottom of the orange glow under it. The red reflection
    # between it and the purple face is part of the blade.
    (2562, 3453), (2456, 3429), (2338, 3382), (2268, 3340), (2197, 3294), (2148, 3250), (2107, 3200),
    (2066, 3150), (2028, 3100), (1990, 3050), (1952, 3000), (1916, 2950), (1882, 2900), (1849, 2850),
    (1819, 2800), (1788, 2750), (1757, 2700), (1726, 2650), (1693, 2600), (1665, 2550), (1636, 2500),
    (1603, 2450), (1572, 2400), (1538, 2350), (1518, 2300), (1497, 2250), (1474, 2200), (1446, 2150),
    (1413, 2100), (1398, 2050), (1374, 2000), (1348, 1950), (1322, 1900), (1305, 1850), (1272, 1805),
    (1282, 1750),
    (1260, 1717), (1140, 1697), (1103, 1670), (1130, 1632),  # guard, left arm
    (1200, 1612), (1173, 1510),  # hilt meets guard
]
outline = right + left

# The sageo tied to the pommel: the cord loop (its inside is cord and folded cloth) and the two
# tasseled ends. The V between the ends is background.
tassel = [
    [(875, 821), (871, 914), (879, 1007), (900, 1079), (943, 1129), (1000, 1150), (1029, 1136),
     (1029, 1129), (1000, 1107), (1007, 1043), (1000, 950), (986, 879), (950, 821), (921, 771), (886, 771)],
    [(886, 771), (843, 743), (793, 686), (754, 600), (761, 521), (793, 557), (821, 614), (864, 700), (900, 764)],
    [(907, 771), (914, 686), (907, 614), (911, 554), (936, 543), (971, 514), (1021, 500), (1032, 521),
     (993, 586), (950, 664), (936, 736), (921, 771)],
]

if __name__ == "__main__":
    src, out = sys.argv[1], sys.argv[2]
    im = Image.open(src).convert("RGB")
    d = ImageDraw.Draw(im)
    d.line(outline + outline[:1], fill=(0, 255, 255), width=6)
    im.crop((900, 1000, 2700, 3550)).resize((900, 1275)).save(out)
    im.crop((1450, 2200, 2650, 3500)).resize((720, 780)).save(out.replace(".png", "-low.png"))
    json.dump(outline, open(out + ".json", "w"))
