// Windows 发布版不弹控制台窗口；debug 版保留，方便看日志
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// 启动阶段的埋点日志：release 版没有控制台，双击闪退时靠这个定位死在哪一步
fn boot_log(msg: &str) {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let dir = std::path::PathBuf::from(appdata).join("mc-launcher");
        let _ = std::fs::create_dir_all(&dir);
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("boot.log"))
        {
            let _ = writeln!(f, "{msg}");
        }
    }
}

fn main() {
    boot_log("=== main entered ===");

    std::panic::set_hook(Box::new(|info| {
        boot_log(&format!("panic: {info}"));
    }));

    boot_log("calling mc_launcher_lib::run()");
    mc_launcher_lib::run();
    boot_log("=== run() returned (should not happen) ===");
}
