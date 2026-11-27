/**
 * 防火墙：安装时自动放行、卸载时自动清理。
 *
 * 运行期不检测、不提示、不弹按钮；唯一一次授权发生在安装完成时。
 * 这里验证接线没有被改回「运行期打扰用户」。
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
const API = readFileSync(path.join(UI_DIR, "src", "api.ts"), "utf8");
const TYPES = readFileSync(path.join(UI_DIR, "src", "types.ts"), "utf8");
const FW = readFileSync(
  path.join(ROOT, "apps", "desktop", "src-tauri", "src", "firewall.rs"),
  "utf8"
);
const LIB = readFileSync(
  path.join(ROOT, "apps", "desktop", "src-tauri", "src", "lib.rs"),
  "utf8"
);
const HOOKS = readFileSync(
  path.join(ROOT, "apps", "desktop", "src-tauri", "windows", "hooks.nsh"),
  "utf8"
);

test("运行期不再暴露任何防火墙概念", () => {
  assert.doesNotMatch(APP, /防火墙|firewall/i);
  assert.doesNotMatch(API, /firewall/i);
  assert.doesNotMatch(TYPES, /FirewallStatus/);
});

test("安装完成后自动调用防火墙安装入口", () => {
  assert.match(HOOKS, /NSIS_HOOK_POSTINSTALL[\s\S]*--firewall-install/);
  assert.match(HOOKS, /--firewall-install --quiet/);
  assert.match(LIB, /"--firewall-install"/);
});

test("安装规则绑定当前程序、限制本地子网，并清理历史规则", () => {
  assert.match(FW, /-Program \$exe/);
  assert.match(FW, /LocalSubnet/);
  assert.match(FW, /Domain,Private,Public/);
  assert.match(FW, /Direction Inbound/);
  assert.match(FW, /Action Allow/);
  assert.match(FW, /DisplayName 'PointHub\*'/);
  assert.match(FW, /Remove-NetFirewallRule/);
});

test("已放行时直接返回，原地升级不重复弹 UAC", () => {
  assert.match(FW, /if has_rule\(\)\?/);
  assert.match(FW, /-not \$enabled/);
});

test("静默安装/卸载带 --quiet，避免在无人值守时弹 UAC", () => {
  assert.match(HOOKS, /--firewall-install --quiet/);
  assert.match(HOOKS, /--purge-data --quiet/);
  assert.match(HOOKS, /--purge-app --quiet/);
});
