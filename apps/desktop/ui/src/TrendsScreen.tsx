/**
 * 统计页：上半「本月每日」、下半「本年度 12 个月」两张曲线图。
 *
 * 为什么单独一页而不塞进总览：整月 31 天 + 整年 12 个月是两个完整坐标系，
 * 挤在卡片里只能缩成装饰。总览卡片继续承担「近 7 日一眼看变化」，要看细的
 * 走这里。这一页是专门读数的，字号与图高整体比卡片大一档（见 `.trends-page`）。
 *
 * 三条约束：
 * - 平台作用域受控（`scope` 由 App 持有）：从总览「月度 ›」跳进来是「全部平台」，
 *   从某个平台面板的弹窗跳进来就只看该平台。
 * - 两张图各有一份图例 state，互不影响。
 * - 月份/年份切换都可能连点，旧响应不许覆盖新选择（见两个 seq 计数器）。
 */
import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import {
  TrendChart,
  TrendLegend,
  dayTrendPoints,
  monthTrendPoints,
  useTrendVisible,
} from "./TrendChart";
import { VendorIcon } from "./VendorIcon";
import type { DayPoint, MonthPoint, StateView } from "./types";

/// 与后端 `credit_year_months` 的年份校验保持一致。
const MIN_YEAR = 2000;

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

/// `2026-11`
function monthKey(year: number, month: number): string {
  return `${year}-${pad2(month)}`;
}

/// 某年某月的天数（下个月第 0 天 = 本月最后一天，含闰年）。
function daysInMonth(year: number, month: number): number {
  return new Date(year, month, 0).getDate();
}

