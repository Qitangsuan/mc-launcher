//! Java 运行时检测。
//!
//! Windows 上 Java 可能装在：JAVA_HOME、PATH、Program Files 各个厂商目录、
//! 以及官方启动器自己的 .minecraft/runtime 里。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInfo {
    pub path: String,
    pub major: u32,
    pub version: String,
}

#[cfg(windows)]
fn no_window(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_window(_cmd: &mut std::process::Command) {}

/// 跑 java -version（输出在 stderr），解析主版本号
pub fn probe(java_exe: &Path) -> Option<JavaInfo> {
    let mut cmd = std::process::Command::new(java_exe);
    cmd.arg("-version");
    no_window(&mut cmd);
    let out = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr);

    // 不能只取第一行：系统设了 JAVA_TOOL_OPTIONS / _JAVA_OPTIONS 时
    // JVM 会先打印 "Picked up JAVA_TOOL_OPTIONS: ..."，第一行里没有版本号
    let line = text.lines().find(|l| l.contains("version \""))?;
    let quoted = line.split('"').nth(1)?;

    let major = if quoted.starts_with("1.") {
        quoted.split('.').nth(1)?.parse::<u32>().ok()?
    } else {
        quoted
            .split(['.', '-', '_'])
            .next()?
            .parse::<u32>()
            .ok()?
    };

    Some(JavaInfo {
        path: java_exe.display().to_string(),
        major,
        version: quoted.to_string(),
    })
}

fn collect_java_under(dir: &Path, out: &mut Vec<PathBuf>, depth: usize, max_depth: usize) {
    if depth > max_depth || !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let exe = path.join("bin").join("java.exe");
        if exe.exists() {
            out.push(exe);
        }
        collect_java_under(&path, out, depth + 1, max_depth);
    }
}

/// 缓存有效期。一次启动流程（select_java → pick_for → ensure_java）会在几秒内
/// 连调 3 次 find_all，30 秒足够覆盖整个启动过程；同时又不会太久——
/// 用户中途新装的 Java 最多 30 秒后就能被重新扫描发现。
const JAVA_CACHE_TTL: Duration = Duration::from_secs(30);

struct JavaCache {
    scanned_at: Instant,
    javas: Vec<JavaInfo>,
}

/// OnceLock 保证 Mutex 只初始化一次；Option 为 None 表示当前没有缓存。
static JAVA_CACHE: OnceLock<Mutex<Option<JavaCache>>> = OnceLock::new();

fn java_cache() -> &'static Mutex<Option<JavaCache>> {
    JAVA_CACHE.get_or_init(|| Mutex::new(None))
}

/// 真正的全盘扫描 + 逐个探测版本（慢路径，只在缓存未命中/过期时走）。
fn scan_all() -> Vec<JavaInfo> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(home) = std::env::var("JAVA_HOME") {
        candidates.push(PathBuf::from(home).join("bin").join("java.exe"));
    }

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(';') {
            if dir.trim().is_empty() {
                continue;
            }
            candidates.push(PathBuf::from(dir).join("java.exe"));
        }
    }

    for base in [
        r"C:\Program Files\Java",
        r"C:\Program Files\Eclipse Adoptium",
        r"C:\Program Files\Microsoft",
        r"C:\Program Files (x86)\Java",
        r"C:\Program Files\BellSoft",
        r"C:\Program Files\Zulu",
    ] {
        collect_java_under(Path::new(base), &mut candidates, 0, 3);
    }

    // 官方启动器装的运行时
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let runtime = PathBuf::from(appdata).join(".minecraft").join("runtime");
        collect_java_under(&runtime, &mut candidates, 0, 5);
    }

    // 本启动器自己下载的运行时
    let our_runtime = crate::core::paths::launcher_dir().join("runtime");
    collect_java_under(&our_runtime, &mut candidates, 0, 4);

    let mut seen = std::collections::HashSet::new();
    let mut found = Vec::new();
    for exe in candidates {
        if !exe.exists() {
            continue;
        }
        let key = exe.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        if let Some(info) = probe(&exe) {
            found.push(info);
        }
    }

    found.sort_by_key(|j| j.major);
    found.dedup_by(|a, b| a.path == b.path);
    found
}

/// 找出本机所有 Java，带 30 秒结果缓存：TTL 内直接返回上次的扫描结果，
/// 避免一次启动流程里重复全盘扫描 + 跑三遍 `java -version` 子进程。
pub fn find_all() -> Vec<JavaInfo> {
    // 锁中毒恢复：前一个持有者若在扫描中途 panic，缓存顶多是一份旧数据，
    // 用 into_inner 取出来继续用即可（最坏也就是多扫一次），
    // 不能让之后每次找 Java 都跟着 panic。
    let mut guard = java_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(cache) = guard.as_ref() {
        if cache.scanned_at.elapsed() < JAVA_CACHE_TTL {
            // 缓存命中：直接返回 clone，不扫盘、不起子进程。
            return cache.javas.clone();
        }
    }

    // 缓存未命中或已过 TTL：重新扫描（用户可能中途装了新 Java）。
    // 扫描期间故意持锁不放：并发的调用会等这次扫完直接命中新缓存，
    // 避免多个线程同时各扫一遍、各起一堆 java -version。
    let found = scan_all();
    *guard = Some(JavaCache {
        scanned_at: Instant::now(),
        javas: found.clone(),
    });
    found
}

/// 手动清空缓存（给「刷新」按钮 / 刚装完新 Java 时用），下次 find_all 强制重扫。
pub fn invalidate_java_cache() {
    let mut guard = java_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
}

/// 挑一个满足 major 要求的版本。
///
/// 这里**不能**兜底返回 all.last()：all 是升序排的，last 是最高版本，
/// 当本机只有低于 required 的 Java 时它会返回那个低版本，
/// 让 select_java 误以为找到了，从而跳过自动下载，最后用错版本跑游戏崩掉。
pub fn pick_for(required: u32) -> Option<JavaInfo> {
    let all = find_all();
    if all.is_empty() {
        return None;
    }
    all.iter()
        .find(|j| j.major == required)
        .or_else(|| all.iter().find(|j| j.major >= required))
        .cloned()
}

/// 精确匹配某个 major，没有就 None（自动下载时用）
pub fn find_exact(major: u32) -> Option<JavaInfo> {
    find_all().into_iter().find(|j| j.major == major)
}
