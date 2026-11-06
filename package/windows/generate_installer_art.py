"""Generate the Windows installer bitmaps from the launcher's own assets.

Usage: python package/windows/generate_installer_art.py <output directory> [release|development]

Inputs: the release cube icon, the default café background, Chakra Petch and the
dark theme tokens. NSIS stretches bitmaps with GDI, which aliases badly, so one
24-bit BMP is written per Windows display scale at the exact size of the MUI2
image controls (measured: 191x386 sidebar, 175x70 header at 100 %). The
installer picks the variant that matches the control at run time.
"""

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[2]
SCALES = (100, 125, 150, 175, 200)
SIDEBAR = (191, 386)
HEADER = (175, 70)

BG = (6, 6, 7)
PANEL = (15, 15, 18)
INK = (236, 236, 238)
MUTED = (154, 154, 163)
ACCENT = (91, 206, 245)

CAFE = Image.open(ROOT / "assets/images/fondo-cafe-1800.png").convert("RGB")
BOLD = ROOT / "assets/fonts/chakrapetch/ChakraPetch-Bold.ttf"
SEMI = ROOT / "assets/fonts/chakrapetch/ChakraPetch-SemiBold.ttf"


def cover(image: Image.Image, size: tuple[int, int], focus_x: float = 0.5) -> Image.Image:
    w, h = size
    ratio = max(w / image.width, h / image.height)
    scaled = image.resize((round(image.width * ratio), round(image.height * ratio)), Image.LANCZOS)
    left = round((scaled.width - w) * focus_x)
    top = (scaled.height - h) // 2
    return scaled.crop((left, top, left + w, top + h))


def vertical_scrim(size: tuple[int, int], start: float, end: float) -> Image.Image:
    w, h = size
    mask = Image.new("L", (1, h))
    for y in range(h):
        t = y / max(h - 1, 1)
        mask.putpixel((0, y), round(255 * (start + (end - start) * t)))
    return mask.resize((w, h))


def glow(size, center, radius, color, strength) -> Image.Image:
    layer = Image.new("RGBA", size, (0, 0, 0, 0))
    cx, cy = center
    ImageDraw.Draw(layer).ellipse((cx - radius, cy - radius, cx + radius, cy + radius), fill=(*color, strength))
    return layer.filter(ImageFilter.GaussianBlur(radius * 0.6))


def spaced_text(draw, width, y, text, fnt, fill, spacing):
    total = sum(draw.textlength(ch, font=fnt) for ch in text) + spacing * (len(text) - 1)
    x = (width - total) / 2
    for ch in text:
        draw.text((x, y), ch, font=fnt, fill=fill)
        x += draw.textlength(ch, font=fnt) + spacing


def sidebar(scale: float) -> Image.Image:
    w, h = round(SIDEBAR[0] * scale), round(SIDEBAR[1] * scale)
    u = w / 191  # layout unit: 1 px at 100 %
    size = (w, h)
    base = cover(CAFE, size, focus_x=0.48)
    base = Image.composite(Image.new("RGB", size, BG), base, vertical_scrim(size, 0.05, 0.85)).convert("RGBA")
    base.alpha_composite(glow(size, (w / 2, 128 * u), 78 * u, ACCENT, 46))

    cube_size = round(128 * u)
    cube = CUBE.resize((cube_size, cube_size), Image.LANCZOS)
    left, top = (w - cube_size) // 2, round(64 * u)
    shadow = Image.new("RGBA", size, (0, 0, 0, 0))
    shadow.paste((0, 0, 0, 255), (left, top + round(9 * u)), cube.split()[3].point(lambda a: a * 0.55))
    base.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(10 * u)))
    base.alpha_composite(cube, (left, top))

    draw = ImageDraw.Draw(base)
    spaced_text(draw, w, 214 * u, "WACHILAND", ImageFont.truetype(str(BOLD), round(24 * u)), INK, 1.3 * u)
    spaced_text(draw, w, 242 * u, "LAUNCHER", ImageFont.truetype(str(SEMI), round(13 * u)), ACCENT, 4.8 * u)
    line_w = 30 * u
    draw.rectangle(((w - line_w) / 2, 271 * u, (w + line_w) / 2, 271 * u + max(1, round(u))), fill=ACCENT)
    spaced_text(draw, w, 352 * u, "Wachiland Elite", ImageFont.truetype(str(SEMI), round(10 * u)), MUTED, 0.6 * u)
    return base.convert("RGB")


def header(scale: float) -> Image.Image:
    w, h = round(HEADER[0] * scale), round(HEADER[1] * scale)
    u = w / 175
    size = (w, h)
    base = Image.new("RGBA", size, (*PANEL, 255))
    base.alpha_composite(glow(size, (w - 40 * u, h / 2), 30 * u, ACCENT, 40))
    cube_size = round(54 * u)
    cube = CUBE.resize((cube_size, cube_size), Image.LANCZOS)
    base.alpha_composite(cube, (w - cube_size - round(13 * u), (h - cube_size) // 2))
    return base.convert("RGB")


if __name__ == "__main__":
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "target/installer/art")
    icon = sys.argv[2] if len(sys.argv) > 2 else "release"
    CUBE = Image.open(ROOT / f"package/icon-sources/{icon}-1024.png").convert("RGBA")
    out.mkdir(parents=True, exist_ok=True)
    for scale in SCALES:
        sidebar(scale / 100).save(out / f"installer-sidebar-{scale}.bmp")
        header(scale / 100).save(out / f"installer-header-{scale}.bmp")
    # Fallback that MUI2 loads before the page picks its exact variant.
    sidebar(2).save(out / "installer-sidebar.bmp")
    header(2).save(out / "installer-header.bmp")
    print(f"installer art written to {out}")
