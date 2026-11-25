//! 微软正版登录：Device Code Flow → Xbox Live → XSTS → Minecraft。
//!
//! 五步任何一步都可能挂，而且 XSTS 的报错码必须翻译成人话，不然用户完全不知道
//! 自己卡在哪（最常见的是「没建 Xbox 档案」）。

use serde::Serialize;

use crate::core::account::Account;

const DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeInfo {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

pub async fn request_device_code(client_id: &str) -> Result<DeviceCodeInfo, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(DEVICE_CODE_URL)
        .form(&[
            ("client_id", client_id),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await
        .map_err(|e| format!("请求设备码失败: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "请求设备码失败 HTTP {status}：{}",
            shorten(&body)
        ));
    }

    let value: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析设备码响应失败: {e}"))?;

    Ok(DeviceCodeInfo {
        device_code: str_field(&value, "device_code"),
        user_code: str_field(&value, "user_code"),
        verification_uri: str_field(&value, "verification_uri"),
        expires_in: value.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(900),
        interval: value.get("interval").and_then(|v| v.as_u64()).unwrap_or(5),
    })
}

struct MsTokens {
    access_token: String,
    refresh_token: String,
}

async fn poll_for_ms_token(
    client: &reqwest::Client,
    client_id: &str,
    device_code: &str,
    mut interval: u64,
    expires_in: u64,
) -> Result<MsTokens, String> {
    let started = std::time::Instant::now();

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        if started.elapsed().as_secs() > expires_in {
            return Err("登录超时：设备码已过期，重新点一次微软登录".into());
        }

        let resp = client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", device_code),
            ])
            .send()
            .await
            .map_err(|e| format!("轮询 token 失败: {e}"))?;

        let value: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("解析 token 响应失败: {e}"))?;

        if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
            match err {
                "authorization_pending" => continue,
                "slow_down" => {
                    interval += 5;
                    continue;
                }
                "expired_token" => return Err("设备码已过期，重新点一次微软登录".into()),
                "authorization_declined" => return Err("你在浏览器里取消了登录".into()),
                "bad_verification_code" => return Err("设备码无效".into()),
                _ => {
                    let desc = value
                        .get("error_description")
                        .and_then(|v| v.as_str())
                        .unwrap_or(err);
                    return Err(format!("微软登录失败：{}", shorten(desc)));
                }
            }
        }

        return Ok(MsTokens {
            access_token: str_field(&value, "access_token"),
            refresh_token: str_field(&value, "refresh_token"),
        });
    }
}

async fn xbl_authenticate(
    client: &reqwest::Client,
    ms_token: &str,
) -> Result<(String, String), String> {
    let body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={ms_token}")
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    });

    let resp = client
        .post(XBL_URL)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Xbox Live 认证请求失败: {e}"))?;

    let value: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 Xbox Live 响应失败: {e}"))?;

    let token = str_field(&value, "Token");
    if token.is_empty() {
        return Err(format!("Xbox Live 认证失败：{}", shorten(&value.to_string())));
    }
    let uhs = value
        .get("DisplayClaims")
        .and_then(|d| d.get("xui"))
        .and_then(|x| x.get(0))
        .and_then(|x| x.get("uhs"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    if uhs.is_empty() {
        return Err("Xbox Live 认证失败：响应里没有 uhs".into());
    }
    Ok((token, uhs))
}

async fn xsts_authorize(
    client: &reqwest::Client,
    xbl_token: &str,
) -> Result<(String, String), String> {
    let body = serde_json::json!({
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [xbl_token]
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT"
    });

    let resp = client
        .post(XSTS_URL)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("XSTS 授权请求失败: {e}"))?;

    let value: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 XSTS 响应失败: {e}"))?;

    if let Some(xerr) = value.get("XErr").and_then(|v| v.as_i64()) {
        let hint = match xerr {
            2148916233 => "这个微软账号还没有 Xbox 档案。先用它登录 xbox.com 创建一次，再回来登录。",
            2148916235 => "这个账号所在的地区被 Xbox Live 限制，无法登录。",
            2148916236 | 2148916237 => "这个账号需要通过成人验证（地区限制）。",
            2148916238 => "这是未成年账号，需要家长先在微软家庭里同意。",
            _ => "XSTS 授权被拒绝。",
        };
        return Err(format!("{hint}（XErr {xerr}）"));
    }

    let token = str_field(&value, "Token");
    if token.is_empty() {
        return Err(format!("XSTS 授权失败：{}", shorten(&value.to_string())));
    }
    let uhs = value
        .get("DisplayClaims")
        .and_then(|d| d.get("xui"))
        .and_then(|x| x.get(0))
        .and_then(|x| x.get("uhs"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    Ok((token, uhs))
}

async fn minecraft_login(
    client: &reqwest::Client,
    uhs: &str,
    xsts_token: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "identityToken": format!("XBL3.0 x={uhs};{xsts_token}")
    });

    let resp = client
        .post(MC_LOGIN_URL)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Minecraft 登录请求失败: {e}"))?;

    let status = resp.status();
    let value: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 Minecraft 登录响应失败: {e}"))?;

    let token = str_field(&value, "access_token");
    if token.is_empty() {
        return Err(format!(
            "Minecraft 登录失败 HTTP {status}：{}",
            shorten(&value.to_string())
        ));
    }
    Ok(token)
}

