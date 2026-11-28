# 安卓构建

## 结论先说

能出包。但 Windows 上有**五个坑**，全都会以「编译失败」的形式出现，且报错各不相同。
本文记录每个坑的现象、根因、以及仓库里已经落地的绕法——你换机器时照着抄就行。

| 坑 | 现象 | 根因 | 绕法 |
|---|---|---|---|
| 1. 中文路径 + NDK | `ld.lld: error: cannot open ...` 路径乱码 | NDK 的 `lld` 不认非 ASCII 路径 | `CARGO_TARGET_DIR` 指到纯英文目录 |
| 2. Gradle 路径检查 | `Your project path contains non-ASCII characters` | AGP 主动拒绝 | `android.overridePathCheck=true`，由 `patch-android-gradle.ps1` 幂等写入 |
| 3. `npm.bat` / `node.bat` 不存在 | `A problem occurred starting process 'command npm.bat'`（或 `node.bat`） | Tauri 的 Gradle rust 插件硬调 `npm.bat` / `node.bat`；新版 Node 只有 `npm.cmd` / `node.exe`，Java 不会补后缀 | `scripts/shims/` 下两个垫片，构建脚本把它放 PATH 最前 |
| 4. 插件回查 CLI 配置 | `npm error ENOENT ... src-tauri/package.json` 或 `cli-options-server.json` 找不到 | 插件在 `src-tauri` 下执行 `npm run tauri android android-studio-script`，靠 CLI 的本地服务取配置 | `src-tauri/package.json` 提供同名脚本（见下） |
| 5. 插件调 `node tauri` | `Cannot find module '...\src-tauri\tauri'` | 新模板在 `src-tauri` 下执行 `node tauri android android-studio-script`，该目录没有可解析的 `tauri` 模块 | `patch-android-sources.ps1` 把参数改写成 `../ui/node_modules/@tauri-apps/cli/tauri.js` |

## 一条命令

```bat
scripts\build-android.bat release --target aarch64
```

脚本依次：检查环境 → 构建前端 → 打 Gradle 补丁 → 调 Tauri CLI 出 APK →
复制到 `dist\积分枢纽PointHub_v<版本>.apk`（已存在则跳过）。
原始产物仍在 `apps/desktop/src-tauri/gen/android/app/build/outputs/apk/`。

## 环境变量

脚本里有默认值，都可以用同名环境变量覆盖：

| 变量 | 默认值 |
|---|---|
| `ANDROID_HOME` | `D:\APP\Android\AndroidSDK` |
| `JAVA_HOME` | `D:\APP\Android\Android Studio\jbr` |
| `NDK_HOME` | `%ANDROID_HOME%\ndk\30.0.16248370` |
| `CARGO_TARGET_DIR` | `E:\Cache\Android_Build\target` |

`CARGO_TARGET_DIR` 必须是**纯英文、无空格**路径（当前默认
`E:\Cache\Android_Build\target`），这是坑 1 的绕法，不是可选项。

## 关于 `src-tauri/package.json`

这个文件是为坑 4 存在的薄壳。它只声明一个 `tauri` 脚本，
因为 Gradle 插件会在 `src-tauri` 目录下执行 `npm run tauri android android-studio-script`。

两个注意点：

1. **它是空操作**（`node -e "process.exit(0)"`）。因为该脚本本意是让运行中的 CLI
   通过本地服务下发配置，而 `tauri android build` 已经把 Kotlin 绑定和资源生成完了，
   再走一遍反而会因为拿不到服务而失败。空操作后 Gradle 自己跑 cargo，链路正常。
2. 如果你要**在 Android Studio 里点 Run**，得先手动跑一次 `scripts\build-android.bat release --target aarch64`，
   让资源与绑定就位，再在 Studio 里运行。直接冷启动 Studio 会因为缺配置而报错。

## 产物与体积

- 正式包：release + `aarch64` 单架构，**实测 8.5 MB**（`Cargo.toml` 已开
  `strip = true` + `lto = true`，AGP 默认剥离 native 符号）。不带 `--target` 时
  会出 universal（4 个 ABI，约 27 MB）；复制脚本优先取 arm64 产物。
