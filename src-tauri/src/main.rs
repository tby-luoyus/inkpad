//! 可执行文件入口
// Windows Release 下不弹控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    sticky_notes_lib::run()
}
