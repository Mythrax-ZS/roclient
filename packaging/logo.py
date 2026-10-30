"""Tenseiga's logo: a katana across a crimson crescent (mikazuki), on a dark tile. Drawn at
4x and downsampled. Run from the repo root with Pillow installed (`python packaging/logo.py`);
it writes every icon the build uses:

    packaging/tenseiga.png             256, the AppImage icon
    crates/ro-client/assets/icon.png   256, the window icon
    crates/ro-client/assets/tenseiga.ico  16-256, the Windows exe icon
    docs/logo.png                      512, for the README
"""
import math
import sys
from PIL import Image, ImageDraw, ImageFilter, ImageChops

S = 4096  # supersampled canvas; final 1024
K = S / 1024


def k(v):
    return v * K


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(len(a)))


def vgrad(size, top, bottom):
    g = Image.new('RGBA', (1, 256))
    for y in range(256):
        g.putpixel((0, y), lerp(top, bottom, y / 255))
    return g.resize(size)


def radial(size, center, r, inner, outer):
    w, h = size
    g = Image.new('L', (256, 256))
    cx, cy = center[0] / w * 256, center[1] / h * 256
    rr = r / w * 256
    for y in range(256):
        for x in range(256):
            d = math.hypot(x - cx, y - cy) / rr
            g.putpixel((x, y), int(min(1, d) * 255))
    g = g.resize(size, Image.BICUBIC)
    a = Image.new('RGBA', size, inner)
    b = Image.new('RGBA', size, outer)
    return Image.composite(b, a, g)


def mask_from(draw_fn):
    m = Image.new('L', (S, S), 0)
    draw_fn(ImageDraw.Draw(m))
    return m


def paint(img, layer, mask):
    img.alpha_composite(Image.composite(layer, Image.new('RGBA', (S, S), (0, 0, 0, 0)), mask))


img = Image.new('RGBA', (S, S), (0, 0, 0, 0))

# The tile: near-black with a crimson warmth, a thin dark-gold rim.
tile = mask_from(lambda d: d.rounded_rectangle([k(24), k(24), k(1000), k(1000)], radius=k(200), fill=255))
paint(img, radial((S, S), (k(560), k(420)), k(760), (58, 14, 20, 255), (10, 5, 8, 255)), tile)
rim = ImageChops.subtract(tile, mask_from(lambda d: d.rounded_rectangle([k(38), k(38), k(986), k(986)], radius=k(186), fill=255)))
paint(img, vgrad((S, S), (190, 150, 80, 255), (90, 60, 30, 255)), rim)

# The crescent (mikazuki): a disc with an offset disc cut away, crimson into ember gold,
# with a soft glow behind it.
C, R = (k(520), k(500)), k(330)
CUT, CR = (k(640), k(380)), k(290)
disc = mask_from(lambda d: d.ellipse([C[0] - R, C[1] - R, C[0] + R, C[1] + R], fill=255))
cut = mask_from(lambda d: d.ellipse([CUT[0] - CR, CUT[1] - CR, CUT[0] + CR, CUT[1] + CR], fill=255))
crescent = ImageChops.subtract(disc, cut)
glow = crescent.filter(ImageFilter.GaussianBlur(k(40)))
glow = ImageChops.multiply(glow, tile)
paint(img, Image.new('RGBA', (S, S), (200, 30, 40, 150)), glow)
paint(img, radial((S, S), (k(330), k(720)), k(520), (255, 178, 80, 255), (178, 14, 34, 255)), crescent)

# The katana, bottom-left to top-right, edge up: the edge is the convex side, the spine
# runs straight into the point, and the edge sweeps up to meet it (the kissaki).
P0 = (k(318), k(706))  # the tsuba
P1 = (k(858), k(166))  # the tip
L = math.hypot(P1[0] - P0[0], P1[1] - P0[1])
dx, dy = (P1[0] - P0[0]) / L, (P1[1] - P0[1]) / L
nx, ny = dy, -dx  # toward the edge (up-left)


