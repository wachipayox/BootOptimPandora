#!/usr/bin/env python3
"""Regenerate official launcher icons from approved raster sources (Pillow 10.0.0)."""
import argparse
import io
import struct
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent
SIZES = (16, 24, 32, 48, 64, 128, 256, 512)
ICNS = ((b"icp4", 16), (b"icp5", 32), (b"icp6", 64), (b"ic07", 128),
        (b"ic08", 256), (b"ic09", 512), (b"ic10", 1024),
        (b"ic11", 32), (b"ic12", 64), (b"ic13", 256), (b"ic14", 512))

def png(source, size):
    image = source.resize((size, size), Image.Resampling.LANCZOS)
    output = io.BytesIO()
    image.save(output, format="PNG", optimize=False, compress_level=9)
    return output.getvalue()

def generate(check=False):
    count = 0
    for variant, target in (("release", ROOT), ("development", ROOT / "development")):
        source = Image.open(ROOT / "icon-sources" / f"{variant}-1024.png").convert("RGBA")
        if source.size != (1024, 1024):
            raise ValueError("Icon sources must be 1024 x 1024")
        images = {size: png(source, size) for size in (*SIZES, 1024)}
        chunks = b"".join(tag + struct.pack(">I", len(images[size]) + 8) + images[size] for tag, size in ICNS)
        outputs = {target / "windows.ico": (ROOT / "icon-sources" / f"{variant}.ico").read_bytes(),
                   target / "mac.icns": b"icns" + struct.pack(">I", len(chunks) + 8) + chunks}
        outputs.update({target / "windows_icons" / f"icon_{size}x{size}.png": images[size] for size in SIZES})
        for path, data in outputs.items():
            if check:
                if not path.exists() or path.read_bytes() != data:
                    raise SystemExit(f"Generated icon differs: {path.relative_to(ROOT)}")
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            count += 1
    print(f"{'Verified' if check else 'Generated'} {count} icon files")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    generate(parser.parse_args().check)
