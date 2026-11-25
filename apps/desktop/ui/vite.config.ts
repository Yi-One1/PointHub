import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// 端口固定 1420：Tauri 的 devUrl 写的是它，改了要同步改 tauri.conf.json。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  build: {
    outDir: "dist",
    target: "chrome110",
    emptyOutDir: true,
  },
});