- 调试包：默认 universal（4 个 ABI）并保留调试符号，约 150–180 MB，仅排障用。

## 界面适配

移动端布局已适配，页面本身不用另做一套：

- 侧栏在 ≤760px 时收成**底部标签栏**（此前是 `display:none`，等于把导航弄丢了）
- 单列布局、KPI 两列、按钮 ≥40px、输入框 16px（防 WebView 聚焦放大）
- `safe-area-inset-bottom` 留白给手势条，`100dvh` 适配地址栏收起
- 三档视口实测（390×844 / 360×640 / 844×390 横屏）三个页面均无横向溢出

## 出包命令

```bat
REM  release 小包（推荐，arm64 单架构）
scripts\build-android.bat release --target aarch64

REM  debug 大包（约 150–180 MB，带调试信息，排障时用）
scripts\build-android.bat --target aarch64
```

release 构建成功后会自动复制到 `dist\积分枢纽PointHub_v<版本>.apk`，
和 Windows 包同名同版本（只是扩展名不同）；已存在的产物不会被覆盖。

`gen/` 整个目录被 git 忽略，`tauri android init` 也会重新生成
`app/build.gradle.kts`。构建脚本会在编译前调用
`scripts/patch-android-gradle.ps1`，幂等地写入两条配置：

1. release 用 debug keystore 签名（自用分发，不需要正式私钥）
2. 去掉 `keepDebugSymbols`，让 native so 被 strip

换电脑或重新 init 后无需手工改 `gen/` 里的文件。

## 后台定时签到（0.6.0）

- Kotlin 侧 `CheckinWorker` + `PeriodicWorkRequest`，每天 **00:16** 触发（默认签到
  00:10 + 最多 5 分钟抖动之后）；已排过则保留原计划，开机后系统自动恢复。
- Worker 通过 JNI 调 `CheckinBridge.runScheduledCheckin(context.dataDir)`；Rust 在
  `android_worker.rs` 里自建 tokio 运行时执行签到，返回 JSON，由 Kotlin 发
  「签到结果」通知。远程模式、未开调度、没有到点账号时静默跳过。
- 打开 App 约 12 秒后补签当天漏掉的（`spawn_mobile_catchup`）。
- `gen/` 不入库：Kotlin 源在 `apps/desktop/android/`，每次构建由
  `scripts/patch-android-sources.ps1` 复制进去，并补 `POST_NOTIFICATIONS` 权限、
  `androidx.work` 依赖、R8 keep 规则与 MainActivity 调用。
- 国产 ROM 需在系统设置里允许后台运行，否则可能被省电策略延后；
  打开 App 补签是兜底。

## 应用内安装更新（0.12.0 已发）

更新时不再跳浏览器，链路如下（D26）：

- Rust 把 APK 下到 `getFilesDir()/updates`。引擎数据目录是 `Context.getDataDir()`，
  所以实际路径是 `<dataDir>/files/updates`——**必须在 `files/` 下**，
  FileProvider 的 `<files-path>` 才能映射到。
- `ApkInstaller.kt` 用 FileProvider（authority `<包名>.fileprovider`）把文件
  交给 `Intent.ACTION_VIEW` + `application/vnd.android.package-archive`。
- Android 8+ 需要 `REQUEST_INSTALL_PACKAGES` 与「安装未知应用」授权；未授权时
  跳到 `ACTION_MANAGE_UNKNOWN_APP_SOURCES`，`MainActivity.onResume` 回来后自动续装。
- `ApkInstaller` 只被 Rust JNI 引用，R8 默认会剥掉，`proguard-pointhub.pro` 必须保留
  `-keep class cn.pointhub.desktop.ApkInstaller { *; }`。
- Rust 侧从 Application 的 `getClassLoader().loadClass(...)` 取类，不用 `FindClass`：
  原生线程没有 Java 调用栈，`FindClass` 会走系统类加载器而找不到应用类。
