/**
 * 图标资产校验。
 *
 * 这里不测像素观感（那要靠人眼比对源图），只保证「文件在、尺寸对、颜色通道对、
 * 平台映射不漏」——这几项一旦错了，UI 会出现空白或糊图，是最容易回归的部分。
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
const ASSETS = path.join(UI_DIR, "src", "assets");

/** 读 PNG 的 IHDR，顺带校验 PNG 签名。 */
function readPng(file) {
  const buf = readFileSync(file);
  const sig = [137, 80, 78, 71, 13, 10, 26, 10];
  for (let i = 0; i < sig.length; i++) {
    assert.equal(buf[i], sig[i], `${file} 不是合法 PNG`);
  }
  assert.equal(buf.toString("latin1", 12, 16), "IHDR", `${file} 缺少 IHDR`);
  return {
    width: buf.readUInt32BE(16),
    height: buf.readUInt32BE(20),
    bitDepth: buf[24],
    colorType: buf[25],
  };
}

test("积分图标：三种色调存在、64×64、RGBA", () => {
  for (const tone of ["bronze", "jade", "ink"]) {
    const file = path.join(ASSETS, "points", `points-${tone}.png`);
    assert.ok(existsSync(file), `缺少积分图标 points-${tone}.png`);
    const png = readPng(file);
    assert.equal(png.width, 64, `${tone} 宽度应为 64`);
    assert.equal(png.height, 64, `${tone} 高度应为 64`);
    assert.equal(png.bitDepth, 8, `${tone} 位深应为 8`);
    assert.equal(png.colorType, 6, `${tone} 应为 RGBA（colorType=6）`);
  }
});

test("品牌 SVG：完整图标与标记都存在且带 viewBox", () => {
  for (const name of ["pointhub-icon.svg", "pointhub-mark.svg"]) {
    const file = path.join(ASSETS, "brand", name);
    assert.ok(existsSync(file), `缺少品牌图标 ${name}`);
    const svg = readFileSync(file, "utf8");
    assert.match(svg, /viewBox="[^"]+"/, `${name} 缺少 viewBox`);
    assert.match(svg, /<svg[\s>]/, `${name} 不是 SVG`);
  }
});

test("平台图标：三平台都是 128×128 透明 PNG", () => {
  for (const name of ["workbuddy.png", "trae.png", "qoder.png"]) {
    const file = path.join(ASSETS, "vendors", name);
    assert.ok(existsSync(file), `缺少平台图标 ${name}`);
    const png = readPng(file);
    assert.equal(png.width, 128, `${name} 宽度应为 128`);
    assert.equal(png.height, 128, `${name} 高度应为 128`);
    assert.equal(png.colorType, 6, `${name} 应为 RGBA（colorType=6）`);
  }
});

test("平台图标已裁掉多余留白（四周透明边不超过 2%）", (t) => {
  const r = spawnSync(
    "python",
    [
      "-c",
      [
        "import sys",
        "from PIL import Image",
        "bad = []",
        "for p in sys.argv[1:]:",
        "    im = Image.open(p).convert('RGBA')",
        "    b = im.split()[3].getbbox()",
        "    w, h = im.size",
        "    margin = max(b[0], b[1], w - b[2], h - b[3])",
        "    if margin > w * 0.02:",
        "        bad.append(f'{p}: margin {margin}px')",
        "print('ok' if not bad else '\\n'.join(bad))",
        "sys.exit(1 if bad else 0)",
      ].join("\n"),
      ...["workbuddy.png", "trae.png", "qoder.png"].map((n) => path.join(ASSETS, "vendors", n)),
    ],
    { encoding: "utf8" }
  );
  if (r.error?.code === "ENOENT") {
    t.skip("本机没有 python，跳过留白校验");
    return;
  }
  assert.equal(r.status, 0, `平台图标留白过大（会被缩得很小）：\n${r.stdout}${r.stderr}`);
});

test("平台名称前都挂了 VendorIcon（总览/账号/导入/能力表/悬浮球）", () => {
  const app = readFileSync(path.join(UI_DIR, "src", "App.tsx"), "utf8");
  const ball = readFileSync(path.join(UI_DIR, "src", "Ball.tsx"), "utf8");

  const appCount = (app.match(/<VendorIcon\b/g) ?? []).length;
  assert.ok(appCount >= 4, `App.tsx 中 VendorIcon 只有 ${appCount} 处，应为 4 处（总览/账号/导入/能力表）`);
  assert.match(ball, /<VendorIcon\b/, "悬浮球的分组标题缺少平台图标");

  // 旧的识别色圆点方案应彻底退场，避免同一个平台出现「点 + 图」两种标识。
  assert.doesNotMatch(app, /VENDOR_TONE|tone-(wb|trae|qoder)/, "App.tsx 仍残留 VENDOR_TONE 圆点");
  const css = readFileSync(path.join(UI_DIR, "src", "styles.css"), "utf8");
  assert.doesNotMatch(css, /\.tone-(wb|trae|qoder)\b/, "styles.css 仍残留旧平台识别色类");
  assert.match(css, /\.vicon\b/, "styles.css 缺少 .vicon 样式");
});

test("平台映射覆盖后端全部 Vendor（防止新增平台漏配 logo）", () => {
  const model = readFileSync(path.join(ROOT, "crates/ph-core/src/model.rs"), "utf8");
  const enumBody = model.match(/pub enum Vendor\s*\{([\s\S]*?)\n\}/);
  assert.ok(enumBody, "没能从 model.rs 解析出 Vendor 枚举");
  const ids = [...enumBody[1].matchAll(/#\[serde\(rename\s*=\s*"([^"]+)"\)\]/g)].map((m) => m[1]);
  assert.ok(ids.length >= 3, `Vendor 数量异常：${ids.join(",")}`);

  const assetsTs = readFileSync(path.join(UI_DIR, "src", "assets.ts"), "utf8");
  const block = assetsTs.match(/VENDOR_ICON[^{]*\{([\s\S]*?)\n\};/);
  assert.ok(block, "assets.ts 中没有 VENDOR_ICON 映射");
  const keys = [...block[1].matchAll(/"([^"]+)"\s*:/g)].map((m) => m[1]);

  for (const id of ids) {
    assert.ok(keys.includes(id), `VENDOR_ICON 缺少平台 ${id}`);
  }
});

test("用户提供的文件目录被忽略且没有任何文件入库", () => {
  const ignore = readFileSync(path.join(ROOT, ".gitignore"), "utf8");
  assert.match(ignore, /^\/用户提供的文件\/\s*$/m, ".gitignore 缺少 /用户提供的文件/");

  const r = spawnSync("git", ["ls-files", "用户提供的文件"], { cwd: ROOT, encoding: "utf8" });
  assert.equal(r.status, 0, `git ls-files 执行失败：${r.stderr}`);
  assert.equal(r.stdout.trim(), "", `用户提供的文件被误入库：\n${r.stdout}`);
});
