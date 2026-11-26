/**
 * 应用图标校验：exe/安装包/托盘/窗口共用同一套图标，任何一处漂移都算回归。
 *
 * 分级：
 * - 结构校验（本文件，纯 Node）：尺寸、ICO 目录、tauri.conf 引用、托盘来源；
 * - 像素校验（调用 scripts/gen-app-icons.py --check）：与用户提供的源图逐像素比对。
 */
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const UI_DIR = path.resolve(HERE, "..");
const ROOT = path.resolve(HERE, "../../../..");
const SRC_DIR = path.join(ROOT, "用户提供的文件", "软件图标");
const TAURI_DIR = path.join(ROOT, "apps", "desktop", "src-tauri");
const ICONS = path.join(TAURI_DIR, "icons");
const ANDROID_RES = path.join(TAURI_DIR, "gen", "android", "app", "src", "main", "res");

const BUNDLE_ICONS = {
  "32x32.png": 32,
  "128x128.png": 128,
  "128x128@2x.png": 256,
  "icon.png": 256,
};

function pngSize(file) {
  const buf = readFileSync(file);
  assert.deepEqual([...buf.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10], `${file} 不是 PNG`);
  return { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) };
}

function icoEntries(file) {
  const buf = readFileSync(file);
  assert.equal(buf.readUInt16LE(0), 0, "ICO reserved 应为 0");
  assert.equal(buf.readUInt16LE(2), 1, "ICO type 应为 1");
  const count = buf.readUInt16LE(4);
  const entries = [];
  for (let i = 0; i < count; i++) {
    const at = 6 + i * 16;
    entries.push({
      width: buf[at] === 0 ? 256 : buf[at],
      height: buf[at + 1] === 0 ? 256 : buf[at + 1],
      bytes: buf.readUInt32LE(at + 8),
      offset: buf.readUInt32LE(at + 12),
    });
  }
  return { buf, entries };
}

test("打包 PNG 尺寸正确（像素与四周留白由 --check 校验）", () => {
  for (const [name, size] of Object.entries(BUNDLE_ICONS)) {
    const out = path.join(ICONS, name);
    assert.ok(existsSync(out), `缺少 ${name}`);
    assert.deepEqual(pngSize(out), { width: size, height: size }, `${name} 尺寸不对`);
  }
  assert.ok(existsSync(path.join(SRC_DIR, "pointhub-appicon-256.png")), "缺少源图，无法校验像素");
});

test("icon.ico 含 16→256 全套尺寸，256 帧为 256×256 PNG", () => {
  const { buf, entries } = icoEntries(path.join(ICONS, "icon.ico"));
  const sizes = entries.map((e) => e.width);
  for (const s of [16, 32, 48, 64, 128, 256]) {
    assert.ok(sizes.includes(s), `icon.ico 缺少 ${s}px`);
  }

  const big = entries.find((e) => e.width === 256);
  assert.ok(big, "icon.ico 缺少 256px 帧");
  const frame = buf.subarray(big.offset, big.offset + big.bytes);
  assert.deepEqual([...frame.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10], "256 帧不是 PNG");
  assert.equal(frame.readUInt32BE(16), 256, "256 帧宽度不对");

  // Pillow 可能把每一帧都写成 PNG，也可能对小于 256 的帧用 BMP；两种都接受。
  for (const entry of entries) {
    const frame = buf.subarray(entry.offset, entry.offset + entry.bytes);
    if (frame.readUInt32BE(0) === 0x89504e47) {
      assert.equal(frame.readUInt32BE(16), entry.width, `${entry.width}px 帧 PNG 宽度不对`);
      assert.equal(frame.readUInt32BE(20), entry.height, `${entry.width}px 帧 PNG 高度不对`);
    } else {
      assert.equal(frame.readUInt32LE(0), 40, `${entry.width}px 帧应为 BITMAPINFOHEADER`);
      assert.equal(frame.readInt32LE(4), entry.width, `${entry.width}px 帧宽度不对`);
      assert.equal(frame.readInt32LE(8), entry.height * 2, `${entry.width}px 帧高度应为两倍（含掩码）`);
    }
  }
});

test("tauri.conf 引用的图标文件都存在", () => {
  const conf = JSON.parse(readFileSync(path.join(TAURI_DIR, "tauri.conf.json"), "utf8"));
  const icons = conf.bundle?.icon ?? [];
  assert.ok(icons.length > 0, "bundle.icon 为空");
  for (const rel of icons) {
    assert.ok(existsSync(path.join(TAURI_DIR, rel)), `bundle.icon 指向不存在的文件：${rel}`);
  }
  assert.ok(icons.includes("icons/icon.ico"), "bundle.icon 应包含 icon.ico");
});

