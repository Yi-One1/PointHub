#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// 只做一件事：启动库里的 app。
// 注意不要把 commands 挂到这里——那样会把它编进 bin crate，
// `crate::` 的指向就和 lib 不一致了，命令层会找不到状态类型。
fn main() {
    pointhub_lib::run()
}
