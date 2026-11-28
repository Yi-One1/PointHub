# 需求规格与实现台账（SPEC）

> 逐条对应你提的 23 条要求。状态只有三种：**已落地**（能跑、有产物）、
> **部分落地**（核心在，缺外壳/缺验证）、**未开始**（说清卡在哪）。
> 不做「看起来做了」的表述。

版本：v0.16.0 · 2026-11-29

---

## 一、需求对照

| # | 需求 | 状态 | 证据 / 卡点 |
|---|---|---|---|
| 1 | 定时自动领取每日积分 | 已落地 | Windows：外壳常驻循环每 45 秒核对（`spawn_scheduler`）；安卓：WorkManager 每天 00:16 触发后台签到，打开 App 约 12 秒后补签（`android_worker.rs`，D17） |
| 2 | 实时显示各 agent 各账号积分 | 已落地 | 总览按平台分组、账号逐行显示（各平台小计 + 合计 KPI）+ 悬浮球；`Engine::aggregate()` 实时汇总。自动刷新：主窗口可见时按 5 / 10 / 30 分钟采样，可关闭，默认 10 分钟（D18） |
| 3 | 同一 agent 多账号 | 已落地 | 账号模型按 id 独立，凭据各自加密，`phctl account add` 可加多个 |
| 4 | Windows + Android 双端互通 | 已落地 | 安卓 App 双模式（连接电脑 / 本机）；持久设备配对后可远程查看、远程签到，`/api/sync/pull` 与 `/api/sync/push` 双向同步账号、快照、历史、活动；凭据不跨电脑；每账号唯一执行端（D03 / D15）。安卓端 WorkManager 每天 00:16 自动签到（第 1 条）。Windows 防火墙：安装器自动写入按程序、仅本地子网的放行规则（D25） |
| 5 | 手动单个签到 + 一键全部签到 | 已落地 | 总览每行「签到」/ 顶部「一键全部签到」/ 悬浮球面板「一键签到」/ 托盘菜单 / `phctl checkin` |
| 6 | 浏览器授权 + 本机导入 | 已落地 | **浏览器授权**：WorkBuddy 走官方插件授权；Trae CN 走授权页 + 本地回调 + `ExchangeToken`；Qoder CN 走设备码轮询，三平台均可用，令牌可续期（D21）。**本机导入**：WorkBuddy / Trae CN / Qoder CN 均可（只读客户端登录态，验证余额后落库，仅 Windows）；手动令牌兜底。三种方式统一按平台用户 id 去重，旧记录导入时自动合并（D19） |
| 7 | 双端更新功能 | 已落地 | 检查/下载链路已通：启动自动查 GitHub Releases（可关）+ 设置页「检查」（查到新版本后原地变「更新」）+ 总览新版本提示可跳转；Windows 下载 `_Setup.exe` 后启动安装向导（**已在旧版上实测走通**）；安卓应用内下载 APK 并调起系统安装器，首次引导「安装未知应用」、返回后自动续装（D26）；来源可切镜像前缀。**未做**：Windows 静默自替换（不接 Tauri updater，D08） |
| 8 | 卸载干净无残留 | 已落地 | 安装包 NSIS 钩子调用 `pointhub.exe --purge-data` / `--purge-app`；默认清理账号、令牌、日志、自启、快捷方式、计划任务、WebView2 缓存与 `PointHub*` 防火墙规则，勾选「保留账号数据」则只删程序痕迹；静默卸载无法提权时跳过防火墙规则并在日志留痕；判定标准见 D10 / D25 |
| 9 | Windows 数据目录可自定义 | 已落地 | 发行版默认 `<exe 所在目录>\data`，旧数据首次启动自动迁移并清空 `%LOCALAPPDATA%\PointHub`（D20）；设置页「更换数据目录」→ 复制 + 指针文件 + 重启生效；`ph_store::locate` |
| 10 | 开机自启动 | 已落地 | `tauri-plugin-autostart`，设置项与系统状态双向对齐 |
| 11 | 桌面悬浮球显示积分 | 已落地 | 透明置顶窗口：球面可绑定指定账号或显示合计，单击展开按平台列出全部账号积分（含小计、可点行切换），圆环显示今日完成度，拖动阈值 5px，双击回主窗口 |
| 12 | 便携 exe / 安装包两种产物 | 已落地 | 便携 exe 可构建；`scripts/build-windows.ps1 -Installer` 出 NSIS 安装包（0.5.0 已出 `_Setup.exe`）；便携 exe 已存在时用 `-InstallerOnly` 单独补安装包 |
| 13 | Windows 浅色 / 深色 | 已落地 | 三态主题（浅色 / 深色 / 跟随系统），CSS 变量整套切换 |
| 14 | 安卓正常 / 深色 + 跟随系统 | 已落地 | 主题跟随系统；≤760px 走移动端布局：侧栏收成底部标签栏、单列、触控区 ≥40px、`safe-area` 留白。已在 390×844 / 360×640 / 844×390 三档实测无横向溢出 |
| 15 | 签到后系统弹窗提示 | 已落地 | Windows 接 `tauri-plugin-notification`；安卓新增「签到结果」通知渠道，首次进入前台申请 Android 13+ 通知权限，后台任务直接发系统通知 |
| 16 | 包名 `中文名英文名_v版本` + 语义化版本 | 已落地 | `积分枢纽PointHub_v<版本>.exe/.apk`；版本号单点来自 `Cargo.toml` 的 `workspace.package.version` |
| 17 | 体积小、占用少 | 已落地 | 便携 exe **5.4 MB**、安卓 release APK **8.5 MB**；`opt-level=z` + LTO + strip 已启用 |
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
├─ platform_uid    平台用户 id（导入去重用，D19）
├─ device_id       Trae 必需（X-Device-Id）
├─ machine_id      授权时的设备标识
├─ device_key      Trae 续期用的设备私钥 PEM（D21）
├─ refresh_token_id Qoder 续期需要的令牌 id
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

