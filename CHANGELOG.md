# 更新日志

本项目的版本号遵循 [语义化版本 2.0.0](https://semver.org/lang/zh-CN/)：
不兼容变更升 MAJOR，向下兼容的新功能升 MINOR，向下兼容的问题修正升 PATCH。

> **产物不覆盖**：`dist/` 里已出的包视为交付历史，构建脚本拒绝覆盖或删除。
> 要再出一版，先升 `Cargo.toml` 的 `[workspace.package] version`。

## [0.11.0] - 2026-11-27

**新增单实例启动**（新能力，升 MINOR），同时修复两处图标问题、重打安卓包。

- 单实例：重复双击只保留一个进程，第二次启动把已有窗口唤起置前
  （托盘最小化状态也会恢复）；依赖官方 `tauri-plugin-single-instance`。
- 清晰度：桌面与任务栏图标改为从 256px 母图裁剪后缩小，不再把小图放大 1.25 倍。
- 对齐：积分图标由「底部对齐」改为基线对齐，视觉中心与数字一致；手机页同步。
- 安卓：APK 由 0.6.0 重打到 0.11.0，包含新启动图标、平台 logo、积分图标与新 UI；
  构建链的 Windows 兼容问题（非 ASCII 路径、`node.bat`、`node tauri` 入口）全部脚本化。

**测试**：`cargo test --workspace` 90 项、前端 20 项、`tsc --noEmit`、`vite build`、
`gen-app-icons.py --check` 全部通过；单实例脚本实测第二个进程 0.2 秒退出；APK
`apksigner verify` 通过（versionName 0.11.0 / versionCode 11000 / targetSdk 36）。

## [0.10.1] - 2026-11-27

**修复：覆盖升级后桌面快捷方式仍显示旧图标**（用户可见故障，升 PATCH）。

- 根因：图标本身已换新，但资源管理器按 `.lnk` 缓存图标，覆盖安装后不会自动失效。
- 安装包：NSIS `POSTINSTALL` 钩子调用 `ie4uinit.exe -show` / `-ClearIconCache`
  重建图标缓存（失败不影响安装）。
- 程序：首次以新版本启动时通过 `SHChangeNotify` 通知 shell 重读桌面（用户/公共）
  与开始菜单三个快捷方式，每版本只做一次，不产生启动闪烁。

## [0.10.0] - 2026-11-27

**新增：图标体系（应用图标 / 平台 logo / 积分单位图标）**（新增功能，升 MINOR）。

### 新增

- **应用图标换新**：完整底板图标用于桌面、任务栏、安装包与 Android launcher；
  透明标记用于侧栏、启动页、托盘、favicon。深色主题与深色任务栏自动用 `*-dark`
  提亮版标记。
- **任务栏图标放大**：Windows 图标按 1.25 倍裁进底板并重新倒圆角——源图四周留白约
  7%，直接缩到 24px 任务栏里图形太小。
- **平台 logo**：总览面板头（20px）、已纳管列表（24px）、导入平台选择、平台能力表、
  悬浮球分组；未收录平台回退成识别色圆点，白底 logo 加 1px 轮廓保证可见。
- **积分单位改图标**：数字后的「分」全部替换为彩色图标（合计/总值青铜、平台小计墨、
  余额/新增玉）；覆盖主窗口 KPI、账号余额、活动增量、趋势悬停、悬浮球与手机页
  （手机页 base64 内联，不发额外请求）。
- **图标生成脚本**：`scripts/gen-app-icons.py` 从用户提供的 PNG 生成 bundle / ICO /
  Android mipmap / 托盘图标，`--check` 做逐像素与留白断言。

### 修复

- **改了图标任务栏不变**：`build.rs` 补 `rerun-if-changed=icons`，`lib.rs` 启动时显式
  `set_icon(icons/icon.png)`；此前只有构建脚本重跑才会更新内嵌图标。
- **预览脚本漏编**：`scripts/preview.ps1` 把 `icons/`、`assets/` 纳入编译输入，
  改图标或 `lan.html` 会自动重编。

### 测试

- 新增前端资产 / 图标 / 积分测试，合计 18 项（尺寸、留白、平台映射覆盖、重编链路、
  无残留「分」）。
- `cargo test --workspace` 90 个全绿；`cargo check --workspace` 与桌面端 `cargo check`
  通过；`tsc --noEmit` + `vite build` 通过。

## [0.9.2] - 2026-11-26

**修复：浏览器授权导入同一账号时没有合并旧记录**（问题修正，升 PATCH）。

### 修复

- **浏览器授权导入后仍留两条**：Trae 本机令牌的 JWT 只有 `data.id`、没有 `sub`，
  身份解析一直返回空，新凭据匹配不到旧记录。`identity` 现在先读 `data.id`，
  再退回 `sub`。
- **旧记录没有身份就永远合不上**：新增 `Engine::adopt_identities`，导入前先用旧凭据
  问一次平台（Trae 读 JWT，Qoder 走 userinfo），能对上同一个 uid 就先补写身份，
  随后的 upsert 才能合并。本机导入 / 浏览器授权 / 手动令牌三条路径都调用。
- **同令牌旧记录兜底**：`upsert_account` 对同一平台、整串令牌完全相同的旧记录
  一并合并，避免令牌轮换前留下的重复项。

### 测试

- 新增 `adopt_identity_merges_legacy_trae_record`、`uid_from_jwt_reads_data_id`。
- `cargo test --workspace` 90 个全绿；桌面端测试与前端 `tsc + vite build` 通过。
- 用真实数据副本验证：导入前两条（本机无身份 / 网页有身份）→ 导入后一条。

## [0.9.1] - 2026-11-26

**修复：本机导入的账号没有平台用户 id，重复导入会新建记录**（升 PATCH）。

- `local_import` 从 Trae 登录态 JSON 读取 `userId`、从 Qoder session 读取
  `user_id / userId / uid`，写入 `Credential.platform_uid`。
- `Provider::identity` 增加默认实现，Qoder 用 `/api/v1/userinfo` 反查 uid。
- `upsert_account` 在比对前先用已存凭据补齐旧记录身份（`backfill_platform_uid`）。

## [0.9.0] - 2026-11-26

**新增：Trae CN / Qoder CN 浏览器授权与令牌续期**（新功能，升 MINOR）。

### 新增

- **Trae CN 浏览器授权**：打开 `https://www.trae.cn/authorization`，本地
  `127.0.0.1` 随机端口接回调，用 PKCE 换 `POST /trae/api/v3/oauth/ExchangeToken`；
  同时生成 ECDSA P-256 设备密钥，续期时对
  `POST\n<path>\n<client_id>\n<refresh>\n<ts>\n<nonce>` 签名。
- **Qoder CN 浏览器授权**：`device/selectAccounts` 打开授权页，按 2 秒轮询
  `deviceToken/poll` 换令牌；续期走 `deviceToken/refresh`，身份走 `userinfo`。
- **Token 续期**：Trae / Qoder 的能力矩阵改为 `token_refresh: true`；
  只在 `expires_at - now < 300` 秒时才续期，避免每次操作都多打一次上游。
- **凭据模型**：新增 `platform_uid`、`machine_id`、`device_key`（Trae 私钥 PEM）、
  `refresh_token_id`（Qoder），均只在加密仓内保存；`Debug` 对设备字段脱敏。
- **客户端加密原语**：新增 `oauth`（PKCE / base64url / 随机数）与 `client_crypto`
  （DPAPI、AES-256-GCM、AES-128-CBC、SHA-512）。
- **界面**：删除「Trae/Qoder 还没接入官方授权」的警告，浏览器授权文案改为通用。

### 测试

- PKCE RFC 7636 测试向量、`uid_from_jwt`、本机导入明文 / 密文夹具、
  Qoder DPAPI + GCM 夹具、Trae 设备校验与 9074 映射等。
- `cargo test --workspace` 88 个全绿（0.9.0 时点）。

## [0.8.0] - 2026-11-26

**新增：Trae CN / Qoder CN 端到端**（新功能，按 SemVer 升 MINOR）。

### 新增

- **Trae CN 真实签到与余额**：`Authorization: Cloud-IDE-JWT` + `X-Device-Id` +
  `X-User-Region: CN`；状态/领取走 `/trae/api/v2/ug/checkin_credits/{status|claim}`，
  余额取 `/trae/api/v2/pay/ide_user_ent_usage` 的 `total_balance`。
  领取前先查状态，避免 Trae 对「今天已领」重复返回成功造成误报。
- **Qoder CN 真实签到与余额**：活动制，`GET /sash/api/v1/me/campaigns` →
  `POST /sash/api/v1/me/campaigns/{id}/claim`；余额为各配额桶 `remaining` 之和。
  空响应按最终活动状态判定；免费账号也能领（D07 修正）。
- **本机导入扩展到三平台**：新增 Trae 登录态解密（AES-128-CBC + SHA-512 校验，
  设备 id 从 `iCubeAuthInfo://icube-dc:` 取）与 Qoder 登录态解密
  （`Local State` 的 DPAPI 密钥 + `auth.v1.dat` 的 AES-256-GCM）。
  仍然只读、不修改客户端文件、不打印令牌。

### 变更

- 引擎只在平台声明 `token_refresh = true` 时调用刷新；Trae / Qoder 直接用 access token，
  不再产生「刷新失败但沿用旧令牌」的日志噪声。
- 能力矩阵：Trae CN / Qoder CN 的签到与余额标记为「已实机」。

### 测试

- 新增 12 个单元测试：Qoder 活动筛选/余额汇总/空响应、Trae 设备校验/9074 映射、
  Trae 明文与密文登录态、Qoder DPAPI+GCM 夹具、过期令牌拒绝、AES/SHA 原语。
- `cargo test --workspace` 77 个全绿；桌面端 6 个通过；前端 `tsc + vite build` 通过。

## [0.7.0] - 2026-11-26

**新增：积分自动刷新**（新功能，按 SemVer 升 MINOR）。

### 新增

- 设置页新增「积分自动刷新」：关闭 / 5 / 10 / 30 分钟，默认 10 分钟；
  只在电脑端显示，手机端不出现。
- 主窗口可见且未最小化时按间隔自动采样；隐藏或最小化后暂停，
  重新可见且已超间隔则立刻补一次。
- 自动刷新与手动「刷新积分」走同一条代码路径（`run_refresh_inner`），
  完成后通知界面重拉状态，不靠前端轮询。
- 后台循环每 30 秒只做一次判断，间隔为 0 时完全关闭，不空转请求。

### 测试

- 新增 3 个单元测试：0 表示关闭、9/10 分钟边界、自定义 30 分钟间隔。
- `cargo test --workspace` 与桌面端测试全绿，前端 `tsc + vite build` 通过。

## [0.6.0] - 2026-11-26

**新增：安卓每日自动签到 + 双端通知**（新功能，按 SemVer 升 MINOR）。

### 新增

- **安卓定时签到**：WorkManager 每天 00:16 触发后台任务，经 JNI 调共享引擎执行；
  远程模式 / 未开调度 / 没有到点账号时静默跳过，不打扰用户（D17）。
- **打开 App 补签**：启动约 12 秒后补一次当天漏掉的签到，
  兜住 Doze 与厂商省电策略。
- **安卓签到通知**：新增「签到结果」通知渠道，首次进入前台申请 Android 13+
  通知权限；后台任务直接发系统通知，Windows 沿用原有通知。
- **构建链路**：新增 `scripts/sync-version.ps1`，安卓包版本号随 Cargo.toml 同步；
  `scripts/patch-android-sources.ps1` 把 `apps/desktop/android/` 的 Kotlin 源
  幂等注入 `gen/`，并补权限、依赖、R8 keep 规则与 MainActivity 调用。

### 修复

- **WorkManager 被 R8 裁掉导致启动崩溃**：Room 与 Worker 靠类名反射创建，
  默认优化会把它们移除；补 keep 规则后正常。
- **后台任务读错数据目录**：安卓上 Tauri 的 `app_data_dir` 解析为
  `context.dataDir`，不是 `filesDir`；按错路径打开会得到一个空引擎。

## [0.5.0] - 2026-11-26

已发布：GitHub Release `v0.5.0`（便携 exe 5.0 MB / 安装包 2.0 MB / APK 7.8 MB，arm64）。

### 新增

- **卸载清理**：安装包卸载前默认删除账号、令牌、日志、自启、快捷方式、计划任务
  与 WebView2 缓存；展开确认页可勾选「保留账号数据」。只删白名单数据文件，
  不误伤程序文件。
- **数据目录跟随安装目录**：发行版默认 `<exe 所在目录>\data`，安装到别的盘、
  便携版换位置，数据都跟着走；旧版首次启动把 `%LOCALAPPDATA%\PointHub`
  的数据搬过去并清空原位置，迁移失败则继续用旧目录。调试版保持旧位置。
- **自动更新**：设置页新增「更新」面板——检查 GitHub Releases、下载安装包、
  手动下载兜底；Windows 下载 `_Setup.exe` 后打开安装向导，安卓跳浏览器下载 APK；
  更新源可切镜像前缀；启动时自动检查（只发通知，不自动安装）。
  版本比较与安装包选择在 `crates/ph-update`。
- **本机导入（仅 Windows）**：直接读取本机已登录的 WorkBuddy 客户端登录态
  （`%USERPROFILE%\.workbuddy\checkin-rt.json`），只读、不改客户端文件；
  导入前联网验证一次余额，验证不过不落库。Trae / Qoder 如实标注暂不支持。
- **账号去重**：浏览器授权 / 本机导入 / 手动令牌统一走 upsert，按 JWT `sub`
  识别同一账号，重复导入只更新凭据；历史重复记录在下次导入时合并；
  非 JWT 令牌按整串完全相同识别，禁止按令牌片段猜。
- **安卓 App 双模式**：本机模式自行管理账号；连接电脑模式经持久设备配对
  拉取电脑的账号/快照/历史/活动，并可远程签到。凭据不出电脑。
- **双端同步协议**：账号元数据、积分快照、签到历史、活动记录双向同步；
  同步载荷不含任何令牌（`GET /api/sync/pull`、`POST /api/sync/push`）。

### 修复

- 卸载不再遗漏 `history.jsonl.bak-*` 等滚动备份，避免 `data` 目录删不干净。
- **Android 17 手机连不上电脑**：系统「本地网络保护」对 targetSdk 37 应用
  静默丢弃局域网 TCP，表现为超时。构建脚本固定 `targetSdk = 36` 后真机通过。
- 连接失败时输出底层原因与 TCP 自检结果，便于区分网络问题与服务问题。

## [0.4.2] - 2026-11-25

**修复：安卓仍闪退——Rust 的 JNI 入口根本没生成**（按 SemVer 升 PATCH）。

### 根因

`lib.rs` 的 `run()` 缺了 `#[cfg_attr(mobile, tauri::mobile_entry_point)]`。
这个宏负责导出 `Java_cn_pointhub_desktop_Rust_onCreate` 等 JNI 函数，没有它，
`libpointhub_lib.so` 照常加载、但符号表里找不到入口：

```
java.lang.UnsatisfiedLinkError: No implementation found for
  void cn.pointhub.desktop.Rust.onCreate(cn.pointhub.desktop.WryActivity)
  at cn.pointhub.desktop.WryActivity.onCreate(WryActivity.kt:136)
```

JVM 在 `onCreate` 就抛异常，进程直接退出——连 Rust 的 `run()` 都没进过，
所以之前修的「数据目录」那条路径其实一直没被执行到（它也是错的，但排在后面）。

### 改法

- `run()` 补上 `#[cfg_attr(mobile, tauri::mobile_entry_point)]`
- 顺带修正了属性与文档注释放错位置的问题

### 验证

在雷电模拟器（x86_64 / Android 9，走 ARM 翻译层）上实测：

- 安装成功，启动后进程存活、`mCurrentFocus` 是本应用
- JNI 全部解析成功（`trampoline_addr` 非 0），日志出现 `Rust_onPageLoaded`
- 前端在跑：调到了 `notification` 插件的 `checkPermissions`
- `llvm-nm -D` 确认 so 已导出 `T Java_cn_pointhub_desktop_Rust_onCreate`

另外把 debug 包的 native 调试符号剥掉，体积从 322 MB 回到 148 MB。

### 体积优化（同一版本，仅打包方式变化）

debug 包（148 MB）能装但太大。改用 release 构建后 **7.4 MB**（缩小约 20 倍）：

- release 默认 strip + R8 + 已有 LTO profile，无需额外配置
- 自用分发，release 用 **debug keystore** 签名（`apksigner verify` 通过），
  不上架应用商店，省去管理私钥；要上架时再换正式 keystore
- `gen/` 被 git 忽略，签名配置没法只改本地；新增受控脚本
  `scripts/patch-android-gradle.ps1`，每次构建前幂等打补丁（含去重清理）
- 构建命令：`scripts\build-android.bat release --target aarch64`

产物：`dist\积分枢纽PointHub_v0.4.2.apk`（7.4 MB）。
模拟器复验：安装成功、进程存活、`mCurrentFocus` 为本应用，
0 条 FATAL / UnsatisfiedLinkError，`Rust_onPageLoaded` 出现 1 次。

## [0.4.1] - 2026-11-25

**修复：安卓端一打开就闪退**（按 SemVer 升 PATCH）。

### 根因

安卓的数据目录写死成 `/data/data/com.pointhub.app/files`，而实际包名是
`cn.pointhub.desktop`——那个目录不存在也没有权限，`create_dir_all` 失败，
`Engine::open` 直接报错，启动代码随后 `exit(1)`。表现就是点开图标立刻退出，
连界面都看不到。

### 改法

- 引擎初始化从 `run()` 挪进 `setup()`：安卓的目录只有拿到 `AppHandle`
  才能问系统要（`app.path().app_data_dir()`），在 `run()` 阶段根本拿不到
- 目录优先级：命令行 `--data-dir` > 安卓应用私有目录 > 各平台默认
- 初始化失败时返回错误而不是 `exit(1)`，至少能在日志里看到原因
- `ph-store` 里那个占位路径同步改成正确包名，并注明正常走不到

桌面端行为不变（`--data-dir` 仍然优先），已回归验证：启动、局域网服务、
`/api/ping` 均正常。

## [0.4.0] - 2026-11-25

**新增：手机上也能看账本、点签到**（新功能，按 SemVer 升 MINOR）。

### 手机访问（局域网互通）

电脑上起一个小服务，手机和电脑连同一个 Wi-Fi 时，用手机浏览器打开地址即可。
不做云端——凭据在电脑上，签到必须在电脑执行，上云等于把账号钥匙交给第三方。

```
GET  /                    手机页面（单文件，跟随系统深浅色）
GET  /api/ping            探测 + 版本
POST /api/pair            {code} → {token}
GET  /api/state           账号 + 余额 + 今日进度（需 token）
POST /api/checkin         一键签到（需 token，且电脑上允许）
```

安全边界三条：

- **只认内网来源**：服务绑 `0.0.0.0`（家里可能多块网卡），但每个连接先验来源 IP，
  公网地址直接断开；
- **配对码**：6 位数字、5 分钟有效、用一次就换新码；配对后换成 32 字节随机 token；
- **只读开关**：关掉「允许手机签到」后，`/api/checkin` 直接 403，手机只能看。

设置页新增「手机访问」面板：显示访问地址、当前配对码与剩余时间，可复制地址。
安卓端本身就是手机，不显示这个面板。

### 移动端界面适配

之前窄窗口只是把侧栏 `display:none`——**导航直接没了**，手机上没法切页面。

- 侧栏改成**底部标签栏**：三个标签均分、高 50px，徽标移到右上角
- 单列布局，KPI 两列；输入框字号 16px（小于它 WebView 聚焦时会放大页面）
- 按钮最小 44/40px，照顾手指；底部留 `env(safe-area-inset-bottom)` 给手势条
- `100dvh` 适配移动端地址栏收起；补 `theme-color` 让状态栏跟主题

实测（WebView2 模拟 390×844 / 360×640 / 844×390 横屏）：三个页面均无横向溢出，
底部标签各 121×50，手机页面在 390 宽下 `scrollWidth == innerWidth`。

## [0.3.2] - 2026-11-25

**修复：积分余额显示错误**（按 SemVer 升 PATCH）。

### 根因

余额取自签到活动接口的 `total_credits`，但那是**本季活动累计得分**，不是账户余额：
实测连签 36 天 × 100 = 3600，而账户实际可用是 201.76。数字看着像余额，其实一直在
往上加，越签越离谱。

### 变更

- 余额改走 `POST /billing/meter/get-user-resource-summary` 的 `data.TotalCredits`
  （与官网同一口径），签到活动接口只用来读「今日是否已签」「连签天数」
- 该字段是**字符串小数**（`"204.62000029"`），原来只按整数解析会一路失败；
  新增 `pick_f64`，`pick_i64` 的字符串分支也兼容小数
- 余额类型 `i64` → `f64`：接口的可用积分本来就有小数位，
  界面按「最多两位小数」格式化（`204.62` / `3,600`）
- 剔除历史里 3 条被误写成 3600 的快照（原文件备份为 `history.jsonl.bak-20261125`）

## [0.3.1] - 2026-11-25

**修复：浏览器登录后没有任何反应**（按 SemVer 升 PATCH）。

### 根因

0.3.0 去网页的 `localStorage` 里找 JWT，但实测 WorkBuddy 网页版**根本不把令牌放在那**：

- 登录态的键名扫出来全是埋点数据（`__BEACON_*`、`aegis_session_id`、`loggedIn`）；
- 真正的凭证是 **HttpOnly 的 `session` cookie**，页面脚本读不到；
- 而 cookie 是随请求轮换的网页会话，拷进软件里用不了几次就失效。

所以程序一直在「等一个永远不会出现的键」，表现出来就是：登录完成了、软件毫无反应、
浏览器也不关。

### 变更

- **改用官方插件授权**（客户端插件登录用的同一套流程）：
  `POST /v2/plugin/auth/state` 拿授权链接 → 在浏览器里确认 → 轮询
  `GET /v2/plugin/auth/token` 取回令牌
- **用你自己的浏览器打开**：不再启动专用浏览器实例，所以在现有窗口里新开标签页，
  不会再弹一个陌生窗口；登录过一次浏览器就记住了
- **拿的是长效令牌**（refreshToken），能自动续期，不是一次性的网页会话
- 授权成功后立刻读一次余额验证；失败会如实写进 `logs/login.log`
- 删除 CDP / WebSocket 相关代码与 `tungstenite` 依赖
- 界面：浏览器没被拉起来时，把授权链接显示出来供手动复制
- 其它平台（Trae CN / Qoder CN）暂未接入该流程，界面会明确提示改用手动令牌，
  不再给一个点了没反应的按钮

## [0.3.0] - 2026-11-25

**登录改用系统浏览器**（替换内嵌窗口，按 SemVer 升 MINOR）。

### 变更

- **浏览器登录改用电脑上的 Edge / Chrome**，不再弹内嵌窗口
  - 原因：WebView2 在部分机器上整片空白（页面确实加载了，但不绘制），
    且内嵌窗口的会话和用户平时的浏览器是两套，等于每次都要重新登录
  - 做法：用**独立浏览器配置目录**启动 Edge/Chrome 并开本机调试端口，
    通过 CDP 在页面里执行只读脚本取回令牌；不开 DevTools、不用 F12、不用复制粘贴
  - 调试端口只绑 `127.0.0.1`；配置目录独立，不碰用户平时浏览器的登录状态；
    登录过一次会记住，下次不必重登
  - 取到令牌后自动关闭这个浏览器实例（只杀自己 spawn 的进程树）

### 修正

- **一键启动预览变慢**：原来每次都跑 `npm run build` + `cargo build`。现在按文件时间判断，
  没改动就直接跑 exe（实测二次启动不再编译，秒开）；逻辑挪到 `scripts/preview.ps1`，
  `.bat` 只剩 6 行壳
- CDP 读取两处实测坑：`read_to_end` 在超时那刻会丢弃已读数据（改为按 `Content-Length` 收）；
  `tungstenite::connect` 拿不到底层流，读无超时会把线程挂死（改为自建 TcpStream + 读超时）

## [0.2.0] - 2026-11-25

**新增浏览器登录导入**（向下兼容的新功能，按 SemVer 升 MINOR）。

### 新增

- **浏览器登录导入账号**：账号页两条路，浏览器登录（推荐）/ 手动粘贴令牌
  - 内嵌官方登录页，注入**只读**脚本从 `localStorage` 取 JWT 形态令牌，
    连同 `navigator.userAgent` 与 `origin` 一并回传
  - 回传走地址哈希（`history.replaceState`）：不需要远程 IPC、不需要本地 HTTP 服务
  - 令牌来自浏览器，网关会校验「会话与 UA 绑定」，故 WorkBuddy 适配器的
    所有请求都原样回放导入时存下的 UA
  - 只认可信接口域名（`codebuddy.cn` / `trae.cn` / `qoder.com`），
    SSO 跳转域名不会被误当接口域名
  - 导入后立刻读一次余额验证；失败直接说原因，不给「导入成功但用不了」的假象
  - 20 秒仍无令牌时回传页面存在的存储键名（只报名不报值），用于定位
    「令牌藏在别的键名下」
- `1. 一键启动预览.bat`：不打包、直接用当前源码编译并启动，验收改动不必先出包
- `--login <平台>` 启动参数：启动即打开登录窗口（排查登录流程用）

### 修正

- **构建卡死**：中文路径下 `target/debug/incremental` 的锁文件删不掉（`os error 5`），
  rustc 会一直等锁——表现为编译无输出、CPU 不涨、最后 exit 1 且无错误信息。
  已在 `.cargo/config.toml` 关闭增量编译根治，判定方法记在 `docs/BUILD-WINDOWS.md`
- 登录窗口诊断信息落文件 `<数据目录>/logs/login.log`：发布版没有控制台，
  `eprintln!` 等于丢掉，出问题时无从查起
- 构建脚本自身：改为纯 ASCII（PS 5.1 按 ANSI 读无 BOM 的 `.ps1`，中文注释会吃掉引号）；
  同步版本号改用 `[regex]::Replace`（PS 5.1 的 `-replace` 不支持脚本块，
  会把脚本块源码写进 JSON）

## [0.1.0] - 2026-11-25

首个可用骨架。目标是「链路能跑通、后面能长东西」，不是功能齐全。

### 新增

**核心（Rust，双端共用）**

- 领域模型：平台、账号、凭据、快照、签到结果、旅行状态机
- 凭据加密仓：Windows DPAPI（绑定当前用户）+ AES-256-GCM 本地密钥回退
- 数据仓：账号、设置、积分历史（JSONL 追加写，写入走临时文件 + 改名）
- 平台适配：WorkBuddy（签到 / 余额 / 令牌自续期 / 派猫猫旅行）、
  Trae CN（签到 / 余额，待实测）、Qoder CN（结构就位，端点待实测）
- 引擎：单账号与全量采样、单账号与一键签到、令牌轮换回写、签到后复核、今日新增推算
- 调度：按平台刷新时刻设默认时间 + 确定性抖动（同一天恒定）、错过补签判定
- 活动流水：签到 / 派猫猫各记一条 `events.jsonl`，界面「最近活动」直接读它
- 趋势：`daily_series` 按天汇总余额并算出每日新增（前一天无数据时留空，不硬填 0）

**Windows 桌面端**

- Tauri 2 外壳：托盘菜单（打开 / 一键签到 / 悬浮球 / 退出）
- **常驻调度循环**：每 45 秒核对一次到点账号，串行执行并复用与手动签到同一条代码路径；
  不补签时只认「到点后 30 分钟」窗口；执行完推 `ph://changed` 让界面刷新，不靠前端轮询
- 桌面悬浮球：透明置顶、可拖动（位移超过 5px 才拖，轻点不误触）
  - 球面默认显示合计，也可在设置或展开面板里绑定到某个账号
  - **单击展开**：按平台分组列出全部账号积分与小计，点某行即切换球面账号
  - 圆环显示今日完成度；双击回主窗口；点击桌面其它位置或按 Esc 收起
- 开机自启（当前用户，无需管理员权限）
- 签到完成 / 失败的系统通知
- 浅色 / 深色 / 跟随系统三态主题（主窗口与悬浮球各自生效）
- 总览：控制台布局——四格 KPI（合计 / 今日已签 / 今日新增 / 下一次自动）、
  按平台分组面板（平台小计 + 账号逐行、积分数字成列对齐）、近 7 日每日新增、最近活动
- 账号（导入 / 删除 / 启停）、设置
- 数据目录可自定义（复制 + 指针文件，重启生效）
- 启动参数 `--data-dir <路径>`：便携版把数据放自己旁边，也方便用演示数据验收界面

**命令行**

- `phctl`：doctor / account / status / checkin / travel / plan

### 已知限制

- Qoder CN 领取端点未实测，能力矩阵中如实标注「待实测」，不提供点了会报错的按钮
- Trae CN 的令牌刷新（v3 ExchangeToken 需设备私钥签名）未实现，过期需重新导入
- Android 端未开始（本机缺 JDK 与 Android SDK）
- 局域网互通、自动更新、卸载清理脚本未开始

### 测试

- `cargo test --workspace`：21 个测试通过（模型、加密往返、数据读写、调度边界、字段候选名）
- 前端 `tsc --noEmit` 与 `vite build` 通过
- 发布版实测：便携 exe 4.5 MB（4,751,360 字节），主进程工作集 76.7 MB / 私有内存 40.2 MB
- 界面按 1196×869 实窗截图验收（浅色 / 深色 / 悬浮球展开面板各一张），确认无重叠、裁切与对比度问题
