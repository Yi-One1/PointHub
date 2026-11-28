/**
 * 每个平台可设置自动签到时间 + 同平台多账号错峰（用户第 6 条需求）。
 *
 * 三条已裁决的产品决定落成断言：
 * 1. 编辑入口两处都有：设置页「签到与通知」分区 + 总览每个平台面板头的跳转按钮；
 * 2. 浮动窗口每平台可调（0 = 不浮动，默认 5 分钟）；
 * 3. 不开放单账号改时间——界面只按平台展示，账号时刻由后端算好后只读展示。
 *
 * 这里守的是「源码里看得见的结构约束」：改坏任何一处，这些断言必须变红。
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const UI_DIR = path.resolve(HERE, "..");
const APP = readFileSync(path.join(UI_DIR, "src", "App.tsx"), "utf8");
const SCREEN = readFileSync(path.join(UI_DIR, "src", "SettingsScreen.tsx"), "utf8");
const SETTINGS = readFileSync(path.join(UI_DIR, "src", "settings.ts"), "utf8");
const TYPES = readFileSync(path.join(UI_DIR, "src", "types.ts"), "utf8");
const CSS = readFileSync(path.join(UI_DIR, "src", "styles.css"), "utf8");
const COMMANDS = readFileSync(
  path.resolve(UI_DIR, "..", "src-tauri", "src", "commands.rs"),
  "utf8",
);
const SCHEDULE_RS = readFileSync(
  path.resolve(UI_DIR, "..", "..", "..", "crates", "ph-core", "src", "schedule.rs"),
  "utf8",
);
const ENGINE_RS = readFileSync(
  path.resolve(UI_DIR, "..", "..", "..", "crates", "ph-engine", "src", "lib.rs"),
  "utf8",
);

test("platform_schedules 归属于「签到与通知」分区", () => {
  assert.match(SETTINGS, /platform_schedules: "checkin"/);
  // 字段归属表的键必须与类型里的 Settings 字段同名，漏填时 TS 会先红。
  assert.match(TYPES, /export interface Settings \{[\s\S]*?platform_schedules: PlatformSchedule\[\];/);
});

test("types.ts 有 PlatformSchedule 结构，账号视图带准确的 HH:MM", () => {
  const block = TYPES.match(/export interface PlatformSchedule \{([\s\S]*?)\}/);
  assert.ok(block, "types.ts 里应有 PlatformSchedule 接口");
  for (const field of ["vendor", "hour", "minute", "spread_minutes"]) {
    assert.match(block[1], new RegExp(`\\b${field}:`), `PlatformSchedule 缺字段 ${field}`);
  }
  assert.match(TYPES, /schedule_hhmm: string;/, "AccountView 需要 schedule_hhmm");
});

test("设置页按平台逐行列出签到时间与浮动窗口", () => {
  assert.match(SCREEN, /settings-group">各平台签到时间</);
  assert.match(SCREEN, /platform_schedules\.map/);
  // 时刻用原生时间控件、窗口用数字框，且窗口限制在 0~30。
  assert.match(SCREEN, /type="time"/);
  assert.match(SCREEN, /type="number"/);
  assert.match(SCREEN, /min=\{0\}/);
  assert.match(SCREEN, /max=\{30\}/);
  // 预览用后端算好的时刻，而不是前端自己再算一遍错峰。
  assert.match(SCREEN, /schedule_hhmm/);
  assert.match(SCREEN, /还没有账号/);
  assert.match(SCREEN, /个账号/);
  // 预览是「今天实际时刻」，不是平台基准：账号在窗口里各占一段、段内每天浮动。
  assert.match(SCREEN, /今天 \$\{accounts\[0\]\.schedule_hhmm\}/);
  assert.match(SCREEN, /个账号 · 今天 \$\{accounts/);
  // 改动走即改即存，且只替换该平台那一行。
  assert.match(SCREEN, /platform_schedules\.map\(\(x\) =>/);
  // 窗口按账号数分段、段内再按日浮动；单账号拿整段窗口。
  assert.match(SCREEN, /浮动窗口由同平台账号平分/);
  assert.match(SCREEN, /每天在自己那段里浮动/);
  assert.match(SCREEN, /只有一个账号时整段窗口都归它/);
  // 窗口比账号数还窄时分钟不够分，界面必须如实提示，不能承诺「不会撞」。
  assert.match(SCREEN, /accounts\.length >= p\.spread_minutes \+ 1/);
  assert.match(SCREEN, /账号数达到「窗口 \+ 1」/);
  assert.match(SCREEN, /窗口够宽时多账号不会撞在同一分钟/);
});

test("客户端模式下不给无效控件，改成一句说明", () => {
  // 客户端模式的签到由电脑执行，本机改时间只写本机设置、对电脑调度无效；
  // 与其摆一个「看着能改」的控件，不如直接说明去哪儿改。
  const at = SCREEN.indexOf('draft.mode === "remote" ?');
  assert.ok(at > 0, "缺少客户端模式分支");
  assert.match(SCREEN, /客户端模式下签到由电脑执行/);
  const branch = SCREEN.slice(at, SCREEN.indexOf(") : (", at));
  assert.ok(!branch.includes('type="time"'), "客户端模式分支里不该出现时间控件");
  assert.ok(!branch.includes("platform_schedules.map"), "客户端模式分支不该渲染平台行");
});

test("设置页不提供单账号改时间", () => {
  // 全页只有一个时刻控件，且它长在「按平台遍历」里面；账号列表只用来拼预览文本。
  const inputs = SCREEN.match(/type="time"/g) ?? [];
  assert.equal(inputs.length, 1, `时刻控件只该有一个，实际 ${inputs.length} 个`);
  assert.ok(
    SCREEN.indexOf('type="time"') > SCREEN.indexOf("platform_schedules"),
    "时刻控件必须挂在平台行里，不能出现在账号行",
  );
  // 单个账号的时间只用于展示（预览拼接），不能有 onChange 写回。
  assert.doesNotMatch(SCREEN, /accounts\[0\]\.schedule_hhmm\s*=/);
});

test("总览平台面板头有「签到时间」，点了跳到设置的 checkin 分区", () => {
  assert.match(APP, />\s*签到时间\s*<\/button>/, "平台面板头需要有「签到时间」按钮");
  assert.match(APP, /onOpenCheckinSettings/, "按钮要回调到 App 层");
  assert.match(
    APP,
    /title=\{`设置 \$\{g\.display\} 的自动签到时间`\}/,
    "按钮标题应说明是这个平台的时间",
  );
  // 回调里必须打开设置并停在「签到与通知」。
  const wiring = APP.match(/onOpenCheckinSettings=\{([\s\S]*?)\}\n/);
  assert.ok(wiring, "App 里应当把 onOpenCheckinSettings 接到总览上");
  assert.match(APP, /onOpenCheckinSettings=\{\(\) => \{[\s\S]*?setSettingsSection\("checkin"\)[\s\S]*?setSettingsOpen\(true\)/);
});

test("后端给出每个账号的准确时刻，远程态也从描述里取", () => {
  assert.match(COMMANDS, /pub schedule_hhmm: String,/);
  assert.match(
    COMMANDS,
    /schedule_hhmm: a\s*\.schedule\s*\.slot_on\(/,
    "本机账号的预览时刻要按 slot_on 取今天实际落点，而不是直接拼基准时分",
  );
  assert.match(COMMANDS, /format\("%H:%M"\)/);
  assert.match(COMMANDS, /fn hhmm_from_text/);
  // 两处构造点都要补：本机 + 远程。
  const fills = COMMANDS.match(/schedule_hhmm[,:]/g) ?? [];
  assert.ok(fills.length >= 3, "AccountView 的两处构造点都要填 schedule_hhmm");
});

test("错峰算法在 ph-core 里，且由引擎按平台重算", () => {
  assert.match(SCHEDULE_RS, /pub fn cell_bounds\(index: usize, total: usize, spread_minutes: u8\) -> \(u8, u8\)/);
  assert.match(SCHEDULE_RS, /pub fn from_platform\(/);
  assert.match(ENGINE_RS, /pub fn reconcile_schedules\(&mut self\) -> Result<usize>/);
  // 新增账号会改变平台账号总数，必须重算。
  assert.match(ENGINE_RS, /self\.reconcile_schedules\(\)\?/);
  // 删账号同样改变总数：3 个删到 2 个时剩下两个要重新铺满窗口，
  // 否则预览会出现「2 个账号 · 00:10 / 00:13」这种与规则矛盾的组合。
  const remove = ENGINE_RS.slice(
    ENGINE_RS.indexOf("pub fn remove_account"),
    ENGINE_RS.indexOf("pub fn set_account_enabled"),
  );
  assert.ok(remove.length > 0, "取不到 remove_account，函数可能被改名");
  assert.match(remove, /reconcile_schedules/, "删账号后必须重算错峰");
});

test("新增样式类都已落地（.psched 系列）", () => {
  assert.match(CSS, /\.psched\s*\{/);
  assert.match(CSS, /\.psched-col\s*\{/);
  assert.match(CSS, /\.psched-input\s*\{/);
  assert.match(CSS, /\.psched-num\s*\{/);
});

test("时间控件的时钟图标用矢量图，非整数缩放不发糊", () => {
  // 原生 ::-webkit-calendar-picker-indicator 是位图，125% / 150% 缩放下会重采样。
  const rule = CSS.match(
    /\.psched-input::-webkit-calendar-picker-indicator\s*\{([\s\S]*?)\}/,
  );
  assert.ok(rule, "缺少时间控件的时钟图标规则");
  assert.match(rule[1], /background:\s*var\(--clock-icon\)/, "图标应使用 --clock-icon 变量");
  assert.match(rule[1], /14px 14px/, "图标尺寸必须固定为整数像素，避免重采样");
  // 浅色 / 深色各一份，深色下不能继续沿用浅色描边。
  const light = CSS.match(/:root\s*\{([\s\S]*?)\}/);
  const dark = CSS.match(/html\[data-theme="dark"\]\s*\{([\s\S]*?)\}/);
  assert.ok(light && dark, "找不到主题变量块");
  assert.match(light[1], /--clock-icon:/, "浅色主题缺少时钟图标");
  assert.match(dark[1], /--clock-icon:/, "深色主题缺少时钟图标");
  // 自绘图标之后仍是原生时间控件，点击才打得开系统时间选择器。
  assert.match(SCREEN, /type="time"/);
});
