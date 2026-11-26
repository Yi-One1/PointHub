# 项目进展交接（progress）

> 给新对话看的**当前快照**：只写结论与可验证事实，过程细节见 `CHANGELOG.md`、
> `docs/SPEC.md`、`DECISIONS.md`。最后更新：2026-11-26，版本
> **v0.9.2（已验收、已发布）**。

仓库：`https://github.com/Yi-One1/PointHub`（`master`）
发布：<https://github.com/Yi-One1/PointHub/releases/tag/v0.9.2>
（更早的 v0.5.0 / v0.6.0 同样已发布）

---

## 已完成的事项

- **核心链路**：Rust 共享核心（模型 / 加密仓 / 数据仓 / 平台适配 / 编排引擎 / CLI）；
  WorkBuddy 每日签到、余额、派猫猫旅行均**真实账号跑通**。
- **三平台端到端（0.8.0）**：
  - Trae CN：`Cloud-IDE-JWT` + `X-Device-Id` + `X-User-Region: CN`；
    `checkin_credits/{status|claim}`；余额 `pay/ide_user_ent_usage.total_balance`。
    领取前先查状态（Trae 对已领会重复返回成功）。
  - Qoder CN：活动制 `campaigns` + `/claim`；余额为 `usage` 各桶 `remaining` 之和；
    免费账号实测可领。
- **浏览器授权（0.9.0）**：三平台均可。
  - WorkBuddy 走官方插件授权；Trae 走授权页 + 本地 `127.0.0.1` 回调 + PKCE 换
    `ExchangeToken`；Qoder 走 `device/selectAccounts` + 2 秒轮询 `deviceToken/poll`。
  - Trae 授权时生成 ECDSA P-256 设备密钥，续期按规范串签名；Qoder 续期用
    `deviceToken/refresh` + `refresh_token_id`。
  - 续期只在 `expires_at - now < 300` 秒时发生，避免每次操作多打一次上游。
- **本机导入（三平台，仅 Windows）**：WorkBuddy 明文 `checkin-rt.json`；
  Trae 解密 `storage.json`（AES-128-CBC + SHA-512）；Qoder 解密 `auth.v1.dat`
  （DPAPI 密钥 + AES-256-GCM）。只读、不改客户端文件、不打印令牌。
- **账号身份与合并（D19）**：身份优先 `platform_uid`，其次浏览器授权下发的 `user_id`，
  再次 JWT（Trae `data.id` / WorkBuddy `sub`），Qoder 走 `userinfo`。
  旧记录导入前先补身份（`adopt_identities`），再 upsert；同一令牌的旧记录一并合并。
  真实数据副本验证：本机 + 网页两条 → 导入后一条。
- **多账号与调度**：独立凭据、独立排期、独立历史；逐账号签到 + 一键全部签到；
  默认 00:10 + 确定性抖动，启动补签。
- **Windows 桌面端**：托盘、悬浮球（只读，单击展开账号列表）、开机自启、系统通知、
  三态主题、可更换数据目录；总览 KPI / 按平台分组 / 近 7 日新增；
  主窗口可见时按 5/10/30 分钟自动刷新积分（默认 10，可关，0.7.0）。
- **局域网互通与安卓**：PC 起 HTTP 服务（默认 47821），手机 App 双模式
  （本机模式 / 连接电脑），持久配对、远程签到；手机浏览器输 6 位配对码也能看账本。
  安卓 WorkManager 每天 00:16 后台签到，打开 App 约 12 秒补签（0.6.0）。
  Android 17 真机需固定 `targetSdk = 36` 规避本地网络保护。
- **自动更新 / 卸载 / 数据目录**：查 GitHub Releases、比较语义化版本、选平台安装包；
  NSIS 卸载钩子默认清账号数据（可勾选保留）；发行版数据目录跟随 exe（D20）。

---

## 关键决策

1. **不做云端**：凭据在 PC，签到必须在 PC 执行；手机经局域网配对使用 PC 账号，
   凭据不跨设备（D03 / D15）。
2. **不读浏览器 localStorage / Cookie**：真凭证是 HttpOnly cookie；改用平台官方授权。
   本机导入读的是**客户端**存储，不是浏览器（D16）。
3. **账号只按平台用户 id 去重**：不按别名、不按令牌片段（D19）。
4. **浏览器授权 = 每账号独立设备密钥**：私钥只随凭据加密保存，换设备必须重新授权（D21）。
5. **SemVer 严格判定**：修用户可见故障 = PATCH（0.9.1 / 0.9.2）；新增能力 = MINOR
   （0.9.0 浏览器授权）。版本单点在 `Cargo.toml [workspace.package] version`，
   `scripts/sync-version.ps1` 同步其余三处。
