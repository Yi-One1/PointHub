/**
 * 积分变化曲线（总额 / 新增 / 消耗三条线）。
 *
 * 两条量纲差 10~20 倍（总额两三千，日增减几十上百），塞进同一个 Y 轴会把
 * 增减压成贴着底边的直线。所以：总额用左轴，新增与消耗共用右轴，两边各自
 * 独立取值域；某条线被隐藏时，对应的轴也跟着消失，不留下没有意义的刻度。
 *
 * 不引图表库：三条曲线 + 悬停竖线用 SVG 画足够，省下一个依赖的包体积。
 */
import { useEffect, useRef, useState } from "react";
import { credits } from "./format";
import type { DayPoint, MonthPoint } from "./types";

export type TrendKey = "total" | "up" | "down";

/** 一条曲线上的一个点；没有当天数据时对应值为 null（断线，不补 0）。 */
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

/**
 * 后端一行数据 → 一个曲线点。**全应用只有这里决定「哪一项该断线」**。
 *
 * 规则只有一条：这一格没有采样（`has === false`）时三项全部给 null。
 * 不能只判 `total`——后端每天的 `up` 恒为 `Some(0.0)`，只判 total 会让
 * 「新增」线从今天往后平拖到月末，画出一条根本没发生过的 0。
 */
export function toTrendPoints(
  rows: { label: string; has: boolean; total: number; up: number | null; down: number | null }[],
): TrendPoint[] {
  return rows.map((r) => ({
    label: r.label,
    total: r.has ? r.total : null,
    up: r.has ? r.up : null,
    down: r.has ? r.down : null,
  }));
}

/** 每日序列 → 曲线点（`known` 为 0 表示当天没有任何余额采样）。 */
export function dayTrendPoints(rows: DayPoint[]): TrendPoint[] {
  return toTrendPoints(
    rows.map((d) => ({
      label: d.date,
      has: d.known > 0,
      total: d.total,
      up: d.up,
      down: d.down,
    })),
  );
}

/** 月度序列 → 曲线点（`days` 为 0 表示整月没采样；标签 `1月`…`12月`）。 */
export function monthTrendPoints(rows: MonthPoint[]): TrendPoint[] {
  return toTrendPoints(
    rows.map((m) => ({
      label: `${Number(m.month.slice(5))}月`,
      has: m.days > 0,
      total: m.total,
      up: m.up,
      down: m.down,
    })),
  );
}

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

/**
 * 把一串相邻点连成平滑曲线（单调三次插值，Fritsch–Carlson 限制切向量）。
 *
 * 为什么不用普通 Catmull-Rom：它会在陡升/陡降的相邻区间里过冲，「新增」「消耗」
 * 这类以 0 为底的数据平滑后会被画到 0 以下（负积分是不存在的事实）。
 *
 * 免过冲靠两个条件，缺一不可（`smooth.test.mjs` 按采样点逐个验证）：
 * 1. 区间两端切向量与区间斜率**不得异号**。异号说明这一段是「先涨后跌 / 先跌
 *    后涨」的拐点，切线指回区间外边，任何取值都会把曲线顶出两端；这种区间
 *    直接归零两端切线，退化成平滑的 S 形过渡。
 * 2. 同号（含 0）时再满足 α² + β² ≤ 9（α、β 为两端切向量与区间斜率之比）。
 *
 * 两条都成立时该段三次 Hermite 是单调的，采样点必落在两端 y 之间，也就不会越过 0。
 */
