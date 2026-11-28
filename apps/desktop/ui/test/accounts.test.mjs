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

test("账号页按平台分组，顺序跟能力表", () => {
  assert.ok(APP.includes('className="acct-group"'), "缺少分组容器");
  assert.ok(APP.includes('className="acct-group-head"'), "缺少分组标题");
  assert.ok(APP.includes('className="acct-group-name"'));
  assert.match(APP, /const order = view\.platforms\.map\(\(p\) => p\.vendor\);/);
});

test("账号名可点开就地改名，Enter 提交、Esc 取消", () => {
  assert.ok(APP.includes('className="acct-name"'), "名字应渲染成可点按钮");
  assert.ok(APP.includes('className="acct-name-input"'), "缺少就地编辑输入框");
  assert.ok(APP.includes("api.renameAccount(a.id, next)"));
  assert.ok(APP.includes("cancelRename.current = true"), "Esc 应能取消提交");
  assert.ok(API.includes('invoke<string>("rename_account", { id, label })'));
  assert.match(CSS, /\.acct-name-input \{/);
});

test("账号名下方只显示导入来源与签到时间，指纹移到悬停", () => {
  assert.ok(APP.includes("importSourceText(a.source)"));
  assert.ok(
    !APP.includes("{a.hint} · {a.schedule_text}"),
    "副标题还在直接显示令牌指纹",
  );
  assert.ok(APP.includes("令牌指纹 ${a.hint}"), "悬停提示应说明指纹是什么、干什么用");
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
