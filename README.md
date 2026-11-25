# 积分枢纽 PointHub

多平台 AI Agent 账号的**每日积分自动签到**、**积分看板**与**双端互通**工具。
Windows 桌面端 + Android 手机端，数据只在本机。

> **仅用于管理你本人合法持有的账号。** 本项目调用的是各平台的非公开接口，
> 可能违反平台服务条款、可能因平台改版失效、可能导致账号异常。
> 使用前请自行判断，风险自担。本项目与任何平台官方无隶属或合作关系。

---

## 现在能做到什么

| 能力 | WorkBuddy | Trae CN | Qoder CN |
|---|---|---|---|
| 每日自动签到 | 已实机验证 | 待实测 | 端点待补（见下） |
| 积分余额 | 已实机验证 | 待实测 | 待实测 |
| 长效令牌自动续期 | 已实机验证 | 不支持 | 不支持 |
| 派猫猫旅行 | 已实机验证 | — | — |
| 多账号 | 支持 | 支持 | 支持 |

「待实测」= 代码已按公开资料实现，但还没有用真实账号跑通。**能力矩阵在设置页里可见**，
界面上不会给你一个点了就报错的按钮。

### Qoder CN 的现状（重要）

Qoder 每日 100 Credits 自 **2026-11-19 10:00** 起只对付费/试用/会员卡账号开放，
体验版（免费）账号的入口被服务端直接隐藏。本程序把这种情况标为「无活动资格」，
不重试、不报错、不计入失败。

---

## 目录结构

```
PointHub/
├── crates/                  Rust 共享核心（Windows 与 Android 共用）
│   ├── ph-core/             领域模型：平台、账号、凭据、快照、调度
│   ├── ph-vault/            凭据加密仓（Windows DPAPI / AES 回退）
│   ├── ph-store/            数据仓：账号、设置、历史（原子写入）
│   ├── ph-providers/        平台适配：workbuddy / trae / qoder
│   ├── ph-engine/           编排：取凭据→刷新→签到→回写→记账
│   └── ph-cli/              phctl 命令行内核
├── apps/desktop/
│   ├── src-tauri/           Tauri 2 外壳：托盘、悬浮球、开机自启、通知
│   └── ui/                  React + TypeScript 界面（双端复用）
├── docs/                    SPEC / 架构 / ADR
├── scripts/                 构建与打包脚本
├── DECISIONS.md             决策台账
└── CONTEXT.md               术语表
```

---

## 跑起来（Windows）

需要：**Rust 1.80+**、**Node 18+**、Windows 10/11（自带 WebView2）。

### 只想验收、还不打包：双击 `1. 一键启动预览.bat`

项目根目录下的 `1. 一键启动预览.bat` 用**当前源码**启动，不产出、也不改动 `dist/`。

它会先比对文件时间：**没改动的部分直接跳过编译**——什么都没改时是秒开；
改了界面才跑前端构建（约 2 秒），改了 Rust 才编译（约 22 秒）。

它的黑窗口是日志窗口，**关掉窗口 = 退出程序**。

### 想自己敲命令

```bash
# 1) 只跑命令行内核（不用装前端依赖，最快验证链路）
cargo run -p phctl -- doctor

# 2) 跑桌面端（开发模式，前端热更新）
cd apps/desktop/ui && npm install
cd ../src-tauri && npx @tauri-apps/cli dev
```

### 打包

```powershell
# 便携 exe（日常只要这个）
powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1

# 便携 exe + 安装包（特殊要求时才用）
powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1 -Installer
```

产物在 `dist/`，命名格式 `积分枢纽PointHub_v0.3.0.exe`。

**`dist/` 里的文件视为已交付的历史版本，脚本不覆盖、不删除。** 要再出一版，
先按语义化版本升 `Cargo.toml` 里 `[workspace.package] version`，再跑脚本；
版本号没变会直接报错退出——逼你先想清楚这次该升哪一位。

**实测（本机 Windows 11 / WebView2 158）**

| 指标 | 数值 |
|---|---|
| 便携 exe 体积 | 4.5 MB（单文件，无外部资源） |
| 空闲内存 | 约 78 MB |
| 冷启动到可见界面 | 1 秒内 |

---

## 命令行（phctl）

界面出问题的时候，先用它把「是不是软件的错」分清楚。

```bash
phctl doctor                                  # 环境自检 + 平台能力矩阵
phctl account add --vendor workbuddy --label 主号 --refresh-token <令牌>
phctl account list
phctl status                                  # 查积分与今日签到状态
phctl checkin                                 # 一键全部签到
phctl travel --id 主号                         # 派猫猫旅行
phctl plan                                    # 每个账号下次执行时间
```

`phctl` 与桌面端**共用同一个数据目录**（默认 `%LOCALAPPDATA%\PointHub`），
所以两边看到的是同一份账号。

---

## 数据与安全

| 项目 | 位置 / 做法 |
|---|---|
| 数据目录 | 默认 `%LOCALAPPDATA%\PointHub`，可在设置里改（搬迁会复制现有数据） |
| 令牌 | 只存在 `vault.bin`，用当前 Windows 用户的 DPAPI 加密；换机后需重新导入 |
| 账号元数据 | `accounts.json`（**不含令牌**） |
| 积分历史 | `history.jsonl`（追加写，一行一条快照） |
| 日志 | `logs/YYYY-MM-DD.log`（不含令牌明文） |

程序不会把任何凭据发到网络上除平台官方域名以外的地方。

---

## 进度

- [x] Rust 核心：模型 / 加密仓 / 数据仓 / WorkBuddy 适配 / 引擎 / CLI
- [x] Windows 外壳：托盘、悬浮球（单击展开全部账号）、开机自启、系统通知、浅色深色
- [x] 多账号管理、逐账号签到 + 一键全部签到
- [x] 总览控制台：四格 KPI、按平台分组、近 7 日每日新增、最近活动
- [x] 调度器常驻（每 45 秒核对，到点执行，错过按设置补签）
- [x] Android 工程与 APK 构建链路（调试包已出，见 `docs/ANDROID.md`）
- [ ] 局域网互通（PC 服务端 + 手机配对）
- [ ] Android 端界面适配（单列布局、触控尺寸、底部导航）
- [ ] 自动更新（Windows updater + Android 装 APK）
- [ ] 卸载清理（安装包侧）

详见 `docs/SPEC.md` 与 `DECISIONS.md`。

---

## 许可

MIT，见 [LICENSE](LICENSE)。

参考过的公开项目与协议边界写在 `DECISIONS.md` 的 D11：
其中两个仓库没有 LICENSE，因此**只借鉴接口事实、未复制代码**。