test("图标变更能触发重编，且窗口图标固定取 icon.png", () => {
  const build = readFileSync(path.join(TAURI_DIR, "build.rs"), "utf8");
  assert.match(build, /rerun-if-changed=icons/, "build.rs 必须跟踪 icons/，否则只改图标不会重编");
  assert.match(build, /rerun-if-changed=tauri\.conf\.json/);

  const lib = readFileSync(path.join(TAURI_DIR, "src", "lib.rs"), "utf8");
  assert.match(lib, /include_bytes!\("\.\.\/icons\/icon\.png"\)/, "窗口图标应显式取 icon.png");
  assert.match(lib, /set_icon\(/, "启动时应显式设置窗口/任务栏图标");
});

test("托盘用透明标记（浅色 + 深色两套），窗口/任务栏用完整底板图标", () => {
  for (const name of ["tray-32.png", "tray-dark-32.png"]) {
    const tray = path.join(ICONS, name);
    assert.ok(existsSync(tray), `缺少托盘图标 ${name}`);
    assert.deepEqual(pngSize(tray), { width: 32, height: 32 }, `${name} 应为 32×32`);
  }

  const lib = readFileSync(path.join(TAURI_DIR, "src", "lib.rs"), "utf8");
  assert.match(lib, /include_bytes!\("\.\.\/icons\/tray-32\.png"\)/, "托盘应嵌入浅色标记");
  assert.match(lib, /include_bytes!\("\.\.\/icons\/tray-dark-32\.png"\)/, "托盘应嵌入深色标记");
  assert.match(lib, /ThemeChanged/, "系统切换深浅色时应同步托盘图标");
  assert.match(lib, /default_window_icon\(\)/, "解码失败时应回退到窗口图标");
});

test("Android mipmap 已写入（工程存在时）", () => {
  if (!existsSync(ANDROID_RES)) {
    return; // gen/ 不入库；没初始化 Android 工程时跳过
  }
  const expected = { mdpi: 48, hdpi: 72, xhdpi: 96, xxhdpi: 144, xxxhdpi: 192 };
  for (const [density, size] of Object.entries(expected)) {
    for (const name of ["ic_launcher.png", "ic_launcher_round.png"]) {
      const file = path.join(ANDROID_RES, `mipmap-${density}`, name);
      assert.ok(existsSync(file), `缺少 Android 图标 ${density}/${name}`);
      assert.deepEqual(pngSize(file), { width: size, height: size }, `${density}/${name} 尺寸不对`);
    }
  }
});

test("图标像素与源图一致（调用 gen-app-icons.py --check）", (t) => {
  const r = spawnSync("python", [path.join("scripts", "gen-app-icons.py"), "--check"], {
    cwd: ROOT,
    encoding: "utf8",
  });
  if (r.error?.code === "ENOENT") {
    t.skip("本机没有 python，跳过像素级校验");
    return;
  }
  assert.equal(r.status, 0, `图标像素校验失败：\n${r.stdout}${r.stderr}`);
});

test("桌面/任务栏图标由 256px 母图缩小，而不是把小图放大", (t) => {
  // 32px 曾经用 32px 源再放大 1.25 倍，任务栏与桌面快捷方式明显发虚。
  const code = [
    "import importlib.util, sys",
    "from PIL import Image, ImageChops, ImageStat",
    "root = sys.argv[1]",
    "spec = importlib.util.spec_from_file_location('g', root + '/scripts/gen-app-icons.py')",
    "g = importlib.util.module_from_spec(spec); spec.loader.exec_module(g)",
    "def diff(a, b):",
    "    s = ImageStat.Stat(ImageChops.difference(a.convert('RGBA'), b.convert('RGBA')))",
    "    return sum(s.mean) / 4",
    "got = Image.open(root + '/apps/desktop/src-tauri/icons/32x32.png')",
    "big = g.tight(Image.open(g.source(256)).convert('RGBA'), 32, g.BUNDLE_ZOOM)",
    "small = g.tight(Image.open(g.source(32)).convert('RGBA'), 32, g.BUNDLE_ZOOM)",
    "print(diff(got, big), diff(got, small))",
  ].join("\n");
  const r = spawnSync("python", ["-c", code, ROOT], { encoding: "utf8" });
  if (r.error?.code === "ENOENT") {
    t.skip("本机没有 python，跳过清晰度来源校验");
    return;
  }
  assert.equal(r.status, 0, r.stderr);
  const [fromBig, fromSmall] = r.stdout.trim().split(/\s+/).map(Number);
  assert.ok(fromBig < 0.6, `32px 图标与「256→32」结果不一致（差 ${fromBig}），可能又走了放大路径`);
  assert.ok(fromSmall > 3, `32px 图标与「32px 源」几乎一致（差 ${fromSmall}），说明仍在使用小图放大`);
});
