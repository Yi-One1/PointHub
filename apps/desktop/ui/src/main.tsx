import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Ball from "./Ball";
import "./styles.css";

// 悬浮球与主窗口共用同一份前端产物，靠查询参数分流，
// 这样只维护一套样式与状态，不必开两个构建。
const params = new URLSearchParams(location.search);
const isBall = params.get("ball") === "1";
document.documentElement.dataset.window = isBall ? "ball" : "main";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{isBall ? <Ball /> : <App />}</React.StrictMode>
);
