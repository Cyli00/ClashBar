use crate::{config::Profile, subscription};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

pub const DEFAULT_AUTO_UPDATE_INTERVAL_HOURS: u32 = 6;
pub const MINIMUM_AUTO_UPDATE_INTERVAL_HOURS: u32 = 1;

fn default_interval() -> u32 {
    DEFAULT_AUTO_UPDATE_INTERVAL_HOURS
}
fn default_enabled() -> bool {
    true
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionInput {
    pub url: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_enabled")]
    pub auto_update_enabled: bool,
    #[serde(default = "default_interval")]
    pub auto_update_interval_hours: u32,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Clone)]
pub struct PreparedSubscription {
    pub url: String,
    pub name: String,
    pub auto_update_enabled: bool,
    pub auto_update_interval_hours: u32,
    pub overwrite: bool,
}

impl SubscriptionInput {
    pub fn prepare(self) -> Result<PreparedSubscription, String> {
        let url = subscription::validate_url(&self.url)?;
        let fallback = inferred_name(&url);
        let name = normalized_name(self.name.as_deref().unwrap_or_default(), &fallback)?;
        Ok(PreparedSubscription {
            url: url.to_string(),
            name,
            auto_update_enabled: self.auto_update_enabled,
            auto_update_interval_hours: self
                .auto_update_interval_hours
                .max(MINIMUM_AUTO_UPDATE_INTERVAL_HOURS),
            overwrite: self.overwrite,
        })
    }
}

