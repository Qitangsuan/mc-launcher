//! 账号管理（支持多账号）。
//!
//! 离线登录的 UUID 必须和 Java 的 `UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes())`
//! 完全一致，否则存档/服务器认不出同一个人。那是「MD5 后手动打版本位」，
//! 不是普通的 UUID v3（uuid crate 的 v3 会额外拼命名空间字节，对不上）。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    /// offline / microsoft
    pub kind: String,
    /// 微软账号才有，用来刷新 access token
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountStore {
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub current: usize,
}

fn account_path() -> PathBuf {
    crate::core::paths::launcher_dir().join("account.json")
}

pub fn load_store() -> AccountStore {
    let Ok(text) = std::fs::read_to_string(account_path()) else {
        return AccountStore::default();
    };

    if let Ok(store) = serde_json::from_str::<AccountStore>(&text) {
        if !store.accounts.is_empty() {
            return store;
        }
    }

    // 兼容旧版本的单账号格式
    if let Ok(single) = serde_json::from_str::<Account>(&text) {
        return AccountStore {
            accounts: vec![single],
            current: 0,
        };
    }

    AccountStore::default()
}

pub fn save_store(store: &AccountStore) -> Result<(), String> {
    let dir = crate::core::paths::launcher_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    let text =
        serde_json::to_string_pretty(store).map_err(|e| format!("序列化账号失败: {e}"))?;
    std::fs::write(account_path(), text).map_err(|e| format!("写入账号失败: {e}"))
}

pub fn current_account() -> Option<Account> {
    let store = load_store();
    store.accounts.get(store.current).cloned()
}

/// 添加或更新账号（同名同 UUID 视为同一个），并切到它
pub fn upsert(account: Account) -> Result<Account, String> {
    let mut store = load_store();
    match store.accounts.iter().position(|a| a.uuid == account.uuid) {
        Some(index) => {
            store.accounts[index] = account.clone();
            store.current = index;
        }
        None => {
            store.accounts.push(account.clone());
            store.current = store.accounts.len() - 1;
        }
    }
    save_store(&store)?;
    Ok(account)
}

pub fn switch(index: usize) -> Result<(), String> {
    let mut store = load_store();
    if index >= store.accounts.len() {
        return Err("账号不存在".into());
    }
    store.current = index;
    save_store(&store)
}

pub fn remove(index: usize) -> Result<(), String> {
    let mut store = load_store();
    if index >= store.accounts.len() {
        return Err("账号不存在".into());
    }
    store.accounts.remove(index);
    // 删掉的是当前账号之前的账号时，索引要跟着前移，
    // 否则「当前账号」会莫名其妙漂移成列表里的下一个
    if index < store.current {
        store.current -= 1;
    }
    if store.accounts.is_empty() {
        store.current = 0;
    } else if store.current >= store.accounts.len() {
        store.current = store.accounts.len() - 1;
    }
    save_store(&store)
}

/// 和 Java 的 nameUUIDFromBytes 逐字节等价
pub fn offline_uuid(username: &str) -> String {
    let mut bytes: [u8; 16] = md5::compute(format!("OfflinePlayer:{username}").as_bytes()).0;

    bytes[6] = (bytes[6] & 0x0f) | 0x30; // version 3
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // IETF variant

    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn validate_username(name: &str) -> Result<(), String> {
    let len = name.chars().count();
    if len == 0 {
        return Err("用户名不能为空".into());
    }
    if len > 16 {
        return Err("用户名最多 16 个字符".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err("用户名只能包含字母、数字和下划线".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- offline_uuid ----------

    #[test]
    fn offline_uuid_matches_java_for_steve() {
        // 参考值用 JDK 的 UUID.nameUUIDFromBytes(("OfflinePlayer:Steve").getBytes()) 验过：
        // 必须是这个 UUID，否则服务器/存档认不出和正版 Java 启动器同一个玩家
        assert_eq!(
            offline_uuid("Steve"),
            "5627dd98-e6be-3c21-b8a8-e92344183641"
        );
    }

    #[test]
    fn offline_uuid_matches_java_for_alex() {
        // 第二个参考值，确认不是碰巧对上 Steve 一个
        assert_eq!(
            offline_uuid("Alex"),
            "36532b5e-c442-3dbb-a24c-c7e55d0f979a"
        );
    }

    #[test]
    fn offline_uuid_differs_per_username() {
        // 不同用户名必须给出不同 UUID
        assert_ne!(offline_uuid("Steve"), offline_uuid("Alex"));
        // 大小写敏感（Java 侧同样区分大小写）
        assert_ne!(offline_uuid("Steve"), offline_uuid("steve"));
        // 结果格式固定为 8-4-4-4-12 且版本位是 3、variant 是 IETF(10xx)
        let uuid = offline_uuid("Steve");
        let groups: Vec<&str> = uuid.split('-').collect();
        assert_eq!(groups.len(), 5);
        assert_eq!(groups[0].len(), 8);
        assert_eq!(groups[1].len(), 4);
        assert_eq!(groups[2].len(), 4);
        assert_eq!(groups[3].len(), 4);
        assert_eq!(groups[4].len(), 12);
        assert!(groups[2].starts_with('3'), "版本位应该是 3：{uuid}");
        assert!(
            groups[3].starts_with('8') || groups[3].starts_with('9') || groups[3].starts_with('a') || groups[3].starts_with('b'),
            "variant 位应该是 8/9/a/b：{uuid}"
        );
    }

    // ---------- validate_username ----------

    #[test]
    fn validate_username_rejects_empty() {
        // 空串必须拒绝
        assert!(validate_username("").is_err());
    }

    #[test]
    fn validate_username_rejects_too_long() {
        // 16 个字符是上限，17 个必须拒绝
        let sixteen = "a".repeat(16);
        assert!(validate_username(&sixteen).is_ok());
        let seventeen = "a".repeat(17);
        assert!(validate_username(&seventeen).is_err());
    }

    #[test]
    fn validate_username_rejects_illegal_characters() {
        // 只允许 ASCII 字母、数字和下划线
        for bad in ["Steve-1", "Steve 1", "Steve!", "史蒂夫", "a.b", "a@b"] {
            assert!(
                validate_username(bad).is_err(),
                "{bad} 不应该通过校验"
            );
        }
    }

    #[test]
    fn validate_username_accepts_normal_names() {
        // 正常用户名通过
        for good in ["Steve", "Alex", "Player_01", "_", "a"] {
            assert!(validate_username(good).is_ok(), "{good} 应该通过校验");
        }
        // 16 个字符刚好是边界值
        assert!(validate_username("0123456789abcdef").is_ok());
    }
}
