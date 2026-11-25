//! 资源下载：并发下载 + SHA1 校验 + 已存在跳过 + 进度回调。

use futures::stream::{FuturesUnordered, StreamExt};
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// 进度回调：(已完成数, 总数)
pub type ProgressFn = Arc<dyn Fn(usize, usize) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub url: String,
    pub sha1: String,
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Debug, Default)]
pub struct DownloadSummary {
    pub downloaded: usize,
    pub skipped: usize,
    pub failed: Vec<(String, String)>,
}

pub struct Downloader {
    client: reqwest::Client,
    concurrency: usize,
}

impl Downloader {
    pub fn new(concurrency: usize) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("MCLauncher/0.1")
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            concurrency: concurrency.max(1),
        }
    }

    pub async fn run(&self, tasks: Vec<DownloadTask>) -> DownloadSummary {
        self.run_with_progress(tasks, Arc::new(|_, _| {})).await
    }

    pub async fn run_with_progress(
        &self,
        tasks: Vec<DownloadTask>,
        on_progress: ProgressFn,
    ) -> DownloadSummary {
        let total = tasks.len();
        let downloaded = Arc::new(AtomicUsize::new(0));
        let skipped = Arc::new(AtomicUsize::new(0));
        let failed: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));

        let sem = Arc::new(tokio::sync::Semaphore::new(self.concurrency));
        let mut futs = FuturesUnordered::new();

        for task in tasks {
            let client = self.client.clone();
            let sem = sem.clone();
            let downloaded = downloaded.clone();
            let skipped = skipped.clone();
            let failed = failed.clone();
            let on_progress = on_progress.clone();

            futs.push(async move {
                let permit = sem.acquire_owned().await;
                match permit {
                    Ok(permit) => {
                        match fetch_one(&client, &task).await {
                            Ok(true) => {
                                downloaded.fetch_add(1, Ordering::Relaxed);
                            }
                            Ok(false) => {
                                skipped.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => {
                                failed.lock().unwrap().push((task.url.clone(), e));
                            }
                        }
                        drop(permit);
                    }
                    Err(_) => {
                        failed
                            .lock()
                            .unwrap()
                            .push((task.url.clone(), "并发信号量关闭".into()));
                    }
                }

                let processed = downloaded.load(Ordering::Relaxed) + skipped.load(Ordering::Relaxed);
                on_progress(processed, total);
            });
        }

        while futs.next().await.is_some() {}

        let failed_list = failed.lock().unwrap().clone();
        DownloadSummary {
            downloaded: downloaded.load(Ordering::Relaxed),
            skipped: skipped.load(Ordering::Relaxed),
            failed: failed_list,
        }
    }
}

/// 单次下载尝试的失败：区分「值得重试」与「重试也没用」。
enum FetchError {
    /// 瞬时性故障：连接失败、超时、传输中断、HTTP 5xx / 429
    Retryable(String),
    /// 确定性故障：HTTP 4xx（429 除外）、SHA1 不匹配、本地目录/写入失败等
    Fatal(String),
}

/// reqwest 错误分类：URL/请求构造错误、重定向策略错误重试也不会恢复；
/// 其余（连接、超时、响应体读取中断等）都视为网络抖动，可重试。
fn reqwest_err_retryable(e: &reqwest::Error) -> bool {
    !(e.is_builder() || e.is_request() || e.is_redirect())
}

async fn fetch_one(client: &reqwest::Client, task: &DownloadTask) -> Result<bool, String> {
    if task.path.exists() {
        if task.sha1.is_empty() {
            // 没有 sha1 的任务（Fabric 那些纯 maven 库）只能按「非空文件」判断，
            // 否则每次启动都会把全部 Fabric 库重下一遍
            if std::fs::metadata(&task.path)
                .map(|m| m.len() > 0)
                .unwrap_or(false)
            {
                return Ok(false);
            }
        } else if let Some(actual) = sha1_file(&task.path) {
            if actual.eq_ignore_ascii_case(&task.sha1) {
                return Ok(false);
            }
        }
    }

    if let Some(parent) = task.path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建目录失败 {parent:?}: {e}"))?;
    }

    const MAX_RETRIES: u32 = 3;
    let mut retries: u32 = 0;
    loop {
        match fetch_attempt(client, task).await {
            Ok(()) => return Ok(true),
            Err(FetchError::Fatal(msg)) => return Err(msg),
            Err(FetchError::Retryable(msg)) => {
                if retries >= MAX_RETRIES {
                    return Err(format!("重试 {MAX_RETRIES} 次后放弃，最后一次错误: {msg}"));
                }
                // 指数退避：500ms、1s、2s
                tokio::time::sleep(std::time::Duration::from_millis(500u64 << retries)).await;
                retries += 1;
            }
        }
    }
}

/// 单次下载尝试（不含重试逻辑），错误已按可重试性分类。
async fn fetch_attempt(client: &reqwest::Client, task: &DownloadTask) -> Result<(), FetchError> {
    let resp = client.get(&task.url).send().await.map_err(|e| {
        let msg = format!("请求失败: {e}");
        if reqwest_err_retryable(&e) {
            FetchError::Retryable(msg)
        } else {
            FetchError::Fatal(msg)
        }
    })?;

    let status = resp.status();
    if !status.is_success() {
        let msg = format!("HTTP {status}");
        return Err(
            if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                FetchError::Retryable(msg)
            } else {
                FetchError::Fatal(msg)
            },
        );
    }

    let bytes = resp.bytes().await.map_err(|e| {
        let msg = format!("读取响应失败: {e}");
        if reqwest_err_retryable(&e) {
            FetchError::Retryable(msg)
        } else {
            FetchError::Fatal(msg)
        }
    })?;

    if !task.sha1.is_empty() {
        let actual = sha1_bytes(&bytes);
        if !actual.eq_ignore_ascii_case(&task.sha1) {
            return Err(FetchError::Fatal(format!(
                "SHA1 不匹配（期望 {}，实际 {}）",
                task.sha1, actual
            )));
        }
    }

    tokio::fs::write(&task.path, &bytes)
        .await
        .map_err(|e| FetchError::Fatal(format!("写入失败 {:?}: {e}", task.path)))?;

    Ok(())
}

pub fn sha1_bytes(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    hex(&h.finalize())
}

pub fn sha1_file(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(sha1_bytes(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
