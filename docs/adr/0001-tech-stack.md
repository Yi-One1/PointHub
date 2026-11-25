# ADR-0001：Rust 共享核心 + Tauri 2 双端，一套 React 界面

- 状态：已采纳
- 日期：2026-11-25
- 相关：`DECISIONS.md` D02 / D03

## 背景

需求同时要求：双端（Windows + Android）、体积尽量小、界面要能频繁改、
Windows 要有桌面悬浮球与托盘、账号凭据必须留在本机。

候选方案有四类：Electron/Python（快但体积大）、.NET/Java（单端强、跨端弱）、
Flutter（一套 UI 但体积中等）、Tauri（体积最小但安卓生态较新）。

## 决策

1. **核心逻辑用 Rust**，编成静态库被两端复用：凭据解密、HTTP、调度、存储、同步协议。
2. **界面用一套 React + TypeScript**，Windows 与 Android 共用同一份产物，靠查询参数区分
   主窗口与悬浮球。
3. **外壳用 Tauri 2**，Windows 直接用官方插件（tray / notification / autostart / updater）；
   Android 侧接受自写少量 Kotlin 插件。

## 理由

- 「体积尽量小」是硬要求：Tauri 复用系统 WebView2，安装包可做到 8–15MB，
  Electron 与 PyInstaller 是它的 5–20 倍。
- 「会多次修改」意味着界面改动频率高于平台能力改动频率：一套 UI 改一次生效两端，
  比两套 UI 每次改两遍划算得多。
- 悬浮球需要透明 + 置顶 + 无边框窗口，这是 Tauri 的原生能力；
  Flutter 在 Windows 上做真透明窗口仍有坑。

## 代价

- Android 的后台定时、前台服务、安装 APK 需要 Kotlin 插件（约 3 个），
  不能只靠 Rust 完成；
- Tauri 2 的 Android 生态相对年轻，可能踩到版本级问题。

## 替代方案与被否原因

| 方案 | 否掉的原因 |
|---|---|
| Rust + Flutter | 体积大一档；Windows 透明悬浮球风险更高 |
| Rust + Tauri(Win) + Compose(Android) | 两套 UI，与「频繁修改」直接冲突 |
| Python + PyInstaller | 打包 100MB+，内存占用高，违背第 17 条 |
| 纯 Web / PWA | 拿不到设备绑定的凭据，只能看不能签 |

## 何时该重新审视

- 若 Tauri 2 在 Android 上无法稳定实现「后台唤醒 + 前台服务」，就把外壳换成
  Flutter，核心与协议不动——这层隔离是本决策刻意保留的可替换点。