## 三、平台接口契约（三平台均已实机验证）

### WorkBuddy

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

### Trae CN

| 动作 | 方法 / 路径 | 鉴权 |
|---|---|---|
| 签到状态 | `POST https://api.trae.cn/trae/api/v2/ug/checkin_credits/status` | `Authorization: Cloud-IDE-JWT <token>` + `X-Device-Id` + `X-User-Region: CN` |
| 领取签到 | `POST .../checkin_credits/claim` | 同上 |
| 余额 | `POST /trae/api/v2/pay/ide_user_ent_usage` → `total_balance` | 同上 |
| 浏览器授权 | `https://www.trae.cn/authorization` + 本地回调 | PKCE |
| 换令牌 / 续期 | `POST /trae/api/v3/oauth/ExchangeToken` | 续期需 ECDSA P-256 设备签名（D21） |

坑：必须带真机 `X-Device-Id`，不一致返回业务码 9074；`X-User-Region: CN`
不能省。Trae 对「今天已领」重复 claim 仍返回成功，所以**先查 status 再 claim**。
身份：本机令牌的 JWT 没有 `sub`，用户 id 在 `data.id`。

### Qoder CN

| 动作 | 方法 / 路径 | 鉴权 |
|---|---|---|
| 活动列表 | `GET https://qoder.com.cn/sash/api/v1/me/campaigns` | `Bearer` |
| 领取 | `POST .../campaigns/{campaignId}/claim` | `Bearer` |
| 余额 | `GET /sash/api/v2/me/usage`（各配额桶 `remaining` 之和） | `Bearer` |
| 浏览器授权 | `https://qoder.com.cn/device/selectAccounts?challenge=&challenge_method=S256&machine_id=&nonce=` | PKCE |
| 轮询令牌 | `GET https://openapi.qoder.com.cn/api/v1/deviceToken/poll?nonce=&verifier=&challenge_method=S256` | — |
| 续期 | `POST /api/v1/deviceToken/refresh` | `Bearer` |
| 身份 | `GET /api/v1/userinfo` | `Bearer` |

