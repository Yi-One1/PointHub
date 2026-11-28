/**
 * G2：总览 KPI 改造。
 *
 * 断言的是「口径」而不是像素：
 * - 四张卡的顺序与文案按确认方案固定，防止后续改动又把「下一次自动」塞回来；
 * - 积分总额卡不再用进度条，改用「已签 x/y · 最近采样」小字；
 * - 今日变化必须涨绿跌红，0 不染色；
 * - 本月领取必须来自后端聚合（前端自己累加 events 会和电脑端口径打架）。
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
const TYPES = readFileSync(path.join(UI_DIR, "src", "types.ts"), "utf8");

test("KPI 顺序为 积分总额 → 今日变化 → 本月领取总积分 → 今日已签", () => {
  const labels = ["积分总额", "今日变化", "本月领取总积分", "今日已签"];
  const positions = labels.map((label) => {
    const i = APP.indexOf(`<div className="klabel">${label}</div>`);
    assert.ok(i > 0, `缺少 KPI 卡：${label}`);
    return i;
  });
  const sorted = [...positions].sort((a, b) => a - b);
  assert.deepEqual(positions, sorted, "KPI 顺序与确认方案不一致");
});

test("旧 KPI 文案不再出现在卡片标题里", () => {
  for (const old of ["积分合计", "今日新增", "下一次自动"]) {
    assert.ok(
      !APP.includes(`<div className="klabel">${old}</div>`),
      `仍有旧 KPI 标题：${old}`,
    );
  }
});

test("积分总额卡用文字小字替代进度条", () => {
  assert.ok(!APP.includes('className="prog"'), "仍在渲染进度条");
  assert.ok(!/^\.prog\b/m.test(CSS), "样式里仍保留 .prog 规则");
  assert.match(APP, /已签 \$\{done\}\/\$\{enabled\.length\}/);
});

test("今日变化涨绿跌红，0 保持中性", () => {
  assert.match(
    APP,
    /const gainTone = todayGain === null \|\| todayGain === 0 \? "" : todayGain > 0 \? "up" : "down";/,
  );
  assert.match(CSS, /\.kval\.up \{\s*color: var\(--accent\);/);
  assert.match(CSS, /\.kval\.down \{\s*color: var\(--danger\);/);
});

test("本月领取总积分来自后端聚合，不在前端自行累加", () => {
  assert.match(TYPES, /interface Aggregate \{[\s\S]*claimed_this_month: number \| null;/);
  assert.match(APP, /view\.aggregate\.claimed_this_month/);
  assert.ok(
    !/events[^\n]*filter\([^\n]*kind === "checkin"/.test(APP),
    "前端不应自己从 events 汇总领取积分",
  );
});
