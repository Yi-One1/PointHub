/**
 * 积分变化折线图（总额 / 新增 / 消耗三条线）。
 *
 * 两条量纲差 10~20 倍（总额两三千，日增减几十上百），塞进同一个 Y 轴会把
 * 增减压成贴着底边的直线。所以：总额用左轴，新增与消耗共用右轴，两边各自
 * 独立取值域；某条线被隐藏时，对应的轴也跟着消失，不留下没有意义的刻度。
 *
 * 不引图表库：三条折线 + 悬停竖线用 SVG 画足够，省下一个依赖的包体积。
 */
import { useEffect, useRef, useState } from "react";
import { credits } from "./format";

export type TrendKey = "total" | "up" | "down";

/** 一条折线上的一个点；没有当天数据时对应值为 null（断线，不补 0）。 */
export interface TrendPoint {
  /** 横轴标签（`11-28` 或 `11月`） */
  label: string;
  total: number | null;
  up: number | null;
  down: number | null;
}

export const TREND_LABEL: Record<TrendKey, string> = {
  total: "积分总额",
  up: "新增",
  down: "消耗",
};

const TREND_COLOR: Record<TrendKey, string> = {
  total: "var(--chart-total)",
  up: "var(--accent)",
  down: "var(--danger)",
};

const ALL_KEYS: TrendKey[] = ["total", "up", "down"];

/// 轴刻度用：过万折成「x.x万」，否则按整数显示。
function axis(n: number): string {
  const abs = Math.abs(n);
  if (abs >= 10000) {
    return `${(n / 10000).toFixed(abs >= 100000 ? 0 : 1)}万`;
  }
  return Math.round(n).toLocaleString("zh-CN");
}

/// 三条线的显示/隐藏：卡片与弹窗共用，避免两处状态逻辑各写一遍。
export function useTrendVisible() {
  const [visible, setVisible] = useState<TrendKey[]>(["total", "up", "down"]);
  const toggle = (k: TrendKey) =>
    setVisible((prev) => {
      if (prev.includes(k)) {
        // 至少留一条线，否则图会变成空白
        return prev.length === 1 ? prev : prev.filter((x) => x !== k);
      }
      return [...prev, k];
    });
  return [visible, toggle] as const;
}

export function TrendLegend({
  visible,
  onToggle,
}: {
  visible: TrendKey[];
  onToggle: (key: TrendKey) => void;
}) {
  return (
    <div className="tlegend">
      {ALL_KEYS.map((k) => {
        const on = visible.includes(k);
        return (
          <button
            key={k}
            className={`tlegend-item ${on ? "" : "off"}`}
            onClick={() => onToggle(k)}
            aria-pressed={on}
            title={on ? `隐藏「${TREND_LABEL[k]}」曲线` : `显示「${TREND_LABEL[k]}」曲线`}
          >
            <i style={{ background: on ? TREND_COLOR[k] : "var(--faint)" }} />
            {TREND_LABEL[k]}
          </button>
        );
      })}
    </div>
  );
}

