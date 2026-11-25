//! 启动器配置（目前只有微软 client_id）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LauncherConfig {
    #[serde(default)]
    pub microsoft_client_id: String,
}

fn path() -> std::path::PathBuf {
    crate::core::paths::launcher_dir().join("config.json")
}

pub fn load() -> LauncherConfig {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(config: &LauncherConfig) -> Result<(), String> {
    let dir = crate::core::paths::launcher_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    let text = serde_json::to_string_pretty(config).map_err(|e| format!("序列化配置失败: {e}"))?;
    std::fs::write(path(), text).map_err(|e| format!("写入配置失败: {e}"))
}