async fn minecraft_profile(
    client: &reqwest::Client,
    mc_token: &str,
) -> Result<(String, String), String> {
    let resp = client
        .get(MC_PROFILE_URL)
        .bearer_auth(mc_token)
        .send()
        .await
        .map_err(|e| format!("获取 Minecraft 档案失败: {e}"))?;

    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("这个账号没有购买 Minecraft（Java 版）。".into());
    }
    if !status.is_success() {
        return Err(format!("获取 Minecraft 档案失败 HTTP {status}"));
    }

    let value: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析档案响应失败: {e}"))?;

    let id = str_field(&value, "id");
    let name = str_field(&value, "name");
    if id.is_empty() || name.is_empty() {
        return Err("Minecraft 档案缺少 id 或 name".into());
    }

    Ok((dash_uuid(&id), name))
}

/// 走完整个五步，返回可直接存档的账号
pub async fn complete_login(
    client_id: &str,
    device_code: &str,
    interval: u64,
    expires_in: u64,
) -> Result<Account, String> {
    let client = reqwest::Client::new();

    let tokens = poll_for_ms_token(&client, client_id, device_code, interval, expires_in).await?;
    let (xbl_token, _xbl_uhs) = xbl_authenticate(&client, &tokens.access_token).await?;
    let (xsts_token, xsts_uhs) = xsts_authorize(&client, &xbl_token).await?;
    let mc_token = minecraft_login(&client, &xsts_uhs, &xsts_token).await?;
    let (uuid, name) = minecraft_profile(&client, &mc_token).await?;

    Ok(Account {
        username: name,
        uuid,
        access_token: mc_token,
        user_type: "msa".into(),
        kind: "microsoft".into(),
        refresh_token: Some(tokens.refresh_token),
    })
}

/// 已经是 MSA 账号时刷新 access token
pub async fn refresh_microsoft_token(
    client_id: &str,
    refresh_token: &str,
) -> Result<Account, String> {
    let client = reqwest::Client::new();

    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await
        .map_err(|e| format!("刷新 token 失败: {e}"))?;

    if !resp.status().is_success() {
        return Err("刷新登录已失效，请重新登录".into());
    }

    let value: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析刷新响应失败: {e}"))?;

    let ms_token = str_field(&value, "access_token");
    let new_refresh = {
        let r = str_field(&value, "refresh_token");
        if r.is_empty() {
            refresh_token.to_string()
        } else {
            r
        }
    };

    let (xbl_token, _) = xbl_authenticate(&client, &ms_token).await?;
    let (xsts_token, xsts_uhs) = xsts_authorize(&client, &xbl_token).await?;
    let mc_token = minecraft_login(&client, &xsts_uhs, &xsts_token).await?;
    let (uuid, name) = minecraft_profile(&client, &mc_token).await?;

    Ok(Account {
        username: name,
        uuid,
        access_token: mc_token,
        user_type: "msa".into(),
        kind: "microsoft".into(),
        refresh_token: Some(new_refresh),
    })
}

fn str_field(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn shorten(s: &str) -> String {
    if s.chars().count() > 300 {
        s.chars().take(300).collect::<String>() + "…"
    } else {
        s.to_string()
    }
}

/// 32 位无横线 hex -> 标准 UUID
fn dash_uuid(hex: &str) -> String {
    if hex.len() == 32 {
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    } else {
        hex.to_string()
    }
}