def axis(t, bow=k(22)):
    off = bow * math.sin(math.pi * t)  # the curve bows toward the edge
    return (P0[0] + dx * L * t + nx * off, P0[1] + dy * L * t + ny * off)


def at(t, off):
    x, y = axis(t)
    return (x + nx * off, y + ny * off)


N, KIS = 120, 0.87  # the kissaki starts at the yokote, 87% along
spine, edge, shinogi, hamon = [], [], [], []
for i in range(N + 1):
    t = i / N
    w = k(28) - k(6) * t  # half width, tapering
    sp = -w
    if t <= KIS:
        e = w
    else:  # the edge rounds over to meet the spine at an angle; the spine dips to the point
        f = (t - KIS) / (1 - KIS)
        sp = -w + 0.3 * w * f * f
        e = sp + (w - sp) * math.cos(f * math.pi / 2) ** 0.7
    spine.append(at(t, sp))
    edge.append(at(t, e))
    if t <= KIS:
        shinogi.append(at(t, -w * 0.38))
        # the hamon: a gentle wave a third of the way in from the edge
        h = w * (0.40 + 0.09 * math.sin(t * 30)) if t > 0.03 else w * 0.42
        hamon.append(at(t, h))
        depth = w - h  # how far in from the edge the hamon runs at the yokote
    else:  # the boshi: the hamon follows the edge round into the point, narrowing
        f = (t - KIS) / (1 - KIS)
        hamon.append(at(t, e - min(depth * (1 - f), (e - sp) * 0.9)))
blade = mask_from(lambda d: d.polygon(spine + edge[::-1], fill=255))
# Mortal Flame: a crimson glow along the blade
paint(img, Image.new('RGBA', (S, S), (255, 70, 50, 200)), blade.filter(ImageFilter.GaussianBlur(k(22))))
# the ji: polished steel
paint(img, vgrad((S, S), (226, 230, 242, 255), (150, 156, 180, 255)), blade)
# the shinogi-ji: the darker flat between the ridge and the spine; past the yokote the ridge
# (ko-shinogi) runs on to the point
nk = len(shinogi)
ridge = shinogi + [spine[-1]]
paint(img, vgrad((S, S), (150, 156, 178, 255), (96, 102, 126, 255)),
      ImageChops.multiply(blade, mask_from(lambda d: d.polygon(spine + ridge[::-1], fill=255))))
# the hamon: the bright tempered edge, round the kissaki to the point
paint(img, Image.new('RGBA', (S, S), (255, 255, 255, 255)),
      ImageChops.multiply(blade, mask_from(lambda d: d.polygon(hamon + edge[::-1], fill=255))))
# the ridge line
lines = mask_from(lambda d: d.line(ridge, fill=255, width=int(k(3)), joint='curve'))
paint(img, Image.new('RGBA', (S, S), (110, 116, 140, 255)), ImageChops.multiply(blade, lines))

# The hilt: dark wrap with crimson diamonds, then a gold kashira.
H0, H1 = P0, (P0[0] - dx * k(230), P0[1] - dy * k(230))
hw = k(30)
hilt_poly = [(H0[0] + nx * hw, H0[1] + ny * hw), (H1[0] + nx * hw, H1[1] + ny * hw),
             (H1[0] - nx * hw, H1[1] - ny * hw), (H0[0] - nx * hw, H0[1] - ny * hw)]
hilt = mask_from(lambda d: d.polygon(hilt_poly, fill=255))
paint(img, Image.new('RGBA', (S, S), (28, 18, 22, 255)), hilt)


def wrap(d):
    for i in range(5):
        t = (i + 0.5) / 5 * k(230)
        cx, cy = H0[0] - dx * t, H0[1] - dy * t
        a = k(20)
        d.polygon([(cx - dx * a, cy - dy * a), (cx + nx * hw * 0.62, cy + ny * hw * 0.62),
                   (cx + dx * a, cy + dy * a), (cx - nx * hw * 0.62, cy - ny * hw * 0.62)], fill=255)


