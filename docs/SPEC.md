# 需求规格与实现台账（SPEC）

> 逐条对应你提的 23 条要求。状态只有三种：**已落地**（能跑、有产物）、
> **部分落地**（核心在，缺外壳/缺验证）、**未开始**（说清卡在哪）。
> 不做「看起来做了」的表述。

版本：v0.4.2 · 2026-11-25

---

## 一、需求对照

| # | 需求 | 状态 | 证据 / 卡点 |
|---|---|---|---|
| 1 | 定时自动领取每日积分 | 已落地 | 外壳常驻循环每 45 秒核对（`spawn_scheduler`），到点串行执行且与手动签到同一路径；不补签时只认到点后 30 分钟窗口；`phctl plan` 可看排期 |
| 2 | 实时显示各 agent 各账号积分 | 已落地 | 总览按平台分组、账号逐行显示（各平台小计 + 合计 KPI）+ 悬浮球；`Engine::aggregate()` 实时汇总 |
| 3 | 同一 agent 多账号 | 已落地 | 账号模型按 id 独立，凭据各自加密，`phctl account add` 可加多个 |
| 4 | Windows + Android 双端互通 | 已落地（手机走浏览器） | Windows 端起服务，手机浏览器配对访问（见第五节）；安卓 release 包 7.4 MB（见 `docs/ANDROID.md`），手机端用浏览器而非原生界面 |
| 5 | 手动单个签到 + 一键全部签到 | 已落地 | 总览每行「签到」/ 顶部「一键全部签到」/ 悬浮球面板「一键签到」/ 托盘菜单 / `phctl checkin` |
| 6 | 浏览器授权 + 手动令牌 | 已落地（WorkBuddy 走官方插件授权） | 账号页两条路：**浏览器授权**（`POST /v2/plugin/auth/state` 取授权链接 → 用**用户自己的浏览器**打开 → 轮询 `/v2/plugin/auth/token` 拿 accessToken + refreshToken → 导入后立刻读一次余额验证）与**手动粘贴令牌**（含设备 id、域名）。不读客户端加密登录态、不扫进程内存、不用内嵌窗口、不开调试端口。网页版 localStorage 里没有令牌（实测只有埋点键），登录态是随请求轮换的 HttpOnly cookie，故不采用 |
| 7 | 双端更新功能 | 未开始 | 方案已定（Tauri updater + APK 自装 + 可配置源），未接 |
| 8 | 卸载干净无残留 | 部分落地 | 干净卸载的判定标准已写入 `DECISIONS.md` D10；安装包侧清理脚本未做（便携版本身不写注册表） |
| 9 | Windows 数据目录可自定义 | 已落地 | 设置页「更换数据目录」→ 复制 + 指针文件 + 重启生效；`ph_store::locate` |
| 10 | 开机自启动 | 已落地 | `tauri-plugin-autostart`，设置项与系统状态双向对齐 |
| 11 | 桌面悬浮球显示积分 | 已落地 | 透明置顶窗口：球面可绑定指定账号或显示合计，单击展开按平台列出全部账号积分（含小计、可点行切换），圆环显示今日完成度，拖动阈值 5px，双击回主窗口 |
| 12 | 便携 exe / 安装包两种产物 | 部分落地 | 便携 exe 可构建；`scripts/build-windows.ps1 -Installer` 出安装包（脚本已备，未在本次验证） |
| 13 | Windows 浅色 / 深色 | 已落地 | 三态主题（浅色 / 深色 / 跟随系统），CSS 变量整套切换 |
| 14 | 安卓正常 / 深色 + 跟随系统 | 已落地 | 主题跟随系统；≤760px 走移动端布局：侧栏收成底部标签栏、单列、触控区 ≥40px、`safe-area` 留白。已在 390×844 / 360×640 / 844×390 三档实测无横向溢出 |
| 15 | 签到后系统弹窗提示 | 已落地 | `tauri-plugin-notification`，成功与失败分别可控；CLI 侧无弹窗（合理） |
| 16 | 包名 `中文名英文名_v版本` + 语义化版本 | 已落地 | `积分枢纽PointHub_v<版本>.exe/.apk`；版本号单点来自 `Cargo.toml` 的 `workspace.package.version` |
| 17 | 体积小、占用少 | 已落地 | 便携 exe **4.6 MB**、安卓 release APK **7.4 MB**；`opt-level=z` + LTO + strip 已启用 |
| 18 | 语言选型（Rust 可行性） | 已落地 | 结论与取舍见 `DECISIONS.md` D02 |
| 19 | 中文名「积分枢纽」/ 英文名 PointHub | 已落地 | 产品名、窗口标题、安装包名、托盘提示均已统一 |
| 20 | WorkBuddy 增加「派猫猫旅行」 | 已落地 | 与签到同模块（非独立分支），按状态机自动「先领后派」 |
| 21 | 开工前必须提问确认 | 已落地 | 三轮共 12 个问题，结论进 `DECISIONS.md` |
| 22 | 参考项目须遵守开源协议 | 已落地 | MIT 的可用；两个无 LICENSE 的**只借鉴接口事实**；见 D11 |
| 23 | 上传到 GitHub 仓库 | 已落地 | `https://github.com/Yi-One1/PointHub`（`master`），本地与远端同步 |

---

## 二、数据模型

