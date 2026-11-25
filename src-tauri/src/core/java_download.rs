//! Java 运行时自动下载（Adoptium Temurin JRE）。
//!
//! 版本要求：1.16.5 及以下要 Java 8，1.17 要 16，1.18+ 要 17，1.20.5+ 要 21。
//! 老版本用新 Java 跑会出各种怪问题（反射被模块系统挡），所以必须按版本给对应的。

use std::io::Read;
use std::path::{Path, PathBuf};

use futures::StreamExt;
use tauri::{AppHandle, Emitter};

use crate::core::java::{self, JavaInfo};
use crate::core::paths;

const ADOPTIUM_URL: &str =
    "https://api.adoptium.net/v3/binary/latest/{major}/ga/windows/x64/jre/hotspot/normal/eclipse";

fn emit(app: &AppHandle, stage: &str, done: usize, total: usize) {
    let _ = app.emit(
        "download-progress",
        serde_json::json!({ "stage": stage, "done": done, "total": total }),
    );
}

/// 确保本地有指定 major 的 Java，没有就下载
pub async fn ensure_java(app: &AppHandle, major: u32) -> Result<JavaInfo, String> {
    if let Some(existing) = java::find_exact(major) {
        return Ok(existing);
    }

    let url = ADOPTIUM_URL.replace("{major}", &major.to_string());
    let runtime_root = paths::launcher_dir().join("runtime");
    std::fs::create_dir_all(&runtime_root).map_err(|e| format!("创建运行时目录失败: {e}"))?;

    let stage = format!("下载 Java {major}");
    emit(app, &stage, 0, 0);

    let client = reqwest::Client::builder()
        .user_agent("MCLauncher/0.1")
        .timeout(std::time::Duration::from_secs(900))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("请求 Adoptium 失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "下载 Java {major} 失败：Adoptium 返回 HTTP {}",
            resp.status()
        ));
    }

    let total_bytes = resp.content_length().unwrap_or(0);
    let total_mb = if total_bytes > 0 {
        (total_bytes / 1_048_576) as usize
    } else {
        0
    };

    let mut stream = resp.bytes_stream();
    let mut buffer: Vec<u8> = Vec::with_capacity(total_bytes as usize);
    let mut downloaded: u64 = 0;
    let mut last_emitted_mb = 0u64;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断: {e}"))?;
        buffer.extend_from_slice(&chunk);
        downloaded += chunk.len() as u64;

        let current_mb = downloaded / 1_048_576;
        if current_mb > last_emitted_mb {
            last_emitted_mb = current_mb;
            emit(app, &stage, current_mb as usize, total_mb);
        }
    }

    emit(app, &stage, total_mb, total_mb);

    let zip_path = runtime_root.join(format!("java-{major}.zip"));
    std::fs::write(&zip_path, &buffer).map_err(|e| format!("写入 Java 压缩包失败: {e}"))?;
    drop(buffer);

    emit(app, &format!("解压 Java {major}"), 0, 0);

    let target = runtime_root.join(format!("java-{major}"));
    std::fs::create_dir_all(&target).map_err(|e| format!("创建解压目录失败: {e}"))?;
    extract_zip(&zip_path, &target)?;
    let _ = std::fs::remove_file(&zip_path);

    let java_exe = find_java_exe(&target)?;
    let info = java::probe(&java_exe)
        .ok_or_else(|| "下载的 Java 跑不起来（可能不是有效的 JRE）".to_string())?;

    // 刚装好新 Java，立刻作废扫描缓存：
    // 否则 30 秒内的下一次 find_all 还拿旧结果，看不到它，会再下载一遍。
    java::invalidate_java_cache();

    Ok(info)
}

fn extract_zip(zip_path: &Path, target: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("打开压缩包失败: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("读取压缩包失败: {e}"))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取压缩包条目失败: {e}"))?;
        let name = entry.name().to_string();
        if name.ends_with('/') {
            continue;
        }

        // 防 zip slip
        let out_path = target.join(&name);
        if !out_path.starts_with(target) {
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("建目录失败: {e}"))?;
        }

        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| format!("解压 {name} 失败: {e}"))?;
        std::fs::write(&out_path, &buf).map_err(|e| format!("写入 {out_path:?} 失败: {e}"))?;
    }

    Ok(())
}

fn find_java_exe(root: &Path) -> Result<PathBuf, String> {
    let direct = root.join("bin").join("java.exe");
    if direct.exists() {
        return Ok(direct);
    }

    let Ok(entries) = std::fs::read_dir(root) else {
        return Err("解压后找不到 bin/java.exe".into());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let exe = path.join("bin").join("java.exe");
            if exe.exists() {
                return Ok(exe);
            }
        }
    }

    Err("解压后找不到 bin/java.exe".into())
}

/// 按版本要求挑 Java：老版本必须精确匹配，17+ 允许用更高版本
pub fn select_java(required: u32) -> Option<JavaInfo> {
    if let Some(exact) = java::find_exact(required) {
        return Some(exact);
    }
    if required >= 17 {
        return java::pick_for(required);
    }
    None
}
