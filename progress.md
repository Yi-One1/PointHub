# 项目进展交接（progress）

> 给新对话看的**当前快照**：只写结论与可验证事实，过程细节见 `CHANGELOG.md`、
> `docs/SPEC.md`、`DECISIONS.md`。最后更新：2026-11-28，版本 **v0.13.0**。

仓库：`https://github.com/Yi-One1/PointHub`（`master`）
最新发布：<https://github.com/Yi-One1/PointHub/releases/tag/v0.11.0>
（v0.12.0 与 v0.13.0 正式包已构建，待发布）
（上一版：<https://github.com/Yi-One1/PointHub/releases/tag/v0.10.1>）

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
  favicon / Android）；平台 logo 进入 5 处界面；积分数值的「分」全部换成彩色图标。
- **覆盖升级图标缓存（0.10.1）**：NSIS `POSTINSTALL` 用 `ie4uinit` 重建图标缓存；
  程序首次以新版本启动时用 `SHChangeNotify` 重读三个快捷方式（每版本一次）。
- **图标清晰度与对齐（0.11.0）**：桌面/任务栏图标统一从 **256px 母图缩小**
  （不再把小图放大 1.25 倍）；积分图标改**基线对齐**，视觉中心与数字一致
  （手机页 `lan.html` 同步）。
- **单实例（0.11.0）**：`tauri-plugin-single-instance` 最先注册，第二次启动唤起并
  置前已有窗口（含托盘最小化）；`scripts/test-single-instance.ps1` 可自动验证。
- **安卓重打（0.11.0）**：APK 从 0.6.0 直接重打到 0.11.0（arm64 / 8.3 MB），
  含新图标、平台 logo、积分图标与新 UI；构建链的 Windows 兼容问题全部脚本化。
- **防火墙自动放行（0.12.0）**：安装器 `POSTINSTALL` 调用
  `pointhub.exe --firewall-install`，按程序写入只允许**本地子网**的入站规则；
  已放行则直接跳过（原地升级不弹 UAC）；提权时 UAC 显示应用自身（不再显示
  Windows PowerShell）；运行期无检测、无提示、无设置项；卸载时提权清除全部
  `PointHub*` 规则。便携版/调试版仍由 Windows 原生弹窗处理。
- **安卓应用内安装更新（0.12.0）**：设置页「下载并安装」不再跳浏览器——APK 下到
  `files/updates`，经 FileProvider 交给系统安装器；首次未授权时跳到「安装未知应用」
  设置页，返回后 `onResume` 自动续装；已下载的包会复用（D26）。
- **安卓 Keystore 凭据托管（0.12.0）**：`vault.bin` 改由 AndroidKeyStore 保管密钥，
  Rust 经 JNI 调 Kotlin 做 AES-256-GCM；旧 AES 仓首开自动重封并删除旧密钥文件，
  用户无需重新导入；系统密钥丢失时原文件改名留档、按空仓启动（D27）。
- **设置页改造（0.13.0）**：左栏「设置」改为左下角齿轮；独立全屏设置页，
  7 类固定分类（通用/外观与桌面/签到与通知/手机访问/更新/数据与安全/关于），
  分区标题在卡片外；主/设置菜单统一图标与悬停阴影；新增按关键词筛选分类的搜索框；
  版本号移到品牌名右侧，「数据只在本机」移到数据与安全。第 1~4 组均已验收：
  改动即改即存、分区恢复默认按差异启用、重要项二次确认、说明精简、镜像默认地址、
  数据目录跟随程序（预览独立）且搬迁后自动清理旧目录；检查到新版本时「检查」原地
  变「更新」，总览右侧提示可跳转设置更新分区。

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
   生成 `src-tauri/icons/`；Android mipmap 由 `build-android.bat` 构建前重写（D22）。
7. **应用图标分两套素材**：完整底板（桌面 / 任务栏 / 安装包 / launcher）+ 透明标记
   （侧栏 / 启动页 / 托盘 / favicon）；Windows 按 1.25 倍裁进底板并重新倒圆角（D22）。
