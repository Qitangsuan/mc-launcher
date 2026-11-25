use crate::core::account::{self, Account, AccountStore};
use crate::core::config;
use crate::core::microsoft::{self, DeviceCodeInfo};

#[tauri::command]
pub fn get_account() -> Result<Option<Account>, String> {
    Ok(account::current_account())
}

#[tauri::command]
pub fn get_accounts() -> Result<AccountStore, String> {
    Ok(account::load_store())
}

#[tauri::command]
pub fn switch_account(index: usize) -> Result<Option<Account>, String> {
    account::switch(index)?;
    Ok(account::current_account())
}

#[tauri::command]
pub fn remove_account(index: usize) -> Result<Option<Account>, String> {
    account::remove(index)?;
    Ok(account::current_account())
}

#[tauri::command]
pub fn login_offline(username: String) -> Result<Account, String> {
    let username = username.trim().to_string();
    account::validate_username(&username)?;

    let acc = Account {
        uuid: account::offline_uuid(&username),
        username,
        access_token: "0".into(),
        user_type: "legacy".into(),
        kind: "offline".into(),
        refresh_token: None,
    };
    account::upsert(acc)
}

#[tauri::command]
pub fn logout() -> Result<(), String> {
    let store = account::load_store();
    if store.accounts.is_empty() {
        return Ok(());
    }
    account::remove(store.current)
}

#[tauri::command]
pub fn get_client_id() -> Result<String, String> {
    Ok(config::load().microsoft_client_id)
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("只允许打开 http/https 链接".into());
    }
    // 这个 URL 会被拼进 `cmd /C start "" <url>`，cmd 会解释 & | ^ < > % "
    // 而这些值来自外部 API 响应（微软 Device Code 的 verification_uri），
    // 不校验就是一条现成的命令注入链
    if url
        .chars()
        .any(|c| matches!(c, '&' | '|' | '^' | '<' | '>' | '%' | '"' | '\r' | '\n'))
    {
        return Err("链接里含有不安全字符".into());
    }

    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url.as_str()])
            .spawn()
            .map_err(|e| format!("打开浏览器失败: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        let _ = url;
    }
    Ok(())
}

/// 打开 mods 目录（不存在就建一个）
#[tauri::command]
pub fn open_mods_dir() -> Result<String, String> {
    let mods = crate::core::paths::default_game_dir().join("mods");
    std::fs::create_dir_all(&mods).map_err(|e| format!("创建 mods 目录失败: {e}"))?;

    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&mods)
            .spawn()
            .map_err(|e| format!("打开资源管理器失败: {e}"))?;
    }

    Ok(mods.display().to_string())
}

#[tauri::command]
pub fn set_client_id(client_id: String) -> Result<(), String> {
    let mut cfg = config::load();
    cfg.microsoft_client_id = client_id.trim().to_string();
    config::save(&cfg)
}

#[tauri::command]
pub async fn ms_request_code() -> Result<DeviceCodeInfo, String> {
    let cfg = config::load();
    if cfg.microsoft_client_id.is_empty() {
        return Err("还没配置微软 client_id，先在账号面板里填上".into());
    }
    microsoft::request_device_code(&cfg.microsoft_client_id).await
}

#[tauri::command]
pub async fn ms_complete_login(
    device_code: String,
    interval: u64,
    expires_in: u64,
) -> Result<Account, String> {
    let cfg = config::load();
    let acc = microsoft::complete_login(
        &cfg.microsoft_client_id,
        &device_code,
        interval,
        expires_in,
    )
    .await?;
    account::upsert(acc)
}

#[tauri::command]
pub async fn ms_refresh() -> Result<Account, String> {
    let cfg = config::load();
    let Some(existing) = account::current_account() else {
        return Err("没有已保存的账号".into());
    };
    let Some(refresh_token) = existing.refresh_token.clone() else {
        return Err("这个账号没有 refresh token，重新登录一次".into());
    };
    let acc = microsoft::refresh_microsoft_token(&cfg.microsoft_client_id, &refresh_token).await?;
    account::upsert(acc)
}
