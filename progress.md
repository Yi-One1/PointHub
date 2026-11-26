# 项目进展交接（progress）

> 给新对话看的**当前快照**：只写结论与可验证事实，过程细节见 `CHANGELOG.md`、
> `docs/SPEC.md`、`DECISIONS.md`。最后更新：2026-11-26，版本 **v0.7.0（已验收，产物已出、未发布）**。

仓库：`https://github.com/Yi-One1/PointHub`（`master`，本地与远端同步）
发布：<https://github.com/Yi-One1/PointHub/releases/tag/v0.6.0>（0.7.0 尚未发布）

---

## 已完成的事项

- **核心链路**：Rust 共享核心（模型 / 加密仓 / 数据仓 / 平台适配 / 编排引擎 / CLI）；
  WorkBuddy 每日签到、余额、派猫猫旅行均**真实账号跑通**。
- **多账号与调度**：独立凭据、独立排期、独立历史；逐账号签到 + 一键全部签到；
  默认 00:10 + 确定性抖动，启动补签。
- **Windows 桌面端**：托盘、悬浮球（只读，单击展开账号列表）、开机自启、系统通知、
  三态主题、可更换数据目录；总览 KPI / 按平台分组 / 近 7 日新增。
- **登录导入**：WorkBuddy 走**官方插件授权**（`POST /v2/plugin/auth/state` →
  用户自己的浏览器新标签页 → 轮询 `token` 拿 access + refresh），长效令牌自动续期；
  Trae CN / Qoder CN 仅手动令牌。三种导入方式统一按 JWT `sub` upsert（D19）。
- **余额口径**：取 `POST /billing/meter/get-user-resource-summary` 的
  `data.TotalCredits`（字符串小数，`f64`）；签到活动的 `total_credits` 是本季累计得分，
  不是余额（曾误显示 3600，实际 201.76）。
- **局域网互通**：PC 起 HTTP 服务（默认 47821），手机浏览器输 6 位配对码可看账本、
  按开关允许签到；来源 IP 限内网、配对码 5 分钟一次性、token 仅内存。
- **移动端界面**：≤760px 侧栏改底部标签栏，单列 + KPI 两列 + ≥40px 触控区 +
  `safe-area`；三档视口实测零横向溢出。
- **安卓端双模式**：本机模式自行管理账号；连接电脑模式经持久设备配对拉取
  账号 / 快照 / 历史 / 活动并远程签到，凭据不出电脑；`crates/ph-client` 承载协议，
  `GET /api/sync/pull` + `POST /api/sync/push` 双向同步（D03 / D15）。
- **本机导入**：只读 `%USERPROFILE%\.workbuddy\checkin-rt.json` 的 Offline 刷新令牌，
  导入前刷新 + 采样验证，通过才落库；Trae / Qoder 如实标注暂不支持（D16）。
- **Android 17 真机**：Redmi K80 Pro 连接超时源于「本地网络保护」对 targetSdk 37
  静默丢弃局域网 TCP；`scripts/patch-android-gradle.ps1` 固定 `targetSdk = 36` 后通过。
- **自动更新**：`crates/ph-update` 查 `releases/latest`、按语义化版本比较、选当前平台
  安装包并校验大小；设置页「更新」面板可检查 / 下载 / 手动下载；Windows 下载
  `_Setup.exe` 后启动安装向导，安卓跳浏览器下载 APK；源可切镜像前缀（D08）。
- **卸载清理**：NSIS 钩子在删程序前调用 `pointhub.exe --purge-data` / `--purge-app`；
  默认删账号、令牌、日志、自启、快捷方式、计划任务、WebView2 缓存，可勾选保留数据；
  只删白名单文件，不整目录删（D10）。
- **数据跟随安装目录（0.5.0）**：发行版默认 `<exe 所在目录>\data`；首次启动把
  `%LOCALAPPDATA%\PointHub` 的旧数据整体迁过去并清空原位置，失败则继续用旧目录；
  调试版仍用旧目录（D20）。
- **安卓定时签到（组 7，0.6.0）**：WorkManager 每天 00:16 触发 `CheckinWorker`，
  经 JNI 调 `android_worker.rs`（自建 tokio 运行时）执行签到；打开 App 约 12 秒后
  补签当天漏掉的；结果回 Kotlin 发「签到结果」通知。Kotlin 源在
  `apps/desktop/android/`，由 `scripts/patch-android-sources.ps1` 幂等注入 `gen/`。
- **积分自动刷新（0.7.0）**：设置页可选 关闭 / 5 / 10 / 30 分钟（默认 10）；
  主窗口可见且未最小化时按间隔采样，复用 `run_refresh_inner`；隐藏期间暂停，
  重新可见时补一次。3 个单元测试覆盖关闭与间隔边界。
- **发布（0.5.0 / 0.6.0）**：两个 tag 与 GitHub Release 均已上线；0.6.0 上传
  便携 exe 5.0 MB、Setup 2.0 MB、APK 8.0 MB。GitHub 会丢弃资产名里的非 ASCII
  前缀，仓库上显示为 `PointHub_v0.6.0_Setup.exe` 等；自动更新按 `_Setup.exe` /
  `.apk` 后缀匹配，不受影响。
- **收尾（本次）**：README / SPEC / ANDROID / DECISIONS / progress 与当前状态对齐；
  仓库体检（无密钥、无账号数据、无 `dist/` 与构建产物入库）；
  本地构建垃圾清理待用户授权（清单见「未完成的待办」）。

---

## 关键决策

1. **不做云端**：凭据在 PC，签到必须在 PC 执行；上云等于把账号钥匙交给第三方。
   手机端通过局域网配对（App 或浏览器）使用 PC 的账号，凭据不跨设备。
