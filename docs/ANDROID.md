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
scripts\build-android.bat
```

脚本会做三件事：检查环境 → 构建前端 → 调 Tauri CLI 出 APK。
产物在 `apps/desktop/src-tauri/gen/android/app/build/outputs/apk/`。

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
2. 如果你要**在 Android Studio 里点 Run**，得先手动跑一次 `scripts\build-android.bat`，
   让资源与绑定就位，再在 Studio 里运行。直接冷启动 Studio 会因为缺配置而报错。

## 产物与体积

- 调试包默认出 **universal**（4 个 ABI 全带上），体积很大（本次 613 MB），
  因为调试构建不剥离符号。**这个体积不代表发布体积。**
- 发布包的做法：`--release` + 只出 `aarch64`，并在 `Cargo.toml` 里保留
  `strip = true` 与 `lto = true`，预计在 10 MB 量级（尚未实测）。

## 待办

- [ ] 发布版 APK 实测体积
- [ ] 签名配置（release 需要 keystore）
- [ ] 真机安装验证（用到 DevEco/ADB，需先确认设备）
