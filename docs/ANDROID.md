# 安卓构建

## 结论先说

能出包。但 Windows 上有**四个坑**，全都会以「编译失败」的形式出现，且报错各不相同。
本文记录每个坑的现象、根因、以及仓库里已经落地的绕法——你换机器时照着抄就行。

| 坑 | 现象 | 根因 | 绕法 |
|---|---|---|---|
| 1. 中文路径 + NDK | `ld.lld: error: cannot open ...` 路径乱码 | NDK 的 `lld` 不认非 ASCII 路径 | `CARGO_TARGET_DIR` 指到纯英文目录 |
| 2. Gradle 路径检查 | `Your project path contains non-ASCII characters` | AGP 主动拒绝 | `android.overridePathCheck=true` |
| 3. `npm.bat` 不存在 | `A problem occurred starting process 'command npm.bat'` | Tauri 的 Gradle rust 插件硬调 `npm.bat`；新版 Node 只有 `npm.cmd`，Java 不会补后缀 | `scripts/shims/npm.bat` 垫片，构建脚本把它放 PATH 最前 |
| 4. 插件回查 CLI 配置 | `npm error ENOENT ... src-tauri/package.json` 或 `cli-options-server.json` 找不到 | 插件在 `src-tauri` 下执行 `npm run tauri android android-studio-script`，靠 CLI 的本地服务取配置 | `src-tauri/package.json` 提供同名脚本（见下） |

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
| `CARGO_TARGET_DIR` | `D:\ph-build\target` |

`CARGO_TARGET_DIR` 必须是**纯英文**路径，这是坑 1 的绕法，不是可选项。

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

- 正式包：release + `aarch64` 单架构，**实测 7.4 MB**（`Cargo.toml` 已开
  `strip = true` + `lto = true`，AGP 默认剥离 native 符号）。
- 调试包：默认 universal（4 个 ABI），保留调试符号，约 181 MB，仅排障用。

## 界面适配（v0.4.0）

移动端布局已适配，页面本身不用另做一套：

- 侧栏在 ≤760px 时收成**底部标签栏**（此前是 `display:none`，等于把导航弄丢了）
- 单列布局、KPI 两列、按钮 ≥40px、输入框 16px（防 WebView 聚焦放大）
- `safe-area-inset-bottom` 留白给手势条，`100dvh` 适配地址栏收起
- 三档视口实测（390×844 / 360×640 / 844×390 横屏）三个页面均无横向溢出

## 出包命令

```bat
REM  release 小包（推荐，arm64 单架构）
scripts\build-android.bat release --target aarch64

REM  debug 大包（181 MB，带调试信息，排障时用）
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

## 最近一次构建

- 2026-11-25，`dist\积分枢纽PointHub_v0.4.2.apk`，**7.4 MB**
  （release / arm64-v8a / debug keystore 签名 / `apksigner verify` 通过）
- 同版本的 debug 包 `dist\积分枢纽PointHub_v0.4.2_arm64-debug.apk` 为 181 MB，
  仅排障用
- 雷电模拟器（x86_64 + ARM 翻译层 / Android 9）实测：安装、启动、
  页面加载正常，0 条崩溃日志

## targetSdk 固定为 36（Android 17 本地网络保护）

Android 17（API 37）对 targetSdk 37 的应用启用「本地网络保护」：访问局域网 TCP
需要 `ACCESS_LOCAL_NETWORK` 权限，否则连接被**静默丢弃**——表现是超时，
不是权限报错，极难排查（真机 Redmi K80 Pro 实测踩到）。

`scripts/patch-android-gradle.ps1` 会把 `targetSdk` 固定为 `36`，
沿用 INTERNET 权限下的隐式局域网许可。**不要**手工改回 37；
`gen/` 重新生成后由脚本幂等补回。

## 待办

- [x] 发布版 APK 实测体积（7.4 MB）
- [x] 签名配置（自用分发用 debug keystore，无需正式 keystore）
- [x] 模拟器安装验证（雷电，见上）
- [x] 真机安装验证（Redmi K80 Pro / Android 17，targetSdk 36 后可连局域网）
