# 项目进展交接（progress）

> 给新对话看的**当前快照**：只写结论与可验证事实，过程细节见 `CHANGELOG.md`、
> `docs/SPEC.md`、`DECISIONS.md`。最后更新：2026-11-28，版本 **v0.13.1**。

仓库：`https://github.com/Yi-One1/PointHub`（`master`）
最新发布：<https://github.com/Yi-One1/PointHub/releases/tag/v0.13.1>

---

## 已完成的事项

- **核心链路**：Rust 共享核心（`ph-core` / `ph-vault` / `ph-store` / `ph-providers` /
  `ph-engine` / `ph-client` / `ph-update` / `ph-cli`）；三平台签到、余额、旅行均实机跑通。
- **三平台端到端（0.8.0）**：
  - WorkBuddy：活动制签到 + 派猫猫旅行（先领后派）。
  - Trae CN：`Cloud-IDE-JWT` + `X-Device-Id` + `X-User-Region: CN`；领取前先查状态
    （对已领重复 claim 也返回成功）。
  - Qoder CN：`campaigns` + `/claim`，余额为 `usage` 各配额桶 `remaining` 之和。
- **导入三方式**：浏览器授权（三平台，可续期）、本机导入（三平台客户端登录态，
  仅 Windows，只读）、手动令牌兜底；统一按平台用户 id 去重，旧重复记录导入时自动合并（D19）。
- **WorkBuddy 5.6+ 本机导入（0.14.0 进行中）**：优先读客户端共享登录态
  （`%LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop.info`），
  支持 `$wbEncrypted` 加密信封；扫描不动客户端，点导入才以 Node 模式无窗口读一次密钥；
  旧 `checkin-rt.json` 兼容。导入面板三平台共用同一套步骤与按钮；
  「凭据存在哪」跳到设置 → 数据与安全并定位数据目录。
- **总览 KPI（0.14.0 进行中）**：五张卡（积分总额 / 今日新增 / 今日消耗 / 本月领取总积分 / 今日已签）；
  新增取签到领取流水、消耗按 昨日余额 + 今日领取 − 今日余额 反推；本月领取只算本月签到到账；
  新增 ruby 红色积分图标，正负号定宽对齐。
- **多账号与调度**：独立凭据 / 排期 / 历史；逐账号签到 + 一键全部签到；
  默认 00:10 + 确定性抖动，启动补签。
- **签到稳定性（0.13.1）**：执行端指向「已不存在的设备」时启动自动收回本机；
  调度器只调度本机执行账号；失败退避（普通 30 分钟 / 凭据失效到次日）；
  失败通知按账号每天一次；总览「今日签到异常」提示条 + 立即重试。
- **Windows 桌面端**：托盘、悬浮球、开机自启、系统通知、三态主题、单实例（0.11.0）。
- **局域网与安卓**：PC HTTP 服务（47821）+ 6 位配对码 + 持久设备配对；
  安卓双模式（本机 / 连接电脑）；WorkManager 每天 00:16 后台签到、打开 App 补签（0.5~0.6）。
- **自动更新 / 卸载 / 数据目录（0.5.0）**：查 GitHub Releases、比较语义化版本；
  NSIS 卸载默认清账号数据（可勾选保留）；发行版数据目录跟随 exe、旧数据自动迁移（D20）。
- **图标体系（0.10~0.11）**：应用图标全链路、平台 logo、积分单位图标；
  小尺寸只从 256px 母图缩小；桌面图标缓存由 NSIS 钩子 + `SHChangeNotify` 重建。
- **防火墙自动放行（0.12.0）**：安装器写入按程序、仅本地子网的入站规则；
  已存在则跳过（原地升级不弹 UAC）；卸载时提权清除（D25）。
- **安卓应用内安装 + Keystore（0.12.0）**：APK 下到私有目录经 FileProvider 交系统安装器；
  `vault.bin` 改由 AndroidKeyStore 托管，旧 AES 仓首开自动重封（D26 / D27）。
- **设置页改造（0.13.0）**：独立全屏设置页、7 类固定分类、按关键词搜索；
  左栏「设置」改左下角齿轮；改动即改即存；分区恢复默认按差异启用；
  数据目录默认跟随程序（预览用 `dist\data` 且与正式版记录隔离，搬迁后清理旧目录）；
  镜像默认 `https://ghfast.top/`；查到新版本「检查」原地变「更新」；
  总览右侧提示新版本并可跳到更新分区。

---

## 关键决策

1. **不做云端**：凭据在 PC，签到必须在 PC 执行；手机经局域网使用 PC 账号（D03 / D15）。
2. **不读浏览器 localStorage / Cookie**：只用平台官方授权；本机导入读的是客户端存储（D16）。
3. **账号只按平台用户 id 去重**：不按别名、不按令牌片段（D19）。
4. **浏览器授权 = 每账号独立设备密钥**：换设备必须重新授权（D21）。
5. **SemVer 严格判定**：修用户可见故障 = PATCH；新增能力 = MINOR。
   版本单点在 `Cargo.toml [workspace.package] version`，`scripts/sync-version.ps1`
   同步其余三处；`dist/` 视为交付历史，不覆盖、不删除（D09）。
6. **设置项即改即存**：没有全局保存按钮；「恢复默认」按内容比较，与默认值一致时不可点。
7. **数据目录默认跟随程序**：发行版 `<exe 所在目录>\data`；一键预览用 `dist\data`，
   两种位置记录相互隔离（D20）。
8. **图标资产不入库**：源文件在 `用户提供的文件/软件图标/`，`scripts/gen-app-icons.py`
   生成 `src-tauri/icons/`；Android mipmap 构建前重写（D22）。