export function TrendsScreen({
  view,
  scope,
  onScope,
}: {
  view: StateView;
  /** null = 全部平台 */
  scope: string | null;
  onScope: (v: string | null) => void;
}) {
  // 每次渲染重算「今天」：跨零点后切换器的禁用态与上界会自动跟着变。
  const today = new Date();
  const curYear = today.getFullYear();
  const curMonth = today.getMonth() + 1;

  const [cursor, setCursor] = useState({ year: curYear, month: curMonth });
  const [year, setYear] = useState(curYear);
  const [days, setDays] = useState<DayPoint[] | null>(null);
  const [months, setMonths] = useState<MonthPoint[] | null>(null);
  // 两张图的错误各记一份：切月份成功不该顺手抹掉上一年的失败提示，反之亦然。
  const [dayError, setDayError] = useState<string | null>(null);
  const [monthError, setMonthError] = useState<string | null>(null);

  // 两张图各一份图例 state。合并成一份正是本次返工要修的 bug（两张图连动）。
  const [monthVisible, toggleMonth] = useTrendVisible();
  const [yearVisible, toggleYear] = useTrendVisible();

  // 请求序号：连点月份/年份时只认最后一次的结果，先发的旧响应直接丢掉。
  const daySeq = useRef(0);
  const monthSeq = useRef(0);

  useEffect(() => {
    const seq = ++daySeq.current;
    const key = monthKey(cursor.year, cursor.month);
    setDays(null);
    setDayError(null);
    api
      .creditDays(scope, `${key}-01`, daysInMonth(cursor.year, cursor.month))
      .then((rows) => {
        if (seq !== daySeq.current) return;
        setDays(rows);
        setDayError(null);
      })
      .catch((e) => {
        if (seq === daySeq.current) setDayError(String(e));
      });
  }, [scope, cursor.year, cursor.month]);

  useEffect(() => {
    const seq = ++monthSeq.current;
    setMonths(null);
    setMonthError(null);
    api
      .creditYearMonths(scope, year)
      .then((rows) => {
        if (seq !== monthSeq.current) return;
        setMonths(rows);
        setMonthError(null);
      })
      .catch((e) => {
        if (seq === monthSeq.current) setMonthError(String(e));
      });
  }, [scope, year]);

  const atCurMonth = cursor.year === curYear && cursor.month === curMonth;
  const canPrevMonth = !(cursor.year <= MIN_YEAR && cursor.month === 1);
  const canNextMonth = !atCurMonth;
  const canPrevYear = year > MIN_YEAR;
  const canNextYear = year < curYear;

  const stepMonth = (delta: number) => {
    setCursor((prev) => {
      const index = prev.year * 12 + (prev.month - 1) + delta;
      const nextYear = Math.floor(index / 12);
      const nextMonth = (index % 12) + 1;
      // 越界时不改状态：按钮已经 disabled，这里只是兜底。
      if (nextYear < MIN_YEAR) return prev;
      if (nextYear > curYear || (nextYear === curYear && nextMonth > curMonth)) return prev;
      return { year: nextYear, month: nextMonth };
    });
  };

  const stepYear = (delta: number) => {
    setYear((prev) => {
      const next = prev + delta;
      if (next < MIN_YEAR || next > curYear) return prev;
      return next;
    });
  };

  const dPoints = days === null ? [] : dayTrendPoints(days);
  const mPoints = months === null ? [] : monthTrendPoints(months);
  const dHasAny = (days ?? []).some((d) => d.known > 0);
  const mHasAny = (months ?? []).some((m) => m.days > 0);

  const scopeDisplay =
    scope === null
      ? "全部平台"
      : (view.platforms.find((p) => p.vendor === scope)?.display ?? scope);

  return (
    <div className="trends-page">
      <div className="head">
        <div>
          <h1>统计</h1>
          <p className="sub">本月每日与本年度每月的积分变化；切换平台只看该平台。</p>
        </div>
      </div>

      <div className="field">
        <label>平台</label>
        <div className="seg seg-platforms">
          <button
            className={`seg-item ${scope === null ? "active" : ""}`}
            onClick={() => onScope(null)}
            title="全部平台"
          >
            <span className="seg-label">全部平台</span>
          </button>
          {view.platforms.map((p) => (
            <button
              key={p.vendor}
              className={`seg-item ${scope === p.vendor ? "active" : ""}`}
              onClick={() => onScope(p.vendor)}
              title={p.display}
            >
              <VendorIcon vendor={p.vendor} size={14} />
              <span className="seg-label">{p.display}</span>
            </button>
          ))}
        </div>
      </div>

      <section className="bcard">
        <div className="tsec">
          <div className="tsec-head">
            <h4>本月 · {scopeDisplay}</h4>
            <div className="seg">
              <button
                className="seg-item"
                onClick={() => stepMonth(-1)}
                disabled={!canPrevMonth}
                aria-label="上一个月"
                title="上一个月"
              >
                ‹
              </button>
              <span className="seg-item active">{monthKey(cursor.year, cursor.month)}</span>
              <button
                className="seg-item"
                onClick={() => stepMonth(1)}
                disabled={!canNextMonth}
                aria-label="下一个月"
                title={canNextMonth ? "下一个月" : "已经是最新的一个月"}
              >
                ›
              </button>
            </div>
          </div>
          <TrendLegend visible={monthVisible} onToggle={toggleMonth} />
          {dayError && <p className="hint warn">{dayError}</p>}
          {days === null ? (
            <p className="panel-empty">正在读取…</p>
          ) : !dHasAny ? (
            <p className="panel-empty">这个月还没有采样数据</p>
          ) : (
            <TrendChart points={dPoints} visible={monthVisible} height={250} />
          )}
        </div>
      </section>

      <section className="bcard" style={{ marginTop: 12 }}>
        <div className="tsec">
          <div className="tsec-head">
            <h4>本年度 · {scopeDisplay}</h4>
            <div className="seg">
              <button
                className="seg-item"
                onClick={() => stepYear(-1)}
                disabled={!canPrevYear}
                aria-label="上一年"
                title="上一年"
              >
                ‹
              </button>
              <span className="seg-item active">{year} 年</span>
              <button
                className="seg-item"
                onClick={() => stepYear(1)}
                disabled={!canNextYear}
                aria-label="下一年"
                title={canNextYear ? "下一年" : "已经是最新的一年"}
              >
                ›
              </button>
            </div>
          </div>
          <TrendLegend visible={yearVisible} onToggle={toggleYear} />
          {monthError && <p className="hint warn">{monthError}</p>}
          {months === null ? (
            <p className="panel-empty">正在读取…</p>
          ) : !mHasAny ? (
            <p className="panel-empty">这一年还没有采样数据</p>
          ) : (
            // labelAll：12 个月必须逐点标注，横轴缺哪个月都得看得见
            <TrendChart points={mPoints} visible={yearVisible} height={230} labelAll />
          )}
        </div>
      </section>
    </div>
  );
}
