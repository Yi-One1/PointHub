import { invoke } from "@tauri-apps/api/core";
import type { CSSProperties, MouseEvent } from "react";
import { useEffect, useState } from "react";
import type { StateView } from "./types";

/**
 * 桌面悬浮球。
 *
 * 它只做两件事：把「总积分 / 待签数」摆在那，双击回主窗口。
 * 不做详细操作——悬浮球上塞按钮，最后一定点错。
 */
export default function Ball() {
  const [view, setView] = useState<StateView | null>(null);
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const v = await invoke<StateView>("bootstrap");
        if (alive) setView(v);
      } catch {
        /* 后端还没起来时静默，下一轮会好 */
      }
    };
    tick();
    const timer = setInterval(tick, 30_000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  const show = view?.settings.ball.show ?? "total_credits";
  const total = view?.aggregate.total_balance ?? 0;
  const pending = view?.aggregate.pending_count ?? 0;
  const ready = view?.aggregate.account_count ?? 0;
  const done = Math.max(ready - pending, 0);
  const ratio = ready === 0 ? 0 : done / ready;

  const label = show === "total_credits" ? fmt(total) : String(pending);
  const caption = show === "total_credits" ? "积分" : "待签";

  const startDrag = async (e: MouseEvent) => {
    if (e.button !== 0) return;
    setDragging(true);
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().startDragging();
    } catch {
      /* 拖动失败不影响显示 */
    } finally {
      setTimeout(() => setDragging(false), 120);
    }
  };

  const openMain = async () => {
    try {
      await invoke("show_main");
    } catch {
      /* 忽略 */
    }
  };

  return (
    <div className={`ball ${dragging ? "ball-dragging" : ""}`} title="拖动移动 · 双击打开主窗口">
      <div
        className="ball-disc"
        onMouseDown={startDrag}
        onDoubleClick={openMain}
        style={{ "--ratio": `${Math.round(ratio * 100)}%` } as CSSProperties}
      >
        <div className="ball-value">{label}</div>
        <div className="ball-caption">{caption}</div>
      </div>
    </div>
  );
}

function fmt(n: number): string {
  if (n >= 100000) return `${(n / 1000).toFixed(0)}k`;
  if (n >= 10000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}
