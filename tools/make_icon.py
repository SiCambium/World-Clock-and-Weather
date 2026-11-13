"""Draws the TimeAtlas app icon and packs it into macos/AppIcon.icns.

Run: python3 tools/make_icon.py   (needs Pillow)
"""
import io
import math
import struct
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

S = 4096  # supersampled canvas, downscaled to 1024
ROOT = Path(__file__).resolve().parent.parent


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(len(a)))


def draw_icon():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    # macOS icon grid: 824/1024 body with room for the drop shadow.
    m = int(S * 100 / 1024)
    box = (m, m, S - m, S - m)
    radius = int(S * 185 / 1024)

    shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(
        (box[0], box[1] + S * 0.012, box[2], box[3] + S * 0.012), radius, fill=(0, 0, 0, 110)
    )
    shadow = shadow.filter(ImageFilter.GaussianBlur(S * 0.018))
    img.alpha_composite(shadow)

    # Vertical gradient body.
    grad = Image.new("RGBA", (S, S))
    top, bottom = (40, 70, 160), (10, 16, 38)
    gd = ImageDraw.Draw(grad)
    for y in range(S):
        gd.line([(0, y), (S, y)], fill=lerp(top, bottom, y / S) + (255,))
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle(box, radius, fill=255)
    img.paste(grad, (0, 0), mask)

    d = ImageDraw.Draw(img)
    cx, cy = S / 2, S / 2
    r = S * 0.27
    line = int(S * 0.016)
    globe = (190, 210, 255, 235)

    # Globe: outline, equator, parallels and meridians.
    d.ellipse((cx - r, cy - r, cx + r, cy + r), outline=globe, width=line)
    for k in (-0.5, 0.0, 0.5):
        y = cy + k * r
        half = math.sqrt(max(r * r - (k * r) ** 2, 0))
        d.line((cx - half, y, cx + half, y), fill=globe, width=int(line * 0.8))
    for w in (0.45, 0.85):
        d.ellipse((cx - r * w, cy - r, cx + r * w, cy + r), outline=globe, width=int(line * 0.8))
    d.line((cx, cy - r, cx, cy + r), fill=globe, width=int(line * 0.8))

    # Clock hands in warm sun yellow, pointing to about 10:10.
    sun = (255, 197, 61, 255)
    for angle, length, width in ((-60, 0.55, 0.05), (60, 0.8, 0.035)):
        a = math.radians(angle - 90)
        x2 = cx + math.cos(a) * r * length
        y2 = cy + math.sin(a) * r * length
        d.line((cx, cy, x2, y2), fill=sun, width=int(S * width))
        d.ellipse((x2 - S * width / 2, y2 - S * width / 2, x2 + S * width / 2, y2 + S * width / 2), fill=sun)
    dot = S * 0.035
    d.ellipse((cx - dot, cy - dot, cx + dot, cy + dot), fill=sun)

    return img.resize((1024, 1024), Image.LANCZOS)


def png_bytes(img, size):
    buf = io.BytesIO()
    img.resize((size, size), Image.LANCZOS).save(buf, format="PNG")
    return buf.getvalue()


def write_icns(img, path):
    # PNG-backed icon types: 16, 32, 64(@2x of 32), 128, 256, 512, 1024.
    entries = [
        (b"icp4", 16), (b"icp5", 32), (b"icp6", 64), (b"ic07", 128),
        (b"ic08", 256), (b"ic09", 512), (b"ic10", 1024),
        (b"ic11", 32), (b"ic12", 64), (b"ic13", 256), (b"ic14", 512),
    ]
    body = b""
    for kind, size in entries:
        data = png_bytes(img, size)
        body += kind + struct.pack(">I", len(data) + 8) + data
    path.write_bytes(b"icns" + struct.pack(">I", len(body) + 8) + body)


if __name__ == "__main__":
    icon = draw_icon()
    out = ROOT / "macos"
    out.mkdir(exist_ok=True)
    icon.save(out / "AppIcon-1024.png")
    write_icns(icon, out / "AppIcon.icns")
    print("wrote", out / "AppIcon.icns")
