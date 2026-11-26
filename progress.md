# 项目进展交接（progress）

> 给新对话看的**当前快照**：只写结论与可验证事实，过程细节见 `CHANGELOG.md`、
> `docs/SPEC.md`、`DECISIONS.md`。最后更新：2026-11-27，版本
> **v0.10.1**。

仓库：`https://github.com/Yi-One1/PointHub`（`master`）
上一发布：<https://github.com/Yi-One1/PointHub/releases/tag/v0.10.1>

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
- **浏览器授权（0.9.0）**：三平台均可（WorkBuddy 插件授权 / Trae PKCE 回调 /
  Qoder 设备码轮询）；续期只在 `expires_at - now < 300` 秒时发生。
- **本机导入（三平台，仅 Windows）**：WorkBuddy 明文 `checkin-rt.json`；
  Trae 解密 `storage.json`；Qoder 解密 `auth.v1.dat`。只读、不改客户端文件。
- **账号身份与合并（D19）**：身份优先 `platform_uid`，其次授权 `user_id`、JWT、
  Qoder `userinfo`；旧记录导入前先补身份（`adopt_identities`），同令牌旧记录一并合并。
- **多账号与调度**：独立凭据、独立排期、独立历史；逐账号签到 + 一键全部签到；
  默认 00:10 + 确定性抖动，启动补签。
- **Windows 桌面端**：托盘、悬浮球、开机自启、系统通知、三态主题、可更换数据目录；
  总览 KPI / 按平台分组 / 近 7 日新增；主窗口可见时按 5/10/30 分钟自动刷新。
- **局域网互通与安卓**：PC 起 HTTP 服务（默认 47821），手机 App 双模式，
  浏览器输 6 位配对码也能看账本；安卓 WorkManager 每天 00:16 后台签到。
- **自动更新 / 卸载 / 数据目录**：查 GitHub Releases、比较语义化版本；NSIS 卸载钩子
  默认清账号数据（可勾选保留）；发行版数据目录跟随 exe（D20）。
- **图标体系（0.10.0）**：应用图标全链路替换（exe / 任务栏 / 托盘 / 侧栏 / 启动页 /
  favicon / Android）；平台 logo 进入 5 处界面；积分数值的「分」全部换成彩色图标
  （主窗口、悬浮球、手机页、趋势悬停）。
- **覆盖升级图标缓存（0.10.1）**：安装包 `POSTINSTALL` 跑 `ie4uinit`，程序首次以新
  版本启动时用 `SHChangeNotify` 重读三个快捷方式；解决「升级后桌面还是旧图标」。
- **覆盖升级图标缓存修复（0.10.1）**：NSIS `POSTINSTALL` 重建图标缓存 + 程序首次以新
  版本启动时 `SHChangeNotify` 通知 shell 重读桌面/开始菜单快捷方式（每版本一次）。

---

## 关键决策

1. **不做云端**：凭据在 PC，签到必须在 PC 执行；手机经局域网使用 PC 账号（D03 / D15）。
2. **不读浏览器 localStorage / Cookie**：改用平台官方授权；本机导入读的是客户端存储（D16）。
3. **账号只按平台用户 id 去重**：不按别名、不按令牌片段（D19）。
4. **浏览器授权 = 每账号独立设备密钥**：换设备必须重新授权（D21）。
5. **SemVer 严格判定**：修用户可见故障 = PATCH；新增能力 = MINOR。
   版本单点在 `Cargo.toml [workspace.package] version`，`scripts/sync-version.ps1`
   同步其余三处；`dist/` 视为交付历史，不覆盖、不删除（D09）。
6. **图标资产不入库**：源文件在 `用户提供的文件/软件图标/`，`scripts/gen-app-icons.py`
   生成 `src-tauri/icons/`；Android 图标由 `scripts/build-android.bat` 构建前重写（D22）。
7. **应用图标分两套素材**：完整底板（桌面 / 任务栏 / 安装包 / launcher）+ 透明标记
   （侧栏 / 启动页 / 托盘 / favicon）；Windows 图标按 1.25 倍裁进底板并重新倒圆角，
   否则 24px 任务栏里图形太小（D22）。
8. **积分图标按位置定色调**：合计/总值青铜、平台小计墨、余额/新增玉；深色主题下墨色
   提亮 2.6 倍；手机页把图标 base64 内联，不发额外请求（D22）。

详见 `DECISIONS.md` D01–D22。

---

## 未完成的待办

- [ ] **发布 v0.10.1**：推送 `master`、打 `v0.10.1` tag、上传便携 exe 与安装包、
      写 Release notes（对比 v0.10.0，修复快捷方式图标缓存）。
