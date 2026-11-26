import type { CSSProperties, PointerEvent as ReactPointerEvent } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { useTheme } from "./theme";
import { PointsIcon } from "./PointsIcon";
import { VendorIcon } from "./VendorIcon";
import type { AccountView, StateView } from "./types";

/**
 * 桌面悬浮球。
 *
 * 交互（对应需求 5）：
 * - 球面默认显示「全部账号合计」，也可在设置里或展开面板里指定某个账号；
 * - **单击展开**：按平台列出全部账号的积分，点某一行就把球面换成那个号；
 * - **按住拖动**：移动超过 5px 才触发窗口拖动，所以轻点不会误拖；
 * - 双击打开主窗口。
 *
 * 为什么点击与拖动要自己算：Tauri 的 `startDragging` 一旦触发就把鼠标交给系统，
 * 之后再收不到 click。用位移阈值区分是最稳的做法。
 */
export default function Ball() {
  const [view, setView] = useState<StateView | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [dragging, setDragging] = useState(false);
  const down = useRef<{ x: number; y: number; moved: boolean } | null>(null);

  const load = useCallback(async () => {
    try {
      const v = await api.bootstrap();
      setView(v);
    } catch {
      /* 后端还没起来时静默，下一轮会好 */
    }
  }, []);

  useEffect(() => {
    load();
    const timer = setInterval(load, 20_000);
    let unlisten: (() => void) | undefined;
    let alive = true;
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        const un = await listen("ph://changed", () => {
          load();
        });
        if (alive) unlisten = un;
        else un();
      } catch {
        /* 忽略 */
      }
    })();
    return () => {
      alive = false;
      clearInterval(timer);
      unlisten?.();
    };
  }, [load]);

  const toggle = useCallback(async (next: boolean) => {
    // 先改窗口尺寸再换内容：否则面板会先挤在 96px 的窗口里闪一下。
    try {
      await api.setBallExpanded(next);
    } catch {
      /* 窗口操作失败不影响显示 */
    }
    setExpanded(next);
  }, []);

  // Esc 收起，符合直觉
  useEffect(() => {
    if (!expanded) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") toggle(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [expanded, toggle]);

  // 点到桌面别处就收起：否则那块 344×468 的透明区会一直挡着鼠标。
  useEffect(() => {
    if (!expanded) return;
    let un: (() => void) | undefined;
    let alive = true;
    (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const cb = await getCurrentWindow().onFocusChanged(({ payload }) => {
          if (!payload) void toggle(false);
        });
        if (alive) un = cb;
        else cb();
      } catch {
        /* 拿不到焦点事件时不强求 */
      }
    })();
    return () => {
      alive = false;
      un?.();
    };
  }, [expanded, toggle]);

  // 悬浮球是独立 WebView，主题要自己挂一次，否则深色模式下这块永远是白的。
  useTheme(view?.settings.theme);

  const selected = view?.settings.ball.selected ?? null;
  const account = selected ? view?.accounts.find((a) => a.id === selected) ?? null : null;
  const show = view?.settings.ball.show ?? "total_credits";
  const total = view?.aggregate.total_balance ?? 0;
  const pending = view?.aggregate.pending_count ?? 0;
  const ready = view?.aggregate.account_count ?? 0;
  const done = Math.max(ready - pending, 0);
  const ratio = ready === 0 ? 0 : done / ready;

  const groups = useMemo(() => groupByVendor(view), [view]);

  const value =
    show === "pending_count" ? String(pending) : account ? (account.balance === null ? "—" : fmt(account.balance)) : fmt(total);
  const caption = show === "pending_count" ? "待签" : account ? account.label : "合计";

  const startDrag = async () => {
    setDragging(true);
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().startDragging();
    } catch {
      /* 拖动失败不影响显示 */
    } finally {
      setTimeout(() => setDragging(false), 140);
    }
  };

  const onPointerDown = (e: ReactPointerEvent) => {
    if (e.button !== 0) return;
    down.current = { x: e.clientX, y: e.clientY, moved: false };
  };

  const onPointerMove = (e: ReactPointerEvent) => {
    const d = down.current;
    if (!d || d.moved) return;
    if (Math.hypot(e.clientX - d.x, e.clientY - d.y) > 5) {
      d.moved = true;
      void startDrag();
    }
  };

  const onPointerUp = () => {
    const d = down.current;
    down.current = null;
    if (d && !d.moved) void toggle(true);
  };

  const openMain = async () => {
    try {
      await api.showMain();
    } catch {
      /* 忽略 */
    }
  };

  const pick = async (id: string | null) => {
    try {
      await api.setBallAccount(id);
      await load();
    } catch {
      /* 忽略 */
    }
    void toggle(false);
  };

  const checkinAll = async () => {
    try {
      await api.checkin();
      await load();
    } catch {
      /* 忽略 */
    }
    void toggle(false);
  };

  if (expanded) {
    return (
      <div className="ball-shell">
        <div className="ball-panel">
          <div className="bp-head">
            <span className="bp-title">积分枢纽</span>
            <span className="bp-sub">
              {done}/{ready} 已签
            </span>
            <button className="bp-close" aria-label="收起" onClick={() => toggle(false)}>
              <svg viewBox="0 0 20 20" width="12" height="12" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M5 5l10 10M15 5L5 15" />
              </svg>
            </button>
          </div>

          <div className="bp-body">
            <button className={`bp-row ${selected === null ? "on" : ""}`} onClick={() => pick(null)}>
              <span className="bp-tick">{selected === null ? "✓" : ""}</span>
              <span className="bp-name">全部合计</span>
              <span className="bp-val">
                {fmt(total)}
                <PointsIcon tone="bronze" size={8} />
              </span>
            </button>

            {groups.map((g) => (
              <div key={g.vendor}>
                <div className="bp-group">
                  <VendorIcon vendor={g.vendor} size={13} />
                  <span>{g.display}</span>
                  <b>
                    {fmt(g.subtotal)}
                    <PointsIcon tone="ink" size={8} />
                  </b>
                </div>
                {g.accounts.map((a) => (
                  <button
                    key={a.id}
                    className={`bp-row ${selected === a.id ? "on" : ""}`}
                    onClick={() => pick(a.id)}
                    title={`${a.label}${a.checked_in_today === true ? " · 今日已签" : ""}`}
                  >
                    <span className="bp-tick">{selected === a.id ? "✓" : ""}</span>
                    <span className="bp-name">{a.label}</span>
                    <span className="bp-val">
                      {a.balance === null ? (
                        "—"
                      ) : (
                        <>
                          {fmt(a.balance)}
                          <PointsIcon tone="jade" size={8} />
                        </>
                      )}
                    </span>
                  </button>
                ))}
              </div>
            ))}

            {groups.length === 0 && <div className="bp-group">还没有账号</div>}
          </div>

          <div className="bp-foot">
            <span className="bp-foot-note">{pending > 0 ? `${pending} 个待签` : "今日已全部完成"}</span>
            <button className="go solid" style={{ marginLeft: "auto" }} onClick={checkinAll} disabled={ready === 0}>
              一键签到
            </button>
            <button className="go" onClick={openMain}>
              主窗口
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="ball-shell">
      <div
        className={`ball ${dragging ? "ball-dragging" : ""}`}
        title="单击展开全部账号 · 拖动移动 · 双击打开主窗口"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={() => (down.current = null)}
        onDoubleClick={openMain}
        style={{ "--ratio": `${Math.round(ratio * 100)}%` } as CSSProperties}
      >
        <svg className="ball-ring-svg" viewBox="0 0 96 96" aria-hidden>
          <circle cx="48" cy="48" r="45" fill="none" stroke="rgba(255,255,255,.18)" strokeWidth="3" />
          <circle
            cx="48"
            cy="48"
            r="45"
            fill="none"
            stroke="rgba(255,255,255,.85)"
            strokeWidth="3"
            strokeLinecap="round"
            strokeDasharray={`${ratio * 283} 283`}
            transform="rotate(-90 48 48)"
          />
        </svg>
        <div className="ball-inner">
          <div className="ball-value">
            {show === "pending_count" ? (
              value
            ) : (
              <>
                {value}
                <PointsIcon tone={account ? "jade" : "bronze"} size={13} />
              </>
            )}
          </div>
          <div className="ball-caption">{caption}</div>
        </div>
      </div>
    </div>
  );
}

function groupByVendor(view: StateView | null) {
  if (!view) return [];
  const map = new Map<string, AccountView[]>();
  for (const a of view.accounts) {
    const list = map.get(a.vendor);
    if (list) list.push(a);
    else map.set(a.vendor, [a]);
  }
  const order = view.platforms.map((p) => p.vendor);
  const keys = [...order.filter((v) => map.has(v)), ...[...map.keys()].filter((v) => !order.includes(v))];
  return keys.map((v) => {
    const list = map.get(v)!;
    const cap = view.platforms.find((p) => p.vendor === v);
    return {
      vendor: v,
      display: cap?.display ?? list[0].vendor_display,
      accounts: list,
      subtotal: list.reduce((n, a) => n + (a.balance ?? 0), 0),
    };
  });
}

function fmt(n: number): string {
  if (n >= 100000) return `${(n / 1000).toFixed(0)}k`;
  if (n >= 10000) return `${(n / 1000).toFixed(1)}k`;
  return n.toLocaleString("zh-CN");
}
