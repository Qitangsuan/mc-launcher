//! 启动参数拼接。
//!
//! 启动器最后要做的事情：把 JVM 参数 + 游戏参数拼成一条 java 命令。
//! 难点在于占位符（${...}）和 rules 过滤，以及新旧两种参数格式。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::core::version::{library_path, rules_allow, Argument, VersionJson};

/// 组装 classpath。顺序：client.jar 在最前，其余按 libraries 顺序。
///
/// `client_jar` 传 None 表示不要原版客户端 jar —— Forge/NeoForge 属于这种情况：
/// 它的 patched jar 已经在 libraries 里了，再塞一个原版 jar 会撞
/// `Module minecraft contains package net.minecraft.server` 模块冲突。
pub fn build_classpath(
    version: &VersionJson,
    libraries_dir: &Path,
    client_jar: Option<&Path>,
    features: &HashMap<String, bool>,
) -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(version.libraries.len() + 1);
    if let Some(jar) = client_jar {
        if jar.exists() {
            out.push(jar.to_path_buf());
        }
    }

    for lib in &version.libraries {
        if !rules_allow(&lib.rules, features) {
            continue;
        }
        let Some(rel) = library_path(&lib.name) else {
            continue;
        };
        let full = libraries_dir.join(rel);
        if full.exists() {
            out.push(full);
        }
    }

    out
}

/// classpath 转成 Windows 用分号连接的字符串
pub fn classpath_string(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(";")
}

/// 按 rules 收集参数（新旧格式都适用）
pub fn collect_args(args: &[Argument], features: &HashMap<String, bool>) -> Vec<String> {
    let mut out = Vec::new();
    for arg in args {
        match arg {
            Argument::Plain(s) => out.push(s.clone()),
            Argument::Conditional { rules, value } => {
                if rules_allow(rules, features) {
                    out.extend(value.as_strings());
                }
            }
        }
    }
    out
}

/// 1.12 及以前的 minecraftArguments 是空格分隔的整串
pub fn split_legacy(s: &str) -> Vec<String> {
    s.split_whitespace().map(|x| x.to_string()).collect()
}

/// 替换 ${xxx} 占位符
pub fn expand(args: &[String], vars: &HashMap<String, String>) -> Vec<String> {
    args.iter()
        .map(|arg| {
            let mut s = arg.clone();
            for (key, value) in vars {
                s = s.replace(&format!("${{{key}}}"), value);
            }
            s
        })
        .collect()
}

/// 生成游戏参数里的标准占位符表
#[allow(clippy::too_many_arguments)]
pub fn game_vars(
    version_id: &str,
    game_dir: &Path,
    assets_dir: &Path,
    asset_index: &str,
    username: &str,
    uuid: &str,
    access_token: &str,
    user_type: &str,
    version_type: &str,
) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("auth_player_name".into(), username.into());
    m.insert("version_name".into(), version_id.into());
    m.insert("game_directory".into(), game_dir.display().to_string());
    m.insert("assets_root".into(), assets_dir.display().to_string());
    m.insert("assets_index_name".into(), asset_index.into());
    m.insert("auth_uuid".into(), uuid.into());
    m.insert("auth_access_token".into(), access_token.into());
    m.insert("user_type".into(), user_type.into());
    m.insert("version_type".into(), version_type.into());
    m.insert("auth_session".into(), format!("token:{access_token}:{uuid}"));
    m.insert("user_properties".into(), "{}".into());
    m.insert("clientid".into(), String::new());
    m.insert("auth_xuid".into(), String::new());
    m.insert("launcher_name".into(), "MCLauncher".into());
    m.insert("launcher_version".into(), "0.1.0".into());
    m
}

