/**
 * 积分单位校验：「分」字必须彻底退场，所有数值后面都跟着积分图标。
 *
 * 覆盖三处界面：主窗口（App.tsx）、悬浮球（Ball.tsx）、手机页（lan.html）。
 * 不校验观感，只保证「位置齐全、色调齐全、没有漏网的文字单位」。
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const UI_DIR = path.resolve(HERE, "..");
const ROOT = path.resolve(HERE, "../../../..");
const APP = readFileSync(path.join(UI_DIR, "src", "App.tsx"), "utf8");
const BALL = readFileSync(path.join(UI_DIR, "src", "Ball.tsx"), "utf8");
const CSS = readFileSync(path.join(UI_DIR, "src", "styles.css"), "utf8");
const LAN = readFileSync(
  path.join(ROOT, "apps", "desktop", "src-tauri", "assets", "lan.html"),
  "utf8"
);

test("主窗口不再出现「分」这个单位", () => {
  assert.doesNotMatch(APP, /<small>分<\/small>/, "账号行还在写「分」");
  assert.doesNotMatch(APP, />\s*分\s*</, "App.tsx 里还有把「分」当单位显示的片段");
});

test("积分图标覆盖主窗口所有数值位置（合计/新增/小计/余额/活动/趋势）", () => {
  const count = (APP.match(/<PointsIcon\b/g) ?? []).length;
  assert.ok(count >= 7, `App.tsx 中 PointsIcon 只有 ${count} 处，至少应有 7 处`);

  for (const tone of ["bronze", "jade", "ink"]) {
    assert.match(APP, new RegExp(`tone="${tone}"`), `App.tsx 缺少 ${tone} 色调的积分图标`);
  }
  assert.match(CSS, /\.pts\b/, "styles.css 缺少 .pts 基础样式");
});

test("悬浮球数值跟随语义色调，且没有文字单位", () => {
  assert.ok((BALL.match(/<PointsIcon\b/g) ?? []).length >= 3, "悬浮球的合计/小计/余额都应带积分图标");
  assert.match(BALL, /tone=\{account \? "jade" : "bronze"\}/, "悬浮球主数值应在合计青铜 / 账号玉之间切换");
  assert.doesNotMatch(BALL, />\s*分\s*</, "悬浮球仍在显示「分」");
});

test("手机页 lan.html 内联积分图标，不依赖额外请求", () => {
  assert.doesNotMatch(LAN, /__PT_/, "lan.html 还有未替换的图标占位符");
  const uris = LAN.match(/data:image\/png;base64,/g) ?? [];
  assert.ok(uris.length >= 3, `lan.html 只内联了 ${uris.length} 个积分图标，应为 3 个色调`);
  assert.match(LAN, /\.pt\.b\s*\{/, "lan.html 缺少青铜色调类");
  assert.match(LAN, /\.pt\.i\s*\{/, "lan.html 缺少墨色色调类");
  assert.match(LAN, /\.pt\.j\s*\{/, "lan.html 缺少玉色色调类");
  assert.match(LAN, /PT\.bronze/, "手机页合计未使用积分图标");
  assert.match(LAN, /PT\.jade/, "手机页余额未使用积分图标");
  assert.doesNotMatch(LAN, />\s*分\s*</, "手机页仍在显示「分」");
});
