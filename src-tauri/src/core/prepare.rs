//! 生成下载任务列表。

use std::collections::HashMap;
use std::path::Path;

use crate::core::download::DownloadTask;
use crate::core::natives::native_jar_relative;
use crate::core::version::{library_path, rules_allow, Library};

pub const ASSETS_BASE: &str = "https://resources.download.minecraft.net/";

/// 普通库 + 老式 natives classifier + Fabric 的纯 maven 库
pub fn library_tasks(
    libs: &[Library],
    libraries_dir: &Path,
    features: &HashMap<String, bool>,
) -> Vec<DownloadTask> {
    let mut out = Vec::new();

    for lib in libs {
        if !rules_allow(&lib.rules, features) {
            continue;
        }

        let mut handled = false;

        if let Some(downloads) = &lib.downloads {
            // 主 artifact
            if let Some(artifact) = &downloads.artifact {
                if !artifact.url.is_empty() {
                    if let Some(rel) = library_path(&lib.name) {
                        out.push(DownloadTask {
                            url: artifact.url.clone(),
                            sha1: artifact.sha1.clone(),
                            path: libraries_dir.join(rel),
                            size: artifact.size,
                        });
                        handled = true;
                    }
                }
            }

            // 老式 natives classifier（1.16 及以前）
            if let Some(pattern) = lib.natives.as_ref().and_then(|n| n.get("windows")) {
                let classifier = pattern.replace("${arch}", "64");
                if let Some(file) = downloads
                    .classifiers
                    .as_ref()
                    .and_then(|c| c.get(&classifier))
                {
                    if !file.url.is_empty() {
                        if let Some(rel) = native_jar_relative(lib, &classifier) {
                            out.push(DownloadTask {
                                url: file.url.clone(),
                                sha1: file.sha1.clone(),
                                path: libraries_dir.join(rel),
                                size: file.size,
                            });
                            handled = true;
                        }
                    }
                }
            }
        }

        // Fabric 这类：只给 maven 仓库地址，没有 downloads
        if !handled {
            if let Some(repo) = &lib.url {
                if let Some(rel) = library_path(&lib.name) {
                    let base = repo.trim_end_matches('/');
                    out.push(DownloadTask {
                        url: format!("{base}/{rel}"),
                        sha1: String::new(),
                        path: libraries_dir.join(rel),
                        size: 0,
                    });
                }
            }
        }
    }

    out
}

/// 资源索引 objects -> 按 hash 分目录的下载任务
pub fn asset_tasks(index_json: &serde_json::Value, assets_dir: &Path) -> Vec<DownloadTask> {
    let mut out = Vec::new();
    let Some(objects) = index_json.get("objects").and_then(|o| o.as_object()) else {
        return out;
    };

    for (_name, obj) in objects {
        let Some(hash) = obj.get("hash").and_then(|h| h.as_str()) else {
            continue;
        };
        // 用 get 而不是 &hash[..2]：hash 若不是 ASCII，字节切片会切在字符边界上 panic
        let Some(sub) = hash.get(0..2) else {
            continue;
        };
        let size = obj.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
        out.push(DownloadTask {
            url: format!("{ASSETS_BASE}{sub}/{hash}"),
            sha1: hash.to_string(),
            path: assets_dir.join("objects").join(sub).join(hash),
            size,
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- asset_tasks ----------

    #[test]
    fn asset_tasks_uses_hash_prefix_dir_and_official_url() {
        // 资源文件按 hash 前两位分目录存放：
        //   assets/objects/<前两位>/<hash>
        // 下载地址同样是 <官方资源站>/<前两位>/<hash>
        let hash = "4f0e2b6c5a1d3e8f9a0b1c2d3e4f5a6b7c8d9e0f";
        let assets_dir = Path::new(r"C:\game\assets");
        let index = serde_json::json!({
            "objects": {
                "minecraft/sounds/ambient/cave/cave1.ogg": {
                    "hash": hash,
                    "size": 12345
                }
            }
        });

        let tasks = asset_tasks(&index, assets_dir);

        assert_eq!(tasks.len(), 1);
        let task = &tasks[0];

        // url：官方资源站 + 前两位 + 完整 hash
        assert_eq!(
            task.url,
            "https://resources.download.minecraft.net/4f/4f0e2b6c5a1d3e8f9a0b1c2d3e4f5a6b7c8d9e0f"
        );

        // 路径：assets/objects/4f/<完整 hash>
        assert_eq!(
            task.path,
            assets_dir
                .join("objects")
                .join("4f")
                .join("4f0e2b6c5a1d3e8f9a0b1c2d3e4f5a6b7c8d9e0f")
        );
        assert!(
            task.path
                .ends_with(r"objects\4f\4f0e2b6c5a1d3e8f9a0b1c2d3e4f5a6b7c8d9e0f"),
            "实际路径：{:?}",
            task.path
        );

        // sha1 字段就是 hash，size 原样带过来
        assert_eq!(task.sha1, hash);
        assert_eq!(task.size, 12345);
    }

    #[test]
    fn asset_tasks_handles_multiple_objects() {
        // 多个资源各生成一条任务，并且都按各自 hash 的前两位分目录
        let index = serde_json::json!({
            "objects": {
                "minecraft/lang/en_us.json": { "hash": "aabbccdd", "size": 1 },
                "minecraft/lang/zh_cn.json": { "hash": "11223344", "size": 2 }
            }
        });
        let assets_dir = Path::new("assets");

        let tasks = asset_tasks(&index, assets_dir);

        assert_eq!(tasks.len(), 2);
        assert!(tasks
            .iter()
            .any(|t| t.url == "https://resources.download.minecraft.net/aa/aabbccdd"
                && t.path == assets_dir.join("objects").join("aa").join("aabbccdd")));
        assert!(tasks
            .iter()
            .any(|t| t.url == "https://resources.download.minecraft.net/11/11223344"
                && t.path == assets_dir.join("objects").join("11").join("11223344")));
    }

    #[test]
    fn asset_tasks_returns_empty_without_objects() {
        // 没有 objects 字段（或者空格子）时返回空列表，不能 panic
        assert!(asset_tasks(&serde_json::json!({}), Path::new("assets")).is_empty());
        assert!(asset_tasks(
            &serde_json::json!({ "objects": {} }),
            Path::new("assets")
        )
        .is_empty());
        // 缺 hash 或 hash 太短的对象直接跳过
        let broken = serde_json::json!({
            "objects": {
                "no_hash": { "size": 1 },
                "too_short": { "hash": "a", "size": 2 }
            }
        });
        assert!(asset_tasks(&broken, Path::new("assets")).is_empty());
    }
}
