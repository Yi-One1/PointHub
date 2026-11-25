import { useEffect } from "react";
import type { ThemeMode } from "./types";

/**
 * 把主题写到 `<html data-theme>` 上。
 *
 * 主窗口和悬浮球是两个独立 WebView，**各自都要挂一次**——
 * 之前只在主窗口挂了，结果深色模式下悬浮球面板一直是白的。
 */
export function useTheme(mode: ThemeMode | undefined) {
  useEffect(() => {
    const m = mode ?? "system";
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = m === "dark" || (m === "system" && mq.matches);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
    };
    apply();
    if (m === "system") {
      mq.addEventListener("change", apply);
      return () => mq.removeEventListener("change", apply);
    }
  }, [mode]);
}
