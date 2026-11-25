mod commands;
pub mod core;

pub fn run() {
    tauri::Builder::default()
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