export function TrendChart({
  points,
  visible,
  height = 148,
}: {
  points: TrendPoint[];
  visible: TrendKey[];
  height?: number;
}) {
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [hover, setHover] = useState<number | null>(null);

  // 宽度靠 ResizeObserver 拿到真实像素：viewBox 缩放会把字号一起拉变形。
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    setWidth(el.clientWidth);
    const ro = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const showTotal = visible.includes("total");
  const showFlow = visible.includes("up") || visible.includes("down");
  const padL = showTotal ? 44 : 9;
  const padR = showFlow ? 40 : 9;
  const padT = 10;
  const padB = 18;
  const innerW = Math.max(8, width - padL - padR);
  const innerH = Math.max(8, height - padT - padB);

  const totals = points.map((p) => p.total).filter((v): v is number => v !== null);
  // 只拿「当前可见」的增减线算右轴：藏掉消耗后轴不该还按消耗的最大值撑高。
  const flowKeys = (["up", "down"] as TrendKey[]).filter((k) => visible.includes(k));
  const flows = points
    .flatMap((p) => flowKeys.map((k) => p[k]))
    .filter((v): v is number => v !== null);

  // 总额：上下各留 18% 余量，否则接近水平的线会贴边看不出变化。
  let tLo = 0;
  let tHi = 1;
  if (totals.length > 0) {
    const min = Math.min(...totals);
    const max = Math.max(...totals);
    const span = max - min;
    const pad = span > 0 ? span * 0.18 : Math.max(1, max * 0.04);
    tLo = Math.max(0, min - pad);
    tHi = max + pad;
    if (tHi - tLo < 1) tHi = tLo + 1;
  }
  // 增减从 0 起，保持「高度=数量」的直觉。
  const fHi = Math.max(...flows, 1) * 1.15;

  const x = (i: number) =>
    points.length <= 1 ? padL + innerW / 2 : padL + (i / (points.length - 1)) * innerW;
  const yTotal = (v: number) => padT + (1 - (v - tLo) / (tHi - tLo)) * innerH;
  const yFlow = (v: number) => padT + (1 - Math.min(v, fHi) / fHi) * innerH;

  const line = (key: TrendKey, yOf: (v: number) => number) => {
    const drawn: string[] = [];
    points.forEach((p, i) => {
      const v = p[key];
      if (v !== null) drawn.push(`${x(i).toFixed(1)},${yOf(v).toFixed(1)}`);
    });
    // 只有一个点时画不出线段（比如历史只有一个月），给一小段横线，
    // 否则图上只剩一个 2px 的点，看起来像「没数据」。
    if (drawn.length === 1) {
      const [px, py] = drawn[0].split(",");
      return `M${(Number(px) - 9).toFixed(1)},${py} L${(Number(px) + 9).toFixed(1)},${py}`;
    }
    return drawn.map((pt, i) => `${i === 0 ? "M" : "L"}${pt}`).join(" ");
  };

  // 点数多时（整月 30/31 天）只画 6 个横轴标签，否则会挤成一团。
  const step = Math.max(1, Math.ceil(points.length / 6));
  const cur = hover !== null ? points[hover] : null;
  const tooltipLeft =
    hover === null ? 0 : Math.min(Math.max(x(hover) + 12, padL), Math.max(padL, width - 128));

  return (
    <div className="tchart" ref={box} style={{ height }}>
      {width > 0 && (
        <svg width={width} height={height} role="img" aria-label="积分变化折线图">
          {/* 横向参考线：上 / 中 / 下三档，够读数值又不喧宾夺主 */}
          {[0, 0.5, 1].map((r) => (
            <line
              key={r}
              x1={padL}
              x2={padL + innerW}
              y1={padT + innerH * r}
              y2={padT + innerH * r}
              stroke="var(--line-soft)"
              strokeDasharray="3 3"
            />
          ))}

          {showTotal && (
            <g className="taxis">
              <text x={padL - 8} y={padT + 4} textAnchor="end">
                {axis(tHi)}
              </text>
              <text x={padL - 8} y={padT + innerH / 2 + 4} textAnchor="end">
                {axis((tHi + tLo) / 2)}
              </text>
              <text x={padL - 8} y={padT + innerH + 4} textAnchor="end">
                {axis(tLo)}
              </text>
            </g>
          )}
          {showFlow && (
            <g className="taxis taxis-right">
              <text x={padL + innerW + 8} y={padT + 4}>
                {axis(fHi)}
              </text>
              <text x={padL + innerW + 8} y={padT + innerH + 4}>
                0
              </text>
            </g>
          )}

          {ALL_KEYS.filter((k) => visible.includes(k)).map((k) => (
            <path
              key={k}
              d={line(k, k === "total" ? yTotal : yFlow)}
              fill="none"
              stroke={TREND_COLOR[k]}
              strokeWidth={k === "total" ? 1.8 : 1.5}
              strokeLinejoin="round"
              strokeLinecap="round"
            />
          ))}

          {ALL_KEYS.filter((k) => visible.includes(k)).map((k) =>
            points.map((p, i) => {
              const v = p[k];
              if (v === null) return null;
              return (
                <circle
                  key={`${k}-${i}`}
                  cx={x(i)}
                  cy={k === "total" ? yTotal(v) : yFlow(v)}
                  r={hover === i ? 3.2 : 2}
                  fill="var(--surface)"
                  stroke={TREND_COLOR[k]}
                  strokeWidth={1.6}
                />
              );
            }),
          )}

          {points.map((p, i) =>
            i % step === 0 || i === points.length - 1 ? (
              <text key={`x-${i}`} className="taxis" x={x(i)} y={height - 4} textAnchor="middle">
                {p.label}
              </text>
            ) : null,
          )}

          {hover !== null && (
            <line
              x1={x(hover)}
              x2={x(hover)}
              y1={padT}
              y2={padT + innerH}
              stroke="var(--line)"
              strokeDasharray="3 3"
            />
          )}

          {/* 透明层负责命中：不用逐个点绑事件，滑到哪列显示哪列 */}
          <rect
            x={padL}
            y={padT}
            width={innerW}
            height={innerH}
            fill="transparent"
            onMouseLeave={() => setHover(null)}
            onMouseMove={(e) => {
              const left = e.currentTarget.getBoundingClientRect().left;
              const ratio = (e.clientX - left) / Math.max(1, innerW);
              const i = Math.round(ratio * Math.max(1, points.length - 1));
              setHover(Math.min(points.length - 1, Math.max(0, i)));
            }}
          />
        </svg>
      )}

      {cur && hover !== null && (
        <div className="ttip" style={{ left: tooltipLeft, top: 2 }}>
          <b>{cur.label}</b>
          {visible.map((k) => {
            const v = cur[k];
            return (
              <span key={k}>
                <i style={{ background: TREND_COLOR[k] }} />
                {TREND_LABEL[k]} {v === null ? "—" : credits(v)}
              </span>
            );
          })}
        </div>
      )}
    </div>
  );
}
