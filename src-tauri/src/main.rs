// Windows 发布版不弹控制台窗口；debug 版保留，方便看日志
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    mc_launcher_lib::run()
}