paint(img, Image.new('RGBA', (S, S), (200, 30, 46, 255)), mask_from(wrap))
kw = k(36)
kas = [(H1[0] + nx * kw, H1[1] + ny * kw), (H1[0] - dx * k(30) + nx * kw * 0.8, H1[1] - dy * k(30) + ny * kw * 0.8),
       (H1[0] - dx * k(30) - nx * kw * 0.8, H1[1] - dy * k(30) - ny * kw * 0.8), (H1[0] - nx * kw, H1[1] - ny * kw)]
paint(img, Image.new('RGBA', (S, S), (214, 170, 84, 255)), mask_from(lambda d: d.polygon(kas, fill=255)))

# The tsuba, seen edge-on like the blade: a narrow iron bar across it with a gold rim,
# gold seppa either side, and the gold habaki collaring the blade.
gold = vgrad((S, S), (250, 214, 130, 255), (170, 118, 50, 255))


def band(d, t0, t1, half, fill=255):
    """A bar across the sword axis from t0 to t1 (distance along it, + toward the tip)."""
    pts = [(P0[0] + dx * t0 + nx * half, P0[1] + dy * t0 + ny * half), (P0[0] + dx * t1 + nx * half, P0[1] + dy * t1 + ny * half),
           (P0[0] + dx * t1 - nx * half, P0[1] + dy * t1 - ny * half), (P0[0] + dx * t0 - nx * half, P0[1] + dy * t0 - ny * half)]
    d.polygon(pts, fill=fill)


def rbar(d, t0, t1, half, fill=255):
    """The same bar with rounded ends."""
    band(d, t0, t1, half - (t1 - t0) / 2, fill)
    r = (t1 - t0) / 2
    for side in (1, -1):
        cx = P0[0] + dx * (t0 + r) + nx * (half - r) * side
        cy = P0[1] + dy * (t0 + r) + ny * (half - r) * side
        d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=fill)


paint(img, gold, mask_from(lambda d: band(d, k(8), k(42), k(31))))  # habaki
paint(img, Image.new('RGBA', (S, S), (120, 84, 36, 255)), mask_from(lambda d: band(d, k(28), k(30), k(31))))
paint(img, gold, mask_from(lambda d: band(d, k(-14), k(10), k(37))))  # seppa
paint(img, Image.new('RGBA', (S, S), (60, 34, 16, 255)), mask_from(lambda d: rbar(d, k(-12), k(8), k(80))))
paint(img, gold, mask_from(lambda d: rbar(d, k(-10), k(6), k(78))))  # tsuba rim
paint(img, vgrad((S, S), (70, 52, 46, 255), (34, 24, 22, 255)), mask_from(lambda d: rbar(d, k(-7), k(3), k(73))))

# Embers drifting up from the crescent.
for (ex, ey, er, c) in [(k(250), k(330), k(13), (255, 190, 90)), (k(200), k(430), k(8), (255, 120, 70)),
                        (k(330), k(250), k(7), (255, 150, 80)), (k(760), k(560), k(9), (255, 170, 90)),
                        (k(820), k(470), k(6), (255, 120, 70))]:
    m = mask_from(lambda d: d.ellipse([ex - er, ey - er, ex + er, ey + er], fill=255))
    paint(img, Image.new('RGBA', (S, S), c + (110,)), m.filter(ImageFilter.GaussianBlur(er)))
    paint(img, Image.new('RGBA', (S, S), c + (255,)), m)

out = img.resize((1024, 1024), Image.LANCZOS)
out.resize((256, 256), Image.LANCZOS).save('packaging/tenseiga.png', optimize=True)
out.resize((256, 256), Image.LANCZOS).save('crates/ro-client/assets/icon.png', optimize=True)
out.resize((512, 512), Image.LANCZOS).save('docs/logo.png', optimize=True)
out.save('crates/ro-client/assets/tenseiga.ico', sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
print('ok')