- 以上改动都由 `scripts/patch-android-sources.ps1` 幂等写入 `gen/`（Manifest 权限、
  `res/xml/file_paths.xml`、R8 规则、`MainActivity`），换机器重 init 不会丢。

## 凭据存储与旧仓迁移（0.12.0 已发）

- 安卓凭据仓（`vault.bin`）改用 AndroidKeyStore 托管密钥（D05 / D27）：
  `VaultKeystore.kt` 用 `AES/GCM/NoPadding` 加解密，密钥只在系统密钥库里生成，
  应用拿不到明文；密文格式是「12 字节 IV + 密文 + 16 字节 tag」。
- Rust 通过 JNI 反调，类加载方式与应用内安装一致（Application 的 ClassLoader）。
  JVM 与应用 Context 由 `VaultKeys.init(context)` 写入，必须早于 `Engine::open`；
  构建补丁会把 `VaultKeys.init(this)` 插到 `MainActivity.onCreate` 的
  `super.onCreate` 之前，`CheckinWorker` 冷启动时也会先调一次。
- 旧仓（文件头 id=2，本地 AES 密钥文件）首次打开会自动解密并重封为 id=3，
  成功后再删除旧密钥文件；用户不需要重新导入。
- 系统密钥丢失（卸载重装 / 清除数据 / 系统恢复）时，原 `vault.bin` 改名为
  `vault.unreadable` 留档，应用按空仓启动，账号显示「缺凭据」，重新导入即可。
- R8 必须保留 `VaultKeystore` 与 `VaultKeys`：前者只被 Rust JNI 引用，后者拥有
  native 初始化方法；规则由 `patch-android-sources.ps1` 写入。
- release 包不可调试，无法用 `run-as` 查看 `vault.bin`；验收以「升级后账号不缺
  凭据 + 重启后仍能签到」为准。

## 最近一次构建

- 2026-11-28，`dist\积分枢纽PointHub_v0.14.0.apk`，**8.6 MB**
  （release / arm64-v8a / versionName 0.14.0 / versionCode 14000 /
  targetSdk 36 / debug keystore 签名 / `apksigner verify` 通过 /
  含最近活动弹窗与关于页）
- 2026-11-28，`dist\积分枢纽PointHub_v0.13.0.apk`，**8.5 MB**
  （release / arm64-v8a / versionName 0.13.0 / versionCode 13000 /
  targetSdk 36 / debug keystore 签名 / `apksigner verify` 通过 /
  mipmap 160–640 五档图标齐全）。
- 2026-11-27，`dist\积分枢纽PointHub_v0.12.0.apk`，8.3 MB
  （release / arm64-v8a / versionName 0.12.0 / versionCode 12000 /
  targetSdk 36 / debug keystore 签名 / `apksigner verify` 通过 /
  mipmap 160–640 五档图标齐全）。
- 2026-11-27，`dist\积分枢纽PointHub_v0.11.0.apk`，8.3 MB（同配置，历史记录）。
- 2026-11-26，`dist\积分枢纽PointHub_v0.6.0.apk`，8.0 MB（同上配置，历史记录）。
- 早期 debug 包（约 150–180 MB）只用于排障，不入库、不发布。
- 验证命令（`aapt` 不认中文路径，先把 APK 复制到纯英文目录再查）：
  `aapt dump badging ph.apk`、`apksigner verify --print-certs ph.apk`。

## targetSdk 固定为 36（Android 17 本地网络保护）

Android 17（API 37）对 targetSdk 37 的应用启用「本地网络保护」：访问局域网 TCP
需要 `ACCESS_LOCAL_NETWORK` 权限，否则连接被**静默丢弃**——表现是超时，
不是权限报错，极难排查（真机 Redmi K80 Pro 实测踩到）。

`scripts/patch-android-gradle.ps1` 会把 `targetSdk` 固定为 `36`，
沿用 INTERNET 权限下的隐式局域网许可。**不要**手工改回 37；
`gen/` 重新生成后由脚本幂等补回。