2. **不读浏览器 localStorage / Cookie**：实测 WorkBuddy 网页存储全是埋点键，
   真凭证是轮换的 HttpOnly `session` cookie；改用官方插件授权换 refresh token。
   本机导入读的是**客户端**存储，不是浏览器（D16）。
3. **SemVer 严格判定**：修用户可见故障 = PATCH（0.3.0→0.3.1→0.3.2、0.4.0→0.4.1→0.4.2）；
   新增手机访问 = MINOR（0.4.0）；数据目录改为跟随安装目录 = MINOR（0.5.0）。
   版本单点在 `Cargo.toml [workspace.package] version`，打包脚本同步其余三处。
4. **安卓自用分发**：release 用 debug keystore 签名；`gen/` 被忽略，
   签名与 strip 补丁放在 `scripts/patch-android-gradle.ps1`，构建前幂等执行。
5. **零新依赖的局域网服务**：手写 HTTP/1.1（`Connection: close`），没引 axum；
   tokio 只补 `net` / `io-util` feature。
6. **令牌不落日志、快照不填 0**：`null` 与 `0` 在界面上是两种文案。
7. **产物不覆盖**：`dist/` 视为交付历史，重出包必须先升版本（D09）。

详见 `DECISIONS.md` D01–D20。

---

## 未完成的待办

- [ ] **积分自动刷新**：窗口可见时每 10 分钟采样、可关闭（D18，需求 2 的剩余部分）。
- [ ] **Trae CN / Qoder CN 端到端**：用户 2026-11-26 授权由我获取前置材料。
      关键新事实：**Qoder 用户不是会员，但新注册账号可每日领 100 积分**，
      D07「免费账号无资格」的旧结论不成立，需按接口实际返回判断。
      Qoder 领取端点未知；Trae 的 ExchangeToken 需设备私钥签名样本（P01 / P02）。
- [ ] **自动更新「下载 → 安装」**：发现新版本已验证；真正的下载安装要等下一版发布时真跑一次。
- [ ] **Windows 防火墙**：首次监听 47821 若局域网连不上，需手动放行入站（管理员权限）。
- [ ] **清理大体积构建产物**（2026-11-26 用户选择暂不清理；需要腾空间时再确认）：
      `target/`（约 4.1 GB）、`apps/desktop/src-tauri/target/`（约 14 GB）、
      `apps/desktop/src-tauri/gen/android/app/build/`（约 3.5 GB，含 181 MB debug APK）、
      `.design/`（约 179 MB 内部日志）、`.workbuddy/`、空的 `.screenshots/`。
      均可再生或为内部笔记，不影响源码与已发布产物。

---

## 其它需记录的内容

### 路径与命令

| 项 | 值 |
|---|---|
| Windows 正式包 | `dist/积分枢纽PointHub_v0.7.0.exe`（5.0 MB）、`dist/积分枢纽PointHub_v0.7.0_Setup.exe`（2.0 MB）；0.6.0 已发布 |
| Android 正式包 | `dist/积分枢纽PointHub_v0.6.0.apk`（8.0 MB，arm64，debug 签名） |
| 数据目录 | 发行版 `<exe 所在目录>\data`（安装版为 `安装目录\data`）；旧版首次启动自动迁移并清空 `%LOCALAPPDATA%\PointHub` |
| 用户真机安装 | `D:\APP\积分枢纽PointHub\PointHub`，数据在 `D:\...\PointHub\data`——**不要动** |
| 局域网地址 | `http://<PC 内网 IP>:47821`，配对码见设置页 |
| 预览启动 | 双击 `1. 一键启动预览.bat`（按文件时间增量编译，无改动秒开） |
| Windows 打包 | `scripts\build-windows.ps1`（便携）；加 `-Installer` 同时出安装包，加 `-InstallerOnly` 只补安装包 |
| Android 打包 | `scripts\build-android.bat release --target aarch64`（首次需 `tauri android init`） |
| 排障 | `phctl doctor` / `phctl status --data-dir <目录>`；日志在 `logs/` |

### 验证过的证据

- `cargo test --workspace` 62 个测试全绿；`cargo check --workspace` 零错误；前端 `tsc` + `vite build` 通过。
- 数据目录迁移 / 卸载清理：在隔离临时目录用 release exe 验证——旧数据搬到
  `<exe>\data`、旧目录清空；保留模式不动 `data`；删除模式清掉 `data`、旧目录与自启项。
- `phctl status`：`余额 201.76 / 已签 / 连签 36`，与汇总接口一致。
- 手机端无头浏览器：错误码 400、无 token 401、重复用码 400、已签账号 `alreadydone`。
- APK：`apksigner verify` 通过（Android Debug 证书）；`aapt2` 显示
  `versionName=0.6.0`、`native-code=arm64-v8a`；`llvm-nm` 可见
  `Java_cn_pointhub_desktop_CheckinBridge_runScheduledCheckin`。
- Release：匿名访问 `releases/latest` 返回 200、`tag_name=v0.6.0`，三个资产状态 `uploaded`。

### 隐私与仓库卫生

- 仓库体检（2026-11-26）：87 个跟踪文件中未发现令牌、私钥、邮箱或账号数据；
  git 包体仅约 487 KB；`dist/`、`target/`、`gen/`、`.design/`、`.workbuddy/` 均不入库。
- `%LOCALAPPDATA%\PointHub\browser-profile`（旧 CDP 方案残留，含当时 cookie）不在仓库内。
- 截图 / 点击类开发脚本放在 `scripts/dev/`，仅本地保留、不入库。

### 接手时的第一件事

先跑 `phctl doctor` 与 `cargo test --workspace` 确认环境，再从「未完成的待办」取任务；
改代码前先确认是否要升版本号，改完同步 `CHANGELOG.md` 与 `progress.md`。