- [ ] **自动更新「下载 → 安装」**：发现新版本已验证；真正的下载安装要在发版后真跑一次。
- [ ] **Windows 防火墙**：首次监听 47821 若局域网连不上，需手动放行入站（管理员权限）。
- [ ] **Android APK 未跟到 0.10.0**：`dist/` 里最新 APK 仍是 v0.6.0；需要时
      `tauri android init` + `scripts/build-android.bat release --target aarch64`。

---

## 其它需记录的内容

### 路径与命令

| 项 | 值 |
|---|---|
| Windows 正式包 | `dist/积分枢纽PointHub_v0.10.1.exe` 与 `_Setup.exe`；历史版本均在 `dist/` |
| Android 正式包 | `dist/积分枢纽PointHub_v0.6.0.apk`（8.0 MB，arm64，debug 签名） |
| 数据目录 | 发行版 `<exe 所在目录>\data`；旧版首次启动自动迁移并清空 `%LOCALAPPDATA%\PointHub` |
| 用户真机安装 | `D:\APP\积分枢纽PointHub\PointHub`，数据在 `D:\...\PointHub\data`——**不要动** |
| 局域网地址 | `http://<PC 内网 IP>:47821`，配对码见设置页 |
| 预览启动 | 双击 `1. 一键启动预览.bat`（按文件时间增量编译，无改动秒开） |
| Windows 打包 | `scripts\build-windows.ps1`；`-Installer` 同时出安装包；`-InstallerOnly` 只补安装包 |
| Android 打包 | `scripts\build-android.bat release --target aarch64` |
| 图标生成/校验 | `python scripts/gen-app-icons.py [--check]`（菜单/窗口/Android 全套） |
| 前端测试 | `cd apps\desktop\ui; node --test "test/*.test.mjs"`（18 项） |
| 排障 | `phctl doctor` / `phctl status --data-dir <目录>`；日志在 `logs/` |

### 图标实现要点（0.10.0）

- **改图标必重编**：`build.rs` 已声明 `rerun-if-changed=icons`；`lib.rs` 启动时显式
  `set_icon(icons/icon.png)`。否则只改 PNG 时任务栏会一直显示旧图。
- **桌面快捷方式图标**：资源管理器按 `.lnk` 缓存，覆盖安装不会自动失效。0.10.1 起
  装机后由 NSIS 钩子重建缓存，程序启动再按版本号补一次 `SHChangeNotify`。
  仍遇到旧图时：重开资源管理器或清 `%LOCALAPPDATA%\Microsoft\Windows\Explorer\iconcache_*.db`。
- **托盘跟随系统深浅色**：嵌入 `tray-32.png` / `tray-dark-32.png`，监听 `ThemeChanged`。
- **平台 logo**：`ui/src/assets/vendors/`，已裁掉透明留白；白底 logo 加 1px 轮廓；
  未收录平台回退成识别色圆点。
- **积分图标**：`ui/src/assets/points/`（bronze / ink / jade，64px RGBA），
  `PointsIcon` 组件统一尺寸与基线；手机页内联 base64。
- 用户提供的原始素材（含深色版）只在本地，换机器需自行放回。

### 验证过的证据

- `cargo test --workspace` **90 个测试全绿**；`cargo check --workspace` 零错误；
  桌面端 `cargo check` 通过；前端 **18/18**、`tsc --noEmit`、`vite build` 通过。
- `gen-app-icons.py --check`：bundle PNG / ICO / Android mipmap / 托盘图标与源图逐像素一致。
- 从 exe 资源实测：内嵌 32px 图标包围盒 `(0,0,32,32)`，与 `icons/32x32.png` 像素差 0。
- 0.8.0 真实账号：Trae 余额约 1478、Qoder 余额 909，`checked_in=true`，重复签到不重复领取。
- 0.9.2 去重合并：真实数据副本两条 → 导入后一条；原数据未被修改。
- 手机端无头浏览器：错误码 400、无 token 401、重复用码 400、已签账号 `alreadydone`。

### 隐私与仓库卫生

- 仓库体检：跟踪文件约 100 个，最大 149 KB，`.git` 约 1 MB；未发现令牌、私钥、
  邮箱或账号数据；`dist/`、`target/`、`gen/`、`node_modules/`、`.trae/`、
  `用户提供的文件/`、`GitHub上的参考项目/` 均不入库。
- 用户真实账号数据只在本机 `data/`，永远不会被提交。

### 接手时的第一件事

先跑 `phctl doctor` 与 `cargo test --workspace` 确认环境，再从「未完成的待办」取任务；
改代码前先确认是否要升版本号，改完同步 `CHANGELOG.md` 与 `progress.md`。
