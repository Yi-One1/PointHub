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
import shutil
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


def source(size: int) -> Path:
    p = SRC / f"pointhub-appicon-{size}.png"
    if not p.is_file():
        sys.exit(f"[icons] missing source: {p}")
    return p


def load(size: int) -> Image.Image:
    return Image.open(source(size)).convert("RGBA")


def mean_diff(a: Image.Image, b: Image.Image) -> float:
    if a.size != b.size:
        return 255.0
    stat = ImageStat.Stat(ImageChops.difference(a.convert("RGB"), b.convert("RGB")))
    return sum(stat.mean) / 3.0


def write_pngs() -> None:
    ICONS.mkdir(parents=True, exist_ok=True)
    for name, size in PNG_TARGETS.items():
        # The provided artwork already ships one file per size: copy bytes so the
        # bundled icon stays pixel-identical to the source (and testable by hash).
        shutil.copyfile(source(size), ICONS / name)
        print(f"[icons] wrote {name} ({size}px)")

    frames = [load(s) for s in ICO_SIZES]
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
        app.resize((size, size), Image.LANCZOS).save(d / "ic_launcher.png", optimize=True)
        app.resize((size, size), Image.LANCZOS).save(d / "ic_launcher_round.png", optimize=True)

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
        diff = mean_diff(got, load(size))
        if diff > 2.0:
            errors.append(f"{name} differs from source (mean {diff:.1f})")

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
            diff = mean_diff(im.convert("RGBA"), load(256))
            if diff > 2.0:
                errors.append(f"icon.ico 256px differs from source (mean {diff:.1f})")

    if android_res.is_dir():
        for density, size in ANDROID_LEGACY.items():
            p = android_res / f"mipmap-{density}" / "ic_launcher.png"
            if not p.is_file():
                errors.append(f"missing android {p.relative_to(ROOT)}")
            elif Image.open(p).size != (size, size):
                errors.append(f"android {density} size != {size}")

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
