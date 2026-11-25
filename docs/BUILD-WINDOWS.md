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
