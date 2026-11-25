//! 版本 JSON 的完整解析、inheritsFrom 合并、rules 过滤。
//!
//! 这是启动器最核心也最容易出错的部分：
//! - 装了 Fabric/Forge 后，版本 JSON 靠 inheritsFrom 继承父版本，库和参数都在两边
//! - libraries 和 arguments 都要合并，不是二选一
//! - rules 决定某个库/参数在 Windows 上到底加不加

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VersionJson {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "mainClass", default)]
    pub main_class: String,
    #[serde(default)]
    pub downloads: Option<Downloads>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(rename = "minecraftArguments", default)]
    pub legacy_arguments: Option<String>,
    #[serde(rename = "assetIndex", default)]
    pub asset_index: Option<AssetIndex>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(rename = "javaVersion", default)]
    pub java_version: Option<JavaVersion>,
    #[serde(rename = "inheritsFrom", default)]
    pub inherits_from: Option<String>,
    #[serde(rename = "type", default)]
    pub version_type: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Downloads {
    #[serde(default)]
    pub client: Option<DownloadFile>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DownloadFile {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Library {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibDownloads>,
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Fabric 这类只给 maven 仓库地址、没有 downloads 的库
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LibDownloads {
    #[serde(default)]
    pub artifact: Option<DownloadFile>,
    #[serde(default)]
    pub classifiers: Option<HashMap<String, DownloadFile>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional {
        #[serde(default)]
        rules: Vec<Rule>,
        #[serde(default)]
        value: ArgValue,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

impl Default for ArgValue {
    fn default() -> Self {
        ArgValue::Many(Vec::new())
    }
}

impl ArgValue {
    pub fn as_strings(&self) -> Vec<String> {
        match self {
            ArgValue::One(s) => vec![s.clone()],
            ArgValue::Many(v) => v.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AssetIndex {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct JavaVersion {
    #[serde(rename = "majorVersion", default)]
    pub major_version: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Rule {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: HashMap<String, bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

fn read_version_json(game_dir: &Path, id: &str) -> Result<VersionJson, String> {
    let path = crate::core::paths::version_json_path(game_dir, id);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取版本文件失败 {path:?}: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("解析版本 JSON 失败 {id}: {e}"))
}

/// 读版本 JSON，并沿 inheritsFrom 链向上合并
pub fn load_resolved(game_dir: &Path, id: &str) -> Result<VersionJson, String> {
    let mut merged = read_version_json(game_dir, id)?;
    let mut parent_id = merged.inherits_from.clone();
    let mut depth = 0;

    while let Some(pid) = parent_id {
        if depth > 16 {
            return Err(format!("inheritsFrom 链过深（可能循环）: {id}"));
        }
        let parent = read_version_json(game_dir, &pid)?;
        parent_id = parent.inherits_from.clone();
        merge_parent(&mut merged, &parent);
        depth += 1;
    }

    Ok(merged)
}

fn merge_parent(child: &mut VersionJson, parent: &VersionJson) {
    if child.main_class.is_empty() {
        child.main_class = parent.main_class.clone();
    }
    if child.version_type.is_empty() {
        child.version_type = parent.version_type.clone();
    }
    if child.asset_index.is_none() {
        child.asset_index = parent.asset_index.clone();
    }
    if child.assets.is_none() {
        child.assets = parent.assets.clone();
    }
    if child.java_version.is_none() {
        child.java_version = parent.java_version.clone();
    }
    if child.downloads.is_none() {
        child.downloads = parent.downloads.clone();
    }
    if child.legacy_arguments.is_none() {
        child.legacy_arguments = parent.legacy_arguments.clone();
    }

    // 库合并：同一坐标只保留子版本的那份（known 随 push 一起更新，
    // 否则父版本内部若有同名库会重复追加）
    let mut known: HashSet<String> = child.libraries.iter().map(|l| l.name.clone()).collect();
    for lib in &parent.libraries {
        if known.insert(lib.name.clone()) {
            child.libraries.push(lib.clone());
        }
    }

    // 参数合并：父在前、子在后
    match (&mut child.arguments, &parent.arguments) {
        (Some(game), Some(parent_args)) => {
            let mut merged_game = parent_args.game.clone();
            merged_game.extend(game.game.clone());
            game.game = merged_game;

            let mut merged_jvm = parent_args.jvm.clone();
            merged_jvm.extend(game.jvm.clone());
            game.jvm = merged_jvm;
        }
        (None, Some(parent_args)) => {
            child.arguments = Some(parent_args.clone());
        }
        _ => {}
    }
}

/// rules 是否放行。空 rules = 放行；否则顺序扫描，最后一条匹配的决定结果。
pub fn rules_allow(rules: &[Rule], features: &HashMap<String, bool>) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, features) {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

fn rule_matches(rule: &Rule, features: &HashMap<String, bool>) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != "windows" {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            if arch != "x86_64" && arch != "amd64" {
                return false;
            }
        }
        if let Some(version_pat) = &os.version {
            let win_ver = windows_version_string();
            match regex::Regex::new(version_pat) {
                Ok(re) => {
                    if !re.is_match(&win_ver) {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }
    }

    for (key, want) in &rule.features {
        let actual = features.get(key).copied().unwrap_or(false);
        if actual != *want {
            return false;
        }
    }

    true
}

/// 近似 Windows 版本号，主要喂给 os.version 正则
fn windows_version_string() -> String {
    String::from("10.0.19045")
}

/// "org.lwjgl:lwjgl:3.3.2" -> "lwjgl-3.3.2.jar" 所在的相对目录
pub fn library_path(name: &str) -> Option<String> {
    let mut parts = name.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next()?;
    let classifier = parts.next();
    let file = match classifier {
        Some(c) => format!("{artifact}-{version}-{c}.jar"),
        None => format!("{artifact}-{version}.jar"),
    };
    Some(format!(
        "{}/{}/{}/{}",
        group.replace('.', "/"),
        artifact,
        version,
        file
    ))
}

/// Windows 下 natives 分类键里取实际文件名模式（可能含 ${arch}）
pub fn natives_pattern(lib: &Library) -> Option<String> {
    let map = lib.natives.as_ref()?;
    map.get("windows").cloned()
}

/// natives 模式里的 ${arch} 替换为 64
pub fn resolve_natives_pattern(pattern: &str) -> String {
    pattern.replace("${arch}", "64")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- 测试用构造辅助 ----------

    /// 造一条只带 os.name 的规则
    fn os_rule(action: &str, os_name: &str) -> Rule {
        Rule {
            action: action.to_string(),
            os: Some(OsRule {
                name: Some(os_name.to_string()),
                arch: None,
                version: None,
            }),
            features: HashMap::new(),
        }
    }

    /// 造一条只带 features 条件的规则（不限制 os）
    fn feature_rule(action: &str, features: HashMap<String, bool>) -> Rule {
        Rule {
            action: action.to_string(),
            os: None,
            features,
        }
    }

    fn no_features() -> HashMap<String, bool> {
        HashMap::new()
    }

    fn lib_with_url(name: &str, url: Option<&str>) -> Library {
        Library {
            name: name.to_string(),
            url: url.map(|u| u.to_string()),
            ..Default::default()
        }
    }

    fn args_with(game: &[&str], jvm: &[&str]) -> Arguments {
        Arguments {
            game: game
                .iter()
                .map(|s| Argument::Plain((*s).to_string()))
                .collect(),
            jvm: jvm
                .iter()
                .map(|s| Argument::Plain((*s).to_string()))
                .collect(),
        }
    }

    fn plain_texts(args: &[Argument]) -> Vec<String> {
        args.iter()
            .map(|a| match a {
                Argument::Plain(s) => s.clone(),
                Argument::Conditional { .. } => panic!("测试里只放纯字符串参数"),
            })
            .collect()
    }

    // ---------- rules_allow ----------

    #[test]
    fn rules_allow_empty_rules_returns_true() {
        // 没有 rules 字段的库/参数一律放行
        assert!(rules_allow(&[], &no_features()));
    }

    #[test]
    fn rules_allow_last_matching_rule_wins() {
        // allow 在后决定 true
        let allow_last = vec![os_rule("disallow", "windows"), os_rule("allow", "windows")];
        assert!(rules_allow(&allow_last, &no_features()));

        // disallow 在后决定 false
        let disallow_last = vec![os_rule("allow", "windows"), os_rule("disallow", "windows")];
        assert!(!rules_allow(&disallow_last, &no_features()));

        // 三段规则：最后一条匹配的才生效
        let three = vec![
            os_rule("allow", "windows"),
            os_rule("disallow", "windows"),
            os_rule("allow", "windows"),
        ];
        assert!(rules_allow(&three, &no_features()));
    }

    #[test]
    fn rules_allow_non_windows_rule_does_not_match() {
        // os.name 不是 windows 的规则视为不匹配：
        // 只有一条 allow(linux) 时，没有任何规则匹配，默认不放行
        let only_linux = vec![os_rule("allow", "linux")];
        assert!(!rules_allow(&only_linux, &no_features()));

        // disallow(linux) 在 Windows 上也不匹配，不影响 allow(windows) 的结果
        let mixed = vec![os_rule("allow", "windows"), os_rule("disallow", "linux")];
        assert!(rules_allow(&mixed, &no_features()));

        // 只有 disallow(windows) 时，结果是「不允许」
        let only_disallow = vec![os_rule("disallow", "windows")];
        assert!(!rules_allow(&only_disallow, &no_features()));
    }

    #[test]
    fn rules_allow_feature_mismatch_returns_false() {
        // 规则要求 has_custom_resolution = true，但没给这个 feature -> 不匹配 -> 不放行
        let mut want_true = HashMap::new();
        want_true.insert("has_custom_resolution".to_string(), true);
        let rule = feature_rule("allow", want_true.clone());
        assert!(!rules_allow(&[rule.clone()], &no_features()));

        // 给了匹配的 feature -> 放行
        assert!(rules_allow(&[rule], &want_true));

        // 反向：规则要求 false，实际给 true -> 不放行
        let mut want_false = HashMap::new();
        want_false.insert("is_demo_user".to_string(), false);
        let rule = feature_rule("allow", want_false);
        let mut actual_true = HashMap::new();
        actual_true.insert("is_demo_user".to_string(), true);
        assert!(!rules_allow(&[rule], &actual_true));
    }

    // ---------- library_path ----------

    #[test]
    fn library_path_builds_maven_layout() {
        // 普通坐标 -> maven 目录结构
        assert_eq!(
            library_path("org.lwjgl:lwjgl:3.3.2").as_deref(),
            Some("org/lwjgl/lwjgl/3.3.2/lwjgl-3.3.2.jar")
        );
    }

    #[test]
    fn library_path_keeps_classifier_in_file_name() {
        // 第四个字段是 classifier，文件名要带上它
        let path = library_path("net.minecraft:client:1.20.1:extra").expect("应该能解析出路径");
        assert_eq!(
            path,
            "net/minecraft/client/1.20.1/client-1.20.1-extra.jar"
        );
        assert!(path.ends_with("client-1.20.1-extra.jar"));
        assert!(path.contains("-extra"));
    }

    #[test]
    fn library_path_rejects_incomplete_coordinate() {
        // 缺版本号时无法拼路径，返回 None 而不是乱拼
        assert!(library_path("org.lwjgl:lwjgl").is_none());
        assert!(library_path("").is_none());
    }

    // ---------- resolve_natives_pattern ----------

    #[test]
    fn resolve_natives_pattern_replaces_arch() {
        // ${arch} 固定换成 64
        assert_eq!(
            resolve_natives_pattern("natives-windows-${arch}"),
            "natives-windows-64"
        );
        // 没有占位符的原样返回
        assert_eq!(resolve_natives_pattern("natives-windows"), "natives-windows");
    }

    // ---------- merge_parent ----------

    #[test]
    fn merge_parent_inherits_main_class_when_child_empty() {
        // 子版本（Fabric/Forge）的 mainClass 为空时要从父版本继承
        let mut child = VersionJson::default();
        child.id = "fabric-loader-1.20.1".into();
        let mut parent = VersionJson::default();
        parent.main_class = "net.minecraft.client.main.Main".into();

        merge_parent(&mut child, &parent);

        assert_eq!(child.main_class, "net.minecraft.client.main.Main");
    }

    #[test]
    fn merge_parent_keeps_child_main_class_when_present() {
        // 子版本自己有 mainClass 时不能被父版本覆盖
        let mut child = VersionJson::default();
        child.main_class = "net.fabricmc.loader.impl.launch.knot.KnotClient".into();
        let mut parent = VersionJson::default();
        parent.main_class = "net.minecraft.client.main.Main".into();

        merge_parent(&mut child, &parent);

        assert_eq!(
            child.main_class,
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
    }

    #[test]
    fn merge_parent_dedups_libraries_by_name() {
        // 同名库只保留子版本那一份，父版本独有的库追加在后面
        let mut child = VersionJson::default();
        child.libraries = vec![lib_with_url("org.lwjgl:lwjgl:3.3.2", Some("child"))];
        let mut parent = VersionJson::default();
        parent.libraries = vec![
            lib_with_url("org.lwjgl:lwjgl:3.3.2", Some("parent")),
            lib_with_url("com.example:extra:1.0", None),
        ];

        merge_parent(&mut child, &parent);

        assert_eq!(child.libraries.len(), 2);
        let lwjgl = child
            .libraries
            .iter()
            .find(|l| l.name == "org.lwjgl:lwjgl:3.3.2")
            .expect("同名库应该还在");
        assert_eq!(
            lwjgl.url.as_deref(),
            Some("child"),
            "同名库要保留子版本那份，不能被父版本顶掉"
        );
        assert!(child
            .libraries
            .iter()
            .any(|l| l.name == "com.example:extra:1.0"));
    }

    #[test]
    fn merge_parent_puts_parent_arguments_before_child() {
        // 参数合并顺序：父在前、子在后
        let mut child = VersionJson::default();
        child.arguments = Some(args_with(&["--child-flag"], &["-Dchild=1"]));
        let mut parent = VersionJson::default();
        parent.arguments = Some(args_with(&["--parent-flag"], &["-Xmx2G"]));

        merge_parent(&mut child, &parent);

        let merged = child.arguments.as_ref().expect("合并后应该有 arguments");
        assert_eq!(
            plain_texts(&merged.game),
            vec!["--parent-flag", "--child-flag"]
        );
        assert_eq!(plain_texts(&merged.jvm), vec!["-Xmx2G", "-Dchild=1"]);
    }

    #[test]
    fn merge_parent_inherits_arguments_when_child_has_none() {
        // 子版本没有 arguments 字段时整份继承父版本的
        let mut child = VersionJson::default();
        let mut parent = VersionJson::default();
        parent.arguments = Some(args_with(&["--parent-flag"], &[]));

        merge_parent(&mut child, &parent);

        let merged = child.arguments.as_ref().expect("应该继承到父版本参数");
        assert_eq!(plain_texts(&merged.game), vec!["--parent-flag"]);
    }
}
