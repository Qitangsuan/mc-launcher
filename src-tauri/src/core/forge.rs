//! Forge / NeoForge 安装。
//!
//! 和 Fabric 完全不同的路线：Forge 没有「直接给 JSON」的 API，
//! 必须下载官方的 installer jar 然后跑它自己的安装逻辑：
//!
//!   java -jar forge-installer.jar --installClient <game_dir>
//!
//! installer 会自己下载库、创建 versions/ 目录。我们能做的只有：给对版本、给对 Java、等它跑完。

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::core::java_download;

const FORGE_PROMOTIONS: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const FORGE_METADATA: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
const NEOFORGE_METADATA: &str =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";
const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersions {
    pub latest: Option<String>,
    pub recommended: Option<String>,
    pub all: Vec<String>,
}

fn emit(app: &AppHandle, stage: &str) {
    let _ = app.emit(
        "download-progress",
        serde_json::json!({ "stage": stage, "done": 0, "total": 0 }),
    );
}

fn push_log(app: &AppHandle, message: &str) {
    let _ = app.emit("launcher-log", serde_json::json!({ "message": message }));
}

/// 查某个游戏版本可用的 Forge 版本。
///
/// 注意：不能只看 promotions_slim.json —— 它只收录 latest / recommended 两个，
/// 很多游戏版本（比如 1.20.1）在里面根本没有条目，会误判成「没有 Forge」。
/// 正确来源是 maven-metadata.xml，里面有 `1.20.1-47.4.29` 这种完整版本列表。
pub async fn list_forge_versions(game_version: &str) -> Result<LoaderVersions, String> {
    let client = reqwest::Client::new();

    let resp = client
        .get(FORGE_METADATA)
        .send()
        .await
        .map_err(|e| format!("获取 Forge 版本清单失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "获取 Forge 版本清单失败：HTTP {}",
            resp.status()
        ));
    }
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取 Forge 版本清单失败: {e}"))?;

    let prefix = format!("{game_version}-");
    let mut all: Vec<String> = Vec::new();
    for segment in text.split("<version>").skip(1) {
        let Some(end) = segment.find("</version>") else {
            continue;
        };
        let full = &segment[..end];
        if let Some(loader) = full.strip_prefix(&prefix) {
            if !loader.is_empty() {
                all.push(loader.to_string());
            }
        }
    }

    if all.is_empty() {
        return Err(format!(
            "Forge 没有为 {game_version} 发布过版本（Forge 只支持到 1.20.6，再新的版本要用 NeoForge）"
        ));
    }

    all.sort_by(|a, b| version_key(a).cmp(&version_key(b)));

    // 推荐版本仍然从 promotions 里取（拿不到就算了，不报错）
    let mut latest = None;
    let mut recommended = None;
    if let Ok(resp) = client.get(FORGE_PROMOTIONS).send().await {
        if let Ok(value) = resp.json::<serde_json::Value>().await {
            if let Some(promotions) = value.get("promotions").and_then(|p| p.as_object()) {
                for (key, val) in promotions {
                    let Some((game, kind)) = key.rsplit_once('-') else {
                        continue;
                    };
                    if game != game_version {
                        continue;
                    }
                    let Some(version) = val.as_str() else {
                        continue;
                    };
                    match kind {
                        "latest" => latest = Some(version.to_string()),
                        "recommended" => recommended = Some(version.to_string()),
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(LoaderVersions {
        latest,
        recommended,
        all,
    })
}

/// 版本号按数字段比较，避免 "47.4.3" 排到 "47.4.29" 后面
fn version_key(v: &str) -> Vec<u32> {
    v.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u32>().ok())
        .collect()
}

/// 查某个游戏版本可用的 NeoForge 版本
pub async fn list_neoforge_versions(game_version: &str) -> Result<LoaderVersions, String> {
    // NeoForge 版本号规则：21.1.66 对应 MC 1.21.1，21.0.x 对应 MC 1.21
    let parts: Vec<&str> = game_version.split('.').collect();
    if parts.len() < 2 {
        return Err("游戏版本号格式不对".into());
    }
    let prefix = if parts.len() >= 3 {
        format!("{}.{}", parts[1], parts[2])
    } else {
        format!("{}.0", parts[1])
    };

    let client = reqwest::Client::new();
    let resp = client
        .get(NEOFORGE_METADATA)
        .send()
        .await
        .map_err(|e| format!("获取 NeoForge 版本列表失败: {e}"))?;

    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取 NeoForge 版本列表失败: {e}"))?;

    let mut all = Vec::new();
    for segment in text.split("<version>").skip(1) {
        let Some(end) = segment.find("</version>") else {
            continue;
        };
        let version = &segment[..end];
        if version.starts_with(&format!("{prefix}.")) {
            all.push(version.to_string());
        }
    }

    if all.is_empty() {
        return Err(format!(
            "NeoForge 没有为 {game_version} 提供版本（需要 1.20.2 及以上）"
        ));
    }

    all.sort_by(|a, b| version_key(a).cmp(&version_key(b)));
    let latest = all.last().cloned();
    Ok(LoaderVersions {
        latest,
        recommended: None,
        all,
    })
}

/// 跑 installer，返回新生成的版本 id
pub async fn install(
    app: &AppHandle,
    game_dir: &Path,
    game_version: &str,
    loader_version: &str,
    neoforge: bool,
) -> Result<String, String> {
    let game_json = crate::core::paths::version_json_path(game_dir, game_version);
    if !game_json.exists() {
        return Err(format!("先安装原版 {game_version}，再装加载器"));
    }

    let before = list_version_dirs(game_dir);

    // 1. 下载 installer
    let (url, file_name) = if neoforge {
        (
            format!("{NEOFORGE_MAVEN}/{loader_version}/neoforge-{loader_version}-installer.jar"),
            format!("neoforge-{loader_version}-installer.jar"),
        )
    } else {
        (
            format!(
                "{FORGE_MAVEN}/{game_version}-{loader_version}/forge-{game_version}-{loader_version}-installer.jar"
            ),
            format!("forge-{game_version}-{loader_version}-installer.jar"),
        )
    };

    let cache_dir = crate::core::paths::launcher_dir().join("cache");
    std::fs::create_dir_all(&cache_dir).map_err(|e| format!("创建缓存目录失败: {e}"))?;
    let installer_path = cache_dir.join(&file_name);

    if !installer_path.exists() {
        push_log(app, &format!("下载 installer：{file_name}"));
        emit(app, "下载 installer");

        let client = reqwest::Client::builder()
            .user_agent("MCLauncher/0.1")
            .timeout(std::time::Duration::from_secs(900))
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("下载 installer 失败: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "下载 installer 失败：HTTP {}（地址 {url}）",
                resp.status()
            ));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("读取 installer 失败: {e}"))?;
        std::fs::write(&installer_path, &bytes)
            .map_err(|e| format!("写入 installer 失败: {e}"))?;
    }

    // 2. 选 Java：installer 的 Java 要求和游戏版本一致（1.20.1 的 Forge 47 要 17）
    emit(app, "准备 Java");
    let required = crate::core::version::load_resolved(game_dir, game_version)
        .ok()
        .and_then(|v| v.java_version.and_then(|j| j.major_version))
        .unwrap_or(if neoforge { 17 } else { 8 });
    let java = java_download::ensure_java(app, required).await?;
    push_log(app, &format!("使用 Java {}：{}", java.major, java.path));

    // 3. 跑 installer
    // Forge installer 会检查 .minecraft/launcher_profiles.json 是否存在，
    // 那是官方启动器的配置文件。没它 installer 直接报
    // "There is no Minecraft launcher profile, you need to run the launcher first" 并退出。
    ensure_launcher_profiles(game_dir)?;

    emit(app, "运行安装器（会下载较多文件，慢是正常的）");
    push_log(app, "开始运行 installer，这一步可能要几分钟…");

    let output = tokio::process::Command::new(&java.path)
        .arg("-Djava.awt.headless=true")
        .arg("-jar")
        .arg(&installer_path)
        .arg("--installClient")
        .arg(game_dir)
        .output()
        .await
        .map_err(|e| format!("启动 installer 失败: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    for line in stdout.lines().chain(stderr.lines()) {
        if !line.trim().is_empty() {
            push_log(app, line);
        }
    }

    if !output.status.success() {
        return Err(format!(
            "installer 执行失败（退出码 {:?}），看日志区的详细输出",
            output.status.code()
        ));
    }

    // 4. 找出新增的版本目录
    let after = list_version_dirs(game_dir);
    let created: Vec<String> = after
        .iter()
        .filter(|id| !before.contains(id))
        .cloned()
        .collect();

    // 重装同一个加载器版本时 installer 不会产生新目录，此时按「已存在的目标目录」判定成功，
    // 不然会把成功当失败
    let new_id = created
        .into_iter()
        .next()
        .or_else(|| {
            after
                .iter()
                .find(|id| id.contains(game_version) && id.contains(loader_version))
                .cloned()
        })
        .ok_or_else(|| {
            "installer 跑完了但没有生成新版本目录。看日志区里 installer 的输出，通常是下载失败。"
                .to_string()
        })?;

    push_log(app, &format!("安装完成：{new_id}"));
    Ok(new_id)
}

fn list_version_dirs(game_dir: &Path) -> Vec<String> {
    let dir = game_dir.join("versions");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}

/// Forge/NeoForge installer 的前置条件：launcher_profiles.json 必须存在
fn ensure_launcher_profiles(game_dir: &Path) -> Result<(), String> {
    let path = game_dir.join("launcher_profiles.json");
    if path.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(game_dir).map_err(|e| format!("创建游戏目录失败: {e}"))?;
    let content = r#"{"profiles":{},"settings":{},"version":3}"#;
    std::fs::write(&path, content).map_err(|e| format!("创建 launcher_profiles.json 失败: {e}"))
}

/// installer jar 的缓存路径（调试用）
#[allow(dead_code)]
pub fn installer_cache(file_name: &str) -> PathBuf {
    crate::core::paths::launcher_dir().join("cache").join(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- version_key ----------

    #[test]
    fn version_key_compares_numerically_not_lexically() {
        // "47.4.29" 必须排在 "47.4.3" 之后（按字符串比会反过来，导致最新版选错）
        assert!(
            version_key("47.4.29") > version_key("47.4.3"),
            "47.4.29 应该大于 47.4.3"
        );
        assert!(version_key("47.4.3") < version_key("47.4.29"));
    }

    #[test]
    fn version_key_parses_numeric_segments() {
        // 版本号被拆成数字段，非数字字符只是分隔符
        assert_eq!(version_key("47.4.29"), vec![47, 4, 29]);
        assert_eq!(version_key("47.4.3"), vec![47, 4, 3]);
        // 1.20.1 这种多段版本同理
        assert_eq!(version_key("1.20.1"), vec![1, 20, 1]);
        assert!(version_key("1.20.10") > version_key("1.20.9"));
        // 有字母后缀时只取数字段（比如 NeoForge 的 21.1.66-beta）
        assert_eq!(version_key("21.1.66-beta"), vec![21, 1, 66]);
        // 空串没有任何数字段
        assert!(version_key("").is_empty());
    }

    #[test]
    fn version_key_ordering_follows_major_then_minor() {
        let mut versions = vec!["47.4.29", "47.4.3", "47.1.0", "48.0.1"];
        versions.sort_by(|a, b| version_key(a).cmp(&version_key(b)));
        assert_eq!(
            versions,
            vec!["47.1.0", "47.4.3", "47.4.29", "48.0.1"],
            "排序结果应该按数字段递增"
        );
    }
}
