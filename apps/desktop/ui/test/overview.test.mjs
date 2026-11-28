/**
 * G2：总览 KPI 改造（含「今日变化」拆分为新增 / 消耗）。
 *
 * 断言的是「口径」而不是像素：
 * - 五张卡的顺序与文案按确认方案固定，防止后续改动又把合并卡塞回来；
 * - 积分总额卡不再用进度条，改用「已签 x/y · 最近采样」小字；
 * - 今日新增涨绿、今日消耗跌红，且消耗用 ruby 图标（不是灰化的绿图标）；
 * - 本月领取必须来自后端聚合（前端自己累加 events 会和电脑端口径打架）。
 */
import assert from "node:assert/strict";
import { existsSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const UI_DIR = path.resolve(HERE, "..");
const APP = readFileSync(path.join(UI_DIR, "src", "App.tsx"), "utf8");
const CSS = readFileSync(path.join(UI_DIR, "src", "styles.css"), "utf8");
const TYPES = readFileSync(path.join(UI_DIR, "src", "types.ts"), "utf8");
const ASSETS = readFileSync(path.join(UI_DIR, "src", "assets.ts"), "utf8");
const COMMANDS = readFileSync(
  path.resolve(UI_DIR, "..", "src-tauri", "src", "commands.rs"),
  "utf8",
);

test("KPI 顺序为 积分总额 → 今日新增 → 今日消耗 → 本月领取总积分 → 今日已签", () => {
  const labels = ["积分总额", "今日新增", "今日消耗", "本月领取总积分", "今日已签"];
  const positions = labels.map((label) => {
    const i = APP.indexOf(`<div className="klabel">${label}</div>`);
    assert.ok(i > 0, `缺少 KPI 卡：${label}`);
    return i;
  });
  const sorted = [...positions].sort((a, b) => a - b);
  assert.deepEqual(positions, sorted, "KPI 顺序与确认方案不一致");
});

test("旧的合并卡与历史文案不再出现在卡片标题里", () => {
  for (const old of ["今日变化", "积分合计", "下一次自动"]) {
    assert.ok(
      !APP.includes(`<div className="klabel">${old}</div>`),
      `仍有旧 KPI 标题：${old}`,
    );
  }
});

test("五张卡一行放得下，且窄窗规则仍在", () => {
  assert.match(CSS, /grid-template-columns: repeat\(5, minmax\(0, 1fr\)\);/);
  assert.match(CSS, /@media \(max-width: 980px\)[\s\S]*?\.kpis \{\s*grid-template-columns: repeat\(2,/);
});

test("积分总额卡用文字小字替代进度条", () => {
  assert.ok(!APP.includes('className="prog"'), "仍在渲染进度条");
  assert.ok(!/^\.prog\b/m.test(CSS), "样式里仍保留 .prog 规则");
  assert.match(APP, /已签 \$\{done\}\/\$\{enabled\.length\}/);
});

test("今日新增涨绿、今日消耗跌红，消耗显示负数并用 ruby 图标", () => {
  assert.ok(APP.includes('className={`kval ${upPositive ? "up" : ""}`}'));
  assert.ok(APP.includes('className={`kval ${downPositive ? "down" : ""}`}'));
  assert.ok(APP.includes('{downPositive ? "-" : ""}'), "消耗应带负号");
  assert.ok(APP.includes("credits(downPositive ? (todayDown ?? 0) : 0)"));
  assert.ok(APP.includes('tone={downPositive ? "ruby" : "ink"}'));
  assert.ok(APP.includes('tone={upPositive ? "jade" : "ink"}'));
  assert.match(CSS, /\.kval\.up \{\s*color: var\(--accent\);/);
  assert.match(CSS, /\.kval\.down \{\s*color: var\(--danger\);/);
});

test("正负号定宽，两张卡的数字起点对齐", () => {
  // Segoe UI 26px 下「+」≈17.6px、「-」≈9.9px；不定宽会让数字起点差 7.6px。
  assert.ok(APP.includes('<span className="ksign">{upPositive ? "+" : ""}</span>'));
  assert.ok(APP.includes('<span className="ksign">{downPositive ? "-" : ""}</span>'));
  assert.match(
    CSS,
    /\.kval \.ksign \{[\s\S]*?min-width:\s*0\.7em;[\s\S]*?text-align:\s*center;/,
  );
});

test("图标不再用滤镜把绿色染灰", () => {
  assert.ok(!CSS.includes(".kval.down .pts"), "仍在使用灰度滤镜方案");
});

test("两张卡的小字说明与新口径一致：新增看签到流水，消耗含当天已用掉的", () => {
  assert.ok(
    APP.includes('<div className="kfoot">签到实际到账</div>'),
    "今日新增的小字应说明来自签到到账",
  );
  assert.ok(
    APP.includes('<div className="kfoot">含当天已用掉的</div>'),
    "今日消耗的小字应说明包含当天已用掉的部分",
  );
  assert.ok(
    !APP.includes('<div className="kfoot">较昨日余额</div>'),
    "旧的「较昨日余额」口径说明应已移除",
  );
});

test("红色积分图标已登记且资源存在", () => {
  assert.ok(ASSETS.includes('import pointsRuby from "./assets/points/points-ruby.png";'));
  assert.ok(ASSETS.includes("ruby: pointsRuby,"), "POINTS_ICON 缺少 ruby 映射");
  assert.ok(ASSETS.includes('"bronze" | "ink" | "jade" | "ruby"'), "PointsTone 未包含 ruby");
  const png = path.join(UI_DIR, "src", "assets", "points", "points-ruby.png");
  assert.ok(existsSync(png), "points-ruby.png 不存在");
  assert.ok(statSync(png).size > 1000, "points-ruby.png 体积异常，可能是空文件");
});

test("新增/消耗来自后端拆分，不在前端自行累加", () => {
  assert.match(TYPES, /interface DayPoint \{[\s\S]*up: number \| null;[\s\S]*down: number \| null;/);
  assert.ok(APP.includes("todayPoint?.up ?? null"));
  assert.ok(APP.includes("todayPoint?.down ?? null"));
  assert.ok(
    !/events[^\n]*filter\([^\n]*kind === "checkin"/.test(APP),
    "前端不应自己从 events 汇总",
  );
});

test("最近活动：卡片只留 4 条，「更多」弹窗复用同一行", () => {
  assert.ok(APP.includes("events.slice(0, 4)"), "卡片应只取前 4 条");
  assert.ok(APP.includes("events.length > list.length"), "超过 4 条时才显示「更多」");
  assert.match(APP, /className="btitle-btn"[\s\S]{0,60}?更多/, "缺少「更多」入口");
  assert.ok(APP.includes('className="modal-ev"'), "弹窗应使用加宽样式");
  assert.ok(APP.includes('className="ev-list"'), "弹窗列表需要独立滚动容器");
  assert.match(CSS, /\.ev-list \{[\s\S]*?max-height:[\s\S]*?overflow-y: auto;/);
  assert.match(CSS, /\.modal-ev \{\s*width: min\(520px, 100%\);/);
  // 后端一次多取一些流水，弹窗才能看到比卡片更多的记录。
  assert.match(COMMANDS, /recent_events\(48\)/);
});

test("最近活动始终显示平台标签，用于区分同名账号", () => {
  assert.match(ASSETS, /export const VENDOR_NAME/);
  assert.ok(ASSETS.includes('"workbuddy": "WorkBuddy"'));
  assert.ok(ASSETS.includes('"trae-cn": "Trae"'));
  assert.ok(ASSETS.includes('"qoder-cn": "Qoder"'));
  assert.ok(APP.includes("VENDOR_NAME[e.vendor]"), "活动行应显示平台名");
  assert.ok(APP.includes('className="ev-vendor"'));
  assert.match(CSS, /\.ev-vendor \{[\s\S]*?flex: none;/);
});
