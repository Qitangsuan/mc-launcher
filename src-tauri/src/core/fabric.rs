//! Fabric 模组加载器安装。
//!
//! 比 Forge 简单太多：Fabric 官方 API 直接给你一份完整的版本 JSON（带 inheritsFrom），
//! 存进 versions/ 目录就能用，不需要跑 installer。

use std::path::Path;

use serde::Serialize;

const FABRIC_META: &str = "https://meta.fabricmc.net/v2";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricLoader {
    pub version: String,
    pub stable: bool,
}

pub async fn list_loaders(game_version: &str) -> Result<Vec<FabricLoader>, String> {
    let client = reqwest::Client::new();
    let url = format!("{FABRIC_META}/versions/loader/{game_version}");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("获取 Fabric 版本失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "获取 Fabric 版本失败：HTTP {}（这个游戏版本可能没有 Fabric 支持）",
            resp.status()
        ));
    }

    let value: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 Fabric 版本失败: {e}"))?;

    let mut out = Vec::new();
    if let Some(list) = value.as_array() {
        for item in list {
            let Some(loader) = item.get("loader") else {
                continue;
            };
            let version = loader
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if version.is_empty() {
                continue;
            }
            out.push(FabricLoader {
                stable: loader.get("stable").and_then(|v| v.as_bool()).unwrap_or(false),
                version,
            });
        }
    }

    Ok(out)
}

/// 装 Fabric，返回新版本目录名（可以直接用 launch_game 启动）
pub async fn install(
    game_dir: &Path,
    game_version: &str,
    loader_version: &str,
) -> Result<String, String> {
    let game_json = crate::core::paths::version_json_path(game_dir, game_version);
    if !game_json.exists() {
        return Err(format!("先安装原版 {game_version}，再装 Fabric"));
    }

    let client = reqwest::Client::new();
    let url = format!("{FABRIC_META}/versions/loader/{game_version}/{loader_version}/profile/json");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("获取 Fabric 配置失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "获取 Fabric 配置失败：HTTP {}",
            resp.status()
        ));
    }

    let value: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 Fabric 配置失败: {e}"))?;

    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if id.is_empty() {
        return Err("Fabric 返回的配置里没有 id".into());
    }
    // id 来自远程响应，直接当目录名用之前必须先校验，防路径穿越
    crate::core::paths::validate_id(&id)?;

    let dir = game_dir.join("versions").join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建版本目录失败: {e}"))?;

    let text = serde_json::to_string_pretty(&value)
        .map_err(|e| format!("序列化 Fabric 配置失败: {e}"))?;
    std::fs::write(dir.join(format!("{id}.json")), text)
        .map_err(|e| format!("写入 Fabric 配置失败: {e}"))?;

    Ok(id)
}
