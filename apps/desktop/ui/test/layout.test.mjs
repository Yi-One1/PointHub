/**
 * G4：账号页细节（删除图标居中、授权步骤、平台选择器、滚动条抖动）。
 *
 * 这几项都是「看得见的 1px / 一次换行」问题，靠肉眼验收容易反复，
 * 所以把几何口径固化成断言：
 * - 图标容器必须清零内边距，否则 UA 的 6px 会把 15px 图标挤偏 1.5px；
 * - 平台选择器必须是等宽换行网格，不能再是固定 3 格的滑轨；
 * - main 必须为滚动条留位，否则切换导入方式时两栏宽度会跳 10px。
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const UI_DIR = path.resolve(HERE, "..");
const APP = readFileSync(path.join(UI_DIR, "src", "App.tsx"), "utf8");
const CSS = readFileSync(path.join(UI_DIR, "src", "styles.css"), "utf8");

/// 取出某个选择器的规则体（第一个匹配）
function rule(selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return CSS.match(new RegExp(escaped + "\\s*\\{[^}]*\\}"))?.[0] ?? "";
}

test("删除图标在悬停方框正中：容器清零内边距并居中", () => {
  const btn = rule(".icon-btn");
  assert.match(btn, /place-items:\s*center;/, "删除按钮应网格居中");
  // UA 默认 padding 是 6px，26px 按钮的内容盒只剩 12px，15px 图标会被挤偏。
  assert.match(btn, /padding:\s*0;/, "必须清零 UA 默认内边距，否则图标偏右 1.5px");
  assert.match(btn, /line-height:\s*0;/, "行高会带来基线偏移，需归零");
});

test("平台选择器等宽换行，不再靠固定 3 格滑轨", () => {
  assert.ok(APP.includes('className="seg seg-platforms"'), "平台选择器应使用 seg-platforms");
  const grid = rule(".seg-platforms");
  assert.match(grid, /display:\s*grid;/, "应为网格布局，平台变多时整行换行");
  assert.match(grid, /repeat\(auto-fit,\s*minmax\(\d+px,\s*1fr\)\)/, "每行放几个由可用宽度决定");
  // 平台名过长时截断而不是撑破单元格，且能通过 title 看全名。
  assert.ok(APP.includes('<span className="seg-label">{p.display}</span>'));
  assert.ok(APP.includes("title={p.display}"));
  assert.match(rule(".seg-label"), /text-overflow:\s*ellipsis;/);
});

test("浏览器授权步骤写成正式的三步", () => {
  assert.ok(APP.includes("<strong>打开授权页</strong>"));
  assert.ok(APP.includes("<strong>完成授权</strong>"));
  assert.ok(APP.includes("<strong>确认结果</strong>"));
  // 旧的随意口吻不应残留
  assert.ok(!APP.includes("你<strong>平时用的浏览器</strong>里新开一个标签页"));
});

test("主内容区为滚动条留位，切换导入方式时宽度不跳", () => {
  assert.match(rule("main"), /scrollbar-gutter:\s*stable;/, "缺 scrollbar-gutter: stable 会抖 10px");
});
