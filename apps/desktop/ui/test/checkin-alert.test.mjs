// 今日签到异常提示的契约测试：
// 后端退避后不再重复弹通知，界面必须留下「今天还有账号没签成功」的痕迹。
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = join(dirname(fileURLToPath(import.meta.url)), "..", "src");
const read = (name) => readFileSync(join(src, name), "utf8");

test("总览渲染今日签到异常并提供立即重试", () => {
  const app = read("App.tsx");
  assert.match(app, /checkin_alerts/);
  assert.match(app, /今日签到异常/);
  assert.match(app, /立即重试/);
});

test("状态类型包含异常列表、签到时间与重试时间", () => {
  const types = read("types.ts");
  assert.match(types, /interface CheckinAlert/);
  assert.match(types, /checkin_alerts: CheckinAlert\[\]/);
  assert.match(types, /last_checkin_at: number \| null/);
  assert.match(types, /retry_after: number \| null/);
});

test("异常提示有独立的警告样式", () => {
  const css = read("styles.css");
  assert.match(css, /\.alert-bar/);
  assert.match(css, /--warn-w/);
});
