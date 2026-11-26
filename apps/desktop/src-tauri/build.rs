fn main() {
    // 图标是编译期嵌进 exe 的。不显式声明依赖的话，只改图标时 cargo 不会重跑
    // build.rs，任务栏会一直显示旧图。
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::build()
}
