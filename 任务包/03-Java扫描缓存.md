# 任务：给 Java 扫描加结果缓存，避免重复全盘扫描

## 背景

这是一个 Minecraft 启动器。每次启动游戏都要找合适版本的 Java。当前 `find_all()` 每次调用都会：

1. 扫 `JAVA_HOME`、`PATH` 每一项、`C:\Program Files\Java`、`Eclipse Adoptium`、`.minecraft\runtime` 等目录
2. 对每个候选 `java.exe` **同步跑一次 `java -version` 子进程**（无超时）

而一次启动流程里它会被调用 3 次（`select_java` → `pick_for` → `ensure_java`），等于扫描+探测三遍，启动白等好几秒。

## 现有代码（`src/core/java.rs`）

```rust
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInfo {
    pub path: String,
    pub major: u32,
    pub version: String,
}

pub fn probe(java_exe: &Path) -> Option<JavaInfo> {
    let mut cmd = std::process::Command::new(java_exe);
    cmd.arg("-version");
    no_window(&mut cmd);
    let out = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let line = text.lines().find(|l| l.contains("version \""))?;
    let quoted = line.split('"').nth(1)?;
    let major = if quoted.starts_with("1.") {
        quoted.split('.').nth(1)?.parse::<u32>().ok()?
    } else {
        quoted.split(['.', '-', '_']).next()?.parse::<u32>().ok()?
    };
    Some(JavaInfo {
        path: java_exe.display().to_string(),
        major,
        version: quoted.to_string(),
    })
}

fn collect_java_under(dir: &Path, out: &mut Vec<PathBuf>, depth: usize, max_depth: usize) {
    // 递归找 bin/java.exe，略
}

pub fn find_all() -> Vec<JavaInfo> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(home) = std::env::var("JAVA_HOME") {
        candidates.push(PathBuf::from(home).join("bin").join("java.exe"));
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(';') {
            if dir.trim().is_empty() { continue; }
            candidates.push(PathBuf::from(dir).join("java.exe"));
        }
    }
    for base in [
        r"C:\Program Files\Java",
        r"C:\Program Files\Eclipse Adoptium",
        r"C:\Program Files\Microsoft",
        r"C:\Program Files (x86)\Java",
    ] {
        collect_java_under(Path::new(base), &mut candidates, 0, 3);
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let runtime = PathBuf::from(appdata).join(".minecraft").join("runtime");
        collect_java_under(&runtime, &mut candidates, 0, 5);
    }
    let our_runtime = crate::core::paths::launcher_dir().join("runtime");
    collect_java_under(&our_runtime, &mut candidates, 0, 4);

    let mut seen = std::collections::HashSet::new();
    let mut found = Vec::new();
    for exe in candidates {
        if !exe.exists() { continue; }
        let key = exe.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(key) { continue; }
        if let Some(info) = probe(&exe) {
            found.push(info);
        }
    }
    found.sort_by_key(|j| j.major);
    found.dedup_by(|a, b| a.path == b.path);
    found
}
```

## 需求

1. `find_all()` 加**结果缓存**：第一次扫描的结果存起来，**TTL 30 秒内**直接返回 clone
2. 缓存用 `std::sync::Mutex` + `std::sync::OnceLock`，**不要引入新依赖**
3. 过期后重新扫描（用户可能中途装了新 Java）
4. 提供 `pub fn invalidate_java_cache()`，手动清缓存（给「刷新」按钮用）
5. `find_all` 签名保持不变：`pub fn find_all() -> Vec<JavaInfo>`
6. 如果锁中毒（前一个持有者 panic），要能恢复，不要跟着 panic
7. 缓存命中和重新扫描两种情况都最好能记一条 `log` 或注释说明（没有 log 依赖就只写注释）

## 输出

1. 新增的缓存结构 + `find_all` 完整新实现 + `invalidate_java_cache`
2. 需要新增的 `use` 语句
3. 简短说明：为什么用 30 秒 TTL、为什么锁中毒要恢复

**必须能通过 `cargo check`。** 注意本机是 Windows，路径用 `java.exe`。
