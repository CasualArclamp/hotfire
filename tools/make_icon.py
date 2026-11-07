# Draws the Hotfire icon (assets/icon.png, assets/icon.ico). Run: python -I tools/make_icon.py
# Pillow + numpy only. Everything is drawn at 4x and scaled down so the edges stay clean.
import math
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

S = 2048          # working size (final art is 512)
K = S / 1024.0    # scale from the 1024 design grid


def lerp(a, b, t):
    return a + (b - a) * t


def palette(t):
    """Flame colour, t 0 (white-hot) .. 1 (deep red), as float rgb 0..1 arrays."""
    stops = [
        (0.00, (1.00, 0.97, 0.85)),
        (0.25, (1.00, 0.80, 0.30)),
        (0.55, (1.00, 0.42, 0.08)),
        (0.85, (0.80, 0.12, 0.03)),
        (1.00, (0.35, 0.03, 0.02)),
    ]
    t = np.clip(t, 0, 1)
    out = np.zeros(t.shape + (3,))
    for (t0, c0), (t1, c1) in zip(stops, stops[1:]):
        m = (t >= t0) & (t <= t1)
        f = ((t - t0) / (t1 - t0))[m, None]
        out[m] = np.array(c0) * (1 - f) + np.array(c1) * f
    return out


yy, xx = np.mgrid[0:S, 0:S].astype(np.float64)
x = xx / K
y = yy / K

# background: deep blue-black, warmer towards the bottom where the plume lights it
bg_top = np.array([0.035, 0.05, 0.10])
bg_bot = np.array([0.11, 0.045, 0.035])
g = np.clip(y / 1024.0, 0, 1)[..., None]
img = bg_top * (1 - g) + bg_bot * g

# plume: from the exit plane (y=520) down; a teardrop with a bright core and shock diamonds
EXIT_Y, TIP_Y, EXIT_HW = 520.0, 1000.0, 200.0
u = np.clip((y - EXIT_Y) / (TIP_Y - EXIT_Y), 0, 1)
hw = EXIT_HW * (1 - u) ** 0.6 * (1 + 0.22 * np.sin(u * 3.1)) + 1e-3
q = np.abs(x - 512.0) / hw
inside = (y >= EXIT_Y - 4) & (u < 1)
body = np.exp(-(q ** 2) * 1.6) * np.clip(1 - u ** 1.6, 0, 1)
core = np.exp(-(q * 2.6) ** 2) * np.clip(1 - u * 1.15, 0, 1)
heat = np.clip(q * 0.75 + u * 0.55, 0, 1)
col = palette(heat) * body[..., None]
col += palette(heat * 0.3) * core[..., None] * 0.9
# shock diamonds on the axis
for k, c in enumerate((0.16, 0.38, 0.60)):
    yc = EXIT_Y + c * (TIP_Y - EXIT_Y)
    d = np.exp(-(((y - yc) / (26 - 5 * k)) ** 2) - ((x - 512.0) / (46 - 10 * k)) ** 2)
    col += np.array([1.0, 0.95, 0.8]) * d[..., None] * (0.75 - 0.18 * k)
col *= inside[..., None]
alpha = np.clip(body * 1.5 + core, 0, 1) * inside
img = img * (1 - alpha[..., None]) + np.clip(col, 0, 1.6) * alpha[..., None]

# glow: blur the bright parts and add them back
im = Image.fromarray((np.clip(img, 0, 1) * 255).astype(np.uint8))
hot = Image.fromarray((np.clip(img - 0.45, 0, 1) * 255 * 1.8).clip(0, 255).astype(np.uint8))
glow = hot.filter(ImageFilter.GaussianBlur(34 * K))
im = Image.fromarray(np.clip(np.asarray(im, np.float64) + np.asarray(glow, np.float64) * 0.9, 0, 255).astype(np.uint8))

# engine: chamber, throat and bell, steel with a hot inside
CH_Y, THROAT_Y = 70.0, 262.0


def hw_at(yv):
    if yv <= CH_Y + 60:
        return 88.0
    if yv <= THROAT_Y:                                   # converging section
        t = (yv - (CH_Y + 60)) / (THROAT_Y - (CH_Y + 60))
        return lerp(88.0, 38.0, math.sin(t * math.pi / 2))
    t = (yv - THROAT_Y) / (EXIT_Y - THROAT_Y)            # bell: opens fast, then runs nearly straight
    return 38.0 + (EXIT_HW - 38.0) * (0.45 * t + 0.55 * (1 - (1 - t) ** 2.4))


ys = np.arange(int(CH_Y), int(EXIT_Y) + 1, 1.0)
left = [((512 - hw_at(v)) * K, v * K) for v in ys]
right = [((512 + hw_at(v)) * K, v * K) for v in ys]
poly = left + right[::-1]
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).polygon(poly, fill=255)
# wall thickness: shrink the mask to get the hot inside
inner = mask.filter(ImageFilter.MinFilter(2 * int(17 * K) // 2 * 2 + 1))
wall_mask = np.asarray(mask, np.float64) / 255.0
inner_mask = np.asarray(inner, np.float64) / 255.0
steel_t = np.clip((x - 330) / 360.0, 0, 1)
steel = (np.array([0.40, 0.45, 0.54]) * (1 - steel_t[..., None]) + np.array([0.82, 0.86, 0.92]) * steel_t[..., None])
steel = steel * (0.78 + 0.22 * np.clip(1 - np.abs(x - 512) / 200, 0, 1))[..., None]
yt = np.clip((y - CH_Y) / (EXIT_Y - CH_Y), 0, 1)
hotc = palette(0.5 * yt + 0.05 * (1 - np.clip(1 - np.abs(x - 512) / 120, 0, 1)))
hotc *= (0.95 - 0.15 * yt)[..., None]
arr = np.asarray(im, np.float64) / 255.0
arr = arr * (1 - wall_mask[..., None]) + steel * wall_mask[..., None]
arr = arr * (1 - inner_mask[..., None]) + hotc * inner_mask[..., None]
# chamber cap and a bright lip at the exit
lip = (np.abs(x - 512) < hw_at(EXIT_Y) + 10) & (y > EXIT_Y - 2) & (y < EXIT_Y + 10)
arr[lip] = np.array([0.86, 0.89, 0.95])
im = Image.fromarray((np.clip(arr, 0, 1) * 255).astype(np.uint8))

# rounded-square tile with a faint edge light
tile = Image.new("L", (S, S), 0)
ImageDraw.Draw(tile).rounded_rectangle([0, 0, S - 1, S - 1], radius=int(210 * K), fill=255)
out = im.convert("RGBA")
out.putalpha(tile)
out = out.resize((512, 512), Image.LANCZOS)
out.save("assets/icon_512.png")
out.resize((256, 256), Image.LANCZOS).save("assets/icon.png")
out.save("assets/icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
print("wrote assets/icon_512.png, icon.png, icon.ico")
