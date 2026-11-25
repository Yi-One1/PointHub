# 项目进展交接（progress）

> 给新对话看的**当前快照**：只写结论与可验证事实，过程细节见 `CHANGELOG.md`、
> `docs/SPEC.md`、`DECISIONS.md`。最后更新：2026-11-26，版本 **v0.4.2**。

仓库：`https://github.com/Yi-One1/PointHub`（`master`，已与本地同步）。

---

## 已完成的事项

- **核心链路**：Rust 共享核心（模型 / 加密仓 / 数据仓 / 平台适配 / 编排引擎 / CLI）；
  WorkBuddy 每日签到、余额、派猫猫旅行均**真实账号跑通**。
- **多账号**：独立凭据、独立排期、独立历史；逐账号签到 + 一键全部签到；
  默认 00:10 + 确定性抖动，启动补签。
- **Windows 桌面端**：托盘、悬浮球（只读，单击展开账号列表）、开机自启、系统通知、
  三态主题、可更换数据目录；总览 KPI / 按平台分组 / 近 7 日新增。
- **登录导入**：WorkBuddy 走**官方插件授权**（`POST /v2/plugin/auth/state` →
  用户自己的浏览器新标签页 → 轮询 `token` 拿 access + refresh），长效令牌自动续期；
  另有手动令牌兜底。Trae CN / Qoder CN 仅手动令牌。
- **余额口径**：走 `POST /billing/meter/get-user-resource-summary` 的
  `data.TotalCredits`（字符串小数，已支持 `f64`）；签到活动里的 `total_credits`
  是**本季活动累计得分**，不能当余额（曾误显示 3600，实际 201.76）。
- **局域网互通**：PC 起 HTTP 服务（默认 47821），手机浏览器输入 6 位配对码访问，
  可看账本、可按开关允许签到；来源 IP 限内网、配对码 5 分钟一次性、token 仅内存。
- **移动端**：≤760px 侧栏改底部标签栏（此前 `display:none` 导致导航消失），
  单列 + KPI 两列 + ≥40px 触控区 + 16px 输入框 + safe-area；三档视口实测零横向溢出。
- **安卓端**：工程可出包；曾两次闪退，均已定位修复（数据目录写死错误包名 +
  `run()` 缺 `#[cfg_attr(mobile, tauri::mobile_entry_point)]` 导致 JNI 符号缺失）；
  雷电模拟器实测启动正常。release 包 **7.4 MB**，`dist/积分枢纽PointHub_v0.4.2.apk`。
- **安卓双模式（组 2）**：本机模式自行导入签到；连接电脑模式经持久设备配对拉取
  电脑账号/快照/历史/活动并远程签到，凭据不出电脑；`crates/ph-client` 承载协议。
- **本机导入（组 3）**：读取 `%USERPROFILE%\.workbuddy\checkin-rt.json` 里的
  Offline 刷新令牌（只读），导入前刷新+采样验证，通过才落库；Trae / Qoder 如实标注
  暂不支持。三种导入方式统一按 JWT `sub` upsert，同一账号只保留一条，
  旧重复记录在下次导入时合并；非 JWT 令牌按整串相同识别（D19）。
- **Android 17 真机**：Redmi K80 Pro 初始连接超时，根因是 Android 17「本地网络保护」
  对 targetSdk 37 应用静默丢弃局域网 TCP；`scripts/patch-android-gradle.ps1` 固定
  `targetSdk = 36` 后真机配对、远程签到通过。
- **自动更新（组 4）**：`crates/ph-update` 查 `releases/latest`、按语义化版本比较、
  选当前平台安装包并校验大小；设置页「更新」面板可检查 / 下载 / 手动下载；
  Windows 下载 `_Setup.exe` 后启动安装向导，安卓跳浏览器下载 APK；
  源可切镜像前缀，启动时自动检查（只发通知，不自动装）。不接 Tauri updater（D08）。
  仓库尚未发布 Release，所以现在检查会显示「更新源里还没有发布记录」。
- **产物**：`dist/` 内 v0.1.0 ~ v0.4.2 的 exe 与 APK 并存，脚本不覆盖历史产物。
- **GitHub**：代码、文档、脚本已推送；`.design/`、`dist/`、`target/`、
  `.workbuddy/` 等均被 `.gitignore` 排除。

---

## 关键决策

