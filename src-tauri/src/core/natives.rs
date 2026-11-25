//! natives 处理。
//!
//! 老版本（1.16 及以前）的 natives 是 classifier jar，要解压到目录再 -Djava.library.path。
//! 1.19+ 把这玩意儿拆成独立库直接进 classpath，LWJGL 自己处理，不用解压。

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::core::version::{natives_pattern, resolve_natives_pattern, Library};

/// org.lwjgl:lwjgl:3.3.2 + natives-windows -> org/lwjgl/lwjgl/3.3.2/lwjgl-3.3.2-natives-windows.jar
pub fn native_jar_relative(lib: &Library, classifier: &str) -> Option<String> {
    let mut parts = lib.name.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next()?;
    Some(format!(
        "{}/{}/{}/{}-{}-{}.jar",
        group.replace('.', "/"),
        artifact,
        version,
        artifact,
        version,
        classifier
    ))
}

/// 解压所有带 natives 字段的库，返回解压文件数
pub fn extract_natives(
    libs: &[Library],
    libraries_dir: &Path,
    natives_dir: &Path,
) -> Result<usize, String> {
    std::fs::create_dir_all(natives_dir).map_err(|e| format!("创建 natives 目录失败: {e}"))?;

    let mut count = 0;
    for lib in libs {
        let Some(pattern) = natives_pattern(lib) else {
            continue;
        };
        let classifier = resolve_natives_pattern(&pattern);
        let Some(rel) = native_jar_relative(lib, &classifier) else {
            continue;
        };
        let jar = libraries_dir.join(rel);
        if !jar.exists() {
            continue;
        }

        let file = std::fs::File::open(&jar).map_err(|e| format!("打开 {jar:?} 失败: {e}"))?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| format!("打开 zip 失败 {jar:?}: {e}"))?;

        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| format!("读取 zip 条目失败: {e}"))?;
            let name = entry.name().to_string();
            if name.starts_with("META-INF/") || name.ends_with('/') {
                continue;
            }

            // 防 zip slip：条目名里出现 .. 一律跳过
            if name.split(['/', '\\']).any(|seg| seg == "..") {
                continue;
            }

            let out_path: PathBuf = natives_dir.join(&name);
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("建目录失败: {e}"))?;
            }
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("解压 {name} 失败: {e}"))?;
            std::fs::write(&out_path, &buf)
                .map_err(|e| format!("写入 {out_path:?} 失败: {e}"))?;
            count += 1;
        }
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- native_jar_relative ----------

    #[test]
    fn native_jar_relative_builds_classifier_jar_path() {
        // org.lwjgl:lwjgl:3.3.2 + natives-windows
        // -> org/lwjgl/lwjgl/3.3.2/lwjgl-3.3.2-natives-windows.jar
        let lib = Library {
            name: "org.lwjgl:lwjgl:3.3.2".to_string(),
            ..Default::default()
        };

        assert_eq!(
            native_jar_relative(&lib, "natives-windows").as_deref(),
            Some("org/lwjgl/lwjgl/3.3.2/lwjgl-3.3.2-natives-windows.jar")
        );
    }

    #[test]
    fn native_jar_relative_accepts_arch_resolved_classifier() {
        // 老版本（1.12 时代）的 classifier 是 natives-windows-${arch}，
        // resolve_natives_pattern 换成 64 之后传进来，文件名要保留 -64
        let lib = Library {
            name: "org.lwjgl:lwjgl:3.2.2".to_string(),
            ..Default::default()
        };

        assert_eq!(
            native_jar_relative(&lib, "natives-windows-64").as_deref(),
            Some("org/lwjgl/lwjgl/3.2.2/lwjgl-3.2.2-natives-windows-64.jar")
        );
    }

    #[test]
    fn native_jar_relative_rejects_incomplete_coordinate() {
        // 缺版本号时返回 None，不能拼出半个路径
        let lib = Library {
            name: "org.lwjgl:lwjgl".to_string(),
            ..Default::default()
        };
        assert!(native_jar_relative(&lib, "natives-windows").is_none());
    }
}
