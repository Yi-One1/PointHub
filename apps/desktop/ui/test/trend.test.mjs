/**
 * G5：近 7 日积分变化曲线 + 平台趋势弹窗。
 *
 * 守的是四件事：
 * - 三条曲线（总额/新增/消耗）都在，且各有独立取值域（总额与增减量纲差 10 倍以上）；
 * - 图例是按钮，能逐条隐藏/显示，且不允许把三条全关掉；
 * - 缺数据的点断线，不补 0；
 * - 平台弹窗只保留近 7 日——本月每日与本年度 12 个月归左侧「趋势」页
 *   （趋势页自身的断言见 trends.test.mjs）。
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const UI_DIR = path.resolve(HERE, "..");
const APP = readFileSync(path.join(UI_DIR, "src", "App.tsx"), "utf8");
const CHART = readFileSync(path.join(UI_DIR, "src", "TrendChart.tsx"), "utf8");
const MODAL = readFileSync(path.join(UI_DIR, "src", "TrendModal.tsx"), "utf8");
const TRENDS = readFileSync(path.join(UI_DIR, "src", "TrendsScreen.tsx"), "utf8");
const API = readFileSync(path.join(UI_DIR, "src", "api.ts"), "utf8");
const CSS = readFileSync(path.join(UI_DIR, "src", "styles.css"), "utf8");
const TAURI = readFileSync(path.join(UI_DIR, "..", "src-tauri", "src", "lib.rs"), "utf8");

test("卡片改名为「近 7 日积分变化」，旧的柱状图退场", () => {
  assert.ok(APP.includes("近 7 日积分变化"));
  assert.ok(!APP.includes("近 7 日每日新增"), "旧标题应已替换");
  assert.ok(!APP.includes('className="bars"'), "旧的柱状图结构应已删除");
});

test("三条曲线各有取值域：总额走左轴，增减共用右轴", () => {
  assert.ok(CHART.includes('total: "积分总额"'));
  assert.ok(CHART.includes('up: "新增"'));
  assert.ok(CHART.includes('down: "消耗"'));
  // 总额与增减量纲差一个数量级，混用同一轴会把增减压成贴底直线
  assert.ok(CHART.includes("yTotal") && CHART.includes("yFlow"));
  assert.match(CSS, /--chart-total:\s*#[0-9a-f]/);
});

test("图例就是开关：可逐条隐藏，但至少留一条", () => {
  assert.ok(CHART.includes("aria-pressed={on}"));
  assert.ok(CHART.includes("useTrendVisible"));
  assert.match(CHART, /prev\.length === 1 \? prev : prev\.filter/);
  assert.match(CSS, /\.tlegend-item\.off/);
});

test("缺数据断线，不补 0", () => {
  // 补 0 会让曲线凭空掉到底，看起来像「积分被清空」。
  // 规则只在 TrendChart 里写一次（toTrendPoints），三张图共用。
  assert.match(CHART, /total: r\.has \? r\.total : null/);
  assert.match(CHART, /up: r\.has \? r\.up : null/);
  assert.match(CHART, /down: r\.has \? r\.down : null/);
  assert.ok(APP.includes("dayTrendPoints(trend)"), "总览卡片走共用映射");
  assert.ok(MODAL.includes("dayTrendPoints(days)"), "平台弹窗走共用映射");
  assert.ok(TRENDS.includes("dayTrendPoints(days)"), "趋势页本月走共用映射");
});

test("平台弹窗只保留近 7 日，月度/年度下钻已迁到趋势页", () => {
  assert.ok(MODAL.includes("近 7 日"));
  assert.ok(MODAL.includes("api.creditDays(vendor, ymd(from), 7)"));
  // 两级下钻（月汇总 → 月内每日）整体搬走，弹窗里不该再出现
  assert.ok(!MODAL.includes("creditYearMonths"), "弹窗不再取月度");
  assert.ok(!MODAL.includes("openMonth"), "弹窗不再有「进入某月」状态");
  assert.ok(!MODAL.includes("返回月度"), "弹窗不再有两级返回");
  assert.ok(!MODAL.includes("mlist"), "月度列表已删除");
  // 底部保留去趋势页的入口
  assert.ok(MODAL.includes("onOpenTrends"));
  assert.ok(MODAL.includes("查看本月与本年度"));
});

test("每个平台面板都能打开自己的趋势，总览卡片跳到趋势页", () => {
  assert.ok(APP.includes('className="phead-btn"'));
  assert.ok(APP.includes("setModalScope({ vendor: g.vendor, display: g.display })"));
  // 总览卡片的「月度 ›」不再开弹窗，而是切到趋势页
  assert.ok(APP.includes("setTab(\"trends\")"));
  assert.ok(APP.includes("onOpenMonths={view.mode === \"remote\" ? undefined : () => onOpenTrends(null)}"));
  // 远程模式下数据在电脑上，本地引擎查不到，入口直接隐藏而不是报错
  assert.ok(APP.includes('view.mode !== "remote"'));
});

test("趋势命令已接通：前后端同名、已注册", () => {
  assert.ok(API.includes('invoke<DayPoint[]>("credit_days"'));
  assert.ok(API.includes('invoke<MonthPoint[]>("credit_year_months"'));
  assert.ok(TAURI.includes("commands::credit_days"));
  assert.ok(TAURI.includes("commands::credit_year_months"));
  assert.ok(!TAURI.includes("commands::credit_months,"), "旧命令不应再注册");
});
