# 更新日志

本项目的版本号遵循 [语义化版本 2.0.0](https://semver.org/lang/zh-CN/)：
不兼容变更升 MAJOR，向下兼容的新功能升 MINOR，向下兼容的问题修正升 PATCH。

> **产物不覆盖**：`dist/` 里已出的包视为交付历史，构建脚本拒绝覆盖或删除。
> 要再出一版，先升 `Cargo.toml` 的 `[workspace.package] version`。

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
