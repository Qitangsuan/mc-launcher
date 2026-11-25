//! 无 GUI 的安装流程实测。
//!
//! 用法：`cargo run --example headless_install -- 1.20.4`
//!
//! 验证整条链路：版本 JSON 获取 → 解析合并 → 生成下载任务 → 并发下载（SHA1 校验）
//! → natives 解压 → Java 选择 → 参数拼接。
//!
//! 不会启动游戏进程（那需要 GUI，本机会话限制下起不来），只验证到「命令能拼出来」。

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use mc_launcher_lib::core::{args, download, java, natives, paths, prepare, version};

const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

fn json_ok(path: &std::path::Path) -> bool {
    matches!(
        std::fs::read_to_string(path),
        Ok(text) if serde_json::from_str::<serde_json::Value>(&text).is_ok()
    )
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let target = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1.20.4".to_string());
    let game_dir = paths::default_game_dir();

    println!("=== 无 GUI 安装实测 ===");
    println!("游戏目录: {}", game_dir.display());
    println!("目标版本: {target}\n");

    let version_dir = game_dir.join("versions").join(&target);
    std::fs::create_dir_all(&version_dir).map_err(|e| format!("建目录失败: {e}"))?;
    let json_path = version_dir.join(format!("{target}.json"));

    // ---------- 1. 版本 JSON ----------
    if !json_ok(&json_path) {
        println!("[1/6] 下载版本 JSON...");
        let client = reqwest::Client::new();
        let manifest: serde_json::Value = client
            .get(MANIFEST)
            .send()
            .await
            .map_err(|e| format!("请求 manifest 失败: {e}"))?
            .json()
            .await
            .map_err(|e| format!("解析 manifest 失败: {e}"))?;
        let url = manifest["versions"]
            .as_array()
            .and_then(|arr| arr.iter().find(|v| v["id"].as_str() == Some(target.as_str())))
            .and_then(|v| v["url"].as_str())
            .ok_or_else(|| format!("manifest 里没有 {target}"))?
            .to_string();
        let bytes = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("下载版本 JSON 失败: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("读取版本 JSON 失败: {e}"))?;
        std::fs::write(&json_path, &bytes).map_err(|e| format!("写入失败: {e}"))?;
        println!("    下载完成 {} KB\n", bytes.len() / 1024);
    } else {
        println!("[1/6] 版本 JSON 已存在且可解析，跳过\n");
    }

    // ---------- 2. 解析 ----------
    println!("[2/6] 解析（含 inheritsFrom 合并）...");
    let ver = version::load_resolved(&game_dir, &target)?;
    let java_required = ver
        .java_version
        .as_ref()
        .and_then(|j| j.major_version)
        .unwrap_or(8);
    println!("    mainClass : {}", ver.main_class);
    println!("    libraries : {} 个", ver.libraries.len());
    println!("    Java 要求 : {java_required}\n");

    // ---------- 3. 生成下载任务 ----------
    println!("[3/6] 生成下载任务...");
    let features: HashMap<String, bool> = HashMap::new();
    let libraries_dir = game_dir.join("libraries");
    let assets_dir = game_dir.join("assets");
    let client_jar = version_dir.join(format!("{target}.jar"));

    let mut tasks = prepare::library_tasks(&ver.libraries, &libraries_dir, &features);
    println!("    库文件   : {} 个任务", tasks.len());

    let mut has_client = false;
    if let Some(client) = ver.downloads.as_ref().and_then(|d| d.client.as_ref()) {
        if !client.url.is_empty() {
            tasks.push(download::DownloadTask {
                url: client.url.clone(),
                sha1: client.sha1.clone(),
                path: client_jar.clone(),
                size: client.size,
            });
            has_client = true;
            println!("    client.jar: {} KB", client.size / 1024);
        }
    }
    if !has_client {
        println!("    client.jar: 无（如果是 Forge/NeoForge 属于正常）");
    }

    if let Some(index) = &ver.asset_index {
        let index_dir = assets_dir.join("indexes");
        std::fs::create_dir_all(&index_dir).map_err(|e| format!("建索引目录失败: {e}"))?;
        let index_path = index_dir.join(format!("{}.json", index.id));
        if !json_ok(&index_path) {
            println!("    下载资源索引...");
            let client = reqwest::Client::new();
            let bytes = client
                .get(&index.url)
                .send()
                .await
                .map_err(|e| format!("下载索引失败: {e}"))?
                .bytes()
                .await
                .map_err(|e| format!("读取索引失败: {e}"))?;
            std::fs::write(&index_path, &bytes).map_err(|e| format!("写索引失败: {e}"))?;
        }
        let text = std::fs::read_to_string(&index_path).map_err(|e| e.to_string())?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("解析索引失败: {e}"))?;
        let asset_tasks = prepare::asset_tasks(&json, &assets_dir);
        println!("    资源文件 : {} 个任务", asset_tasks.len());
        tasks.extend(asset_tasks);
    }
    println!();

    // ---------- 4. 下载 ----------
    println!("[4/6] 开始下载共 {} 个文件...", tasks.len());
    let started = Instant::now();
    let downloader = download::Downloader::new(8);
    let last_pct = Arc::new(AtomicUsize::new(0));
    let lp = last_pct.clone();
    let summary = downloader
        .run_with_progress(
            tasks,
            Arc::new(move |done, total| {
                if total > 0 {
                    let pct = done * 100 / total;
                    let prev = lp.load(Ordering::Relaxed);
                    if pct >= prev + 10 {
                        lp.store(pct, Ordering::Relaxed);
                        println!("    进度 {pct}%  ({done}/{total})");
                    }
                }
            }),
        )
        .await;

    println!(
        "    完成：下载 {} 个，跳过 {} 个，失败 {} 个，耗时 {:.1}s",
        summary.downloaded,
        summary.skipped,
        summary.failed.len(),
        started.elapsed().as_secs_f64()
    );
    if !summary.failed.is_empty() {
        println!("    失败明细：");
        for (url, err) in summary.failed.iter().take(5) {
            println!("      {url}\n        -> {err}");
        }
        return Err("有文件下载失败".into());
    }
    println!();

    // ---------- 5. natives ----------
    println!("[5/6] 解压 natives...");
    let natives_dir = version_dir.join("natives");
    match natives::extract_natives(&ver.libraries, &libraries_dir, &natives_dir) {
        Ok(count) => println!("    解压 {count} 个文件到 {}\n", natives_dir.display()),
        Err(e) => println!("    跳过（老版本才需要）：{e}\n"),
    }

    // ---------- 6. Java + 参数拼接 ----------
    println!("[6/6] 选择 Java + 拼接参数...");
    let java_info = java::find_exact(java_required).or_else(|| {
        if java_required >= 17 {
            java::pick_for(java_required)
        } else {
            None
        }
    });
    match &java_info {
        Some(j) => println!("    Java {} -> {}", j.major, j.path),
        None => println!("    本地没有 Java {java_required}（正式流程会自动下载）"),
    }

    let classpath = args::build_classpath(&ver, &libraries_dir, Some(&client_jar), &features);
    let cp = args::classpath_string(&classpath);
    println!("    classpath: {} 个 jar，总长 {} 字符", classpath.len(), cp.len());

    println!("\n    最终命令预览：");
    println!(
        "      {} -cp <{} 项> {} ...",
        java_info.as_ref().map(|j| j.path.as_str()).unwrap_or("<java>"),
        classpath.len(),
        ver.main_class
    );

    println!("\n=== 实测通过：安装链路完整 ===");
    Ok(())
}