```
Account            账号元数据（不含令牌）
├─ id              不可变 uuid
├─ vendor          workbuddy | trae-cn | qoder-cn
├─ label           用户起的别名
├─ hint            脱敏标识（前 6 后 4）
├─ schedule        { hour, minute, jitter_minutes }
└─ last_error      最近一次失败原因（人话）

Credential         凭据（只在加密仓里）
├─ refresh_token   长效令牌
├─ access_token    短期令牌
├─ expires_at      access_token 过期时间
├─ device_id       Trae 必需
├─ cookie          网页会话形态
├─ domain          WorkBuddy 鉴权域名 / Qoder 自定义端点
└─ user_agent      会话绑定的 UA（需原样回放时用）

VendorSnapshot     一次采样
├─ balance / checked_in_today / streak_days / daily_credit
├─ today_gain      与「今天之前最后一条」的余额差
├─ travel          { state, location, arrive_at, reward_credit, daily_limit_reached }
└─ note            平台原话或异常说明
```

**约定**：拿不到的数值一律 `null`，不写 0。`null` 与 `0` 在界面上是两种文案。

---

## 三、平台接口契约（WorkBuddy，已实机验证）

| 动作 | 方法 / 路径 | 鉴权 |
|---|---|---|
| 换取接口令牌 | `POST https://copilot.tencent.com/v2/plugin/auth/token/refresh` | 头 `X-Refresh-Token` + `X-Auth-Refresh-Source: plugin` + `X-Domain` |
| 签到状态 | `POST https://<domain>/v2/billing/meter/checkin-activity-status` | `Bearer` |
| 领取签到 | `POST https://<domain>/v2/billing/meter/daily-checkin` | `Bearer` |
| 旅行状态 | `GET https://www.workbuddy.cn/activity/growth/buddy/travel/status` | `Bearer` |
| 领取旅行奖励 | `POST .../travel/claim` | `Bearer` |
| 派出 | `POST .../travel/depart` `{"location_id":1..4}` | `Bearer` |

三条不能踩的坑：

1. **签到域名不许硬编码**，以凭据里的 `domain` 为准（`copilot.tencent.com` 会 404）；
2. **旅行的域名与签到不同，且路径不带 `/v2`**；
3. **余额要从汇总接口取**：`checkin-activity-status` 的 `total_credits` 是本季活动累计得分，不是余额；
   余额取 `POST /billing/meter/get-user-resource-summary` 的 `data.TotalCredits`（字符串小数）。

签发令牌会**轮换**：每次刷新都返回新的长效令牌，必须回写，否则下次用不了。

---

## 四、错误分类（决定要不要重试）

| 类型 | 触发 | 是否重试 | 界面文案 |
|---|---|---|---|
| `AuthFailed` | 401/403、令牌过期、设备指纹不一致 | 否，需重新导入 | 「凭据失效，请重新导入」 |
| `NotEligible` | 账号不参与活动（Qoder 免费号） | 否，不告警 | 「当前账号不参与该活动」 |
| `AlreadyDone` | 今天已领（幂等） | 不适用 | 「今天已签」 |
| `Failed` | 5xx、429、未知业务码 | 是 | 带平台原话 |
| `Network` | 超时、连不上 | 是 | 「网络不通」 |

`NotEligible` 与 `Failed` 必须分开：前者重试一万次也没用，后者重试一次可能就好了。
混在一起会出现「明明正常却天天报红」。

---

## 五、双端互通协议（已实现）

**拓扑**：PC 服务端 + 手机客户端，局域网内直连，不经第三方。

```
手机                      PC
 │  GET  /api/ping         │  探测 + 版本
 │  POST /api/pair         │  提交 6 位配对码 → 换 session token
 │  GET  /api/state        │  账号 + 最新快照 + 汇总
 │  POST /api/checkin      │  { ids?: [] } → 结果列表
```

**实现要点**：`apps/desktop/src-tauri/src/lan.rs` + `assets/lan.html`（单文件页面，
编译期嵌入）。手机端**不需要装 App**，用浏览器打开即可。

**安全要求与实际做法**：

| 要求 | 做法 |
|---|---|
| 只对内网开放 | 绑 `0.0.0.0`（多网卡），但每个连接先验来源 IP 是否私有网段，公网直接断开 |
| 配对码 | 6 位数字、5 分钟有效、**用一次即换新码** |
| session token | 32 字节随机 hex，只存 PC 内存（重启需重新配对），最多保留 16 台 |
| 只读模式 | `sync.allow_remote_checkin=false` 时 `/api/checkin` 返回 403 |
| 防重放 | **未做 HMAC**：token 仅在内网、仅 HTTP，且来源已被内网限制卡死；真要更强应该整体上 TLS，而不是加一层 HMAC |

**实测**（curl + 无头浏览器模拟手机）：错误码 400、无 token 401、配对拿 token、
`/api/state` 返回余额与今日进度、`/api/checkin` 对已签账号幂等返回 `alreadydone`。

**为什么不做云端**：凭据在 PC，签到必须在 PC 执行；上云等于把账号钥匙交给第三方，
而需求根本用不到跨网。

---

## 六、验收状态（v0.4.2）

- [x] `cargo test --workspace` 全绿；`cargo check --workspace` 零错误
- [x] 前端 `tsc --noEmit` 零错误，`vite build` 成功
- [x] 桌面端启动、数据目录可创建、CLI 全链路闭环
- [x] WorkBuddy 真实账号端到端：签到、余额、旅行均跑通
- [x] 发布版体积实测：exe 4.6 MB / APK 7.4 MB
- [x] 安卓出包，并在模拟器上验证安装与启动
- [ ] Redmi K80 Pro / Android 15 真机验证

---

## 七、下一步（按优先级）

1. **真机验证**：Android 15 安装 v0.4.2 APK；异常则抓 logcat。
2. **Trae / Qoder 补齐**：Qoder 领取端点需真实抓包；Trae 刷新令牌需设备凭据样本。
3. **自动更新**：Windows updater + Android 下载 APK 自装，源可配置。
4. **卸载清理**：安装包侧清理脚本（便携版本身不写注册表）。
5. 更细的当前快照与阻塞项见 `progress.md`。
