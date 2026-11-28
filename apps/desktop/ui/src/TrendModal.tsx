/**
 * 平台面板的「趋势」弹窗：只显示近 7 日。
 *
 * 整月每日、本年度 12 个月已经搬到左侧「统计」页（那是两个完整坐标系，
 * 弹窗里塞不下），这里只保留一眼能读的 7 个点，并把用户带到统计页。
 *
 * 数据按需从后端取（`credit_days`），不进 bootstrap：一年 365 个点跟着每次
 * 刷新一起下发，代价比打开弹窗时取一次大得多。
 */
import { useEffect, useState } from "react";
import { api } from "./api";
import type { DayPoint } from "./types";
import { TrendChart, TrendLegend, dayTrendPoints, useTrendVisible } from "./TrendChart";

/// 本地日期 → `YYYY-MM-DD`（后端只认这个格式）。
function ymd(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

export function TrendModal({
  vendor,
  display,
  onClose,
  onOpenTrends,
}: {
  /** null 表示全部平台 */
  vendor: string | null;
  display: string;
  onClose: () => void;
  /** 跳到「统计」页看本月与本年度（由 App 负责切页并关掉弹窗） */
  onOpenTrends: () => void;
}) {
  const [days, setDays] = useState<DayPoint[] | null>(null);
  // 弹窗自己一份图例 state：与总览卡片、趋势页两张图互不影响。
  const [visible, toggle] = useTrendVisible();
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const from = new Date();
    from.setDate(from.getDate() - 6);
    void (async () => {
      try {
        const d = await api.creditDays(vendor, ymd(from), 7);
        if (!alive) return;
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
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const points = days ? dayTrendPoints(days) : [];

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

          <section className="tsec">
            <div className="tsec-head">
              <h4>近 7 日</h4>
              <TrendLegend visible={visible} onToggle={toggle} />
            </div>
            {days === null ? (
              <p className="panel-empty">正在读取…</p>
            ) : points.length === 0 ? (
              <p className="panel-empty">还没有采样数据</p>
            ) : (
              <TrendChart points={points} visible={visible} height={176} />
            )}
          </section>
        </div>

        <div className="modal-actions" style={{ marginTop: 14 }}>
          <button className="btn" onClick={onOpenTrends}>
            查看本月与本年度 →
          </button>
        </div>
      </div>
    </div>
  );
}