坑：签到是**活动制**，不在窗口内返回空列表，按「无活动资格」处理，不重试不告警；
令牌不透明，身份只能问 userinfo。续期需要保存返回的 `refresh_token_id`。

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

## 五、双端互通（已实现）

**拓扑**：PC 服务端 + 手机客户端，局域网内直连，不经第三方。

安卓 App 有两种模式：

- **本机模式**：账号导入、签到、历史都在手机上；
- **连接电脑**：配对电脑后拉取电脑的账号与积分、请求电脑代签；
  凭据始终留在电脑，不跨设备（D15）。

```
手机                      PC
 │  GET  /api/ping         │  探测 + 版本
 │  POST /api/pair         │  提交 6 位配对码 → 换 session token
 │  GET  /api/state        │  账号 + 最新快照 + 汇总
 │  POST /api/checkin      │  { ids?: [] } → 结果列表
 │  GET  /api/sync/pull    │  账号元数据 + 快照 + 历史 + 活动（不含凭据）
 │  POST /api/sync/push    │  手机侧变更上行合并
```

**实现要点**：PC 侧 `apps/desktop/src-tauri/src/lan.rs` + `assets/lan.html`
（浏览器入口，编译期嵌入）；手机侧 `crates/ph-client` 与 `commands.rs` 的 remote 分支。

**配对持久化**：配对成功后 PC 发持久设备令牌，手机存本地，重启后自动重连；
配对码仍是 6 位、5 分钟有效、用一次即换新。手机浏览器入口保留，不装 App 也能用。

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

## 六、验收状态（截至 v0.16.0）

- [x] `cargo test --workspace` 106 个测试全绿；桌面端 16 项、前端 49 项通过；`cargo check --workspace` 零错误
- [x] 前端 `tsc --noEmit` 零错误，`vite build` 成功
- [x] 桌面端启动、数据目录可创建、CLI 全链路闭环
- [x] WorkBuddy 真实账号端到端：签到、余额、旅行均跑通
- [x] Trae CN 真实账号端到端：状态 / 领取 / 余额均跑通（领取前预检，避免重复「成功」）
- [x] Qoder CN 真实账号端到端：活动查询 / 领取 / 余额均跑通（免费号实测可领）
- [x] 本机导入：WorkBuddy / Trae CN / Qoder CN 三平台登录态解密与验证导入
- [x] 发布版体积实测：exe 5.4 MB / APK 8.5 MB
- [x] 安卓出包，并在模拟器上验证安装与启动
- [x] Redmi K80 Pro / Android 17 真机验证（targetSdk 36 后可连局域网）
- [x] 安卓 App 配对电脑、远程查看与远程签到真机跑通
- [x] 自动更新：v0.5.0 已发布 Release，能读到最新版本；检查 / 下载 / 镜像源配置均已落地
- [x] 卸载清理：NSIS 钩子 + 默认删除 / 勾选保留，隔离目录验证通过
- [x] 数据目录跟随安装目录：旧数据迁移、清空旧位置、删除/保留清理均验证
- [x] 安卓后台签到：WorkManager 任务注册成功，APK 内 JNI 符号与 4 个 Kotlin 类校验通过；真机按验收步骤确认
- [x] 积分自动刷新：3 个单元测试覆盖关闭 / 间隔边界 / 自定义间隔；全量测试与前端构建通过
- [x] Trae CN / Qoder CN 浏览器授权：授权页打开、回调 / 轮询、换令牌与续期均跑通（D21）
- [x] 同一账号跨导入方式去重：用真实数据副本验证「本机 + 网页」两条 → 导入后合并为一条
- [x] 图标体系：应用图标（exe / 任务栏 / 托盘 / 侧栏 / 启动页 / favicon / Android）、
      平台 logo、积分单位图标全部落地；前端 20 项资产测试与 `gen-app-icons.py --check` 通过
