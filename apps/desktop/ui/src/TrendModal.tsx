/**
 * 积分变化详情弹窗：近 7 日 → 近 12 个月 → 某个月的每日。
 *
 * 数据都从后端按需取（`credit_days` / `credit_months`），不进 bootstrap：
 * 一年 365 个点跟着每次刷新一起下发，代价比打开弹窗时取一次大得多。
 */
import { useEffect, useState } from "react";
import { api } from "./api";
import type { DayPoint, MonthPoint } from "./types";
import { credits } from "./format";
import { TrendChart, TrendLegend, useTrendVisible, type TrendPoint } from "./TrendChart";

/// 本地日期 → `YYYY-MM-DD`（后端只认这个格式）。
function ymd(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

function dayPoints(rows: DayPoint[]): TrendPoint[] {
  return rows.map((d) => ({
    label: d.date,
    // 当天没有采样时断线：补 0 会让曲线凭空掉到底。
    total: d.known > 0 ? d.total : null,
    up: d.up,
    down: d.down,
  }));
}

function monthPoints(rows: MonthPoint[]): TrendPoint[] {
  return rows.map((m) => ({
    label: `${Number(m.month.slice(5))}月`,
    total: m.days > 0 ? m.total : null,
    up: m.up,
    down: m.down,
  }));
}

export function TrendModal({
  vendor,
  display,
  onClose,
}: {
  /** null 表示全部平台 */
  vendor: string | null;
  display: string;
  onClose: () => void;
}) {
  const [days, setDays] = useState<DayPoint[] | null>(null);
  const [months, setMonths] = useState<MonthPoint[] | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [monthDays, setMonthDays] = useState<DayPoint[] | null>(null);
  const [visible, toggle] = useTrendVisible();
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const from = new Date();
    from.setDate(from.getDate() - 6);
    void (async () => {
      try {
        const [m, d] = await Promise.all([
          api.creditMonths(vendor, 12),
          api.creditDays(vendor, ymd(from), 7),
        ]);
        if (!alive) return;
        setMonths(m);
        setDays(d);
      } catch (e) {
        if (alive) setError(String(e));
      }
    })();
    return () => {
      alive = false;
    };
  }, [vendor]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // Esc 先退回月度，再关弹窗，避免一下退出两层
        if (open) setOpen(null);
        else onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  const openMonth = (month: string) => {
    setOpen(month);
    setMonthDays(null);
    const [y, m] = month.split("-").map(Number);
    const length = new Date(y, m, 0).getDate();
    void api
      .creditDays(vendor, `${month}-01`, length)
      .then(setMonthDays)
      .catch((e) => setError(String(e)));
  };

  const dPoints = days ? dayPoints(days) : [];
  const mPoints = months ? monthPoints(months) : [];

  return (
    <div
      className="modal-mask"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="modal trend-modal" role="dialog" aria-modal="true" aria-label={`积分变化 ${display}`}>
        <div className="modal-head">
          <h3>积分变化 · {display}</h3>
          <button className="icon-btn" onClick={onClose} title="关闭" aria-label="关闭">
            ✕
          </button>
        </div>

        <div className="trend-body">
          {error && <p className="hint warn">{error}</p>}

          {open === null ? (
            <>
              <section className="tsec">
                <div className="tsec-head">
                  <h4>近 7 日</h4>
                  <TrendLegend visible={visible} onToggle={toggle} />
                </div>
                {days === null ? (
                  <p className="panel-empty">正在读取…</p>
                ) : dPoints.length === 0 ? (
                  <p className="panel-empty">还没有采样数据</p>
                ) : (
                  <TrendChart points={dPoints} visible={visible} />
                )}
              </section>

              <section className="tsec">
                <div className="tsec-head">
                  <h4>按月（近 12 个月）</h4>
                  <TrendLegend visible={visible} onToggle={toggle} />
                </div>
                {months === null ? (
                  <p className="panel-empty">正在读取…</p>
                ) : mPoints.length === 0 ? (
                  <p className="panel-empty">还没有足够的历史数据</p>
                ) : (
                  <>
                    <TrendChart points={mPoints} visible={visible} height={132} />
                    <ul className="mlist">
                      {[...(months ?? [])].reverse().map((m) => (
                        <li key={m.month}>
                          <button onClick={() => openMonth(m.month)} title={`查看 ${m.month} 每日`}>
                            <span className="mname">{m.month}</span>
                            <span className="mnum">
                              总额 {credits(m.total)}
                            </span>
                            <span className="mnum up">
                              +{credits(m.up)}
                            </span>
                            <span className="mnum down">
                              −{credits(m.down)}
                            </span>
                            <span className="mgo">每日 ›</span>
                          </button>
                        </li>
                      ))}
                    </ul>
                  </>
                )}
              </section>
            </>
          ) : (
            <section className="tsec">
              <div className="tsec-head">
                <button className="link-back" onClick={() => setOpen(null)}>
                  ← 返回月度
                </button>
                <TrendLegend visible={visible} onToggle={toggle} />
              </div>
              <h4 className="tsec-title">{open} 每日</h4>
              {monthDays === null ? (
                <p className="panel-empty">正在读取…</p>
              ) : (
                <TrendChart points={dayPoints(monthDays)} visible={visible} height={176} />
              )}
            </section>
          )}
        </div>
      </div>
    </div>
  );
}
