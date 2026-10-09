use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const MAX_CONFIG_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ProxyPorts {
    pub port: u16,
    pub socks_port: u16,
    pub mixed_port: u16,
    pub redir_port: u16,
    pub tproxy_port: u16,
}

impl ProxyPorts {
    pub fn validate(&self) -> Result<(), String> {
        let mut ports = std::collections::HashSet::new();
        for port in [
            self.port,
            self.socks_port,
            self.mixed_port,
            self.redir_port,
            self.tproxy_port,
        ] {
            if port > 0 && !ports.insert(port) {
                return Err("已启用的代理端口不能重复；填 0 可关闭对应监听。".into());
            }
        }
        Ok(())
    }

    pub fn system_ports(&self) -> (Option<u16>, Option<u16>) {
        let mixed = (self.mixed_port > 0).then_some(self.mixed_port);
        (
            mixed.or((self.port > 0).then_some(self.port)),
            mixed.or((self.socks_port > 0).then_some(self.socks_port)),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoreBooleanSetting {
    AllowLan,
    Ipv6,
    TcpConcurrent,
}

impl CoreBooleanSetting {
    pub fn key(self) -> &'static str {
        match self {
            Self::AllowLan => "allow-lan",
            Self::Ipv6 => "ipv6",
            Self::TcpConcurrent => "tcp-concurrent",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CorePreferences {
    pub allow_lan: bool,
    pub ipv6: Option<bool>,
    pub tcp_concurrent: Option<bool>,
    pub tun_enabled: bool,
    pub tun_stack: Option<String>,
    pub mode: Option<String>,
    pub log_level: Option<String>,
}

impl CorePreferences {
    pub fn set(&mut self, setting: CoreBooleanSetting, value: bool) {
        match setting {
            CoreBooleanSetting::AllowLan => self.allow_lan = value,
            CoreBooleanSetting::Ipv6 => self.ipv6 = Some(value),
            CoreBooleanSetting::TcpConcurrent => self.tcp_concurrent = Some(value),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub core_path: Option<PathBuf>,
    pub config_name: Option<String>,
    pub mixed_port: u16,
    pub controller_port: u16,
    pub http_port: u16,
    pub socks_port: u16,
    pub redir_port: u16,
    pub tproxy_port: u16,
    pub system_proxy_exceptions: Vec<String>,
    pub profiles: Vec<Profile>,
    pub active_profile_id: Option<String>,
    pub core_preferences: CorePreferences,
    pub remote_machines: Vec<crate::remote::RemoteMachine>,
    pub active_remote_id: Option<String>,
    pub auto_start_core: bool,
    pub ssid_enabled: bool,
    pub ssid_rules: Vec<crate::ssid::SsidRule>,
    pub subscriptions: Vec<crate::subscriptions::RemoteSubscription>,
    pub status_bar_style: String,
    pub profile_sources: std::collections::HashMap<String, String>,
    pub ui_language: String,
    pub desired_system_proxy: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            core_path: None,
            config_name: None,
            mixed_port: 7890,
            controller_port: 19090,
            http_port: 0,
            socks_port: 0,
            redir_port: 0,
            tproxy_port: 0,
            system_proxy_exceptions: crate::system_proxy::default_exceptions(),
            profiles: Vec::new(),
            active_profile_id: None,
            core_preferences: CorePreferences::default(),
            remote_machines: Vec::new(),
            active_remote_id: None,
            auto_start_core: false,
            ssid_enabled: false,
            ssid_rules: Vec::new(),
            subscriptions: Vec::new(),
            status_bar_style: "iconAndSpeed".into(),
            profile_sources: std::collections::HashMap::new(),
            ui_language: "zh-CN".into(),
            desired_system_proxy: false,
        }
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("settings.json");
        let settings: Self = match fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|_| "保存的设置格式无效。".to_owned())?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(format!("Cannot open saved settings: {e}")),
        };
        settings.validate_local_ports()?;
        crate::system_proxy::bypass_list(&settings.system_proxy_exceptions)?;
        crate::subscriptions::validate_subscriptions(&settings.subscriptions, &settings.profiles)?;
        if let Some(stack) = &settings.core_preferences.tun_stack {
            crate::controller::normalize_tun_stack(stack)?;
        }
        if settings.remote_machines.len() > 64 {
            return Err("最多保存 64 台远程机器。".into());
        }
        let mut ids = std::collections::HashSet::new();
        for machine in &settings.remote_machines {
            machine.validate()?;
            if !ids.insert(&machine.id) {
                return Err("远程机器标识重复。".into());
            }
        }
        if settings
            .active_remote_id
            .as_ref()
            .is_some_and(|id| !ids.contains(id))
        {
            return Err("选中的远程机器不存在。".into());
        }
        if settings.profiles.len() > 128
            || settings.profiles.iter().any(|profile| {
                profile.id.len() != 64 || !profile.id.bytes().all(|b| b.is_ascii_hexdigit())
            })
            || settings
                .active_profile_id
                .as_ref()
                .is_some_and(|id| !settings.profiles.iter().any(|profile| &profile.id == id))
        {
            return Err("Saved profile metadata is invalid.".into());
        }
        Ok(settings)
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        atomic_write(&dir.join("settings.json"), &bytes)
    }

    pub fn proxy_ports(&self) -> ProxyPorts {
        ProxyPorts {
            port: self.http_port,
            socks_port: self.socks_port,
            mixed_port: self.mixed_port,
            redir_port: self.redir_port,
            tproxy_port: self.tproxy_port,
        }
    }

    pub fn set_proxy_ports(&mut self, ports: &ProxyPorts) {
        self.http_port = ports.port;
        self.socks_port = ports.socks_port;
        self.mixed_port = ports.mixed_port;
        self.redir_port = ports.redir_port;
        self.tproxy_port = ports.tproxy_port;
    }

    pub fn validate_local_ports(&self) -> Result<(), String> {
        let ports = self.proxy_ports();
        ports.validate()?;
        if self.controller_port < 1024 {
            return Err("控制端口必须在 1024 至 65535 之间。".into());
        }
        if [
            ports.port,
            ports.socks_port,
            ports.mixed_port,
            ports.redir_port,
            ports.tproxy_port,
        ]
        .contains(&self.controller_port)
        {
            return Err("代理端口不能与控制端口重复。".into());
        }
        Ok(())
    }
}

pub fn validate_ports(mixed: u16, controller: u16) -> Result<(), String> {
    if mixed < 1024 || controller < 1024 {
        return Err("Ports must be between 1024 and 65535.".into());
    }
    if mixed == controller {
        return Err("Proxy and controller ports must be different.".into());
    }
    Ok(())
}

pub fn secret() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

fn key(name: &str) -> Value {
    Value::String(name.to_owned())
}
fn put(map: &mut Mapping, name: &str, value: impl Into<Value>) {
    map.insert(key(name), value.into());
}

pub fn parse(bytes: &[u8]) -> Result<Mapping, String> {
    if bytes.is_empty() || bytes.len() > MAX_CONFIG_BYTES {
        return Err("The profile must be between 1 byte and 8 MiB.".into());
    }
    let mut value: Value =
        serde_yaml::from_slice(bytes).map_err(|e| format!("Invalid YAML profile: {e}"))?;
    value
        .apply_merge()
        .map_err(|e| format!("Invalid YAML merge keys: {e}"))?;
    let map = value
        .as_mapping()
        .cloned()
        .ok_or("The profile must be a YAML object.")?;
    if !["proxies", "proxy-providers", "proxy-groups", "rules"]
        .iter()
        .any(|name| map.contains_key(key(name)))
    {
        return Err("This is not a mihomo profile: expected proxies, proxy-providers, proxy-groups, or rules.".into());
    }
    Ok(map)
}

/// Imported YAML is preserved separately. Only this derived configuration is executed.
pub fn runtime(bytes: &[u8], settings: &Settings, token: &str) -> Result<Vec<u8>, String> {
    settings.validate_local_ports()?;
    let mut map = parse(bytes)?;
    for name in [
        "external-controller-tls",
        "external-controller-unix",
        "external-controller-pipe",
    ] {
        map.remove(key(name));
    }
    for (name, port) in [
        ("port", settings.http_port),
        ("socks-port", settings.socks_port),
        ("mixed-port", settings.mixed_port),
        ("redir-port", settings.redir_port),
        ("tproxy-port", settings.tproxy_port),
    ] {
        put(&mut map, name, port as u64);
    }
    put(&mut map, "allow-lan", settings.core_preferences.allow_lan);
    put(
        &mut map,
        "bind-address",
        if settings.core_preferences.allow_lan {
            "*"
        } else {
            "127.0.0.1"
        },
    );
    for (name, value) in [
        ("ipv6", settings.core_preferences.ipv6),
        ("tcp-concurrent", settings.core_preferences.tcp_concurrent),
    ] {
        if let Some(value) = value {
            put(&mut map, name, value);
        }
    }
    put(
        &mut map,
        "external-controller",
        format!("127.0.0.1:{}", settings.controller_port),
    );
    put(&mut map, "secret", token);
    for (key_name, value) in [
        ("mode", &settings.core_preferences.mode),
        ("log-level", &settings.core_preferences.log_level),
    ] {
        if let Some(value) = value {
            put(&mut map, key_name, value.as_str());
        }
    }
    let mut tun = map
        .get(key("tun"))
        .and_then(Value::as_mapping)
        .cloned()
        .unwrap_or_default();
    put(&mut tun, "enable", settings.core_preferences.tun_enabled);
    if let Some(stack) = &settings.core_preferences.tun_stack {
        put(
            &mut tun,
            "stack",
            crate::controller::normalize_tun_stack(stack)?,
        );
    }
    if settings.core_preferences.tun_enabled {
        tun.entry(key("stack"))
            .or_insert(Value::String("mixed".into()));
        tun.entry(key("auto-route")).or_insert(Value::Bool(true));
        tun.entry(key("auto-detect-interface"))
            .or_insert(Value::Bool(true));
        tun.entry(key("dns-hijack"))
            .or_insert(Value::Sequence(vec![Value::String("any:53".into())]));
    }
    map.insert(key("tun"), Value::Mapping(tun));
    let mut cors = Mapping::new();
    cors.insert(
        key("allow-origins"),
        Value::Sequence(vec![Value::String("https://metacubex.github.io".into())]),
    );
    put(&mut cors, "allow-private-network", true);
    map.insert(key("external-controller-cors"), Value::Mapping(cors));
    // Provider caches must not write to paths supplied by an untrusted subscription.
    let cache_id = profile_id(bytes);
    if map.contains_key(key("external-ui")) || map.contains_key(key("external-ui-url")) {
        put(&mut map, "external-ui", format!("./ui/{cache_id}"));
        if let Some(name) = map.get(key("external-ui-name")).and_then(Value::as_str) {
            if name.contains(['/', '\\'])
                || name == "."
                || name == ".."
                || name.chars().any(char::is_control)
            {
                return Err("Web UI 名称不能包含目录分隔符或控制字符。".into());
            }
        }
    }
    for kind in ["proxy-providers", "rule-providers"] {
        if let Some(Value::Mapping(providers)) = map.get_mut(key(kind)) {
            for (index, (_, provider)) in providers.iter_mut().enumerate() {
                if let Some(provider) = provider.as_mapping_mut() {
                    put(
                        provider,
                        "path",
                        format!("./providers/{cache_id}/{kind}/{index}.yaml"),
                    );
                }
            }
        }
    }
    serde_yaml::to_string(&Value::Mapping(map))
        .map(String::into_bytes)
        .map_err(|e| e.to_string())
}

pub fn profile_id(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension("tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // Both paths remain alive and NUL terminated until MoveFileExW returns.
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(format!(
                "Cannot commit saved file: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    #[cfg(not(windows))]
    fs::rename(temporary, path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overrides_remote_control_and_inbound_options() {
        let input = b"proxies: []\nexternal-controller: 0.0.0.0:9999\nexternal-controller-pipe: danger\nsecret: public\nallow-lan: true\ntun: {enable: true}\nlisteners: [{port: 8080}]\ndns: {enable: true, listen: '0.0.0.0:53'}\n";
        let output = runtime(input, &Settings::default(), "private").unwrap();
        let map = parse(&output).unwrap();
        assert_eq!(
            map[key("external-controller")].as_str(),
            Some("127.0.0.1:19090")
        );
        assert_eq!(map[key("secret")].as_str(), Some("private"));
        assert_eq!(map[key("allow-lan")].as_bool(), Some(false));
        assert_eq!(map[key("tun")]["enable"].as_bool(), Some(false));
        assert_eq!(map[key("dns")]["enable"].as_bool(), Some(true));
        assert_eq!(map[key("dns")]["listen"].as_str(), Some("0.0.0.0:53"));
        assert!(map.contains_key(key("listeners")));
        assert!(!map.contains_key(key("external-controller-pipe")));
    }
    #[test]
    fn preferences_override_only_explicit_settings_and_keep_controller_local() {
        let input = b"proxies: []\nipv6: true\ntcp-concurrent: true\n";
        let mut settings = Settings::default();
        let inherited = parse(&runtime(input, &settings, "private").unwrap()).unwrap();
        assert_eq!(inherited[key("ipv6")].as_bool(), Some(true));
        settings
            .core_preferences
            .set(CoreBooleanSetting::Ipv6, false);
        settings
            .core_preferences
            .set(CoreBooleanSetting::AllowLan, true);
        let map = parse(&runtime(input, &settings, "private").unwrap()).unwrap();
        assert_eq!(map[key("ipv6")].as_bool(), Some(false));
        assert_eq!(map[key("tcp-concurrent")].as_bool(), Some(true));
        assert_eq!(map[key("bind-address")].as_str(), Some("*"));
        assert_eq!(
            map[key("external-controller")].as_str(),
            Some("127.0.0.1:19090")
        );
        assert_eq!(map[key("secret")].as_str(), Some("private"));
        let legacy: Settings = serde_json::from_str("{}").unwrap();
        assert!(!legacy.core_preferences.allow_lan);
        assert_eq!(legacy.core_preferences.ipv6, None);
    }

    #[test]
    fn provider_cache_cannot_escape_directory() {
        let output = runtime(b"proxy-providers:\n  p:\n    type: http\n    path: ../../outside\n    url: https://example.com/p\n", &Settings::default(), "token").unwrap();
        let map = parse(&output).unwrap();
        assert!(map[key("proxy-providers")]["p"]["path"]
            .as_str()
            .unwrap()
            .ends_with("/proxy-providers/0.yaml"));
        assert!(runtime(
            b"proxy-providers: {p: {type: file, path: secret}}",
            &Settings::default(),
            "token"
        )
        .is_ok());
    }
    #[test]
    fn resolves_anchored_provider_and_group_fields() {
        let input = b"base: &base {type: http, path: ../../old, url: https://example.com/p}\nproxy-providers: {p: {<<: *base}}\nselect: &select {type: select, use: [p]}\nproxy-groups: [{<<: *select, name: auto}]\n";
        let output = runtime(input, &Settings::default(), "secret").unwrap();
        let map = parse(&output).unwrap();
        assert_eq!(
            map[key("proxy-providers")]["p"]["type"].as_str(),
            Some("http")
        );
        assert_eq!(map[key("proxy-groups")][0]["use"][0].as_str(), Some("p"));
        assert!(runtime(
            b"base: &base {type: file, path: secret}\nproxy-providers: {p: {<<: *base}}",
            &Settings::default(),
            "secret"
        )
        .is_ok());
    }
    #[test]
    fn preserves_inbound_authentication_and_isolates_provider_cache() {
        let output = runtime(
            b"proxies: []\nauthentication: [user:pass]",
            &Settings::default(),
            "secret",
        )
        .unwrap();
        assert_eq!(
            parse(&output).unwrap()[key("authentication")][0].as_str(),
            Some("user:pass")
        );
        assert_ne!(profile_id(b"first profile"), profile_id(b"second profile"));
    }
    #[test]
    fn rejects_invalid_profiles_and_ports() {
        assert!(parse(b"<html>not a profile</html>").is_err());
        assert!(parse(b"name: irrelevant").is_err());
        assert!(validate_ports(7890, 7890).is_err());
        assert!(validate_ports(80, 19090).is_err());
    }
    #[test]
    fn settings_round_trip_and_replace() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        settings.save(dir.path()).unwrap();
        settings.mixed_port = 8888;
        settings.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).unwrap().mixed_port, 8888);
    }
}
