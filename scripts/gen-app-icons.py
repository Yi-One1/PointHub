#!/usr/bin/env python3
"""Generate PointHub app icons from the user-provided PNG set.

Why this script exists: the source artwork lives in `用户提供的文件/` (never committed),
while `src-tauri/icons/` and the Android mipmaps are build inputs that must stay in sync.
Pillow cannot rasterize SVG, so every output is derived from the provided sized PNGs.

Usage:
    python scripts/gen-app-icons.py                 # write everything found
    python scripts/gen-app-icons.py --check         # verify, write nothing (exit 1 on drift)
    python scripts/gen-app-icons.py --android DIR   # write mipmaps into an explicit res/ dir

Exit codes: 0 ok, 1 mismatch (check mode) or missing source.
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageStat

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "用户提供的文件" / "软件图标"
ICONS = ROOT / "apps" / "desktop" / "src-tauri" / "icons"
ANDROID_RES = ROOT / "apps" / "desktop" / "src-tauri" / "gen" / "android" / "app" / "src" / "main" / "res"

# Windows bundle icons (tauri.conf.json -> bundle.icon) plus the tray/window icon.
PNG_TARGETS = {
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 256,
}
ICO_SIZES = [16, 32, 48, 64, 128, 256]
ANDROID_LEGACY = {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}
ADAPTIVE_BG = "#F4FBF7"  # matches the rounded background of pointhub-icon.svg
# Source artwork keeps ~7% transparent margin on every side. On a 30px sidebar tile
# that reads as "the icon is tiny", so outputs are trimmed to the artwork and repadded
# to a thin, uniform 3% edge.
PAD_RATIO = 0.03


def source(size: int) -> Path:
    p = SRC / f"pointhub-appicon-{size}.png"
    if not p.is_file():
        sys.exit(f"[icons] missing source: {p}")
    return p


def load(size: int) -> Image.Image:
    return Image.open(source(size)).convert("RGBA")


def tight(im: Image.Image, size: int) -> Image.Image:
    """Trim transparent margin, center on a square canvas, repad by PAD_RATIO."""
    im = im.convert("RGBA")
    bbox = im.split()[3].getbbox()
    if bbox:
        im = im.crop(bbox)
    w, h = im.size
    side = min(w, h)
    im = im.crop(((w - side) // 2, (h - side) // 2, (w - side) // 2 + side, (h - side) // 2 + side))
    pad = max(1, round(size * PAD_RATIO))
    inner = max(1, size - 2 * pad)
    im = im.resize((inner, inner), Image.LANCZOS)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    canvas.paste(im, (pad, pad), im)
    return canvas


def mean_diff(a: Image.Image, b: Image.Image) -> float:
    if a.size != b.size:
        return 255.0
    stat = ImageStat.Stat(ImageChops.difference(a.convert("RGB"), b.convert("RGB")))
    return sum(stat.mean) / 3.0


def write_pngs() -> None:
    ICONS.mkdir(parents=True, exist_ok=True)
    for name, size in PNG_TARGETS.items():
        tight(load(size), size).save(ICONS / name, optimize=True)
        print(f"[icons] wrote {name} ({size}px)")

    frames = [tight(load(s), s) for s in ICO_SIZES]
    frames[-1].save(
        ICONS / "icon.ico",
        format="ICO",
        sizes=[(s, s) for s in ICO_SIZES],
        append_images=frames[:-1],
    )
    print(f"[icons] wrote icon.ico {ICO_SIZES}")


def write_android(res: Path) -> None:
    if not res.is_dir():
        print(f"[icons] android res not found, skipped: {res}")
        return

    app = load(256)
    for density, size in ANDROID_LEGACY.items():
        d = res / f"mipmap-{density}"
        d.mkdir(parents=True, exist_ok=True)
        icon = tight(app, size)
        icon.save(d / "ic_launcher.png", optimize=True)
        icon.save(d / "ic_launcher_round.png", optimize=True)

    # Adaptive icon (API 26+): foreground is the transparent mark inside a 108dp canvas,
    # with the artwork kept inside the 66dp safe zone.
    if (res / "mipmap-anydpi-v26").is_dir():
        mark = Image.open(SRC / "pointhub-mark-256.png").convert("RGBA")
        for density, size in ANDROID_LEGACY.items():
            canvas = int(size * 108 / 48)
            inner = int(canvas * 0.55)
            fg = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
            scaled = mark.copy()
            scaled.thumbnail((inner, inner), Image.LANCZOS)
            fg.paste(scaled, ((canvas - scaled.width) // 2, (canvas - scaled.height) // 2), scaled)
            fg.save(res / f"mipmap-{density}" / "ic_launcher_foreground.png", optimize=True)

        values = res / "values"
        values.mkdir(parents=True, exist_ok=True)
        (values / "ic_launcher_background.xml").write_text(
            '<?xml version="1.0" encoding="utf-8"?>\n'
            "<resources>\n"
            f'    <color name="ic_launcher_background">{ADAPTIVE_BG}</color>\n'
            "</resources>\n",
            encoding="utf-8",
        )
    print(f"[icons] wrote android mipmaps -> {res}")


def framing_ok(im: Image.Image, size: int) -> str | None:
    """Outputs must show the artwork with only the thin PAD_RATIO edge, not the 7% source margin."""
    pad = max(1, round(size * PAD_RATIO))
    expected = (pad, pad, size - pad, size - pad)
    bbox = im.split()[3].getbbox()
    if bbox is None:
        return "fully transparent"
    if any(abs(a - b) > 1 for a, b in zip(bbox, expected)):
        return f"framing {bbox} != expected {expected} (too much or too little margin)"
    return None


def check(android_res: Path) -> int:
    errors: list[str] = []
    for name, size in PNG_TARGETS.items():
        out = ICONS / name
        if not out.is_file():
            errors.append(f"missing {name}")
            continue
        got = Image.open(out).convert("RGBA")
        if got.size != (size, size):
            errors.append(f"{name} size {got.size} != {(size, size)}")
            continue
        diff = mean_diff(got, tight(load(size), size))
        if diff > 0.5:
            errors.append(f"{name} differs from freshly trimmed source (mean {diff:.1f})")
        bad = framing_ok(got, size)
        if bad:
            errors.append(f"{name} {bad}")

    ico = ICONS / "icon.ico"
    if not ico.is_file():
        errors.append("missing icon.ico")
    else:
        with Image.open(ico) as im:
            sizes = {s[0] for s in im.ico.sizes()}
            missing = [s for s in ICO_SIZES if s not in sizes]
            if missing:
                errors.append(f"icon.ico missing sizes {missing}")
            im.size = (256, 256)
            im.load()
            big = im.convert("RGBA")
            diff = mean_diff(big, tight(load(256), 256))
            if diff > 0.5:
                errors.append(f"icon.ico 256px differs from freshly trimmed source (mean {diff:.1f})")
            bad = framing_ok(big, 256)
            if bad:
                errors.append(f"icon.ico 256px {bad}")

    if android_res.is_dir():
        for density, size in ANDROID_LEGACY.items():
            p = android_res / f"mipmap-{density}" / "ic_launcher.png"
            if not p.is_file():
                errors.append(f"missing android {p.relative_to(ROOT)}")
                continue
            got = Image.open(p).convert("RGBA")
            if got.size != (size, size):
                errors.append(f"android {density} size != {size}")
                continue
            bad = framing_ok(got, size)
            if bad:
                errors.append(f"android {density} {bad}")

    if errors:
        for e in errors:
            print(f"[icons] FAIL {e}")
        return 1
    print("[icons] OK: bundle icons, ico and android mipmaps match the provided artwork")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true", help="verify only, write nothing")
    ap.add_argument("--android", type=Path, default=None, help="android res/ directory")
    args = ap.parse_args()

    android_res = args.android or ANDROID_RES
    if args.check:
        return check(android_res)

    write_pngs()
    write_android(android_res)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
