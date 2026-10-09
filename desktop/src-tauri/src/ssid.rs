use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(windows)]
#[path = "ssid/windows.rs"]
mod windows;

pub const LOCATION_SETTINGS_URI: &str = "ms-settings:privacy-location";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SsidStatus {
    #[default]
    NotRequested,
    Available,
    Disconnected,
    PermissionDenied,
    ServiceUnavailable,
    AdapterUnavailable,
    Error,
    Unsupported,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SsidSnapshot {
    pub current_ssid: Option<String>,
    pub status: SsidStatus,
    pub error: Option<String>,
}

impl SsidSnapshot {
    pub fn can_retry_automatically(&self) -> bool {
        !matches!(
            self.status,
            SsidStatus::NotRequested | SsidStatus::PermissionDenied | SsidStatus::Unsupported
        )
    }
}

/// 仅在用户主动启用或刷新 SSID 功能后调用；首次查询可能显示 Windows 定位授权提示。
/// 不扫描附近网络，也不读取 Wi-Fi 配置、密码或其他凭据。
pub fn query_current_ssid() -> SsidSnapshot {
    #[cfg(windows)]
    {
        windows::query()
    }
    #[cfg(not(windows))]
    {
        SsidSnapshot {
            status: SsidStatus::Unsupported,
            error: Some("此平台尚未接入 Wi-Fi 网络名称查询。".into()),
            ..SsidSnapshot::default()
        }
    }
}

#[cfg(any(windows, test))]
#[derive(Clone, Debug, Eq, PartialEq)]
enum SsidReadError {
    PermissionDenied,
    ServiceUnavailable,
    System(u32),
    InvalidData,
    InvalidEncoding,
}

#[cfg(any(windows, test))]
impl SsidReadError {
    fn snapshot(self) -> SsidSnapshot {
        let (status, error) = match self {
            Self::PermissionDenied => (
                SsidStatus::PermissionDenied,
                "无法读取当前 Wi-Fi 名称。请在 Windows 定位设置中允许访问后重试。".into(),
            ),
            Self::ServiceUnavailable => (
                SsidStatus::ServiceUnavailable,
                "Windows WLAN 服务未运行或暂时不可用。".into(),
            ),
            Self::System(code) => (
                SsidStatus::Error,
                format!("读取 Wi-Fi 状态失败，Windows 错误码：{code}。"),
            ),
            Self::InvalidData => (SsidStatus::Error, "Windows 返回的 Wi-Fi 数据无效。".into()),
            Self::InvalidEncoding => (
                SsidStatus::Error,
                "当前 Wi-Fi 名称不是有效的 UTF-8 文本，无法绑定到配置。".into(),
            ),
        };
        SsidSnapshot {
            current_ssid: None,
            status,
            error: Some(error),
        }
    }
}

#[cfg(any(windows, test))]
trait SsidSource {
    fn interface_states(&mut self) -> Result<Vec<bool>, SsidReadError>;
    fn connected_ssid(&mut self, index: usize) -> Result<Option<String>, SsidReadError>;
}

#[cfg(any(windows, test))]
fn read_current_ssid(source: &mut impl SsidSource) -> SsidSnapshot {
    let states = match source.interface_states() {
        Ok(states) => states,
        Err(error) => return error.snapshot(),
    };
    if states.is_empty() {
        return SsidSnapshot {
            status: SsidStatus::AdapterUnavailable,
            ..SsidSnapshot::default()
        };
    }
    let mut first_error = None;
    for (index, connected) in states.into_iter().enumerate() {
        if !connected {
            continue;
        }
        match source.connected_ssid(index) {
            Ok(Some(ssid)) if !ssid.trim().is_empty() => {
                return SsidSnapshot {
                    current_ssid: Some(ssid.trim().to_owned()),
                    status: SsidStatus::Available,
                    error: None,
                };
            }
            Ok(_) => {}
            Err(error) => {
                if error == SsidReadError::PermissionDenied || first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    first_error
        .map(SsidReadError::snapshot)
        .unwrap_or(SsidSnapshot {
            status: SsidStatus::Disconnected,
            ..SsidSnapshot::default()
        })
}

#[cfg(any(windows, test))]
fn decode_ssid(bytes: &[u8], length: u32) -> Result<Option<String>, SsidReadError> {
    let length = length as usize;
    if length > 32 || length > bytes.len() {
        return Err(SsidReadError::InvalidData);
    }
    // 使用长度而非零终止符；有损转换会把不同的原始 SSID 合并成同一条规则。
    let ssid = std::str::from_utf8(&bytes[..length])
        .map_err(|_| SsidReadError::InvalidEncoding)?
        .trim();
    Ok((!ssid.is_empty()).then(|| ssid.to_owned()))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SsidRule {
    pub ssid: String,
    pub config_file_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SsidResolution {
    NoAction,
    SwitchToConfig(String),
    MissingConfig(String),
}

pub fn normalize_rules(rules: &[SsidRule]) -> Vec<SsidRule> {
    let mut normalized = Vec::with_capacity(rules.len());
    let mut indexes = HashMap::new();
    for rule in rules {
        let ssid = rule.ssid.trim();
        let config_file_name = rule.config_file_name.trim();
        if ssid.is_empty() || config_file_name.is_empty() {
            continue;
        }
        let rule = SsidRule {
            ssid: ssid.to_owned(),
            config_file_name: config_file_name.to_owned(),
        };
        // 同名 SSID 使用最后一次绑定，列表顺序保留第一次出现的位置。
        if let Some(&index) = indexes.get(ssid) {
            normalized[index] = rule;
        } else {
            indexes.insert(ssid, normalized.len());
            normalized.push(rule);
        }
    }
    normalized
}

pub fn upsert_rule(rules: &[SsidRule], ssid: &str, config_file_name: &str) -> Vec<SsidRule> {
    let mut next = rules.to_vec();
    next.push(SsidRule {
        ssid: ssid.to_owned(),
        config_file_name: config_file_name.to_owned(),
    });
    normalize_rules(&next)
}

pub fn remove_rule(rules: &[SsidRule], ssid: &str) -> Vec<SsidRule> {
    let ssid = ssid.trim();
    normalize_rules(rules)
        .into_iter()
        .filter(|rule| rule.ssid != ssid)
        .collect()
}

pub fn resolve_config(
    current_ssid: Option<&str>,
    current_config_name: Option<&str>,
    rules: &[SsidRule],
    available_config_names: &[String],
) -> SsidResolution {
    let Some(ssid) = current_ssid.map(str::trim).filter(|ssid| !ssid.is_empty()) else {
        return SsidResolution::NoAction;
    };
    let Some(rule) = normalize_rules(rules)
        .into_iter()
        .find(|rule| rule.ssid == ssid)
    else {
        return SsidResolution::NoAction;
    };
    if !available_config_names
        .iter()
        .any(|name| name.trim() == rule.config_file_name)
    {
        return SsidResolution::MissingConfig(rule.config_file_name);
    }
    if current_config_name.map(str::trim) == Some(rule.config_file_name.as_str()) {
        return SsidResolution::NoAction;
    }
    SsidResolution::SwitchToConfig(rule.config_file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSource {
        states: Result<Vec<bool>, SsidReadError>,
        replies: HashMap<usize, Result<Option<String>, SsidReadError>>,
        queried: Vec<usize>,
    }

    impl FakeSource {
        fn new(states: Vec<bool>) -> Self {
            Self {
                states: Ok(states),
                replies: HashMap::new(),
                queried: Vec::new(),
            }
        }
    }

    impl SsidSource for FakeSource {
        fn interface_states(&mut self) -> Result<Vec<bool>, SsidReadError> {
            self.states.clone()
        }

        fn connected_ssid(&mut self, index: usize) -> Result<Option<String>, SsidReadError> {
            self.queried.push(index);
            self.replies
                .remove(&index)
                .expect("测试必须提供每次查询的响应")
        }
    }

    #[test]
    fn default_and_denied_states_require_an_explicit_retry() {
        assert_eq!(SsidSnapshot::default().status, SsidStatus::NotRequested);
        assert!(!SsidSnapshot::default().can_retry_automatically());
        assert!(!SsidReadError::PermissionDenied
            .snapshot()
            .can_retry_automatically());
        assert!(SsidReadError::ServiceUnavailable
            .snapshot()
            .can_retry_automatically());
    }

    #[test]
    fn no_adapters_and_disconnected_adapters_do_not_query_wifi_details() {
        let mut none = FakeSource::new(vec![]);
        assert_eq!(
            read_current_ssid(&mut none).status,
            SsidStatus::AdapterUnavailable
        );
        assert!(none.queried.is_empty());
        let mut disconnected = FakeSource::new(vec![false, false]);
        assert_eq!(
            read_current_ssid(&mut disconnected).status,
            SsidStatus::Disconnected
        );
        assert!(disconnected.queried.is_empty());
    }

    #[test]
    fn connected_interface_returns_real_utf8_name_without_scanning_other_interfaces() {
        let mut source = FakeSource::new(vec![false, true, true]);
        source
            .replies
            .insert(1, Ok(Some("\u{3000}办公室 Wi-Fi ".into())));
        let snapshot = read_current_ssid(&mut source);
        assert_eq!(snapshot.status, SsidStatus::Available);
        assert_eq!(snapshot.current_ssid.as_deref(), Some("办公室 Wi-Fi"));
        assert!(snapshot.error.is_none());
        assert_eq!(source.queried, vec![1]);
    }

    #[test]
    fn another_adapter_can_succeed_after_one_interface_disconnects_or_fails() {
        let mut source = FakeSource::new(vec![true, true, true]);
        source.replies.insert(0, Ok(None));
        source.replies.insert(1, Err(SsidReadError::System(87)));
        source.replies.insert(2, Ok(Some("Home".into())));
        assert_eq!(
            read_current_ssid(&mut source).current_ssid.as_deref(),
            Some("Home")
        );
        assert_eq!(source.queried, vec![0, 1, 2]);
    }

    #[test]
    fn access_denied_is_not_reported_as_disconnected_and_stops_automatic_retries() {
        let mut source = FakeSource::new(vec![true, true]);
        source.replies.insert(0, Err(SsidReadError::System(87)));
        source
            .replies
            .insert(1, Err(SsidReadError::PermissionDenied));
        let snapshot = read_current_ssid(&mut source);
        assert_eq!(snapshot.status, SsidStatus::PermissionDenied);
        assert!(snapshot.current_ssid.is_none());
        assert!(snapshot.error.is_some());
        assert!(!snapshot.can_retry_automatically());
    }

    #[test]
    fn service_failure_is_visible_and_does_not_query_interfaces() {
        let mut source = FakeSource::new(vec![]);
        source.states = Err(SsidReadError::ServiceUnavailable);
        assert_eq!(
            read_current_ssid(&mut source).status,
            SsidStatus::ServiceUnavailable
        );
        assert!(source.queried.is_empty());
    }

    #[test]
    fn blank_or_disappearing_networks_are_disconnected() {
        let mut source = FakeSource::new(vec![true, true]);
        source.replies.insert(0, Ok(Some("\t \n".into())));
        source.replies.insert(1, Ok(None));
        assert_eq!(
            read_current_ssid(&mut source).status,
            SsidStatus::Disconnected
        );
    }

    #[test]
    fn ssid_decoding_respects_reported_length_and_preserves_embedded_zero() {
        assert_eq!(decode_ssid(b"Homeignored", 4).unwrap(), Some("Home".into()));
        assert_eq!(decode_ssid(b"a\0b", 3).unwrap(), Some("a\0b".into()));
        assert_eq!(decode_ssid(&[b'a'; 32], 32).unwrap(), Some("a".repeat(32)));
        assert_eq!(decode_ssid(&[], 0).unwrap(), None);
        let name = "办公室".as_bytes();
        assert_eq!(
            decode_ssid(name, name.len() as u32).unwrap(),
            Some("办公室".into())
        );
    }

    #[test]
    fn invalid_ssid_bytes_cannot_collapse_to_a_different_network_rule() {
        assert_eq!(decode_ssid(&[0xff], 1), Err(SsidReadError::InvalidEncoding));
        assert_eq!(
            decode_ssid(&[b'a'; 33], 33),
            Err(SsidReadError::InvalidData)
        );
        assert_eq!(decode_ssid(b"Home", 5), Err(SsidReadError::InvalidData));
    }

    fn rule(ssid: &str, config: &str) -> SsidRule {
        SsidRule {
            ssid: ssid.into(),
            config_file_name: config.into(),
        }
    }

    #[test]
    fn normalization_trims_deduplicates_and_retains_first_position() {
        let rules = vec![
            rule(" Office ", " old.yaml "),
            rule("Home", "home.yaml"),
            rule("Office", "new.yaml"),
            rule("\u{3000}", "unused.yaml"),
            rule("Guest", " \n"),
        ];
        assert_eq!(
            normalize_rules(&rules),
            vec![rule("Office", "new.yaml"), rule("Home", "home.yaml")]
        );
    }

    #[test]
    fn ssid_names_are_case_sensitive_and_preserve_interior_spaces() {
        let rules = vec![
            rule("Office", "upper.yaml"),
            rule("office", "lower.yaml"),
            rule("My  Wi-Fi", "spaced.yaml"),
        ];
        assert_eq!(normalize_rules(&rules), rules);
        assert_eq!(
            resolve_config(Some("My Wi-Fi"), None, &rules, &["spaced.yaml".into()]),
            SsidResolution::NoAction
        );
    }

    #[test]
    fn upsert_replaces_only_the_matching_ssid_and_ignores_empty_drafts() {
        let rules = vec![rule(" Office ", "old.yaml"), rule("Home", "home.yaml")];
        let updated = upsert_rule(&rules, "Office", " new.yaml ");
        assert_eq!(
            updated,
            vec![rule("Office", "new.yaml"), rule("Home", "home.yaml")]
        );
        assert_eq!(upsert_rule(&updated, "", "ignored.yaml"), updated);
        assert_eq!(upsert_rule(&updated, "Office", " \t"), updated);
    }

    #[test]
    fn remove_normalizes_existing_rules_and_matches_case_sensitively() {
        let rules = vec![
            rule(" Office ", "office.yaml"),
            rule("office", "other.yaml"),
        ];
        assert_eq!(
            remove_rule(&rules, " Office "),
            vec![rule("office", "other.yaml")]
        );
        assert_eq!(remove_rule(&rules, " \t"), normalize_rules(&rules));
    }

    #[test]
    fn disconnected_unknown_and_current_networks_do_not_switch() {
        let rules = vec![rule("Home", "home.yaml")];
        let available = vec!["home.yaml".into()];
        for ssid in [None, Some(" \n"), Some("Unknown"), Some("home")] {
            assert_eq!(
                resolve_config(ssid, None, &rules, &available),
                SsidResolution::NoAction
            );
        }
        assert_eq!(
            resolve_config(Some(" Home "), Some(" home.yaml "), &rules, &available),
            SsidResolution::NoAction
        );
    }

    #[test]
    fn matched_rule_switches_to_existing_trimmed_configuration() {
        assert_eq!(
            resolve_config(
                Some("\u{3000}Home\u{3000}"),
                Some("office.yaml"),
                &[rule("Home", " home.yaml ")],
                &[" home.yaml\n".into()],
            ),
            SsidResolution::SwitchToConfig("home.yaml".into())
        );
    }

    #[test]
    fn missing_config_is_reported_even_when_it_is_the_current_name() {
        let rules = vec![rule("Home", "missing.yaml")];
        assert_eq!(
            resolve_config(Some("Home"), Some("missing.yaml"), &rules, &[]),
            SsidResolution::MissingConfig("missing.yaml".into())
        );
    }

    #[test]
    fn resolution_uses_latest_binding_for_duplicate_ssid() {
        assert_eq!(
            resolve_config(
                Some("Home"),
                None,
                &[rule("Home", "old.yaml"), rule("Home", "new.yaml")],
                &["old.yaml".into(), "new.yaml".into()],
            ),
            SsidResolution::SwitchToConfig("new.yaml".into())
        );
    }

    #[test]
    fn persisted_rules_round_trip_with_swift_camel_case_keys() {
        let encoded = r#"[{"ssid":"Home","configFileName":"home.yaml"}]"#;
        let rules: Vec<SsidRule> = serde_json::from_str(encoded).unwrap();
        assert_eq!(rules, vec![rule("Home", "home.yaml")]);
        assert_eq!(serde_json::to_string(&rules).unwrap(), encoded);
    }
}
