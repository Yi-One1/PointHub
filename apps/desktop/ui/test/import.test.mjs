/**
 * G1：WorkBuddy 5.6+ 本机导入与设置页深链。
 *
 * 这些断言防的是“界面看起来改了、后端却没接上”：
 * - 扫描结果必须带 needs_key，界面才会把 allowKeyFetch 传到后端；
 * - 三个平台的本机导入面板共用同一份步骤说明与按钮，不会各写一套；
 * - 凭据说明链接必须落到 data 分区并定位数据目录。
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
const SCREEN = readFileSync(path.join(UI_DIR, "src", "SettingsScreen.tsx"), "utf8");
const COMMANDS = readFileSync(
  path.resolve(UI_DIR, "..", "src-tauri", "src", "commands.rs"),
  "utf8",
);
const LOCAL_IMPORT = readFileSync(
  path.resolve(UI_DIR, "..", "..", "..", "crates", "ph-providers", "src", "local_import.rs"),
  "utf8",
);

test("本机导入状态包含 needs_key 字段", () => {
  assert.match(TYPES, /interface LocalImportStatus \{[\s\S]*needs_key: boolean;/);
});

test("扫描与导入命令都把 needs_key / allow_key_fetch 接上", () => {
  assert.match(COMMANDS, /pub needs_key: bool/);
  assert.match(COMMANDS, /needs_key: s\.needs_key/);
  assert.match(COMMANDS, /allow_key_fetch: Option<bool>/);
  assert.match(COMMANDS, /local_import::import\(v, allow_key_fetch\.unwrap_or\(false\)\)/);
});

test("WorkBuddy 5.x 登录态优先走共享 auth 文件", () => {
  assert.match(LOCAL_IMPORT, /wb_auth::auth_file_path\(\)/);
  assert.match(LOCAL_IMPORT, /workbuddy_v5_credential/);
  assert.match(LOCAL_IMPORT, /Error::NeedsKey/);
});

test("需要读取密钥时仍显示同样的步骤与导入按钮", () => {
  // 共用同一份步骤说明：可用与“需授权”走同一条渲染路径，面板结构不会各写一套。
  const shared = APP.match(/function LocalImportSteps\(\)[\s\S]*?\n}\n/);
  assert.ok(shared, "应存在共用的 LocalImportSteps 组件");
  assert.equal((shared[0].match(/<li>/g) ?? []).length, 4, "步骤说明应有 4 条");
  assert.equal(
    (APP.match(/<LocalImportSteps \/>/g) ?? []).length,
    1,
    "步骤说明只渲染一处，三个平台结构一致",
  );

  assert.match(APP, /localStatus\.available \|\| localStatus\.needs_key/);
  assert.match(APP, /startLocalImport\(Boolean\(localStatus\.needs_key\)\)/);
  assert.equal(
    (APP.match(/从本机导入 \$\{cap\?\.display \?\? vendor\}/g) ?? []).length,
    1,
    "可用与需授权共用同一个导入按钮",
  );
  assert.doesNotMatch(APP, /允许一次并导入/);
});

test("需要读取密钥时用普通提示，不再降级成警告块", () => {
  assert.match(APP, /\?\s*"hint"\s*:\s*"hint warn"/);
  // “手动令牌”已由上方导入方式标签提供，面板里不再重复一个链接入口。
  assert.doesNotMatch(APP, /改用手动令牌/);
  assert.match(API, /allowKeyFetch = false/);
  assert.match(API, /invoke<string>\("local_import", \{ vendor, allowKeyFetch \}\)/);
});

test("凭据说明链接落到数据与安全并定位数据目录", () => {
  assert.match(APP, /onOpenSettings\("data"\)/);
  assert.match(APP, /凭据存在哪？去「设置 → 数据与安全 → 数据位置」查看/);
  assert.match(SCREEN, /initialAnchor\?: "data-location"/);
  assert.match(SCREEN, /id="settings-data-location"/);
  assert.match(SCREEN, /scrollIntoView/);
});
