"""Side-by-side and difference images of the Electron screenshots (old) and the Rust ones (new).

    python -I scripts/compare.py <old dir> <new dir> <out dir> [name ...]

Each output shows old | new | difference on a mid-grey background (the PNGs are transparent), all
aligned at the top-left; differences are amplified so 1-2 px shifts stand out.
"""
import sys
from pathlib import Path

from PIL import Image, ImageChops

old_dir, new_dir, out_dir = map(Path, sys.argv[1:4])
names = sys.argv[4:] or sorted(p.stem for p in new_dir.glob("*.png") if (old_dir / p.name).exists())
out_dir.mkdir(parents=True, exist_ok=True)
BG = (128, 128, 128, 255)


def flat(img, size):
    canvas = Image.new("RGBA", size, BG)
    canvas.alpha_composite(img.convert("RGBA"))
    return canvas


for name in names:
    # Cropped to what's drawn, so margins of different sizes don't shift the comparison.
    old = Image.open(old_dir / f"{name}.png").convert("RGBA")
    new = Image.open(new_dir / f"{name}.png").convert("RGBA")
    old = old.crop(old.getchannel("A").getbbox())
    new = new.crop(new.getchannel("A").getbbox())
    size = (max(old.width, new.width), max(old.height, new.height))
    a, b = flat(old, size), flat(new, size)
    diff = ImageChops.difference(a.convert("RGB"), b.convert("RGB")).point(lambda v: min(255, v * 4))
    out = Image.new("RGB", (size[0] * 3 + 40, size[1] + 20), (40, 40, 40))
    out.paste(a.convert("RGB"), (10, 10))
    out.paste(b.convert("RGB"), (size[0] + 20, 10))
    out.paste(diff, (size[0] * 2 + 30, 10))
    out.save(out_dir / f"{name}.png")
    print(f"{name}: old {old.size} new {new.size}")