impl PreparedSubscription {
    pub fn bind(&self, profile_id: String, checked_at: u64) -> Result<RemoteSubscription, String> {
        let subscription = RemoteSubscription {
            profile_id,
            url: self.url.clone(),
            auto_update_enabled: self.auto_update_enabled,
            auto_update_interval_hours: self.auto_update_interval_hours,
            last_checked_at: Some(checked_at),
            last_updated_at: Some(checked_at),
            last_error: None,
        };
        subscription.validate()?;
        Ok(subscription)
    }
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSubscription {
    pub profile_id: String,
    pub url: String,
    #[serde(default)]
    pub auto_update_enabled: bool,
    #[serde(default = "default_interval")]
    pub auto_update_interval_hours: u32,
    #[serde(default)]
    pub last_checked_at: Option<u64>,
    #[serde(default)]
    pub last_updated_at: Option<u64>,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl fmt::Debug for RemoteSubscription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteSubscription")
            .field("profile_id", &self.profile_id)
            .field("auto_update_enabled", &self.auto_update_enabled)
            .field(
                "auto_update_interval_hours",
                &self.auto_update_interval_hours,
            )
            .field("last_checked_at", &self.last_checked_at)
            .field("last_updated_at", &self.last_updated_at)
            .field("has_error", &self.last_error.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionSummary {
    pub profile_id: String,
    pub source_host: String,
    pub auto_update_enabled: bool,
    pub auto_update_interval_hours: u32,
    pub last_checked_at: Option<u64>,
    pub last_updated_at: Option<u64>,
    pub next_update_at: Option<u64>,
    pub last_error: Option<String>,
}

impl RemoteSubscription {
    pub fn validate(&self) -> Result<(), String> {
        if self.profile_id.len() != 64
            || !self.profile_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("订阅关联的配置标识无效。".into());
        }
        if self.auto_update_interval_hours < MINIMUM_AUTO_UPDATE_INTERVAL_HOURS {
            return Err("订阅自动更新间隔至少为 1 小时。".into());
        }
        subscription::validate_url(&self.url).map(|_| ())
    }

    pub fn next_update_at(&self) -> Option<u64> {
        if !self.auto_update_enabled {
            return None;
        }
        self.last_checked_at.map(|checked| {
            checked.saturating_add(u64::from(self.auto_update_interval_hours) * 3_600_000)
        })
    }

    pub fn is_due(&self, now: u64) -> bool {
        self.auto_update_enabled && self.next_update_at().is_none_or(|next| now >= next)
    }

    pub fn mark_success(&mut self, checked_at: u64, changed: bool) {
        self.last_checked_at = Some(checked_at);
        if changed {
            self.last_updated_at = Some(checked_at);
        }
        self.last_error = None;
    }

    pub fn mark_failed(&mut self, checked_at: u64, error: &str) {
        // 失败同样推进检查时间，避免损坏订阅每分钟重复请求。
        self.last_checked_at = Some(checked_at);
        self.last_error = Some(safe_error(error));
    }

    pub fn summary(&self) -> SubscriptionSummary {
        SubscriptionSummary {
            profile_id: self.profile_id.clone(),
            source_host: Url::parse(&self.url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .unwrap_or_default(),
            auto_update_enabled: self.auto_update_enabled,
            auto_update_interval_hours: self.auto_update_interval_hours,
            last_checked_at: self.last_checked_at,
            last_updated_at: self.last_updated_at,
            next_update_at: self.next_update_at(),
            last_error: self.last_error.as_deref().map(safe_error),
        }
    }
}

pub fn validate_subscriptions(
    subscriptions: &[RemoteSubscription],
    profiles: &[Profile],
) -> Result<(), String> {
    if subscriptions.len() > 128 {
        return Err("最多保存 128 个订阅。".into());
    }
    let mut ids = HashSet::new();
    for subscription in subscriptions {
        subscription.validate()?;
        if !ids.insert(&subscription.profile_id) {
            return Err("一个配置只能关联一个订阅来源。".into());
        }
        if !profiles
            .iter()
            .any(|profile| profile.id == subscription.profile_id)
        {
            return Err("订阅关联的配置不存在。".into());
        }
    }
    Ok(())
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub fn safe_error(error: &str) -> String {
    // 下载器只返回固定文本；其他错误可能携带 YAML、URL 或令牌，禁止直接显示或持久化。
    let fixed = [
        "订阅地址过长。",
        "请输入有效的 HTTP 或 HTTPS 订阅地址。",
        "订阅地址不能包含用户名、密码或片段。",
        "订阅地址需要有效的主机名和端口。",
        "订阅 DNS 查询超时。",
        "无法解析订阅主机名。",
        "无法创建订阅下载连接。",
        "无法下载订阅，请检查地址和网络连接。",
        "订阅下载已中断。",
        "订阅超过 8 MiB 大小限制。",
        "订阅重定向次数过多。",
        "订阅服务器返回无效重定向地址。",
        "订阅重定向不能访问本地、私有或保留地址。",
        "订阅重定向不能从 HTTPS 降级为 HTTP。",
        "订阅下载超时。",
        "订阅内容不是有效的 mihomo YAML 配置。",
        "无法保存订阅配置。",
        "订阅更新失败，请检查地址、网络连接和配置内容。",
    ];
    if fixed.contains(&error) {
        return error.to_owned();
    }
    if let Some(status) = error
        .strip_prefix("订阅服务器返回 HTTP ")
        .and_then(|suffix| suffix.strip_suffix('。'))
    {
        if let Ok(status) = status.parse::<u16>() {
            if (100..=599).contains(&status) {
                return format!("订阅服务器返回 HTTP {status}。");
            }
        }
    }
    "订阅更新失败，请检查地址、网络连接和配置内容。".into()
}

pub fn normalized_name(input: &str, fallback: &str) -> Result<String, String> {
    let input = if input.trim().is_empty() {
        fallback
    } else {
        input.trim()
    };
    let name = input.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().any(char::is_control)
        || name.chars().count() > 180
    {
        return Err("配置名称需为 1–180 个字符，且不能包含控制字符。".into());
    }
    let extension = name.rsplit_once('.').map(|(_, extension)| extension);
    let normalized = match extension {
        None | Some("") => format!("{name}.yaml"),
        Some(extension)
            if extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml") =>
        {
            name.to_owned()
        }
        _ => return Err("配置名称需使用 .yaml 或 .yml 扩展名。".into()),
    };
    if normalized.chars().count() > 180 {
        return Err("配置名称含扩展名最多为 180 个字符。".into());
    }
    Ok(normalized)
}

fn inferred_name(url: &Url) -> String {
    let segment = url
        .path()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let name = decode_segment(segment);
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." {
        return "remote-config.yaml".into();
    }
    match name.rsplit_once('.') {
        Some((_, extension))
            if extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml") =>
        {
            name.into()
        }
        Some((stem, _)) if !stem.trim().is_empty() => format!("{}.yaml", stem.trim()),
        Some(_) => "remote-config.yaml".into(),
        None => format!("{name}.yaml"),
    }
}

fn decode_segment(input: &str) -> String {
    let mut bytes = input.bytes();
    let mut decoded = Vec::new();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let first = bytes.next();
            let second = bytes.next();
            if let (Some(first), Some(second)) = (first, second) {
                if let (Some(high), Some(low)) =
                    ((first as char).to_digit(16), (second as char).to_digit(16))
                {
                    decoded.push((high * 16 + low) as u8);
                    continue;
                }
                decoded.extend_from_slice(&[byte, first, second]);
            } else {
                decoded.push(byte);
                if let Some(first) = first {
                    decoded.push(first);
                }
            }
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared() -> PreparedSubscription {
        serde_json::from_value::<SubscriptionInput>(
            serde_json::json!({ "url": "https://example.com/config.yaml?token=private-value" }),
        )
        .unwrap()
        .prepare()
        .unwrap()
    }

    fn subscription() -> RemoteSubscription {
        prepared().bind("a".repeat(64), 1_000).unwrap()
    }

    #[test]
    fn import_defaults_match_old_dialog_and_names() {
        let input = prepared();
        assert!(input.auto_update_enabled);
        assert_eq!(input.auto_update_interval_hours, 6);
        assert_eq!(input.name, "config.yaml");
        for (url, expected) in [
            ("https://example.com/", "remote-config.yaml"),
            ("https://example.com/api/subscribe", "subscribe.yaml"),
            ("https://example.com/api/subscribe/", "subscribe.yaml"),
            (
                "https://example.com/api/file.php?token=private",
                "file.yaml",
            ),
            ("https://example.com/%E4%B8%AD%E6%96%87.YML", "中文.YML"),
        ] {
            assert_eq!(inferred_name(&Url::parse(url).unwrap()), expected);
        }
        assert_eq!(
            normalized_name("  自定义名称  ", "fallback.yaml").unwrap(),
            "自定义名称.yaml"
        );
        assert_eq!(
            normalized_name("folder/config.yml", "fallback.yaml").unwrap(),
            "config.yml"
        );
        assert!(normalized_name("secret.txt", "fallback.yaml").is_err());
        assert!(normalized_name("bad\nname.yaml", "fallback.yaml").is_err());
    }

    #[test]
    fn minimum_interval_and_due_boundary_match_swift() {
        let mut value = subscription();
        assert!(!value.is_due(21_600_999));
        assert!(value.is_due(21_601_000));
        value.auto_update_enabled = false;
        assert_eq!(value.next_update_at(), None);
        assert!(!value.is_due(u64::MAX));
        value.last_checked_at = None;
        value.auto_update_enabled = true;
        assert!(value.is_due(0));
        let mut input = serde_json::from_value::<SubscriptionInput>(
            serde_json::json!({ "url": "https://example.com/a", "autoUpdateIntervalHours": 0 }),
        )
        .unwrap();
        assert_eq!(
            input.clone().prepare().unwrap().auto_update_interval_hours,
            1
        );
        input.auto_update_interval_hours = u32::MAX;
        value.auto_update_interval_hours = input.prepare().unwrap().auto_update_interval_hours;
        value.last_checked_at = Some(u64::MAX - 1);
        assert_eq!(value.next_update_at(), Some(u64::MAX));
    }

    #[test]
    fn unchanged_content_and_failed_requests_preserve_update_time() {
        let mut value = subscription();
        value.mark_success(2_000, false);
        assert_eq!(value.last_updated_at, Some(1_000));
        value.mark_failed(3_000, "订阅服务器返回 HTTP 503。");
        assert_eq!(value.last_checked_at, Some(3_000));
        assert_eq!(value.last_updated_at, Some(1_000));
        assert!(!value.is_due(3_001));
        assert_eq!(
            value.last_error.as_deref(),
            Some("订阅服务器返回 HTTP 503。")
        );
        value.mark_success(4_000, true);
        assert_eq!(value.last_updated_at, Some(4_000));
        assert!(value.last_error.is_none());
    }

    #[test]
    fn persistence_preserves_source_but_summary_and_debug_do_not_expose_tokens() {
        let mut value = subscription();
        let stored = serde_json::to_vec(&value).unwrap();
        let restored: RemoteSubscription = serde_json::from_slice(&stored).unwrap();
        assert_eq!(restored, value);
        value.mark_failed(
            2_000,
            "request https://example.com/private-value?token=private-value failed",
        );
        for text in [
            format!("{value:?}"),
            serde_json::to_string(&value.summary()).unwrap(),
        ] {
            assert!(!text.contains("private-value"));
            assert!(!text.contains("?token"));
        }
        value.last_error = Some("Invalid YAML: token: private-value".into());
        assert!(!serde_json::to_string(&value.summary())
            .unwrap()
            .contains("private-value"));
    }

    #[test]
    fn historical_source_without_schedule_stays_manual() {
        let value: RemoteSubscription = serde_json::from_value(
            serde_json::json!({"profileId": "a".repeat(64), "url": "https://example.com/config"}),
        )
        .unwrap();
        assert!(!value.auto_update_enabled);
        assert_eq!(value.auto_update_interval_hours, 6);
        assert!(!value.is_due(u64::MAX));
    }

    #[test]
    fn metadata_rejects_missing_duplicate_and_malformed_profile_ids() {
        let value = subscription();
        let profiles = [Profile {
            id: value.profile_id.clone(),
            name: "config.yaml".into(),
        }];
        assert!(validate_subscriptions(std::slice::from_ref(&value), &profiles).is_ok());
        assert!(validate_subscriptions(std::slice::from_ref(&value), &[]).is_err());
        assert!(validate_subscriptions(&[value.clone(), value.clone()], &profiles).is_err());
        let mut invalid = value;
        invalid.profile_id = "../settings".into();
        assert!(invalid.validate().is_err());
    }
}