1. **不做云端**：凭据在 PC，签到必须在 PC 执行；上云等于把账号钥匙交给第三方。
   手机端通过局域网配对（App 或浏览器）使用 PC 的账号，凭据不跨设备。
2. **不读浏览器 localStorage / Cookie**：实测 WorkBuddy 网页存储全是埋点键，
   真凭证是轮换的 HttpOnly `session` cookie，拷出来很快失效；改用官方插件授权换 refresh token。
3. **SemVer 严格判定**：修用户可见故障 = PATCH（0.3.0→0.3.1、0.3.1→0.3.2、
   0.4.0→0.4.1→0.4.2）；新增手机访问 = MINOR（0.3.2→0.4.0）。
   版本单点在 `Cargo.toml [workspace.package] version`，打包脚本自动同步其余三处。
4. **安卓自用分发**：release 用 debug keystore 签名，省去管理私钥；上架时再换正式 keystore。
   `gen/` 被 git 忽略且会被 Tauri 重新生成，所以签名/strip 补丁放在受控脚本
   `scripts/patch-android-gradle.ps1`，构建前幂等执行。
5. **零新依赖的局域网服务**：手写 HTTP/1.1（`Connection: close`），没引 axum；
   tokio 只补 `net` / `io-util` feature。
6. **令牌不落日志、快照不填 0**：`null` 与 `0` 在界面上是两种文案。
7. **产物不覆盖**：`dist/` 视为交付历史，重出包必须先升版本。

详见 `DECISIONS.md` D01–D14。

---

## 未完成的待办

- [ ] **Trae CN / Qoder CN 端到端**：Qoder 领取端点需一次付费/试用账号抓包；
      Trae 的 ExchangeToken 需设备私钥签名样本（见 `DECISIONS.md` P01/P02）。
- [ ] **发布 0.5.0**：升版本、出双端产物、打 tag 并发布 GitHub Release；
  发布后自动更新的「发现新版本 → 下载」才能真跑一遍。
- [ ] **卸载清理**：安装包侧清理脚本（便携版本身不写注册表）。
- [ ] **Windows 防火墙**：首次监听 47821 若是局域网连不上，需手动放行入站（需管理员权限）。
- [ ] 清理旧的大体积 debug APK（148 / 181 MB）以腾地方——**未擅自删除**。

---

## 其它需记录的内容

### 路径与命令

| 项 | 值 |
|---|---|
| Windows 正式包 | `dist/积分枢纽PointHub_v0.4.2.exe`（4.6 MB） |
| Android 正式包 | `dist/积分枢纽PointHub_v0.4.2.apk`（7.4 MB，arm64） |
| 数据目录 | `%LOCALAPPDATA%\PointHub` |
| 局域网地址 | `http://<PC 内网 IP>:47821`，配对码见设置页 |
| 预览启动 | 双击 `1. 一键启动预览.bat`（按文件时间增量编译，无改动秒开） |
| Windows 打包 | `powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1` |
| Android 打包 | `scripts\build-android.bat release --target aarch64` |
| 排障 | `phctl doctor` / `phctl status --data-dir <目录>`；日志在 `logs/` |

### 验证过的证据

- `cargo test --workspace` 全绿；`cargo check --workspace` 零错误；前端 `tsc` + `vite build` 通过。
- `phctl status`：`余额 201.76 / 已签 / 连签 36`，与汇总接口一致。
- 手机端无头浏览器：错误码 400、无 token 401、重复用码 400、已签账号 `alreadydone`。
- 模拟器：`llvm-nm -D` 有 `Java_cn_pointhub_desktop_Rust_onCreate`；
  装 7.4 MB 包后 0 条 FATAL / UnsatisfiedLinkError。

### 隐私与仓库卫生

- 浏览器 CDP 探测时产生的 cookie 与 summary 临时文件**已删除**；
  `%LOCALAPPDATA%\PointHub\browser-profile`（旧 CDP 方案残留，含当时 cookie）不在仓库内。
- `.design/`（184 MB 日志与构建产物）和 `.workbuddy/`（内部笔记）均不入库。
- 截图/点击类开发脚本放在 `scripts/dev/`，仅本地保留、不入库，避免误把屏幕内容推到 GitHub。

### 接手时的第一件事

先跑 `phctl doctor` 与 `cargo test --workspace` 确认环境，再按「未完成的待办」取任务；
改任何代码前先确认是否要升版本号，改完同步 `CHANGELOG.md`。