/// JVM 参数里需要的路径占位符
pub fn jvm_vars(
    natives_dir: &Path,
    libraries_dir: &Path,
    classpath: &str,
) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("natives_directory".into(), natives_dir.display().to_string());
    m.insert("launcher_name".into(), "MCLauncher".into());
    m.insert("launcher_version".into(), "0.1.0".into());
    m.insert("classpath".into(), classpath.into());
    m.insert("classpath_separator".into(), ";".into());
    m.insert("library_directory".into(), libraries_dir.display().to_string());
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::version::Library;

    /// 每个测试用独立的临时目录，避免并行时互相踩
    fn temp_dir(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("mc_launcher_test_{}_{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建临时目录失败");
        dir
    }

    /// 按 library_path 的规则在 libraries_dir 下落一个真实的（空内容）jar
    fn touch_library(libraries_dir: &Path, name: &str) -> PathBuf {
        let rel = library_path(name).expect("库坐标应该能解析出路径");
        let full = libraries_dir.join(rel);
        std::fs::create_dir_all(full.parent().expect("库文件应该有父目录"))
            .expect("创建库目录失败");
        std::fs::write(&full, b"jar").expect("写库文件失败");
        full
    }

    // ---------- split_legacy ----------

    #[test]
    fn split_legacy_splits_legacy_arguments_by_whitespace() {
        // 1.12 及以前的 minecraftArguments 是一整串空格分隔的参数：
        // "--username a --uuid b" 是两个「参数名 + 值」对，按空白切成 4 段
        let parts = split_legacy("--username a --uuid b");
        assert_eq!(parts.len(), 4);
        assert_eq!(parts, vec!["--username", "a", "--uuid", "b"]);

        // 单个「参数名 + 值」对切成 2 段
        assert_eq!(split_legacy("--username a").len(), 2);

        // 多余空白不会产生空元素
        assert_eq!(split_legacy("  --username   a  "), vec!["--username", "a"]);
        assert!(split_legacy("").is_empty());
    }

    // ---------- expand ----------

    #[test]
    fn expand_replaces_placeholders_with_var_values() {
        let mut vars = HashMap::new();
        vars.insert("bar".to_string(), "42".to_string());
        vars.insert("mc_version".to_string(), "1.20.1".to_string());

        let out = expand(
            &[
                "-Dfoo=${bar}".to_string(),
                "${mc_version}".to_string(),
                "plain".to_string(),
                "-Duntouched=${not_in_vars}".to_string(),
            ],
            &vars,
        );

        assert_eq!(out[0], "-Dfoo=42");
        assert_eq!(out[1], "1.20.1");
        // 没有占位符的参数原样保留
        assert_eq!(out[2], "plain");
        // 变量表里没有的占位符保持原样，不会被清空
        assert_eq!(out[3], "-Duntouched=${not_in_vars}");
    }

    #[test]
    fn expand_replaces_all_occurrences_in_one_arg() {
        let mut vars = HashMap::new();
        vars.insert("a".to_string(), "X".to_string());

        let out = expand(&["${a}-${a}".to_string()], &vars);
        assert_eq!(out[0], "X-X");
    }

    // ---------- classpath_string ----------

    #[test]
    fn classpath_string_joins_with_semicolon_on_windows() {
        let paths = vec![
            PathBuf::from(r"C:\game\versions\1.20.1\1.20.1.jar"),
            PathBuf::from(r"C:\game\libraries\lwjgl-3.3.2.jar"),
        ];
        // Windows 的 classpath 分隔符是分号
        assert_eq!(
            classpath_string(&paths),
            r"C:\game\versions\1.20.1\1.20.1.jar;C:\game\libraries\lwjgl-3.3.2.jar"
        );
        // 空列表给空串
        assert_eq!(classpath_string(&[]), "");
    }

    // ---------- build_classpath ----------

    #[test]
    fn build_classpath_excludes_client_jar_when_none() {
        // 传 None（Forge/NeoForge 的情况）时不能塞原版 client.jar
        let root = temp_dir("build_classpath_none");
        let libraries_dir = root.join("libraries");
        let present = touch_library(&libraries_dir, "com.example:present:1.0");
        // 顺带造一个真实存在的 client.jar，确认它不会因为「存在」就被加进来
        let client_jar = root.join("1.20.1.jar");
        std::fs::write(&client_jar, b"jar").expect("写 client.jar 失败");

        let mut version = VersionJson::default();
        version.libraries = vec![Library {
            name: "com.example:present:1.0".into(),
            ..Default::default()
        }];

        let classpath = build_classpath(&version, &libraries_dir, None, &HashMap::new());

        assert_eq!(classpath.len(), 1);
        assert_eq!(classpath[0], present);
        assert!(!classpath.iter().any(|p| p == &client_jar));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn build_classpath_puts_existing_client_jar_first() {
        // 传了 client.jar 且文件存在：排在最前面
        let root = temp_dir("build_classpath_client");
        let libraries_dir = root.join("libraries");
        let present = touch_library(&libraries_dir, "com.example:present:1.0");
        let client_jar = root.join("1.20.1.jar");
        std::fs::write(&client_jar, b"jar").expect("写 client.jar 失败");

        let mut version = VersionJson::default();
        version.libraries = vec![Library {
            name: "com.example:present:1.0".into(),
            ..Default::default()
        }];

        let classpath = build_classpath(
            &version,
            &libraries_dir,
            Some(&client_jar),
            &HashMap::new(),
        );

        assert_eq!(classpath.len(), 2);
        assert_eq!(classpath[0], client_jar, "client.jar 应该在 classpath 最前");
        assert_eq!(classpath[1], present);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn build_classpath_skips_libraries_whose_file_is_missing() {
        // 只有磁盘上真实存在的库才进 classpath，缺文件的不进
        let root = temp_dir("build_classpath_missing");
        let libraries_dir = root.join("libraries");
        touch_library(&libraries_dir, "com.example:present:1.0");

        let mut version = VersionJson::default();
        version.libraries = vec![
            Library {
                name: "com.example:present:1.0".into(),
                ..Default::default()
            },
            Library {
                name: "com.example:missing:1.0".into(),
                ..Default::default()
            },
        ];

        let classpath = build_classpath(&version, &libraries_dir, None, &HashMap::new());

        assert_eq!(classpath.len(), 1);
        assert!(
            classpath[0].ends_with("com/example/present/1.0/present-1.0.jar"),
            "实际拿到：{:?}",
            classpath[0]
        );

        // client.jar 传了但文件不存在时也不进 classpath
        let ghost = root.join("ghost.jar");
        let classpath = build_classpath(&version, &libraries_dir, Some(&ghost), &HashMap::new());
        assert_eq!(classpath.len(), 1);
        assert!(!classpath.contains(&ghost));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn build_classpath_respects_library_rules() {
        // 被 rules 拒绝的库（比如只允许 linux）不进 classpath
        let root = temp_dir("build_classpath_rules");
        let libraries_dir = root.join("libraries");
        touch_library(&libraries_dir, "com.example:present:1.0");
        touch_library(&libraries_dir, "com.example:linux_only:1.0");

        let mut version = VersionJson::default();
        version.libraries = vec![
            Library {
                name: "com.example:present:1.0".into(),
                ..Default::default()
            },
            Library {
                name: "com.example:linux_only:1.0".into(),
                rules: vec![crate::core::version::Rule {
                    action: "allow".into(),
                    os: Some(crate::core::version::OsRule {
                        name: Some("linux".into()),
                        arch: None,
                        version: None,
                    }),
                    features: HashMap::new(),
                }],
                ..Default::default()
            },
        ];

        let classpath = build_classpath(&version, &libraries_dir, None, &HashMap::new());

        assert_eq!(classpath.len(), 1);
        assert!(classpath[0].ends_with("present-1.0.jar"));

        std::fs::remove_dir_all(&root).ok();
    }
}