- [x] 图标清晰度与对齐：桌面/任务栏图标从 256px 母图缩小；积分图标基线对齐，
      视觉中心与数字一致（含手机页）
- [x] 单实例：重复启动只保留一个进程，第二次唤起并置前已有窗口（含托盘最小化），
      `scripts/test-single-instance.ps1` 实测第二个进程 0.2 秒退出
- [x] 安卓包重打到 0.11.0：versionName 0.11.0 / versionCode 11000 / targetSdk 36 /
      arm64-v8a / 8.3 MB，`apksigner verify` 通过
- [x] 防火墙：安装时按程序自动放行（仅本地子网）；安装包实测写入
      `PointHub LAN`，手机访问通过，原地升级不弹 UAC
- [x] 安卓应用内安装：验收专用包真机走通检查 → 下载 → 授权 → 系统安装器覆盖安装
- [x] 安卓凭据仓 Keystore：旧 AES 仓覆盖安装自动迁移，重启后仍可签到；
      系统密钥丢失时不阻塞启动（单元测试覆盖，真机未模拟系统恢复）
- [x] 安卓应用内安装更新：真机走通 检查 → 下载 → 授权「安装未知应用」→ 系统安装器
      覆盖安装 → 数据保留；release APK 含安装权限、FileProvider 与安装器类
- [x] 0.12.0 正式包：Windows exe FileVersion 0.12.0（5.3 MB / 安装包 2.2 MB）；
      APK versionName 0.12.0 / versionCode 12000 / arm64-v8a / 8.3 MB，
      `apksigner verify` 通过
- [x] 0.13.0 设置页改造：独立分类页（7 类）、设置搜索、改动即改即存、
      分区恢复默认按差异启用、检查更新原地变「更新」、总览新版本提示可跳转；
      `cargo test --workspace` 99 项、前端 46 项与 `tsc + vite build` 通过
- [x] 0.13.0 正式包：Windows exe FileVersion 0.13.0（5.4 MB / 安装包 2.2 MB）；
      APK versionName 0.13.0 / versionCode 13000 / arm64-v8a / 8.5 MB，
      `apksigner verify` 通过
- [x] 0.13.1 签到修复：执行端自愈、调度只签本机账号、失败退避与通知去重、
      总览「今日签到异常」；workspace 106 项、桌面端 16 项、前端 49 项与
      `tsc + vite build` 通过；真实数据副本验证执行端收回本机并恢复签到
- [x] 0.13.1 正式包：Windows exe FileVersion 0.13.1（5.4 MB / 安装包 2.2 MB）；
      安卓包沿用 0.13.0
- [x] 0.14.0（G1–G6）：workspace 136 项、桌面端 20 项、前端 115 项与
      `tsc + vite build` 通过；G6 各平台签到时间已验收（含两处清晰度问题修复后复验）
- [x] 0.14.0（G7）：最近活动「更多」弹窗（最多 48 条）、关于页作者 / 仓库 / Issues /
      MIT 许可，移除「退出程序」；workspace 136 项、桌面端 20 项、前端 117 项通过
- [x] 0.15.0（G8）：更新页检测到新版本时展示 Release 更新内容（轻量排版、窗内滚动）；
      修复趋势用例的午夜时间边界；workspace 136 / 桌面端 20 / 前端 118 通过
- [x] 0.16.0：签到金额兜底（Trae 无金额时用 daily_credit）；最近活动平台标签、
      「今天已签」停止写入并从历史展示中过滤；workspace 138 / 桌面端 20 / 前端 119 通过

---

## 七、下一步（按优先级）

1. **Windows 静默自替换**：需接 Tauri updater 签名链，暂不做（D08）。
2. 当前功能已全部完成并发布 0.16.0；后续维护看 `progress.md` 与 `DECISIONS.md`。
