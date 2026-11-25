use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::core::args;
use crate::core::download::{DownloadSummary, DownloadTask, Downloader};
use crate::core::forge;
use crate::core::java_download;
use crate::core::manifest;
use crate::core::natives;
use crate::core::paths;
use crate::core::prepare;
use crate::core::version;

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub stage: String,
    pub done: usize,
    pub total: usize,
}

fn emit_progress(app: &AppHandle, stage: &str, done: usize, total: usize) {
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            stage: stage.to_string(),
            done,
            total,
        },
    );
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub game_dir: String,
    pub versions: Vec<manifest::VersionInfo>,
}

#[tauri::command]
pub async fn scan_versions() -> Result<ScanResult, String> {
    let dir = paths::default_game_dir();
    let versions = manifest::scan_installed(&dir);
    Ok(ScanResult {
        game_dir: dir.display().to_string(),
        versions,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub release_time: String,
}

#[tauri::command]
pub async fn list_remote_versions() -> Result<Vec<RemoteVersion>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(MANIFEST_URL)
        .send()
        .await
        .map_err(|e| format!("获取版本清单失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("获取版本清单失败: HTTP {}", resp.status()));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析版本清单失败: {e}"))?;

    let mut out = Vec::new();
    if let Some(list) = json.get("versions").and_then(|v| v.as_array()) {
        for item in list {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            out.push(RemoteVersion {
                id: id.to_string(),
                version_type: item
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("release")
                    .to_string(),
                release_time: item
                    .get("releaseTime")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }
    Ok(out)
}

async fn download_with_progress(
    app: &AppHandle,
    downloader: &Downloader,
    tasks: Vec<DownloadTask>,
    stage: &'static str,
) -> DownloadSummary {
    if tasks.is_empty() {
        return DownloadSummary::default();
    }
    emit_progress(app, stage, 0, tasks.len());

    let app_handle = app.clone();
    let last_emit = Arc::new(AtomicUsize::new(0));
    downloader
        .run_with_progress(
            tasks,
            Arc::new(move |done, total| {
                let prev = last_emit.load(Ordering::Relaxed);
                if done == total || done.saturating_sub(prev) >= 20 {
                    last_emit.store(done, Ordering::Relaxed);
                    emit_progress(&app_handle, stage, done, total);
                }
            }),
        )
        .await
}

async fn ensure_files(
    app: &AppHandle,
    game_dir: &Path,
    version_id: &str,
) -> Result<(version::VersionJson, DownloadSummary, DownloadSummary), String> {
    let version = version::load_resolved(game_dir, version_id)?;
    let features: HashMap<String, bool> = HashMap::new();

    let libraries_dir = game_dir.join("libraries");
    let assets_dir = game_dir.join("assets");
    let version_dir = game_dir.join("versions").join(version_id);
    let client_jar = version_dir.join(format!("{version_id}.jar"));

    let mut tasks = prepare::library_tasks(&version.libraries, &libraries_dir, &features);

    // Forge/NeoForge 不需要原版 client.jar（patched jar 已在 libraries 里），
    // 下载了反而会在 classpath 上造成 module 冲突
    let is_forge_like = version_id.to_ascii_lowercase().contains("forge");
    if !is_forge_like {
        if let Some(client) = version.downloads.as_ref().and_then(|d| d.client.as_ref()) {
            if !client.url.is_empty() {
                tasks.push(DownloadTask {
                    url: client.url.clone(),
                    sha1: client.sha1.clone(),
                    path: client_jar.clone(),
                    size: client.size,
                });
            }
        }
    }

    let downloader = Downloader::new(8);
    let lib_summary = download_with_progress(app, &downloader, tasks, "下载库文件").await;

    if !lib_summary.failed.is_empty() {
        let detail = lib_summary
            .failed
            .iter()
            .take(3)
            .map(|(url, err)| format!("{url} -> {err}"))
            .collect::<Vec<_>>()
            .join(" | ");
        return Err(format!(
            "{} 个文件下载失败：{}",
            lib_summary.failed.len(),
            detail
        ));
    }

    let mut asset_summary = DownloadSummary::default();
    if let Some(index) = &version.asset_index {
        let index_dir = assets_dir.join("indexes");
        tokio::fs::create_dir_all(&index_dir)
            .await
            .map_err(|e| format!("创建索引目录失败: {e}"))?;
        let index_path = index_dir.join(format!("{}.json", index.id));

        let need = match crate::core::download::sha1_file(&index_path) {
            Some(existing) => !existing.eq_ignore_ascii_case(&index.sha1),
            None => true,
        };

        if need {
            emit_progress(app, "下载资源索引", 0, 1);
            let client = reqwest::Client::new();
            let resp = client
                .get(&index.url)
                .send()
                .await
                .map_err(|e| format!("下载资源索引失败: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("下载资源索引失败: HTTP {}", resp.status()));
            }
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| format!("读取资源索引失败: {e}"))?;
            tokio::fs::write(&index_path, &bytes)
                .await
                .map_err(|e| format!("写入资源索引失败: {e}"))?;
            emit_progress(app, "下载资源索引", 1, 1);
        }

        let text = tokio::fs::read_to_string(&index_path)
            .await
            .map_err(|e| format!("读取资源索引失败: {e}"))?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("解析资源索引失败: {e}"))?;
        let asset_tasks = prepare::asset_tasks(&json, &assets_dir);
        asset_summary = download_with_progress(app, &downloader, asset_tasks, "下载资源文件").await;

        // 库下载失败会直接报错，资源下载失败以前是静默跳过的，
        // 结果就是「安装完成」但进游戏没声音没材质
        if !asset_summary.failed.is_empty() {
            let detail = asset_summary
                .failed
                .iter()
                .take(3)
                .map(|(url, err)| format!("{url} -> {err}"))
                .collect::<Vec<_>>()
                .join(" | ");
            return Err(format!(
                "{} 个资源文件下载失败（游戏可能缺声音或材质）：{}",
                asset_summary.failed.len(),
                detail
            ));
        }
    }

    Ok((version, lib_summary, asset_summary))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchReport {
    pub java_path: String,
    pub java_major: u32,
    pub downloaded: usize,
    pub skipped: usize,
    pub natives_extracted: usize,
}

#[tauri::command]
pub async fn install_version(app: AppHandle, version_id: String) -> Result<usize, String> {
    paths::validate_id(&version_id)?;
    let game_dir = paths::default_game_dir();
    let version_dir = game_dir.join("versions").join(&version_id);
    tokio::fs::create_dir_all(&version_dir)
        .await
        .map_err(|e| format!("创建版本目录失败: {e}"))?;

    let json_path = version_dir.join(format!("{version_id}.json"));
    // 不能只看文件在不在：上次中断可能留下半截 JSON，
    // 那种情况之后每次解析都失败，重装也修不好，所以这里必须试解析
    let need_json = match std::fs::read_to_string(&json_path) {
        Ok(text) => serde_json::from_str::<serde_json::Value>(&text).is_err(),
        Err(_) => true,
    };
    if need_json {
        emit_progress(&app, "获取版本信息", 0, 1);
        let client = reqwest::Client::new();
        let manifest: serde_json::Value = client
            .get(MANIFEST_URL)
            .send()
            .await
            .map_err(|e| format!("获取版本清单失败: {e}"))?
            .json()
            .await
            .map_err(|e| format!("解析版本清单失败: {e}"))?;

        let url = manifest
            .get("versions")
            .and_then(|v| v.as_array())
            .and_then(|arr| {
                arr.iter().find(|item| {
                    item.get("id").and_then(|i| i.as_str()) == Some(version_id.as_str())
                })
            })
            .and_then(|item| item.get("url"))
            .and_then(|u| u.as_str())
            .ok_or_else(|| format!("版本清单里找不到 {version_id}"))?
            .to_string();

        let bytes = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("下载版本 JSON 失败: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("读取版本 JSON 失败: {e}"))?;
        tokio::fs::write(&json_path, &bytes)
            .await
            .map_err(|e| format!("写入版本 JSON 失败: {e}"))?;
        emit_progress(&app, "获取版本信息", 1, 1);
    }

    let (_version, lib_summary, asset_summary) =
        ensure_files(&app, &game_dir, &version_id).await?;
    emit_progress(&app, "安装完成", 1, 1);
    Ok(lib_summary.downloaded + asset_summary.downloaded)
}

#[tauri::command]
pub async fn install_fabric(app: AppHandle, game_version: String) -> Result<String, String> {
    paths::validate_id(&game_version)?;
    let game_dir = paths::default_game_dir();

    emit_progress(&app, "获取 Fabric 版本", 0, 1);
    let loaders = crate::core::fabric::list_loaders(&game_version).await?;
    let loader = loaders
        .iter()
        .find(|l| l.stable)
        .or_else(|| loaders.first())
        .ok_or_else(|| format!("{game_version} 没有可用的 Fabric 加载器"))?
        .clone();
    emit_progress(&app, "获取 Fabric 版本", 1, 1);

    push_log(&app, &format!("准备安装 Fabric {}（{game_version}）", loader.version));

    let id = crate::core::fabric::install(&game_dir, &game_version, &loader.version).await?;
    push_log(&app, &format!("Fabric 配置已写好：{id}"));
    Ok(id)
}

fn push_log(app: &AppHandle, message: &str) {
    let _ = app.emit(
        "launcher-log",
        serde_json::json!({ "message": message }),
    );
}

#[tauri::command]
pub async fn list_forge_versions(game_version: String) -> Result<forge::LoaderVersions, String> {
    forge::list_forge_versions(&game_version).await
}

#[tauri::command]
pub async fn list_neoforge_versions(
    game_version: String,
) -> Result<forge::LoaderVersions, String> {
    forge::list_neoforge_versions(&game_version).await
}

#[tauri::command]
pub async fn install_forge(
    app: AppHandle,
    game_version: String,
    loader_version: String,
    neoforge: bool,
) -> Result<String, String> {
    paths::validate_id(&game_version)?;
    paths::validate_id(&loader_version)?;
    let game_dir = paths::default_game_dir();
    forge::install(&app, &game_dir, &game_version, &loader_version, neoforge).await
}

#[tauri::command]
pub async fn launch_game(app: AppHandle, version_id: String) -> Result<LaunchReport, String> {
    paths::validate_id(&version_id)?;
    let game_dir = paths::default_game_dir();
    let (version, lib_summary, asset_summary) =
        ensure_files(&app, &game_dir, &version_id).await?;
    let features: HashMap<String, bool> = HashMap::new();

    let libraries_dir = game_dir.join("libraries");
    let assets_dir = game_dir.join("assets");
    let version_dir = game_dir.join("versions").join(&version_id);
    let client_jar = version_dir.join(format!("{version_id}.jar"));
    let is_forge_like = version_id.to_ascii_lowercase().contains("forge");

    emit_progress(&app, "解压 natives", 0, 1);
    let natives_dir = version_dir.join("natives");
    let natives_extracted =
        natives::extract_natives(&version.libraries, &libraries_dir, &natives_dir)?;
    emit_progress(&app, "解压 natives", 1, 1);

    emit_progress(&app, "检测 Java", 0, 1);
    let required = version
        .java_version
        .as_ref()
        .and_then(|j| j.major_version)
        .unwrap_or(8);

    let java_info = match java_download::select_java(required) {
        Some(found) => found,
        None => {
            // 本地没有匹配的版本（老版本必须精确匹配），自动下一个
            emit_progress(&app, &format!("准备 Java {required}"), 0, 0);
            java_download::ensure_java(&app, required).await?
        }
    };
    emit_progress(&app, "检测 Java", 1, 1);

    let classpath = args::build_classpath(
        &version,
        &libraries_dir,
        if is_forge_like {
            None
        } else {
            Some(client_jar.as_path())
        },
        &features,
    );
    let cp = args::classpath_string(&classpath);

    let mut jvm_args = version
        .arguments
        .as_ref()
        .map(|a| args::collect_args(&a.jvm, &features))
        .unwrap_or_default();
    jvm_args.push("-Djava.library.path=${natives_directory}".into());

    let mut final_jvm = {
        let vars = args::jvm_vars(&natives_dir, &libraries_dir, &cp);
        args::expand(&jvm_args, &vars)
    };
    if !final_jvm.iter().any(|a| a == "-cp" || a == "-classpath") {
        final_jvm.push("-cp".into());
        final_jvm.push(cp.clone());
    }

    let raw_game_args = version
        .arguments
        .as_ref()
        .map(|a| args::collect_args(&a.game, &features))
        .unwrap_or_else(|| args::split_legacy(version.legacy_arguments.as_deref().unwrap_or("")));

    let index_id = version
        .asset_index
        .as_ref()
        .map(|i| i.id.clone())
        .or_else(|| version.assets.clone())
        .unwrap_or_else(|| "legacy".into());

    let account = crate::core::account::current_account();
    let (username, uuid, access_token, user_type) = match &account {
        Some(a) => (
            a.username.clone(),
            a.uuid.clone(),
            a.access_token.clone(),
            a.user_type.clone(),
        ),
        None => (
            "Player".to_string(),
            "00000000000000000000000000000000".to_string(),
            "0".to_string(),
            "legacy".to_string(),
        ),
    };

    // 快照/旧版要把自己的 type 传下去，别一律当 release
    let version_type = if version.version_type.is_empty() {
        "release".to_string()
    } else {
        version.version_type.clone()
    };

    let game_vars = args::game_vars(
        &version_id,
        &game_dir,
        &assets_dir,
        &index_id,
        &username,
        &uuid,
        &access_token,
        &user_type,
        &version_type,
    );
    let game_args = args::expand(&raw_game_args, &game_vars);

    emit_progress(&app, "启动游戏", 0, 1);

    // 强制 UTF-8 输出，否则 Windows 控制台编码会让中文日志变成乱码。
    // Java 8~18 认 sun.stdout.encoding，Java 19+ 认 stdout.encoding，两个都塞。
    if !final_jvm.iter().any(|a| a.starts_with("-Dfile.encoding")) {
        final_jvm.push("-Dfile.encoding=UTF-8".into());
        final_jvm.push("-Dsun.stdout.encoding=UTF-8".into());
        final_jvm.push("-Dsun.stderr.encoding=UTF-8".into());
        final_jvm.push("-Dstdout.encoding=UTF-8".into());
        final_jvm.push("-Dstderr.encoding=UTF-8".into());
    }

    let mut cmd = std::process::Command::new(&java_info.path);
    cmd.args(&final_jvm);
    cmd.arg(&version.main_class);
    cmd.args(&game_args);
    cmd.current_dir(&game_dir);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 不弹黑窗，日志走管道进启动器
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| format!("启动 Java 失败: {e}"))?;
    emit_progress(&app, "启动游戏", 1, 1);

    // 日志同时落盘一份
    let logs_dir = game_dir.join("logs");
    let _ = std::fs::create_dir_all(&logs_dir);
    let log_file = std::fs::File::create(logs_dir.join("launcher-latest.log")).ok();

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    if let Some(stdout) = stdout {
        let app_handle = app.clone();
        let log_file = log_file.as_ref().and_then(|f| f.try_clone().ok());
        std::thread::spawn(move || {
            pump_stream(app_handle, stdout, log_file);
        });
    }
    if let Some(stderr) = stderr {
        let app_handle = app.clone();
        let log_file = log_file.as_ref().and_then(|f| f.try_clone().ok());
        std::thread::spawn(move || {
            pump_stream(app_handle, stderr, log_file);
        });
    }

    // 等游戏退出，把退出码告诉前端
    let app_handle = app.clone();
    let version_name = version_id.clone();
    std::thread::spawn(move || {
        let status = child.wait();
        let code = match status {
            Ok(s) => s.code().unwrap_or(-1),
            Err(_) => -1,
        };
        let _ = app_handle.emit(
            "game-exit",
            serde_json::json!({ "version": version_name, "code": code }),
        );
    });

    Ok(LaunchReport {
        java_path: java_info.path.clone(),
        java_major: java_info.major,
        downloaded: lib_summary.downloaded + asset_summary.downloaded,
        skipped: lib_summary.skipped + asset_summary.skipped,
        natives_extracted,
    })
}

/// 逐行读子进程输出，推给前端并写文件
fn pump_stream<R: std::io::Read + Send + 'static>(
    app: AppHandle,
    stream: R,
    mut file: Option<std::fs::File>,
) {
    use std::io::Write;

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else {
            break;
        };
        if let Some(f) = file.as_mut() {
            let _ = writeln!(f, "{line}");
        }
        let _ = app.emit("game-log", serde_json::json!({ "line": line }));
    }
}
