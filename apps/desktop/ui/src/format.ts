/// 积分展示：最多两位小数，整数不带小数点（`204.62` / `3,600`）。
///
/// 上游会给出 `204.62000029` 这种字符串小数，直接显示会带一串噪声。
/// 控制台、图表、弹窗共用这一份，避免三处各自取整导致数字对不上。
export function credits(n: number): string {
  return n.toLocaleString("zh-CN", { maximumFractionDigits: 2 });
}