6. **零新依赖的局域网服务**：手写 HTTP/1.1，不引 axum（D03）。
7. **令牌不落日志、快照不填 0**：`null` 与 `0` 在界面上是两种文案。
8. **产物不覆盖**：`dist/` 视为交付历史，重出包必须先升版本（D09）。

详见 `DECISIONS.md` D01–D21。

---

## 未完成的待办

- [x] **发布 v0.9.2**：`master` 已推送、`v0.9.2` tag 已打、便携 exe 与安装包已上传，
      Release notes 已写（对比 v0.6.0）。
- [ ] **自动更新「下载 → 安装」**：发现新版本已验证；真正的下载安装要在发版后真跑一次。
- [ ] **Windows 防火墙**：首次监听 47821 若局域网连不上，需手动放行入站（管理员权限）。
- [x] **清理构建产物（2026-11-26 用户授权）**：已删除 `target/`、
      `apps/desktop/src-tauri/target/`、`gen/`、`ui/node_modules/`、`ui/dist/`、
      `.design/`、`.workbuddy/`、`.screenshots/`，共释放约 27 GB。
      按用户选择保留 `dist/` 历史产物（103 MB）与 `dist/data/`（验收数据）。
      删掉 `gen/` 后安卓需重新 `tauri android init`；删掉 `target/` 后首次构建会全量编译。

---

## 其它需记录的内容

### 路径与命令

| 项 | 值 |
|---|---|
| Windows 正式包 | `dist/积分枢纽PointHub_v0.9.2.exe`（5.2 MB）与 `_Setup.exe`（2.1 MB）；历史版本均在 `dist/` |
| Android 正式包 | `dist/积分枢纽PointHub_v0.6.0.apk`（8.0 MB，arm64，debug 签名） |
| 数据目录 | 发行版 `<exe 所在目录>\data`；旧版首次启动自动迁移并清空 `%LOCALAPPDATA%\PointHub` |
| 用户真机安装 | `D:\APP\积分枢纽PointHub\PointHub`，数据在 `D:\...\PointHub\data`——**不要动** |
| 局域网地址 | `http://<PC 内网 IP>:47821`，配对码见设置页 |
| 预览启动 | 双击 `1. 一键启动预览.bat`（按文件时间增量编译，无改动秒开） |
| Windows 打包 | `scripts\build-windows.ps1`（便携）；`-Installer` 同时出安装包 |
| Android 打包 | `scripts\build-android.bat release --target aarch64`（首次需 `tauri android init`） |
| 排障 | `phctl doctor` / `phctl status --data-dir <目录>`；日志在 `logs/` |

### 验证过的证据

- `cargo test --workspace` **90 个测试全绿**；`cargo check --workspace` 零错误；
  前端 `tsc --noEmit` + `vite build` 通过。
- 0.8.0 真实账号：Trae 余额约 1478、Qoder 余额 909，两者 `checked_in=true`；
  重复签到均返回「今天已签」，不重复领取。
- 0.9.x 浏览器授权：Trae 回调换令牌、Qoder 设备码轮询、续期路径均跑通。
- 0.9.2 去重合并：真实数据副本 `before` 两条 → `import: updated=true merged=1` →
  `after` 一条；原数据未被修改。
- 数据目录迁移 / 卸载清理：隔离临时目录用 release exe 验证（迁移、清空、保留模式）。
- 手机端无头浏览器：错误码 400、无 token 401、重复用码 400、已签账号 `alreadydone`。
- APK：`apksigner verify` 通过；`versionName=0.6.0`、`native-code=arm64-v8a`。

### 隐私与仓库卫生

- 仓库体检（2026-11-26 收尾）：96 个跟踪文件，最大 149 KB，`.git` 仅约 0.9 MB；
  未发现令牌、私钥、邮箱或账号数据；`dist/`、`target/`、`gen/`、`.design/`、
  `.workbuddy/`、`scripts/dev/` 均不入库。
- 2026-11-26 已清理本机构建缓存与内部笔记约 27 GB（清单见「未完成的待办」）。
- `%LOCALAPPDATA%\PointHub\browser-profile`（旧 CDP 方案残留，含当时 cookie）不在仓库内。
- 用户真实账号数据只在本机 `data/`，永远不会被提交。

### 接手时的第一件事

先跑 `phctl doctor` 与 `cargo test --workspace` 确认环境，再从「未完成的待办」取任务；
改代码前先确认是否要升版本号，改完同步 `CHANGELOG.md` 与 `progress.md`。
