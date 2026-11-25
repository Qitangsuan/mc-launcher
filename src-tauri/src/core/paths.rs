use std::path::PathBuf;

/// 默认游戏目录：%APPDATA%\.minecraft
pub fn default_game_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join(".minecraft")
    } else {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".minecraft")
    }
}

/// 启动器自己的配置目录：%APPDATA%\mc-launcher
pub fn launcher_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("mc-launcher")
    } else {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".mc-launcher")
    }
}

/// 版本 JSON 所在目录
pub fn version_json_path(game_dir: &std::path::Path, id: &str) -> PathBuf {
    game_dir.join("versions").join(id).join(format!("{id}.json"))
}

/// 会参与路径拼接的标识符（版本 id、loader 版本号）必须先过这关，
/// 否则 `..\..\..\Users\Public\x` 这类值能让安装写到游戏目录之外。
pub fn validate_id(id: &str) -> Result<(), String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("版本标识不能为空".into());
    }
    if trimmed == "." || trimmed == ".." {
        return Err(format!("版本标识不合法: {id}"));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    {
        return Err(format!("版本标识含非法字符: {id}"));
    }
    Ok(())
}
