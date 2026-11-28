/**
 * 趋势返工：左侧新增「趋势」页（本月每日 + 本年度 12 个月）+ 平滑曲线。
 *
 * 用户验收的五条问题逐条落成断言：
 * 1. 曲线要平滑（单调三次插值，不过冲）；
 * 2. 「月度 ›」不再开弹窗，改为切到左侧「趋势」页，页内可切月份与年份；
 * 3. 平台弹窗只留近 7 日；
 * 4. 总览「近 7 日积分变化」卡片保留，按钮改成跳转；
 * 5. 图例按图独立，不能共用一份 state。
 *
 * 这些都是「源码里看得见的结构约束」，跑不了组件就守源码：一次改动把某条
 * 约束抹掉时，这里必须红。
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
const ICONS = readFileSync(path.join(UI_DIR, "src", "icons.tsx"), "utf8");
const API = readFileSync(path.join(UI_DIR, "src", "api.ts"), "utf8");
const CSS = readFileSync(path.join(UI_DIR, "src", "styles.css"), "utf8");
const COMMANDS = readFileSync(path.join(UI_DIR, "..", "src-tauri", "src", "commands.rs"), "utf8");
const ENGINE = readFileSync(
  path.resolve(UI_DIR, "..", "..", "..", "crates", "ph-engine", "src", "lib.rs"),
  "utf8"
);

test("左侧菜单新增「趋势」一栏，且在远程模式下不出现", () => {
  assert.match(APP, /type Tab = "overview" \| "accounts" \| "trends";/, "Tab 要含 trends");
  assert.ok(ICONS.includes("export function IconTrend"), "需要一枚趋势图标");
  assert.ok(APP.includes("<IconTrend />"), "菜单项要用趋势图标");
  assert.ok(APP.includes('label="趋势"'));
  // 远程模式下 credit_* 命令会被后端拒绝，入口直接隐藏而不是点了报错
  assert.match(APP, /view\.mode !== "remote" && \(\s*<NavItem[\s\S]{0,200}label="趋势"/);
});

test("趋势页有「本月」与「本年度」两个区块，各取各的数据", () => {
  assert.ok(TRENDS.includes("本月"));
  assert.ok(TRENDS.includes("本年度"));
  assert.match(TRENDS, /\.creditDays\(scope,/, "本月每日走 creditDays");
  assert.match(TRENDS, /\.creditYearMonths\(scope, year\)/, "本年度走 creditYearMonths");
  // 两条曲线都走共用映射（唯一口径在 TrendChart.toTrendPoints）
  assert.ok(TRENDS.includes("dayTrendPoints(days)"));
  assert.ok(TRENDS.includes("monthTrendPoints(months)"));
  // 年度图必须是 12 个点：接口固定返回 12 条，标签用 1月…12月
  assert.ok(CHART.includes("`${Number(m.month.slice(5))}月`"));
  // 没数据的月/年给空状态，不画空白坐标轴
  assert.ok(TRENDS.includes("这个月还没有采样数据"));
  assert.ok(TRENDS.includes("这一年还没有采样数据"));
});

test("两张图的请求错误各记各的，不互相抹掉", () => {
  assert.ok(TRENDS.includes("const [dayError, setDayError]"));
  assert.ok(TRENDS.includes("const [monthError, setMonthError]"));
  assert.ok(!/const \[error, setError\] = useState/.test(TRENDS), "不该再共用一个 error");
  // 本月卡片只渲染 dayError，本年度卡片只渲染 monthError
  const dayCard = TRENDS.slice(TRENDS.indexOf("本月 ·"), TRENDS.indexOf("本年度 ·"));
  assert.ok(dayCard.includes("{dayError &&"), "本月区块应只显示 dayError");
  assert.ok(!dayCard.includes("monthError"), "本月区块不该显示年份请求的错误");
  const yearCard = TRENDS.slice(TRENDS.indexOf("本年度 ·"));
  assert.ok(yearCard.includes("{monthError &&"), "本年度区块应只显示 monthError");
  assert.ok(!yearCard.includes("dayError"), "本年度区块不该显示月份请求的错误");
});

test("远程模式配对成功时把用户从趋势页送回总览（不留白屏）", () => {
  assert.match(
    APP,
    /useEffect\(\(\) => \{\s*if \(view\?\.mode === "remote" && tab === "trends"\) setTab\("overview"\);\s*\}, \[view\?\.mode, tab\]\);/,
    "缺这条兜底，停在趋势页时配对成功会只剩空白主区"
  );
});

test("月份/年份切换器：不越过今天，越界时按钮 disabled", () => {
  assert.ok(TRENDS.includes('aria-label="上一个月"'));
  assert.ok(TRENDS.includes('aria-label="下一个月"'));
  assert.ok(TRENDS.includes('aria-label="上一年"'));
  assert.ok(TRENDS.includes('aria-label="下一年"'));
  assert.ok(TRENDS.includes("disabled={!canNextMonth}"), "已到当前月时「下一个」应禁用");
  assert.ok(TRENDS.includes("disabled={!canNextYear}"), "已到今年时「下一年」应禁用");
  assert.match(TRENDS, /const atCurMonth = cursor\.year === curYear && cursor\.month === curMonth;/);
  // stepMonth/stepYear 自己也要兜底，不能只靠按钮 disabled
  assert.match(TRENDS, /nextYear > curYear \|\| \(nextYear === curYear && nextMonth > curMonth\)/);
  assert.match(CSS, /\.seg-item:disabled\s*\{[^}]*opacity/);
});

test("图例按图独立：趋势页两次 useTrendVisible，不共用一份 state", () => {
  const calls = TRENDS.match(/useTrendVisible\(\)/g) ?? [];
  assert.ok(calls.length >= 2, `趋势页要两次独立调用，实际 ${calls.length} 次`);
  assert.ok(TRENDS.includes("const [monthVisible, toggleMonth] = useTrendVisible();"));
  assert.ok(TRENDS.includes("const [yearVisible, toggleYear] = useTrendVisible();"));
  // 两张图各用各的变量，绝不能拿同一个 visible 喂两张图
  assert.ok(TRENDS.includes("visible={monthVisible}"));
  assert.ok(TRENDS.includes("visible={yearVisible}"));
  // 趋势页不从父组件接 visible（接了就又变成共用一份）
  assert.ok(!/visible\??\s*:\s*TrendKey\[\]/.test(TRENDS), "趋势页不该有 visible 入参");
  // 总览卡片、平台弹窗各自也是独立一份
  assert.ok((APP.match(/useTrendVisible\(\)/g) ?? []).length >= 1);
  assert.ok(MODAL.includes("useTrendVisible()"));
  assert.ok(!/visible=\{trend\.visible\}/.test(APP));
});

test("平台弹窗里不再有月度/年度两级的任何残留", () => {
  assert.ok(!MODAL.includes("creditYearMonths"), "弹窗不该再取月度汇总");
  assert.ok(!MODAL.includes("openMonth"), "弹窗不该再有「进入某月」状态");
  assert.ok(!MODAL.includes("返回月度"), "弹窗不该再有二级返回");
  assert.ok(!MODAL.includes("monthDays"));
  assert.ok(!MODAL.includes("mlist"));
  // 只保留近 7 日 + 去趋势页的入口
  assert.ok(MODAL.includes("近 7 日"));
  assert.ok(MODAL.includes("查看本月与本年度"));
});

test("总览卡片按钮触发切页，而不是开弹窗", () => {
  assert.match(APP, /const openTrends = useCallback\(\(vendor: string \| null\) => \{\s*setTrendScope\(vendor\);\s*setTab\("trends"\);/);
  assert.ok(APP.includes("onOpenMonths={view.mode === \"remote\" ? undefined : () => onOpenTrends(null)}"));
  // 弹窗底部按钮同样走这条路径（先关弹窗再切页）
  assert.match(APP, /onOpenTrends=\{\(\) => \{[\s\S]{0,160}setModalScope\(null\);[\s\S]{0,80}onOpenTrends\(vendor\);/);
  // 趋势页平台筛选受控：从弹窗进来只显示该平台
  assert.ok(APP.includes("scope={trendScope}"));
  assert.ok(APP.includes("onScope={setTrendScope}"));
});

test("平滑曲线的接线：贝塞尔、双条件免过冲、按空值切段", () => {
  // 真正的「不过冲」由 smooth.test.mjs 按采样点数值验证；这里只守接线没被拆掉。
  assert.ok(CHART.includes("export function smoothPath("), "平滑函数要在，且可被审查");
  assert.match(CHART, /out \+= ` C\$/, "相邻点之间用三次贝塞尔的 C 段");
  // 免过冲的两个条件：切向量与斜率同号 + 圆盘限幅
  assert.match(CHART, /if \(a < 0 \|\| b < 0\)/, "异号切线必须归零，否则拐点处必然过冲");
  assert.ok(CHART.includes("s > 9"), "缺少 s > 9 的单调性限幅，曲线会在 0 附近探头");
  assert.ok(CHART.includes("const t = 3 / Math.sqrt(s);"));
  // 平坦段切线归零，避免平台段鼓包
  assert.match(CHART, /if \(d\[i\] === 0\) \{[\s\S]{0,120}m\[i\] = 0;\s*m\[i \+ 1\] = 0;/);
  // 空值按连续段切开，各自平滑（跨空档平滑会把「没数据」画成斜线）
  assert.ok(CHART.includes("export function segmentsOf("), "断线切段逻辑要可被审查");
  assert.match(CHART, /segmentsOf\(values\)[\s\S]{0,200}smoothPath\(/s, "line() 要按段平滑");
  // 只有一个有效点时保持 ±9px 横线；两个点时直线即可
  assert.ok(CHART.includes("pts.length === 1"));
  assert.ok(CHART.includes("pts.length === 2"));
  assert.ok(!/drawn\.map\(\(pt, i\) => `\$\{i === 0 \? "M" : "L"\}/.test(CHART), "旧的折线直连应已删除");
});

test("缺数据的口径只有一处：三张图共用 toTrendPoints", () => {
  // 只判 total 会让「新增」线从今天一直平拖到月末（后端每天 up 恒为 0）
  assert.match(CHART, /export function toTrendPoints\(/);
  assert.match(CHART, /up: r\.has \? r\.up : null/, "up 也要跟着 has 断线");
  assert.match(CHART, /down: r\.has \? r\.down : null/, "down 也要跟着 has 断线");
  assert.ok(CHART.includes("export function dayTrendPoints("));
  assert.ok(CHART.includes("export function monthTrendPoints("));
  // 三个调用点都不许再各写一份
  for (const [name, src] of [
    ["App.tsx 总览卡片", APP],
    ["TrendsScreen 本月", TRENDS],
    ["TrendModal 近 7 日", MODAL],
  ]) {
    assert.ok(
      src.includes("dayTrendPoints("),
      `${name} 应调用 dayTrendPoints，而不是自己 map 一份`
    );
    assert.ok(
      !/total: d\.known > 0 \? d\.total : null/.test(src),
      `${name} 里还留着局部的断线规则`
    );
  }
});

test("后端按年取 12 个月：固定 12 条、空月占位、删掉旧口径", () => {
  assert.ok(ENGINE.includes("pub fn monthly_series_of_year(&self, vendor: Option<Vendor>, year: i32) -> Vec<MonthPoint>"));
  assert.ok(!ENGINE.includes("pub fn monthly_series("), "旧的两套月口径已删除");
  assert.match(ENGINE, /\(1\.\.=12\)/);
  assert.ok(COMMANDS.includes("pub async fn credit_year_months("));
  assert.ok(!COMMANDS.includes("credit_months("), "旧命令已删除");
  assert.ok(COMMANDS.includes("年份应落在 2000–2100 之间"));
  assert.match(API, /creditYearMonths: \(vendor: string \| null, year: number\)/);
  assert.ok(!API.includes("creditMonths"));
});
