/**
 * G3：账号身份与分组。
 *
 * 守的是三件事：
 * - 账号页按平台分组，顺序跟能力表，而不是按导入先后；
 * - 账号名可以点开就地改，改过之后重导不被平台昵称覆盖（后端另有用例）；
 * - 令牌指纹不再摆在账号名下面，只留在悬停提示里。
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const UI_DIR = path.resolve(HERE, "..");
const APP = readFileSync(path.join(UI_DIR, "src", "App.tsx"), "utf8");
const TYPES = readFileSync(path.join(UI_DIR, "src", "types.ts"), "utf8");
const API = readFileSync(path.join(UI_DIR, "src", "api.ts"), "utf8");
const CSS = readFileSync(path.join(UI_DIR, "src", "styles.css"), "utf8");

test("账号页每个平台一张卡，与总览页同构", () => {
  assert.match(
    APP,
    /groups\.map\(\(g\) => \(\s*<section className="panel" key=\{g\.vendor\}>/,
    "每个平台应各自是一张卡",
  );
  assert.ok(!APP.includes('className="acct-group"'), "平台不该再挤在同一张卡里分组");
  assert.ok(APP.includes('{g.accounts.length} 账号'), "平台卡头应显示该平台账号数");
  assert.match(APP, /const order = view\.platforms\.map\(\(p\) => p\.vendor\);/);
});

test("「已纳管」标题与数量下移到页面标题右侧", () => {
  assert.ok(!APP.includes("已纳管"), "「已纳管」已按反馈删除");
  assert.ok(APP.includes('<span className="head-count">{view.accounts.length} 个</span>'));
  assert.match(CSS, /\.head-count \{[^}]*font-variant-numeric: tabular-nums;/);
});

test("账号名可点开就地改名，Enter 提交、Esc 取消", () => {
  assert.ok(APP.includes('className="acct-name"'), "名字应渲染成可点按钮");
  assert.ok(APP.includes('className="acct-name-input"'), "缺少就地编辑输入框");
  assert.ok(APP.includes("api.renameAccount(a.id, next)"));
  assert.ok(APP.includes("cancelRename.current = true"), "Esc 应能取消提交");
  assert.ok(API.includes('invoke<string>("rename_account", { id, label })'));
  assert.match(CSS, /\.acct-name-input \{/);
});

test("账号页与总览页的副标题都只显示来源与签到时间", () => {
  // 两处（账号行、总览平台行）必须同一口径，否则会出现「一边改了另一边没改」。
  const subs = APP.match(/\[importSourceText\(a\.source\), a\.schedule_text\]/g) ?? [];
  assert.equal(subs.length, 2, `副标题口径只落地了 ${subs.length} 处，应为 2 处`);
  assert.ok(!/^\s*\{a\.hint\}\s*$/m.test(APP), "仍有地方把令牌指纹当成正文渲染");
  assert.ok(APP.includes("令牌指纹 ${a.hint}"), "悬停提示应说明指纹是什么、干什么用");
  assert.equal(
    (APP.match(/title=\{accountMetaTitle\(a\)\}/g) ?? []).length,
    2,
    "两页应共用同一份悬停文案",
  );
});

test("导入来源有中文文案", () => {
  assert.ok(APP.includes('return "本机导入";'));
  assert.ok(APP.includes('return "浏览器授权";'));
  assert.ok(APP.includes('return "手动令牌";'));
});

test("账号视图带上昵称、自定义标记与来源", () => {
  assert.match(TYPES, /nickname: string \| null;/);
  assert.match(TYPES, /label_custom: boolean;/);
  assert.match(TYPES, /source: "local" \| "browser" \| "token" \| null;/);
});

test("账号副标题与页面其他小字同一字体，不再单独用等宽字体", () => {
  const rule = CSS.match(/\.rhint\s*\{([\s\S]*?)\}/);
  assert.ok(rule, "缺少 .rhint 规则");
  assert.doesNotMatch(
    rule[1],
    /font-family/,
    "副标题不应再单独指定等宽字体：中文会回退到另一套字体，11px 下显糊",
  );
  assert.match(rule[1], /font-variant-numeric: tabular-nums;/, "时间数字仍需等宽对齐");
});
