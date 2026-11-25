# Windows 构建避坑

项目放在含中文的路径下（`E:\AI工作空间\积分枢纽PointHub`），Windows 工具链里有一批组件
不吃非 ASCII 路径。安卓侧的四个坑见 [ANDROID.md](ANDROID.md)，本文记录桌面端与 Cargo 侧的坑。

## 增量编译卡死（现象最像「程序挂了」，其实不是）

**现象**：`cargo build` / `cargo test` 长时间没有任何输出，最后一次输出停在
`Compiling <某个 crate>`。任务管理器里 `rustc.exe` 活着，但 CPU 时间几乎不涨
（实测 17 分钟只用了 5 秒）。最后进程自己以 `exit 1` 退出，**且没有任何错误信息**。

**根因**：`target/debug/incremental/` 下的会话锁文件在非 ASCII 路径上删不掉，
rustc 启动时先去清理上一轮的锁，被系统拒绝：

```
warning: error deleting lock file for incremental compilation session directory
  `\\?\E:\...\target\debug\incremental\xxx\s-xxx.lock`: 拒绝访问。 (os error 5)
```

清不掉就一直等锁。日志里那几行 `拒绝访问` 是唯一线索，很容易被当成无关警告划过去。

**判定方法**（不用猜）：

```powershell
# CPU 时间不涨 = 在等锁，不是在编译
Get-Process rustc | Select-Object Id,CPU,StartTime
```

隔 5 秒再看一次 `CPU`，若几乎不变即可确诊。线程状态会显示 `Wait` / `EventPairLow`。

**绕法**（已落地）：仓库根目录 `.cargo/config.toml` 关掉增量编译。

```toml
[build]
incremental = false
```

代价是 Rust 代码改动后重编稍慢；本项目主要改 TypeScript，Rust 改动频率低，稳定性优先。

**另一种触发方式**：同时跑两个 cargo（例如上一个后台任务没结束就又开一个），会争抢
`target/debug/.cargo-build-lock`，报 `failed to open: ...\.cargo-build-lock`。
看到这个错就先确认没有残留的 `cargo.exe` / `rustc.exe`，别急着删锁文件。

**强杀后的清理**：被超时强杀过的编译，`target/debug/incremental` 里会留下无主锁。
直接整目录删掉即可，Cargo 会自己重建：

```bash
rm -rf apps/desktop/src-tauri/target/debug/incremental
```

## 编译很慢是否正常

`cargo build` 在本机全量编桌面端约 **9~10 分钟**（含 ph-core/ph-store/ph-providers/ph-engine
与 Tauri 外壳，`opt-level` 见 workspace 配置）。这只是首次或大改后；只改 UI 时不需要重编 Rust。

如果远超这个量级且 CPU 不动，按上一节判定是否为等锁。

## 产物不覆盖（版本号规则）

`dist/` 里放的是**已交付的历史版本**，不是工作区。构建脚本的规矩：

- 目标文件已存在 → **直接报错退出**，不覆盖、不删除
- 要再出一版 → 先升 `Cargo.toml` 的 `[workspace.package] version`

升哪一位按语义化版本判定（0.x 阶段也一样）：

| 改了什么 | 升哪一位 | 例子 |
|---|---|---|
| 修 bug、改文案、补日志 | PATCH | 0.2.0 → 0.2.1 |
| 新增功能、新增命令、新增平台 | MINOR | 0.2.1 → 0.3.0 |
| 破坏兼容（数据格式、接口、配置项不兼容） | 0.x 阶段用 MINOR；1.0.0 起用 MAJOR | 0.3.0 → 0.4.0 |

版本号在四处出现，脚本以 `Cargo.toml` 为准自动同步 `tauri.conf.json` 与
`ui/package.json`；程序内显示的版本取自 `CARGO_PKG_VERSION`，不会各说各话。

## 不打包也能验收：`1. 一键启动预览.bat`

项目根目录下双击即可。`.bat` 只是 6 行壳，真正逻辑在 `scripts/preview.ps1`。

它按**文件时间**决定编不编，不是每次都全跑：

| 情况 | 行为 | 实测耗时 |
|---|---|---|
| 什么都没改 | 直接运行 `target\debug\pointhub.exe` | 秒开 |
| 只改界面 | `npm run build` → 编 Rust | 前端 1.4s + Rust 22s |
| 改了 Rust | 只编 Rust | 约 22s |

为什么改界面也要重编 Rust：Tauri 在前端产物在**编译期嵌入可执行文件**，
不是运行时读目录。

- **不产出、也不改动 `dist/`**，看的是当前源码
- 那个黑窗口是日志窗口，**关掉窗口 = 退出程序**

## 脚本文件必须是纯 ASCII（或带 BOM）

第 5 个坑，也是最阴的一个：**Windows PowerShell 5.1 读无 BOM 的 `.ps1` 时用系统 ANSI 代码页**，
UTF-8 的中文注释会被解成乱码，进而吃掉引号，报一堆莫名其妙的「表达式缺少 }」。

```
表达式或语句中包含意外的标记")"
```

`.bat` 同理（cmd.exe 按 GBK 读）。三条应对：

1. **首选纯 ASCII**：脚本里的中文一律挪进文档；确实需要中文（比如产物文件名）用码点拼：

   ```powershell
   $cnName = -join ([char[]](0x79EF, 0x5206, 0x67A2, 0x7EBD))   # 积分枢纽
   ```

2. **改 JSON 别用 `Get-Content -Raw` 裸读**：PS 5.1 默认按 ANSI 解，中文会被写坏。
   必须 `-Encoding UTF8` 读、`UTF8Encoding($false)` 写（带 BOM 的 JSON 会让 serde_json 报错）。
3. **`-replace` 不支持脚本块**（那是 PS 6+）。写成脚本块的话，它会把**脚本块的源码文本**
   写进文件——JSON 当场废掉。用 `[regex]::Replace($raw, $pattern, ('${1}' + $v + '${2}'))`。

