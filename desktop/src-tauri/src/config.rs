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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub core_path: Option<PathBuf>,
    pub config_name: Option<String>,
    pub mixed_port: u16,
    pub controller_port: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            core_path: None,
            config_name: None,
            mixed_port: 7890,
            controller_port: 19090,
        }
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("settings.json");
        let settings: Self = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("Cannot read saved settings: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(format!("Cannot open saved settings: {e}")),
        };
        validate_ports(settings.mixed_port, settings.controller_port)?;
        Ok(settings)
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        atomic_write(&dir.join("settings.json"), &bytes)
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
    validate_ports(settings.mixed_port, settings.controller_port)?;
    let mut map = parse(bytes)?;
    if map.get(key("authentication")).is_some_and(|value| {
        !matches!(value, Value::Null) && value.as_sequence().is_none_or(|values| !values.is_empty())
    }) {
        return Err("Inbound proxy authentication is not supported by the Windows system-proxy flow. Remove the profile's authentication list before importing.".into());
    }
    for name in [
        "external-controller-tls",
        "external-controller-unix",
        "external-controller-pipe",
        "external-ui",
        "external-ui-name",
        "external-ui-url",
        "external-doh-server",
        "listeners",
    ] {
        map.remove(key(name));
    }
    put(&mut map, "mixed-port", settings.mixed_port as u64);
    for name in ["port", "socks-port", "redir-port", "tproxy-port"] {
        put(&mut map, name, 0u64);
    }
    put(&mut map, "allow-lan", false);
    put(&mut map, "bind-address", "127.0.0.1");
    put(
        &mut map,
        "external-controller",
        format!("127.0.0.1:{}", settings.controller_port),
    );
    put(&mut map, "secret", token);
    put(&mut map, "log-level", "info");
    let mut tun = Mapping::new();
    put(&mut tun, "enable", false);
    map.insert(key("tun"), Value::Mapping(tun));
    // Preserve core DNS resolution, but do not expose a DNS listener.
    if let Some(Value::Mapping(dns)) = map.get_mut(key("dns")) {
        dns.remove(key("listen"));
    }
    let mut cors = Mapping::new();
    cors.insert(key("allow-origins"), Value::Sequence(vec![]));
    put(&mut cors, "allow-private-network", false);
    map.insert(key("external-controller-cors"), Value::Mapping(cors));
    // Provider caches must not write to paths supplied by an untrusted subscription.
    let cache_id = profile_id(bytes);
    for kind in ["proxy-providers", "rule-providers"] {
        if let Some(Value::Mapping(providers)) = map.get_mut(key(kind)) {
            for (index, (_, provider)) in providers.iter_mut().enumerate() {
                if let Some(provider) = provider.as_mapping_mut() {
                    if provider.get(key("type")).and_then(Value::as_str) == Some("file") {
                        return Err("File-based providers are not supported in imported profiles. Use inline or HTTPS providers.".into());
                    }
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
        assert!(map[key("dns")]
            .as_mapping()
            .unwrap()
            .get(key("listen"))
            .is_none());
        assert!(!map.contains_key(key("listeners")));
        assert!(!map.contains_key(key("external-controller-pipe")));
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
        .is_err());
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
        .is_err());
    }
    #[test]
    fn rejects_inbound_authentication_and_isolates_provider_cache() {
        assert!(runtime(
            b"proxies: []\nauthentication: [user:pass]",
            &Settings::default(),
            "secret"
        )
        .is_err());
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