9. **两套图标素材**：完整底板用于桌面 / 任务栏 / 安装包 / launcher；透明标记用于
   侧栏 / 启动页 / 托盘 / favicon；小尺寸只缩小不放大。
10. **安卓构建补丁全部留在 `scripts/`**：`gen/` 不入库，每次构建幂等重打，
    细节见 `docs/ANDROID.md`。

详见 `DECISIONS.md` D01–D27。

---

## 未完成的待办

- [ ] **Windows 静默自替换**：需接 Tauri updater 签名链，暂不做（D08）；
  目前仍由安装向导确认。
- [ ] **自定义签到时间**：界面只有平台默认时刻 + 确定性抖动（D06），
  后续支持每账号自定义时间。
- [ ] **0.14.0 体验改造（分组进行中）**：G1（本机导入）与 G2（总览 KPI）已完成；
  待做 G3 账号身份与分组、G4 账号页细节、G5 趋势图、G6 自定义签到时间、G7 活动与关于页。
  G4 含一项已确认的布局缺陷：导入方式切换时纵向滚动条出现/消失，两栏宽度抖动 10px。

---

## 其它需记录的内容

### 路径与命令

| 项 | 值 |
|---|---|
| Windows 正式包 | `dist/积分枢纽PointHub_v0.13.1.exe`（5.4 MB）与 `_Setup.exe`（2.2 MB）；历史版本均在 `dist/` |
| Android 正式包 | `dist/积分枢纽PointHub_v0.13.0.apk`（8.5 MB，arm64，debug 签名；0.13.1 未重打） |
| 数据目录 | 发行版 `<exe 所在目录>\data`；预览 `dist\data`；旧版首次启动自动迁移（D20） |
| 用户真机安装 | `D:\APP\积分枢纽PointHub\PointHub`，数据在 `D:\...\PointHub\data`——**不要动** |
| 局域网地址 | `http://<PC 内网 IP>:47821`，配对码见设置页 |
| 预览启动 | 双击 `1. 一键启动预览.bat`（按文件时间增量编译，无改动秒开） |
| Windows 打包 | `scripts\build-windows.ps1`；`-Installer` 同时出安装包；`-InstallerOnly` 只补安装包 |
| Android 打包 | `scripts\build-android.bat release --target aarch64`（构建缓存 `E:\Cache\Android_Build`） |
| 图标生成/校验 | `python scripts/gen-app-icons.py [--check]` |
| 前端测试 | `cd apps\desktop\ui; node --test "test/*.test.mjs"`（65 项；需 Python 带 Pillow。Git Bash 下用 `PATH="/d/APP/Python 3.14.6:$PATH"` 指定解释器；PowerShell 用 `$env:PATH = "D:\APP\Python 3.14.6;$env:PATH"`） |
| Rust 测试 | `cargo test --workspace`（115 项）；桌面端 16 项 |
| 防火墙手动修复 | `netsh advfirewall firewall add rule name="PointHub LAN" dir=in action=allow program="<exe 完整路径>" profile=domain,private,public remoteip=localsubnet`（需管理员） |
| 单实例验证 | `powershell -File scripts\test-single-instance.ps1`（需先 `cargo build` 出 debug exe） |
| 排障 | `phctl doctor` / `phctl status --data-dir <目录>`；日志在 `logs/` |

### 验证过的证据

- `cargo test --workspace` **115 项全绿**；桌面端 16 项、前端 **65/65**、`tsc --noEmit`、`vite build` 通过。
- 0.13.1 修复用真实数据副本实测：三个账号 `executor` 从幽灵 id 收回本机、
  日志有记录、`phctl status` 恢复「已签」；预览版总览异常提示条出现并在补签后自动消失。
- 0.13.1 产物：Windows exe FileVersion 0.13.1（5.4 MB / 安装包 2.2 MB）；
  安卓包沿用 0.13.0（本轮未重打）。
- 0.13.0 产物：Windows exe FileVersion 0.13.0（5.4 MB / 安装包 2.2 MB）；
  APK `versionName=0.13.0`、`versionCode=13000`、targetSdk 36、仅 arm64-v8a、8.5 MB，
  `apksigner verify` 通过（debug 证书）。
- 安卓：应用内安装（检查 → 下载 → 授权 → 系统安装器）、Keystore 旧仓迁移均真机走通；
  Redmi K80 Pro / Android 17 可连局域网（targetSdk 36）。
- 防火墙：安装包实测写入 `PointHub LAN`（程序级 / `RemoteAddress=LocalSubnet`）；
  二次安装不弹 UAC，UAC 显示应用自身。
- 单实例：第二个进程 0.2 秒退出，首个窗口被唤起。
- 真实账号：Trae 余额约 1478、Qoder 余额 909；重复签到不重复领取。

### 隐私与仓库卫生

- 仓库 133 个跟踪文件，最大为 `Cargo.lock`（约 150 KB）；无 exe / apk / 数据文件。
- `dist/`、`target/`、`gen/`、`node_modules/`、`.trae/`、`用户提供的文件/`、
  `GitHub上的参考项目/` 均不入库；运行产物 `debug.log` / `device.json` / `logs/` 已忽略。
- 用户真实账号数据只在本机 `data/`，永远不会被提交。

### 接手时的第一件事

先跑 `phctl doctor` 与 `cargo test --workspace` 确认环境，再从「未完成的待办」取任务；
改代码前先确认是否要升版本号，改完同步 `CHANGELOG.md` 与 `progress.md`。
