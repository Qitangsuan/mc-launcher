use serde::{Deserialize, Serialize};
use std::path::Path;

/// 前端展示用的版本信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    /// 版本目录名，例如 1.20.4 / fabric-loader-0.15.11-1.20.1
    pub id: String,
    /// 对应的原版游戏版本
    pub game_version: String,
    /// vanilla / fabric / forge / neoforge / quilt
    pub loader: String,
    /// 需要的 Java 大版本
    pub java_major: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
struct VersionMeta {
    #[serde(default)]
    id: String,
    #[serde(rename = "type", default)]
    version_type: String,
    #[serde(rename = "mainClass", default)]
    main_class: String,
    #[serde(rename = "javaVersion", default)]
    java_version: Option<JavaVersionInfo>,
    #[serde(rename = "inheritsFrom", default)]
    inherits_from: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct JavaVersionInfo {
    #[serde(rename = "majorVersion", default)]
    major_version: Option<u32>,
}

fn read_meta(path: &Path) -> Option<VersionMeta> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// 沿 inheritsFrom 向上查找 javaVersion
fn resolve_java_major(game_dir: &Path, meta: &VersionMeta) -> Option<u32> {
    if let Some(j) = &meta.java_version {
        if j.major_version.is_some() {
            return j.major_version;
        }
    }
    let mut current = meta.inherits_from.clone();
    let mut depth = 0;
    while let Some(pid) = current {
        if depth > 32 {
            break;
        }
        let path = crate::core::paths::version_json_path(game_dir, &pid);
        let Some(parent) = read_meta(&path) else {
            break;
        };
        if let Some(j) = &parent.java_version {
            if j.major_version.is_some() {
                return j.major_version;
            }
        }
        current = parent.inherits_from;
        depth += 1;
    }
    None
}

fn detect_loader(id: &str, meta: &VersionMeta) -> String {
    let lower = format!("{} {}", id, meta.version_type).to_ascii_lowercase();
    if lower.contains("neoforge") {
        "neoforge".into()
    } else if lower.contains("fabric") {
        "fabric".into()
    } else if lower.contains("quilt") {
        "quilt".into()
    } else if lower.contains("forge") {
        "forge".into()
    } else if lower.contains("optifine") {
        "optifine".into()
    } else {
        "vanilla".into()
    }
}

fn detect_game_version(meta: &VersionMeta, id: &str) -> String {
    // 装了 Mod 加载器时，inheritsFrom / 父 id 才是原版版本号
    if let Some(parent) = &meta.inherits_from {
        return parent.clone();
    }
    if !meta.id.is_empty() {
        return meta.id.clone();
    }
    id.to_string()
}

fn version_key(v: &str) -> Vec<u32> {
    v.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u32>().ok())
        .collect()
}

pub fn scan_installed(game_dir: &Path) -> Vec<VersionInfo> {
    let dir = game_dir.join("versions");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        let json = crate::core::paths::version_json_path(game_dir, &id);
        if !json.exists() {
            continue;
        }
        let Some(meta) = read_meta(&json) else {
            continue;
        };
        out.push(VersionInfo {
            game_version: detect_game_version(&meta, &id),
            loader: detect_loader(&id, &meta),
            java_major: resolve_java_major(game_dir, &meta),
            id,
        });
    }

    out.sort_by(|a, b| {
        version_key(&b.game_version)
            .cmp(&version_key(&a.game_version))
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}