export function smoothPath(pts: { x: number; y: number }[]): string {
  if (pts.length === 0) return "";
  // 只有一个点时画不出线段（比如历史只有一个月），给一小段横线，
  // 否则图上只剩一个 2px 的点，看起来像「没数据」。
  if (pts.length === 1) {
    return `M${(pts[0].x - 9).toFixed(1)},${pts[0].y.toFixed(1)} L${(pts[0].x + 9).toFixed(1)},${pts[0].y.toFixed(1)}`;
  }
  // 两点之间没有「中间形状」可修，直线即可，也省一次贝塞尔求值。
  if (pts.length === 2) {
    return `M${pts[0].x.toFixed(1)},${pts[0].y.toFixed(1)} L${pts[1].x.toFixed(1)},${pts[1].y.toFixed(1)}`;
  }

  const n = pts.length;
  const d: number[] = []; // 相邻点之间的斜率
  for (let i = 0; i < n - 1; i++) {
    d.push((pts[i + 1].y - pts[i].y) / (pts[i + 1].x - pts[i].x));
  }
  const m: number[] = new Array<number>(n).fill(0); // 每个点的切向量
  m[0] = d[0];
  m[n - 1] = d[n - 2];
  for (let i = 1; i < n - 1; i++) m[i] = (d[i - 1] + d[i]) / 2;

  // Fritsch–Carlson：把每段两端的切向量收进「单调区」，曲线就不会冲出区间。
  for (let i = 0; i < n - 1; i++) {
    if (d[i] === 0) {
      // 平台段：两端切线归零，避免曲线在水平段里鼓一个包
      m[i] = 0;
      m[i + 1] = 0;
      continue;
    }
    const a = m[i] / d[i];
    const b = m[i + 1] / d[i];
    // 切向量与区间斜率异号（α<0 或 β<0）：切线指回区间外，必然过冲。
    // 注意不能写成 `a * b <= 0`——它会把「a=0、b>0」这种正常情形也归零，
    // 于是一路连坐把尖峰削成折线；α、β 恰好为 0 是允许的，交给下面的圆盘限幅。
    if (a < 0 || b < 0) {
      m[i] = 0;
      m[i + 1] = 0;
      continue;
    }
    const s = a * a + b * b;
    if (s > 9) {
      const t = 3 / Math.sqrt(s);
      m[i] = t * a * d[i];
      m[i + 1] = t * b * d[i];
    }
  }

  let out = `M${pts[0].x.toFixed(1)},${pts[0].y.toFixed(1)}`;
  for (let i = 0; i < n - 1; i++) {
    const h = pts[i + 1].x - pts[i].x;
    // 三次贝塞尔的两个控制点放在 1/3、2/3 处，切线长度按切向量取值
    const c1x = pts[i].x + h / 3;
    const c1y = pts[i].y + (m[i] * h) / 3;
    const c2x = pts[i + 1].x - h / 3;
    const c2y = pts[i + 1].y - (m[i + 1] * h) / 3;
    out += ` C${c1x.toFixed(1)},${c1y.toFixed(1)} ${c2x.toFixed(1)},${c2y.toFixed(1)} ${pts[i + 1].x.toFixed(1)},${pts[i + 1].y.toFixed(1)}`;
  }
  return out;
}

/**
 * 把带空值的取值序列切成若干连续段，返回每段的下标数组。
 *
 * 断线用：跨着空档去平滑，会把「那天没数据」画成一条凭空出现的斜线。
 */
export function segmentsOf(values: (number | null)[]): number[][] {
  const out: number[][] = [];
  let run: number[] = [];
  values.forEach((v, i) => {
    if (v === null) {
      if (run.length > 0) {
        out.push(run);
        run = [];
      }
      return;
    }
    run.push(i);
  });
  if (run.length > 0) out.push(run);
  return out;
}

/// 三条线的显示/隐藏。
///
/// 每次调用都是一份**独立** state：卡片、趋势页的两张图、平台弹窗各调一次，
/// 关掉一张图的某条线不会影响另一张。不要为了「省一次调用」把它提到父组件
/// 用 props 传下去——那正是之前两张图连动的成因。
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

  // 一条曲线可能被 null（当天没采样）切成若干连续段，每段单独平滑：
  // 跨着空档去平滑会把「没数据」画成一条凭空出现的斜线。
  const line = (key: TrendKey, yOf: (v: number) => number) => {
    const values = points.map((p) => p[key]);
    return segmentsOf(values)
      .map((idx) => smoothPath(idx.map((i) => ({ x: x(i), y: yOf(values[i] as number) }))))
      .join(" ");
  };

  // 点数多时（整月 30/31 天）只画 6 个横轴标签，否则会挤成一团。
  const step = Math.max(1, Math.ceil(points.length / 6));
  const cur = hover !== null ? points[hover] : null;
  const tooltipLeft =
    hover === null ? 0 : Math.min(Math.max(x(hover) + 12, padL), Math.max(padL, width - 128));

  return (
    <div className="tchart" ref={box} style={{ height }}>
      {width > 0 && (
        <svg width={width} height={height} role="img" aria-label="积分变化曲线图">
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