8. **小尺寸只缩小不放大**：所有桌面尺寸从 256px 母图裁→缩；放大必然发虚（0.11.0）。
9. **积分图标按位置定色调**：合计/总值青铜、平台小计墨、余额/新增玉；深色主题下墨色
   提亮；手机页 base64 内联（D22）。
10. **安卓构建补丁全部留在 `scripts/`**：`gen/` 不入库，每次构建幂等重打——
    Gradle 非 ASCII 路径、`node.bat` / `npm.bat` 垫片、`node tauri` 入口改写
    （详见 `docs/ANDROID.md` 的五个坑）；发布包优先 arm64 单架构。

详见 `DECISIONS.md` D01–D27。

---

## 未完成的待办

- [ ] **Windows 静默自替换**：需接 Tauri updater 签名链，暂不做（D08）；目前仍由安装向导确认。
- [ ] **自定义签到时间**：界面目前只有平台默认时刻 + 确定性抖动（D06），用户不能
  手动调整；后续增加每账号自定义时间（也能让后台签到验收不再受固定时刻限制）。

---

## 其它需记录的内容

### 路径与命令

| 项 | 值 |
|---|---|
| Windows 正式包 | `dist/积分枢纽PointHub_v0.13.0.exe`（5.4 MB）与 `_Setup.exe`（2.2 MB）；历史版本均在 `dist/` |
| Android 正式包 | `dist/积分枢纽PointHub_v0.13.0.apk`（8.5 MB，arm64，debug 签名） |
| 数据目录 | 发行版 `<exe 所在目录>\data`；旧版首次启动自动迁移并清空 `%LOCALAPPDATA%\PointHub` |
| 用户真机安装 | `D:\APP\积分枢纽PointHub\PointHub`，数据在 `D:\...\PointHub\data`——**不要动** |
| 局域网地址 | `http://<PC 内网 IP>:47821`，配对码见设置页 |
| 预览启动 | 双击 `1. 一键启动预览.bat`（按文件时间增量编译，无改动秒开） |
| Windows 打包 | `scripts\build-windows.ps1`；`-Installer` 同时出安装包；`-InstallerOnly` 只补安装包 |
| Android 打包 | `scripts\build-android.bat release --target aarch64`（需先 `npm install` 于 `apps\desktop\ui`） |
| 图标生成/校验 | `python scripts/gen-app-icons.py [--check]` |
| 前端测试 | `cd apps\desktop\ui; node --test "test/*.test.mjs"`（25 项） |
| 桌面端测试 | `cargo test --manifest-path apps\desktop\src-tauri\Cargo.toml`（16 项） |
| 防火墙手动修复 | `netsh advfirewall firewall add rule name="PointHub LAN" dir=in action=allow program="<exe 完整路径>" profile=domain,private,public remoteip=localsubnet`（需管理员） |
| 单实例验证 | `powershell -File scripts\test-single-instance.ps1`（需先 `cargo build` 出 debug exe） |
| 排障 | `phctl doctor` / `phctl status --data-dir <目录>`；日志在 `logs/` |

### 安卓构建的五个坑（细节见 `docs/ANDROID.md`）

1. NDK 不认中文路径 → `CARGO_TARGET_DIR=E:\Cache\Android_Build\target`；
2. Gradle 拒绝非 ASCII 路径 → `android.overridePathCheck=true`（脚本幂等写入）；
3. 插件硬调 `npm.bat` / `node.bat` → `scripts/shims/` 垫片放进 PATH 最前；
4. 插件回查 CLI 配置 → `src-tauri/package.json` 提供同名空脚本；
5. 新模板执行 `node tauri ...` → `patch-android-sources.ps1` 改写为
   `../ui/node_modules/@tauri-apps/cli/tauri.js`。

### 图标实现要点

- **改图标必重编**：`build.rs` 已声明 `rerun-if-changed=icons`；`lib.rs` 启动时显式
  `set_icon(icons/icon.png)`。
