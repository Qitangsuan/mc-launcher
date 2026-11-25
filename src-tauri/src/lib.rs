mod commands;
pub mod core;

pub fn run() {
    crate::boot_log("lib::run: building tauri app");
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::version::scan_versions,
            commands::version::list_remote_versions,
            commands::version::install_version,
            commands::version::install_fabric,
            commands::version::list_forge_versions,
            commands::version::list_neoforge_versions,
            commands::version::install_forge,
            commands::version::launch_game,
            commands::auth::get_account,
            commands::auth::get_accounts,
            commands::auth::switch_account,
            commands::auth::remove_account,
            commands::auth::login_offline,
            commands::auth::logout,
            commands::auth::open_mods_dir,
            commands::auth::get_client_id,
            commands::auth::set_client_id,
            commands::auth::open_url,
            commands::auth::ms_request_code,
            commands::auth::ms_complete_login,
            commands::auth::ms_refresh,
        ])
        .build(tauri::generate_context!());

    match app {
        Ok(app) => {
            crate::boot_log("lib::run: build ok, entering run");
            app.run(|_app, _event| {});
            crate::boot_log("lib::run: run returned");
        }
        Err(e) => {
            crate::boot_log(&format!("lib::run: build FAILED: {e}"));
            std::process::exit(1);
        }
    }
}

/// 给 main.rs 用的启动日志
pub fn boot_log(msg: &str) {
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
