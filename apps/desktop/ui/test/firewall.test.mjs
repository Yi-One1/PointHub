/**
 * 防火墙放行：验证「按需出现」的接线与规则边界。
 *
 * 不校验系统实际状态（那要管理员权限），只保证：
 * 命令注册、提示出现条件、规则参数、静默卸载参数没有被改坏。
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
const HOOKS = readFileSync(
  path.join(ROOT, "apps", "desktop", "src-tauri", "windows", "hooks.nsh"),
  "utf8"
);

test("防火墙提示只在局域网已启动且确实未放行时出现", () => {
  assert.match(APP, /lan\?\.running/);
  assert.match(APP, /firewall\?\.state === "missing"/);
  assert.match(APP, /Windows 防火墙拦住了手机访问端口/);
  // 不再常驻设置页
  assert.doesNotMatch(APP, /<div className="row-title">防火墙放行<\/div>/);
});

test("界面只保留放行入口，不暴露清理等系统概念", () => {
  assert.match(APP, /\.firewallStatus\(\)/);
  assert.match(APP, /\.firewallAllow\(\)/);
  assert.doesNotMatch(APP, /firewallCleanup/);
  assert.match(API, /invoke<FirewallStatus>\("firewall_status"\)/);
  assert.match(API, /invoke<FirewallStatus>\("firewall_allow"\)/);
  assert.doesNotMatch(API, /firewall_cleanup/);
  assert.match(TYPES, /interface FirewallStatus/);
});

test("放行规则只开当前端口、限制本地子网，并顺手清理旧规则", () => {
  assert.match(FW, /localport=\{port\}/);
  assert.match(FW, /remoteip=localsubnet/);
  assert.match(FW, /profile=domain,private,public/);
  assert.match(FW, /dir=in action=allow/);
  assert.match(FW, /-eq \$own -and \$enabled/, "必须保留本次创建的规则");
  assert.match(FW, /-ieq \$exe/, "必须保留当前运行的 exe");
  assert.match(FW, /DisplayName 'PointHub\*'/);
});

test("静默卸载带 --quiet，避免在无人值守时弹 UAC", () => {
  assert.match(HOOKS, /--purge-data --quiet/);
  assert.match(HOOKS, /--purge-app --quiet/);
});
