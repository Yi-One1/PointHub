/**
 * 设置页：分类固定、字段归属唯一、入口在左下角齿轮。
 *
 * 这些是「以后新增设置项也不跑偏」的合同：新增字段时 TS 会强制补归属，
 * 这里再验证至少 7 个分类、字段无遗漏、侧栏不再有「设置」导航项。
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const UI_DIR = path.resolve(HERE, "..");
const SETTINGS = readFileSync(path.join(UI_DIR, "src", "settings.ts"), "utf8");
const TYPES = readFileSync(path.join(UI_DIR, "src", "types.ts"), "utf8");
const APP = readFileSync(path.join(UI_DIR, "src", "App.tsx"), "utf8");
const SCREEN = readFileSync(path.join(UI_DIR, "src", "SettingsScreen.tsx"), "utf8");
const ICONS = readFileSync(path.join(UI_DIR, "src", "icons.tsx"), "utf8");
const CSS = readFileSync(path.join(UI_DIR, "src", "styles.css"), "utf8");
const API = readFileSync(path.join(UI_DIR, "src", "api.ts"), "utf8");
const COMMANDS = readFileSync(
  path.resolve(UI_DIR, "..", "src-tauri", "src", "commands.rs"),
  "utf8",
);
const LIB = readFileSync(
  path.resolve(UI_DIR, "..", "src-tauri", "src", "lib.rs"),
  "utf8",
);
const LAN = readFileSync(path.resolve(UI_DIR, "..", "src-tauri", "src", "lan.rs"), "utf8");
const PREVIEW = readFileSync(
  path.resolve(UI_DIR, "..", "..", "..", "scripts", "preview.ps1"),
  "utf8",
);
const STORE = readFileSync(
  path.resolve(UI_DIR, "..", "..", "..", "crates", "ph-store", "src", "lib.rs"),
  "utf8",
);

function sectionIds() {
  return [...SETTINGS.matchAll(/\{ id: "([^"]+)", label: "([^"]+)"/g)].map((m) => m[1]);
}

test("设置分类固定为 7 类且顺序稳定", () => {
  assert.deepEqual(sectionIds(), [
    "general",
    "appearance",
    "checkin",
    "phone",
    "update",
    "data",
    "about",
  ]);
});

test("每个可保存设置字段都有且只有一个分类归属", () => {
  const settingsBlock = TYPES.match(/export interface Settings \{([\s\S]*?)\}/);
  assert.ok(settingsBlock, "types.ts 里应当有 Settings 接口");
  const fieldKeys = [...settingsBlock[1].matchAll(/^\s{2}(\w+):/gm)].map((m) => m[1]);
  assert.ok(fieldKeys.length >= 10, "Settings 字段数量异常");

  const mapBlock = SETTINGS.match(/SETTINGS_FIELD_SECTIONS[^{]*\{([\s\S]*?)\n\};/);
  assert.ok(mapBlock, "settings.ts 里应当有字段归属表");
  const entries = [...mapBlock[1].matchAll(/^\s{2}(\w+): "([^"]+)"/gm)];
  const mapped = entries.map((m) => m[1]);

  assert.deepEqual(
    [...mapped].sort(),
    [...fieldKeys].sort(),
    "字段归属表必须与 Settings 字段一一对应，不能漏也不能多",
  );
  assert.equal(new Set(mapped).size, mapped.length, "同一个字段不能归属两个分类");

  const ids = new Set(sectionIds());
  for (const [, key, section] of entries) {
    assert.ok(ids.has(section), `${key} 归属到了不存在的分类 ${section}`);
  }
});

test("设置入口改为左下角齿轮，左侧导航不再有「设置」项", () => {
  assert.doesNotMatch(APP, /tab === "settings"/, "不应再保留「设置」标签页");
  assert.doesNotMatch(APP, /setTab\("settings"\)/, "不应再通过标签页切换打开设置");
  assert.match(APP, /rail-settings/, "侧栏底部应有设置齿轮");
  assert.match(APP, /aria-label="设置"/, "齿轮需要有可访问名称");
  assert.match(APP, /<SettingsScreen/, "点击齿轮应打开独立设置页");
});

test("设置页是独立页面：返回应用 + 左侧分类 + 右侧内容", () => {
  assert.match(SCREEN, /settings-screen/);
  assert.match(SCREEN, /返回应用/);
  assert.match(SCREEN, /settings-nav/);
  assert.match(SCREEN, /settings-body/);
});

test("设置页与主页面共用同一个侧栏宽度", () => {
  assert.match(CSS, /--rail-w:\s*212px/);
  assert.match(CSS, /\.app\s*\{[\s\S]*?grid-template-columns:\s*var\(--rail-w\)/);
  assert.match(CSS, /\.settings-main\s*\{[\s\S]*?grid-template-columns:\s*var\(--rail-w\)/);
});

test("菜单悬停有阴影反馈，齿轮上方有分隔线", () => {
  assert.match(CSS, /\.nav:hover\s*\{[\s\S]*?box-shadow:\s*var\(--shadow-hover\)/);
  assert.match(
    CSS,
    /\.settings-nav-item:hover\s*\{[\s\S]*?box-shadow:\s*var\(--shadow-hover\)/,
  );
  assert.match(CSS, /\.rail-settings\s*\{[\s\S]*?border-top-color:\s*var\(--line\)/);
});

test("所有菜单项都有图标", () => {
  for (const name of [
    "IconOverview",
    "IconAccounts",
    "IconGear",
    "IconGeneral",
    "IconAppearance",
    "IconCheckin",
    "IconPhone",
    "IconUpdate",
    "IconData",
    "IconAbout",
  ]) {
    assert.match(ICONS, new RegExp(`export function ${name}\\(`), `缺少 ${name}`);
  }
  assert.match(APP, /<IconOverview \/>/);
  assert.match(APP, /<IconAccounts \/>/);
  assert.match(APP, /<IconGear \/>/);
  assert.match(SCREEN, /SECTION_ICONS/);
});

test("设置页顶部有搜索框，且按关键词筛选分类", () => {
  assert.match(SCREEN, /placeholder="搜索设置…"/);
  assert.match(SCREEN, /settings-search/);
  assert.match(SCREEN, /visibleSections/);
  assert.match(SETTINGS, /keywords:/);
});

test("分区标题在卡片外，主页面左下角信息已移除", () => {
  assert.match(SCREEN, /className="settings-group"/);
  assert.doesNotMatch(SCREEN, /className="phead"/);
  assert.match(
    CSS,
    /\.settings-group\s*\{[\s\S]*?padding:\s*0 17px;[\s\S]*?color:\s*var\(--text\)/,
    "分区标题应与卡片内文字左对齐，并使用正文黑色",
  );
  assert.doesNotMatch(APP, /className="afoot"/);
  assert.match(APP, /className="bver"/);
  assert.match(SCREEN, /数据只在本机/);
});

test("设置改动即时保存，不再有全局保存按钮或状态文字", () => {
  assert.match(SCREEN, /const apply = async/);
  assert.match(SCREEN, /onSave\(next, true\)/, "即时保存应静默，避免每次改动都弹通知");
  assert.doesNotMatch(SCREEN, /改动立即生效|已自动保存|settings-save-state/);
  assert.doesNotMatch(SCREEN, /先保存|再点检查|保存后/);
});

test("镜像地址并入来源小字，不新增行", () => {
  assert.doesNotMatch(SCREEN, /镜像前缀/);
  assert.match(SCREEN, /addr-link/);
  assert.match(SCREEN, /https:\/\/github\.com\/Yi-One1\/PointHub\/releases\/latest/);
  assert.match(SCREEN, /patchMirror/);
});

test("系统通知合并为一个开关", () => {
  assert.match(SCREEN, /notify_on_checkin: v, notify_on_failure: v/);
  assert.doesNotMatch(SCREEN, /成功 \{draft\.notify_on_checkin/);
  assert.doesNotMatch(SCREEN, /失败 \{draft\.notify_on_failure/);
});

test("关于页有作者、仓库、反馈入口与许可，不再放退出按钮", () => {
  const about = SCREEN.slice(SCREEN.indexOf('settings-group">关于</h2>'));
  assert.ok(about.includes("毅One"), "关于页缺少作者名");
  assert.ok(about.includes("GitHub 仓库") && about.includes("Yi-One1/PointHub"));
  assert.ok(about.includes("问题反馈") && about.includes('openProjectLink("issues")'));
  assert.ok(about.includes('openProjectLink("repo")'), "仓库入口应可打开");
  assert.ok(about.includes("MIT License"), "关于页缺少许可说明");
  assert.ok(!about.includes("退出程序"), "关于页不应再有「退出程序」按钮");
  assert.ok(!SCREEN.includes("api.quit"), "设置页不应再调用 quit");
  // 后端只放行仓库与 Issues 两个固定地址，不接受任意 URL。
  assert.match(API, /openProjectLink: \(link: "repo" \| "issues"\)/);
  assert.match(COMMANDS, /pub async fn open_project_link/);
  assert.match(COMMANDS, /"repo" => "https:\/\/github\.com\/Yi-One1\/PointHub"/);
  assert.match(COMMANDS, /"issues" => "https:\/\/github\.com\/Yi-One1\/PointHub\/issues"/);
  assert.match(LIB, /commands::open_project_link/);
});

test("数据目录合并为一行，用系统选择文件夹", () => {
  assert.doesNotMatch(SCREEN, /row-title">更换数据目录</);
  assert.match(SCREEN, /pickDataDir/);
  assert.match(SCREEN, /dir-picker/);
  assert.match(SCREEN, /清理旧目录/);
  assert.match(API, /pickDataDir/);
  assert.match(COMMANDS, /pub async fn pick_data_dir/);
  assert.match(COMMANDS, /write_cleanup_marker/);
});

test("预览版数据目录在软件路径而非 C 盘，且与正式版位置记录隔离", () => {
  assert.match(PREVIEW, /POINTHUB_DEFAULT_DATA_DIR/);
  assert.match(PREVIEW, /dist\\data/);
  assert.match(PREVIEW, /POINTHUB_POINTER_DIR/);
  assert.match(STORE, /POINTHUB_POINTER_DIR/);
});

test("镜像有默认地址", () => {
  assert.match(SCREEN, /https:\/\/ghfast\.top\//);
});

test("检查到新版本时原地变「更新」，总览提示可跳转", () => {
  // 设置页：查到新版本后同一个按钮变成「更新」，不再单开一行「下载并安装」。
  assert.match(SCREEN, /update\?\.latest \? \(/);
  assert.match(SCREEN, /applying \? "更新中…" : "更新"/);
  assert.doesNotMatch(SCREEN, /下载并安装/);
  // 检查结果提升到 App 层，总览与设置共用。
  assert.match(APP, /updateVersion/);
  assert.match(APP, /有新版本 v\{updateVersion\} 可更新/);
  assert.match(APP, /setSettingsSection\("update"\)/);
  assert.match(APP, /initialSection/);
  assert.match(APP, /api\.updateCheck\(\)/);
  assert.match(APP, /settings\.update\.enabled/);
});

test("设置页去掉重复解释，只保留必要状态", () => {
  for (const text of [
    "写入当前用户启动项",
    "常驻桌面；单击",
    "按每个账号的平台刷新时间",
    "签到结束弹一条",
    "关掉后手机只能看账本",
    "发现新版本只弹通知",
    "在电脑端「设置 → 手机访问」里能看到地址",
  ]) {
    assert.doesNotMatch(SCREEN, new RegExp(text), `过期说明仍在：${text}`);
  }
});

test("每个分区可恢复默认，默认值来自后端", () => {
  assert.match(SCREEN, /恢复默认设置/);
  assert.match(SCREEN, /SETTINGS_FIELD_SECTIONS/);
  assert.match(API, /defaultSettings/);
  assert.match(COMMANDS, /pub fn default_settings\(\)/);
});

test("恢复默认只在当前分区与默认值不同时可用", () => {
  // 与默认值一致时按钮灰色不可点，有改动才可用。
  assert.match(SCREEN, /deepEqual/);
  assert.match(SCREEN, /sectionDiffers/);
  assert.match(SCREEN, /disabled=\{!canReset\}/);
  assert.match(SCREEN, /当前已是默认设置/);
  // 数据与安全页把数据目录也纳入「恢复默认」。
  assert.match(SCREEN, /dirDiffers/);
  assert.match(SCREEN, /恢复默认数据目录/);
  assert.match(API, /defaultDataDir/);
  assert.match(API, /resetDataDir/);
  assert.match(COMMANDS, /pub async fn reset_data_dir/);
  assert.match(COMMANDS, /pub fn default_data_dir/);
});

test("重要设置修改前有二次确认", () => {
  assert.match(SCREEN, /<Modal/);
  for (const text of [
    "关闭「最小化到托盘」",
    "关闭定时自动签到",
    "关闭错过补签",
    "关闭局域网访问",
    "关闭手机签到",
    "更换数据目录",
  ]) {
    assert.match(SCREEN, new RegExp(text), `缺少「${text}」的确认文案`);
  }
});

test("局域网开关即时生效，不再提示重启", () => {
  assert.match(SCREEN, /打开开关后立即生效/);
  assert.doesNotMatch(SCREEN, /局域网访问已关闭，打开开关并重启应用后生效/);
  assert.match(COMMANDS, /ctx\.stop\(\)/);
  assert.match(LAN, /shutdown/);
});