- **桌面快捷方式图标**：资源管理器按 `.lnk` 缓存，覆盖安装不会自动失效。0.10.1 起
  装机后由 NSIS 钩子重建缓存，程序启动再按版本号补一次 `SHChangeNotify`。
  仍遇到旧图时：重开资源管理器或清 `%LOCALAPPDATA%\Microsoft\Windows\Explorer\iconcache_*.db`。
- **托盘跟随系统深浅色**：嵌入 `tray-32.png` / `tray-dark-32.png`，监听 `ThemeChanged`。
- **平台 logo**：`ui/src/assets/vendors/`，已裁掉透明留白；白底 logo 加 1px 轮廓；
  未收录平台回退成识别色圆点。
- **积分图标**：`ui/src/assets/points/`（bronze / ink / jade，64px RGBA），
  `PointsIcon` 统一尺寸与基线；手机页内联 base64。
- **素材**：用户提供的原始素材（含深色版）只在本地，换机器需自行放回。

### 验证过的证据

- `cargo test --workspace` **95 个测试全绿**；`cargo check --workspace` 零错误。
- 桌面端 **16/16**；前端 **25/25**、`tsc --noEmit`、`vite build` 通过；
  `gen-app-icons.py --check` 通过。
- 安卓应用内安装：验收专用包（内部版本标 0.10.0、安装包仍 0.11.0）真机走通
  检查 → 下载 → 授权「安装未知应用」→ 系统安装器覆盖安装，账号数据保留；
  打包产物核验含 `REQUEST_INSTALL_PACKAGES`、FileProvider、`ApkInstaller` 类。
- 安卓 Keystore：真机覆盖安装后账号**不缺凭据**、重启后仍能签到（旧 AES 仓自动
  迁移）；密钥丢失的降级路径由单元测试覆盖，真机尚未模拟系统恢复。
- 防火墙：安装包实测写入 `PointHub LAN`（程序级、`RemoteAddress=LocalSubnet`），
  手机访问通过；二次安装（规则已存在）不弹 UAC；提权弹窗显示应用自身。
- 0.13.0 产物：`cargo test --workspace` 99 项、前端 46 项、`tsc + vite build` 通过；
  Windows exe FileVersion 0.13.0（5.4 MB / 安装包 2.2 MB）；APK `versionName=0.13.0`、
  `versionCode=13000`、targetSdk 36、仅 arm64-v8a、8.5 MB，`apksigner verify` 通过
  （debug 证书）。
- 0.12.0 产物：Windows exe FileVersion 0.12.0（5.3 MB / 安装包 2.2 MB）；
  APK `versionName=0.12.0`、`versionCode=12000`、targetSdk 36、仅 arm64-v8a、
  8.3 MB，`apksigner verify` 通过（debug 证书）。
- 0.11.0 APK：`aapt dump badging` 显示 versionName 0.11.0 / versionCode 11000 /
  targetSdk 36 / native-code 仅 arm64-v8a；mipmap 160–640 五档齐全；
  `apksigner verify` 通过（debug 证书）。
- 单实例：第二个进程 0.2 秒退出（exit=0），首个窗口被唤起。
- 从 exe 资源实测：内嵌 32px 图标包围盒 `(0,0,32,32)`，与 `icons/32x32.png` 像素差 0。
- 0.8.0 真实账号：Trae 余额约 1478、Qoder 余额 909，`checked_in=true`，重复签到不重复领取。
- 手机端无头浏览器：错误码 400、无 token 401、重复用码 400、已签账号 `alreadydone`。

### 隐私与仓库卫生

- 仓库体检：只跟踪源码与文档，最大文件远小于 1 MB；未发现令牌、私钥、邮箱或账号数据。
- `dist/`、`target/`、`gen/`、`node_modules/`、`.trae/`、`用户提供的文件/`、
  `GitHub上的参考项目/` 均不入库；运行产物 `debug.log` / `device.json` / `logs/` /
  `.icons-version` 已忽略。
- 用户真实账号数据只在本机 `data/`，永远不会被提交。

### 接手时的第一件事

先跑 `phctl doctor` 与 `cargo test --workspace` 确认环境，再从「未完成的待办」取任务；
改代码前先确认是否要升版本号，改完同步 `CHANGELOG.md` 与 `progress.md`。
