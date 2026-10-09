use crate::config::{CoreBooleanSetting, ProxyPorts};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, Method};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    http::header::{HeaderValue, AUTHORIZATION},
    protocol::{Message, WebSocketConfig},
};
use url::Url;

const MAX_RESPONSE: usize = 16 * 1024 * 1024;
const MAX_MEMORY_SAMPLE: usize = 4096;
const MAX_MAINTENANCE_RESPONSE: usize = 64 * 1024;
const DELAY_QUERY: &[(&str, &str)] = &[
    ("timeout", "5000"),
    ("url", "https://www.gstatic.com/generate_204"),
];

#[derive(Clone)]
pub struct Controller {
    client: Client,
    base: Url,
    secret: String,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub proxies: Value,
    pub configs: Value,
    pub rules: Value,
    pub connections: Value,
    pub providers: Value,
    #[serde(rename = "ruleProviders")]
    pub rule_providers: Value,
    /// 内核实际内存用量；旧内核可能不提供此指标。
    pub memory: Option<u64>,
    pub traffic: Option<crate::streams::TrafficSample>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum CoreUpgradeResult {
    Succeeded,
    AlreadyLatest { version: Option<String> },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebUiMetadata {
    pub has_configured_external_ui: bool,
    pub external_ui_name: Option<String>,
}

pub const MIPS_MINIMUM_CORE_VERSION: &str = "1.19.31";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TunSettings {
    pub enable: bool,
    pub stack: Option<String>,
}

impl Controller {
    pub fn new(port: u16, secret: String) -> Result<Self, String> {
        Self::with_endpoint(
            Url::parse(&format!("http://127.0.0.1:{port}/")).map_err(|_| "本机控制器地址无效。")?,
            secret,
        )
    }

    pub fn remote(machine: &crate::remote::RemoteMachine) -> Result<Self, String> {
        machine.validate()?;
        Self::with_endpoint(machine.endpoint()?, machine.secret.clone())
    }

    fn with_endpoint(base: Url, secret: String) -> Result<Self, String> {
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            base,
            secret,
        })
    }

    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("loopback URL is a base")
            .extend(segments);
        url
    }

    async fn request(
        &self,
        method: Method,
        segments: &[&str],
        body: Option<Value>,
        query: &[(&str, &str)],
    ) -> Result<Value, String> {
        let mut request = self
            .client
            .request(method, self.url(segments))
            .timeout(Duration::from_secs(10))
            .query(query);
        if !self.secret.is_empty() {
            request = request.bearer_auth(&self.secret);
        }
        if segments == ["upgrade", "geo"] {
            request = request.timeout(Duration::from_secs(60));
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| "控制器未响应，请检查地址、网络和 TLS 证书。")?;
        if !response.status().is_success() {
            return Err(format!("控制器返回 HTTP {}。", response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE as u64)
        {
            return Err("The core response is too large.".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "控制器响应中断。")? {
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err("The core response is too large.".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|_| "The core returned invalid JSON.".into())
    }

    pub async fn version(&self) -> Result<String, String> {
        let value = self.request(Method::GET, &["version"], None, &[]).await?;
        value
            .get("version")
            .and_then(Value::as_str)
            .filter(|v| v.len() < 200)
            .map(|version| {
                if self.secret.is_empty() {
                    version.to_owned()
                } else {
                    version.replace(&self.secret, "[已隐藏]")
                }
            })
            .ok_or("The executable did not expose a compatible mihomo controller.".into())
    }

    pub async fn web_ui_metadata(&self) -> Result<WebUiMetadata, String> {
        let configs = self.request(Method::GET, &["configs"], None, &[]).await?;
        if !configs.is_object() {
            return Err("内核返回的 WebUI 配置无效。".into());
        }
        let has_configured_external_ui = configs
            .get("external-ui-url")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        let external_ui_name = has_configured_external_ui
            .then(|| configs.get("external-ui-name").and_then(Value::as_str))
            .flatten()
            .map(|name| name.trim().trim_matches('/').trim())
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        if let Some(name) = &external_ui_name {
            check_name(name)?;
        }
        Ok(WebUiMetadata {
            has_configured_external_ui,
            external_ui_name,
        })
    }

    pub fn web_ui_url(&self, metadata: &WebUiMetadata) -> Result<Url, String> {
        if metadata.has_configured_external_ui {
            let mut url = self.base.clone();
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| "WebUI 控制器地址无效。")?;
            segments.clear().push("ui");
            if let Some(name) = &metadata.external_ui_name {
                let name = name.trim().trim_matches('/').trim();
                if !name.is_empty() {
                    check_name(name)?;
                    segments.push(name);
                }
            }
            drop(segments);
            url.set_query(None);
            url.set_fragment(None);
            return Ok(url);
        }
        let host = match self.base.host().ok_or("WebUI 控制器缺少主机名。")? {
            url::Host::Domain(host) => host.to_owned(),
            url::Host::Ipv4(host) => host.to_string(),
            url::Host::Ipv6(host) => host.to_string(),
        };
        let port = self
            .base
            .port_or_known_default()
            .ok_or("WebUI 控制器缺少端口。")?
            .to_string();
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair(
                "http",
                if self.base.scheme() == "https" {
                    "false"
                } else {
                    "true"
                },
            )
            .append_pair("hostname", &host)
            .append_pair("port", &port)
            .append_pair("secret", self.secret.trim())
            .finish();
        let mut url =
            Url::parse("https://metacubex.github.io/metacubexd/").expect("固定 MetaCubeXD 地址");
        // fragment 不随浏览器 HTTP 请求发送；此地址只交给后端浏览器启动器。
        url.set_fragment(Some(&format!("/setup?{query}")));
        Ok(url)
    }

    pub async fn receive_logs(
        &self,
        logs: &crate::process::Logs,
        level: &str,
        on_ready: impl FnOnce(),
    ) -> Result<(), String> {
        if !valid_log_level(level) {
            return Err("日志级别无效。".into());
        }
        let mut request = self
            .client
            .get(self.url(&["logs"]))
            .query(&[("level", level)]);
        if !self.secret.is_empty() {
            request = request.bearer_auth(&self.secret);
        }
        let mut response = tokio::time::timeout(Duration::from_secs(3), request.send())
            .await
            .map_err(|_| "日志流连接超时。")?
            .map_err(|_| "无法连接日志流。")?;
        if !response.status().is_success() {
            return Err(format!("日志流返回 HTTP {}。", response.status().as_u16()));
        }
        on_ready();
        let mut line = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "日志流连接中断。")? {
            for byte in chunk {
                if byte == b'\n' {
                    self.record_stream_log(logs, &line)?;
                    line.clear();
                } else {
                    if line.len() >= 64 * 1024 {
                        return Err("日志流单条消息超过 64 KiB。".into());
                    }
                    line.push(byte);
                }
            }
        }
        if !line.is_empty() {
            self.record_stream_log(logs, &line)?;
        }
        Err("日志流已结束，正在重新连接。".into())
    }

    pub async fn receive_log_events(
        &self,
        logs: &crate::process::Logs,
        level: &str,
        on_ready: impl FnMut(),
    ) -> Result<(), String> {
        if !valid_log_level(level) {
            return Err("日志级别无效。".into());
        }
        self.receive_events("logs", &[("level", level)], 64 * 1024, on_ready, |bytes| {
            self.record_stream_log(logs, bytes)
        })
        .await
    }

    pub(crate) async fn receive_events(
        &self,
        endpoint: &str,
        query: &[(&str, &str)],
        maximum: usize,
        mut on_ready: impl FnMut(),
        mut on_payload: impl FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        if !["traffic", "memory", "connections", "logs"].contains(&endpoint) {
            return Err("监控流类型无效。".into());
        }
        let mut url = self.url(&[endpoint]);
        url.set_scheme(if self.base.scheme() == "https" {
            "wss"
        } else {
            "ws"
        })
        .map_err(|_| "监控流地址无效。")?;
        url.query_pairs_mut().extend_pairs(query.iter().copied());
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| "监控流地址无效。")?;
        if !self.secret.is_empty() {
            let authorization = HeaderValue::from_str(&format!("Bearer {}", self.secret))
                .map_err(|_| "监控流鉴权设置无效。")?;
            request.headers_mut().insert(AUTHORIZATION, authorization);
        }
        let config = WebSocketConfig::default()
            .read_buffer_size(8192)
            .max_message_size(Some(maximum))
            .max_frame_size(Some(maximum));
        let connection = tokio::time::timeout(
            Duration::from_secs(3),
            tokio_tungstenite::connect_async_with_config(request, Some(config), false),
        )
        .await
        .map_err(|_| "监控流连接超时。")?;
        let (mut socket, _) = match connection {
            Ok(connection) => connection,
            Err(tokio_tungstenite::tungstenite::Error::Http(response))
                if [200, 400, 404, 405, 426].contains(&response.status().as_u16()) =>
            {
                // 部分兼容内核只实现换行 JSON；鉴权失败与 TLS 失败不会降级重试。
                return self
                    .receive_http_events(endpoint, query, maximum, on_ready, on_payload)
                    .await;
            }
            Err(_) => return Err("无法连接监控流，请检查地址、网络和 TLS 证书。".into()),
        };
        on_ready();
        loop {
            let incoming = tokio::time::timeout(Duration::from_secs(45), socket.next())
                .await
                .map_err(|_| "监控流长时间未响应，正在重新连接。")?;
            match incoming {
                Some(Ok(Message::Text(text))) => on_payload(text.as_bytes())?,
                Some(Ok(Message::Binary(bytes))) => on_payload(&bytes)?,
                Some(Ok(Message::Ping(bytes))) => socket
                    .send(Message::Pong(bytes))
                    .await
                    .map_err(|_| "监控流连接中断。")?,
                Some(Ok(Message::Pong(_))) | Some(Ok(Message::Frame(_))) => {}
                Some(Ok(Message::Close(_))) | None => {
                    return Err("监控流已结束，正在重新连接。".into())
                }
                Some(Err(_)) => return Err("监控流连接中断或消息超过大小限制。".into()),
            }
        }
    }

    async fn receive_http_events(
        &self,
        endpoint: &str,
        query: &[(&str, &str)],
        maximum: usize,
        mut on_ready: impl FnMut(),
        mut on_payload: impl FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut request = self.client.get(self.url(&[endpoint])).query(query);
        if !self.secret.is_empty() {
            request = request.bearer_auth(&self.secret);
        }
        let mut response = tokio::time::timeout(Duration::from_secs(3), request.send())
            .await
            .map_err(|_| "监控流连接超时。")?
            .map_err(|_| "无法连接监控流。")?;
        if !response.status().is_success() {
            return Err(format!("监控流返回 HTTP {}。", response.status().as_u16()));
        }
        on_ready();
        let mut line = Vec::new();
        loop {
            let chunk = tokio::time::timeout(Duration::from_secs(45), response.chunk())
                .await
                .map_err(|_| "监控流长时间未响应，正在重新连接。")?
                .map_err(|_| "监控流连接中断。")?;
            let Some(chunk) = chunk else {
                break;
            };
            for byte in chunk {
                if byte == b'\n' {
                    if !line.iter().all(u8::is_ascii_whitespace) {
                        on_payload(&line)?;
                    }
                    line.clear();
                } else {
                    if line.len() >= maximum {
                        return Err("监控流消息超过大小限制。".into());
                    }
                    line.push(byte);
                }
            }
        }
        if !line.iter().all(u8::is_ascii_whitespace) {
            on_payload(&line)?;
        }
        Err("监控流已结束，正在重新连接。".into())
    }

    fn record_stream_log(&self, logs: &crate::process::Logs, line: &[u8]) -> Result<(), String> {
        if line.iter().all(u8::is_ascii_whitespace) {
            return Ok(());
        }
        let entry = serde_json::from_slice::<Value>(line).ok();
        let entry = entry.as_ref().map(|entry| {
            entry
                .get("logs")
                .and_then(Value::as_array)
                .and_then(|logs| logs.first())
                .unwrap_or(entry)
        });
        let fallback = String::from_utf8_lossy(line);
        let message = entry
            .and_then(|entry| entry.get("payload"))
            .and_then(Value::as_str)
            .unwrap_or(&fallback)
            .trim();
        if message.is_empty() {
            return Ok(());
        }
        let level = entry
            .and_then(|entry| entry.get("type"))
            .and_then(Value::as_str)
            .filter(|value| valid_log_level(value))
            .unwrap_or("info");
        crate::process::emit_line(logs, format!("{level} {message}").as_bytes(), &self.secret);
        Ok(())
    }

    pub async fn snapshot(&self) -> Result<Snapshot, String> {
        let (proxies, configs, mut rules, mut connections, mut providers, rule_providers) = tokio::try_join!(
            self.request(Method::GET, &["proxies"], None, &[]),
            self.request(Method::GET, &["configs"], None, &[]),
            self.request(Method::GET, &["rules"], None, &[]),
            self.request(Method::GET, &["connections"], None, &[]),
            self.request(Method::GET, &["providers", "proxies"], None, &[]),
            self.rule_providers()
        )?;
        // mihomo 的空 Go slice/map 会编码为 null；它表示空集合，不代表 API 不兼容。
        for (value, name, empty) in [
            (&mut rules, "rules", json!([])),
            (&mut connections, "connections", json!([])),
            (&mut providers, "providers", json!({})),
        ] {
            if value.get(name).is_some_and(Value::is_null) {
                value[name] = empty;
            }
        }
        if !proxies.get("proxies").is_some_and(Value::is_object)
            || !rules.get("rules").is_some_and(Value::is_array)
            || !connections.get("connections").is_some_and(Value::is_array)
            || !providers.get("providers").is_some_and(Value::is_object)
        {
            return Err("The core returned an incompatible dashboard response.".into());
        }
        let mode = configs
            .get("mode")
            .and_then(Value::as_str)
            .ok_or("The core response has no mode.")?;
        let log_level = configs
            .get("log-level")
            .and_then(Value::as_str)
            .filter(|level| valid_log_level(level));
        // Modern mihomo includes memory in /connections. Avoid opening a stream
        // on each poll when the same response already contains the metric.
        let memory = match connections.get("memory").and_then(Value::as_u64) {
            Some(memory) => Some(memory),
            None => self.memory_usage().await,
        };
        // /configs may contain the API secret. Only allow UI configuration fields.
        let mut public_configs = json!({"mode": mode, "log-level": log_level,
            "allow-lan": configs.get("allow-lan").and_then(Value::as_bool),
            "ipv6": configs.get("ipv6").and_then(Value::as_bool),
            "tcp-concurrent": configs.get("tcp-concurrent").and_then(Value::as_bool)
        });
        for key in [
            "port",
            "socks-port",
            "mixed-port",
            "redir-port",
            "tproxy-port",
        ] {
            if let Some(port) = configs
                .get(key)
                .and_then(Value::as_u64)
                .filter(|port| *port <= u16::MAX as u64)
            {
                public_configs[key] = json!(port);
            }
        }
        if let Ok(tun) = tun_settings_from_configs(&configs) {
            public_configs["tun"] = json!(tun);
        }
        Ok(Snapshot {
            proxies,
            configs: public_configs,
            rules,
            connections,
            providers,
            rule_providers,
            memory,
            traffic: None,
        })
    }

    async fn memory_usage(&self) -> Option<u64> {
        // /memory is newline-delimited JSON that normally never reaches EOF.
        // Read one bounded sample and drop the stream; unsupported/slow/malformed
        // telemetry must not break the otherwise usable dashboard snapshot.
        let mut request = self
            .client
            .get(self.url(&["memory"]))
            .timeout(Duration::from_secs(2));
        if !self.secret.is_empty() {
            request = request.bearer_auth(&self.secret);
        }
        let mut response = request.send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            let newline = chunk.iter().position(|byte| *byte == b'\n');
            let sample = &chunk[..newline.unwrap_or(chunk.len())];
            if bytes.len() + sample.len() > MAX_MEMORY_SAMPLE {
                return None;
            }
            bytes.extend_from_slice(sample);
            if newline.is_some() {
                break;
            }
        }
        serde_json::from_slice::<Value>(&bytes)
            .ok()?
            .get("inuse")
            .and_then(Value::as_u64)
    }

    pub async fn set_mode(&self, mode: &str) -> Result<(), String> {
        if !["rule", "global", "direct"].contains(&mode) {
            return Err("Unknown proxy mode.".into());
        }
        self.request(
            Method::PATCH,
            &["configs"],
            Some(json!({"mode": mode})),
            &[],
        )
        .await
        .map(|_| ())
    }
    pub async fn set_log_level(&self, level: &str) -> Result<(), String> {
        if !valid_log_level(level) {
            return Err("Unknown log level.".into());
        }
        self.request(
            Method::PATCH,
            &["configs"],
            Some(json!({"log-level": level})),
            &[],
        )
        .await
        .map(|_| ())
    }

    pub async fn boolean_setting(&self, setting: CoreBooleanSetting) -> Result<bool, String> {
        let configs = self.request(Method::GET, &["configs"], None, &[]).await?;
        configs
            .get(setting.key())
            .and_then(Value::as_bool)
            .ok_or_else(|| format!("当前内核未返回 {} 设置。", setting.key()))
    }

    pub async fn tun_settings(&self) -> Result<TunSettings, String> {
        let configs = self.request(Method::GET, &["configs"], None, &[]).await?;
        tun_settings_from_configs(&configs)
    }

    pub async fn set_tun_settings(
        &self,
        enable: bool,
        stack: Option<&str>,
    ) -> Result<TunSettings, String> {
        let stack = stack.map(normalize_tun_stack).transpose()?;
        if stack.as_deref() == Some("mips") {
            let version = self.version().await?;
            if !supports_mips_stack(&version) {
                return Err(format!(
                    "mips 协议栈需要 mihomo {MIPS_MINIMUM_CORE_VERSION} 或更新版本。"
                ));
            }
        }
        let mut tun = json!({"enable": enable});
        if let Some(stack) = &stack {
            tun["stack"] = json!(stack);
        }
        self.request(Method::PATCH, &["configs"], Some(json!({"tun": tun})), &[])
            .await?;
        let actual = self.tun_settings().await?;
        if actual.enable != enable {
            return Err("TUN 运行状态与请求不一致，请检查内核权限与日志。".into());
        }
        if let Some(stack) = &stack {
            if !actual
                .stack
                .as_ref()
                .is_some_and(|value| value.eq_ignore_ascii_case(stack))
            {
                return Err("内核未应用所选 TUN 协议栈，请检查内核版本与日志。".into());
            }
        }
        Ok(actual)
    }

    pub async fn set_boolean_setting(
        &self,
        setting: CoreBooleanSetting,
        value: bool,
    ) -> Result<(), String> {
        let mut body = json!({setting.key(): value});
        if matches!(setting, CoreBooleanSetting::AllowLan) {
            body["bind-address"] = json!(if value { "*" } else { "127.0.0.1" });
        }
        self.request(Method::PATCH, &["configs"], Some(body), &[])
            .await
            .map(|_| ())
    }

    pub async fn proxy_ports(&self) -> Result<(Option<u16>, Option<u16>), String> {
        Ok(self.proxy_port_settings().await?.system_ports())
    }

    pub async fn proxy_port_settings(&self) -> Result<ProxyPorts, String> {
        let configs = self.request(Method::GET, &["configs"], None, &[]).await?;
        let port = |key| {
            configs
                .get(key)
                .and_then(Value::as_u64)
                .filter(|port| *port <= u16::MAX as u64)
                .map(|port| port as u16)
                .ok_or_else(|| format!("内核未返回有效的 {key} 端口。"))
        };
        Ok(ProxyPorts {
            port: port("port")?,
            socks_port: port("socks-port")?,
            mixed_port: port("mixed-port")?,
            redir_port: port("redir-port")?,
            tproxy_port: port("tproxy-port")?,
        })
    }

    pub async fn set_proxy_ports(&self, ports: &ProxyPorts) -> Result<(), String> {
        ports.validate()?;
        self.request(
            Method::PATCH,
            &["configs"],
            Some(serde_json::to_value(ports).map_err(|_| "代理端口格式无效。")?),
            &[],
        )
        .await
        .map(|_| ())
    }
    pub async fn select_proxy(&self, group: &str, name: &str) -> Result<(), String> {
        check_name(group)?;
        check_name(name)?;
        self.request(
            Method::PUT,
            &["proxies", group],
            Some(json!({"name": name})),
            &[],
        )
        .await
        .map(|_| ())
    }

    pub async fn set_remote_boolean_setting(
        &self,
        setting: CoreBooleanSetting,
        value: bool,
    ) -> Result<(), String> {
        self.request(
            Method::PATCH,
            &["configs"],
            Some(json!({setting.key(): value})),
            &[],
        )
        .await
        .map(|_| ())
    }
    pub async fn test_delay(&self, name: &str) -> Result<Value, String> {
        check_name(name)?;
        let result = self
            .request(Method::GET, &["proxies", name, "delay"], None, DELAY_QUERY)
            .await?;
        let delay = result
            .get("delay")
            .and_then(Value::as_u64)
            .ok_or("The proxy latency test did not return a delay.")?;
        Ok(json!({"delay": delay}))
    }
    /// Test all members; mihomo also clears fixed automatic-group selections.
    pub async fn test_group_delay(&self, name: &str) -> Result<Value, String> {
        check_name(name)?;
        let result = self
            .request(Method::GET, &["group", name, "delay"], None, DELAY_QUERY)
            .await?;
        if !result.as_object().is_some_and(|delays| {
            delays
                .values()
                .all(|delay| delay.as_u64().is_some_and(|delay| delay <= u16::MAX as u64))
        }) {
            return Err("The group latency test returned invalid delays.".into());
        }
        Ok(result)
    }
    pub async fn close_connection(&self, id: &str) -> Result<(), String> {
        check_name(id)?;
        self.request(Method::DELETE, &["connections", id], None, &[])
            .await
            .map(|_| ())
    }
    pub async fn close_all_connections(&self) -> Result<(), String> {
        self.request(Method::DELETE, &["connections"], None, &[])
            .await
            .map(|_| ())
    }
    pub async fn update_provider(&self, name: &str) -> Result<(), String> {
        check_name(name)?;
        self.request(Method::PUT, &["providers", "proxies", name], None, &[])
            .await
            .map(|_| ())
    }

    pub async fn proxy_providers(&self) -> Result<Value, String> {
        let value = self
            .request(Method::GET, &["providers", "proxies"], None, &[])
            .await?;
        if !value.get("providers").is_some_and(Value::is_object) {
            return Err("内核返回的代理提供者数据无效。".into());
        }
        Ok(value)
    }

    pub async fn proxy_group_icon(&self, name: &str) -> Result<Option<String>, String> {
        check_name(name)?;
        let value = self.request(Method::GET, &["proxies"], None, &[]).await?;
        let group = value
            .get("proxies")
            .and_then(|proxies| proxies.get(name))
            .ok_or("代理组不存在。")?;
        if !group.get("all").is_some_and(Value::is_array) {
            return Err("所选项目不是代理组。".into());
        }
        Ok(group.get("icon").and_then(Value::as_str).map(str::to_owned))
    }
    pub async fn provider_healthcheck(&self, name: &str) -> Result<(), String> {
        check_name(name)?;
        self.request(
            Method::GET,
            &["providers", "proxies", name, "healthcheck"],
            None,
            &[],
        )
        .await
        .map(|_| ())
    }

    pub async fn rule_providers(&self) -> Result<Value, String> {
        let result = self
            .request(Method::GET, &["providers", "rules"], None, &[])
            .await?;
        if !result.get("providers").is_some_and(Value::is_object) {
            return Err("内核返回的规则提供者数据无效。".into());
        }
        Ok(result)
    }

    pub async fn update_rule_provider(&self, name: &str) -> Result<(), String> {
        check_name(name)?;
        self.request(Method::PUT, &["providers", "rules", name], None, &[])
            .await
            .map(|_| ())
    }

    pub async fn refresh_rule_providers(&self) -> Result<(), String> {
        let inventory = self.rule_providers().await?;
        let providers = inventory["providers"]
            .as_object()
            .expect("validated inventory");
        let mut names: Vec<_> = providers.keys().collect();
        names.sort();
        let mut failures = Vec::new();
        for name in names {
            // 单项失败后继续更新其他提供者，与旧端的批量刷新行为一致。
            if self.update_rule_provider(name).await.is_err() {
                failures.push(name.as_str());
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "规则提供者更新完成 {}/{}；失败：{}。可再次刷新重试。",
                providers.len() - failures.len(),
                providers.len(),
                failures.join("、")
            ))
        }
    }

    pub async fn flush_fakeip_cache(&self) -> Result<(), String> {
        self.request(Method::POST, &["cache", "fakeip", "flush"], None, &[])
            .await
            .map(|_| ())
    }

    pub async fn flush_dns_cache(&self) -> Result<(), String> {
        self.request(Method::POST, &["cache", "dns", "flush"], None, &[])
            .await
            .map(|_| ())
    }

    pub async fn upgrade_geo(&self) -> Result<(), String> {
        self.request(
            Method::POST,
            &["upgrade", "geo"],
            Some(json!({"path": "", "payload": ""})),
            &[],
        )
        .await
        .map(|_| ())
    }

    pub async fn upgrade_core(&self) -> Result<CoreUpgradeResult, String> {
        let mut request = self
            .client
            .post(self.url(&["upgrade"]))
            .timeout(Duration::from_secs(60))
            .json(&json!({"path": "", "payload": ""}));
        if !self.secret.is_empty() {
            request = request.bearer_auth(&self.secret);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| "内核升级请求未完成，请检查控制器连接后重试。")?;
        let status = response.status().as_u16();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_MAINTENANCE_RESPONSE as u64)
        {
            return Err("内核升级响应超过 64 KiB。".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "内核升级响应中断。")? {
            if bytes.len() + chunk.len() > MAX_MAINTENANCE_RESPONSE {
                return Err("内核升级响应超过 64 KiB。".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        self.parse_upgrade_response(status, &bytes)
    }

    fn parse_upgrade_response(
        &self,
        status: u16,
        bytes: &[u8],
    ) -> Result<CoreUpgradeResult, String> {
        let response = serde_json::from_slice::<Value>(bytes).ok();
        if response
            .as_ref()
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("ok"))
        {
            return Ok(CoreUpgradeResult::Succeeded);
        }
        let message = response
            .as_ref()
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned());
        let message = if self.secret.is_empty() {
            message
        } else {
            message.replace(&self.secret, "[已隐藏]")
        };
        let message = message.trim();
        if message
            .to_ascii_lowercase()
            .contains("already using latest version")
        {
            return Ok(CoreUpgradeResult::AlreadyLatest {
                version: latest_core_version(message),
            });
        }
        if message.is_empty() || (200..300).contains(&status) && response.is_none() {
            return Err(format!("内核升级返回了无效响应（HTTP {status}）。"));
        }
        let message: String = message
            .chars()
            .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
            .take(2048)
            .collect();
        Err(format!("内核升级失败（HTTP {status}）：{message}"))
    }
}

fn latest_core_version(message: &str) -> Option<String> {
    message
        .split(|character: char| !character.is_ascii_digit() && character != '.')
        .filter_map(|candidate| {
            let candidate = candidate.trim_matches('.');
            let components: Vec<_> = candidate.split('.').collect();
            (components.len() > 1 && components.iter().all(|part| !part.is_empty()))
                .then(|| {
                    components
                        .iter()
                        .map(|part| part.parse::<u64>())
                        .collect::<Result<Vec<_>, _>>()
                        .ok()
                })
                .flatten()
                .map(|parts| {
                    parts
                        .iter()
                        .map(u64::to_string)
                        .collect::<Vec<_>>()
                        .join(".")
                })
        })
        .next_back()
}

pub fn normalize_tun_stack(stack: &str) -> Result<String, String> {
    let stack = stack.trim().to_ascii_lowercase();
    if ["system", "gvisor", "mixed", "mips"].contains(&stack.as_str()) {
        Ok(stack)
    } else {
        Err("TUN 协议栈必须为 system、gvisor、mixed 或 mips。".into())
    }
}

pub fn supports_mips_stack(version: &str) -> bool {
    let version = crate::releases::normalized_display_version(version);
    !version.is_empty()
        && version
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
        && !crate::releases::is_newer_release(MIPS_MINIMUM_CORE_VERSION, &version)
}

fn tun_settings_from_configs(configs: &Value) -> Result<TunSettings, String> {
    let tun = configs.get("tun").ok_or("当前内核未返回 TUN 配置。")?;
    let enable = tun
        .get("enable")
        .and_then(|value| {
            value
                .as_bool()
                .or_else(|| value.as_i64().map(|value| value != 0))
                .or_else(
                    || match value.as_str()?.trim().to_ascii_lowercase().as_str() {
                        "true" | "yes" | "on" | "1" => Some(true),
                        "false" | "no" | "off" | "0" => Some(false),
                        _ => None,
                    },
                )
        })
        .ok_or("当前内核未返回有效的 TUN 启用状态。")?;
    let stack = tun
        .get("stack")
        .and_then(Value::as_str)
        .and_then(|stack| normalize_tun_stack(stack).ok());
    Ok(TunSettings { enable, stack })
}

fn valid_log_level(level: &str) -> bool {
    ["debug", "info", "warning", "error", "silent"].contains(&level)
}

fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.len() > 1024
        || name.chars().any(char::is_control)
    {
        return Err("Invalid core resource name.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::mpsc,
        thread::{self, JoinHandle},
        time::Instant,
    };

    struct Response {
        method: &'static str,
        path: &'static str,
        status: u16,
        body: String,
    }

    impl Response {
        fn new(method: &'static str, path: &'static str, status: u16, body: Value) -> Self {
            Self {
                method,
                path,
                status,
                body: if status == 204 {
                    String::new()
                } else {
                    body.to_string()
                },
            }
        }
    }

    #[derive(Debug)]
    struct Request {
        method: String,
        target: String,
        authorization: String,
        body: Vec<u8>,
    }

    fn read_request(stream: &mut TcpStream) -> Request {
        // Winsock accept inherits the listener's nonblocking mode. The listener
        // polls with a deadline, but this HTTP parser requires blocking reads
        // with its own timeout (including when a request arrives in fragments).
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0; 1024];
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0, "fixture received an incomplete request");
            bytes.extend_from_slice(&chunk[..count]);
            assert!(bytes.len() < 16 * 1024);
            if let Some(index) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
        let mut lines = header.lines();
        let mut first = lines.next().unwrap().split_whitespace();
        let method = first.next().unwrap().to_owned();
        let target = first.next().unwrap().to_owned();
        let mut authorization = String::new();
        let mut length = 0;
        for line in lines {
            if let Some((name, value)) = line.split_once(':') {
                match name.to_ascii_lowercase().as_str() {
                    "authorization" => authorization = value.trim().to_owned(),
                    "content-length" => length = value.trim().parse::<usize>().unwrap(),
                    _ => {}
                }
            }
        }
        while bytes.len() < header_end + length {
            let mut chunk = [0; 1024];
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
        }
        Request {
            method,
            target,
            authorization,
            body: bytes[header_end..header_end + length].to_vec(),
        }
    }

    /// An actual loopback HTTP fixture verifies reqwest serialization and headers,
    /// without shelling out or reaching a public network/service.
    fn fixture(mut responses: Vec<Response>) -> (Controller, JoinHandle<Vec<Request>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut requests = Vec::new();
            while !responses.is_empty() {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "fixture expected another request"
                        );
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                };
                let request = read_request(&mut stream);
                let path = request.target.split('?').next().unwrap();
                let index = responses
                    .iter()
                    .position(|response| response.method == request.method && response.path == path)
                    .unwrap_or_else(|| panic!("unexpected fixture request: {request:?}"));
                let response = responses.remove(index);
                let header = format!(
                    "HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.status,
                    response.body.len()
                );
                stream.write_all(header.as_bytes()).unwrap();
                stream.write_all(response.body.as_bytes()).unwrap();
                requests.push(request);
            }
            requests
        });
        (
            Controller::new(port, "fixture-secret".into()).unwrap(),
            server,
        )
    }

    fn snapshot_responses(memory: Option<u64>) -> Vec<Response> {
        let mut connections = json!({"connections": [], "uploadTotal": 7, "downloadTotal": 11});
        if let Some(memory) = memory {
            connections["memory"] = json!(memory);
        }
        vec![
            Response::new("GET", "/proxies", 200, json!({"proxies": {}})),
            Response::new(
                "GET",
                "/configs",
                200,
                json!({
                    "mode": "rule", "log-level": "info", "secret": "must-not-cross-ipc",
                    "allow-lan": false, "ipv6": true, "tcp-concurrent": false,
                    "external-controller": "127.0.0.1:9090"
                }),
            ),
            Response::new("GET", "/rules", 200, json!({"rules": []})),
            Response::new("GET", "/connections", 200, connections),
            Response::new("GET", "/providers/proxies", 200, json!({"providers": {}})),
            Response::new(
                "GET",
                "/providers/rules",
                200,
                json!({"providers": {"test": {"ruleCount": 42}}}),
            ),
        ]
    }

    #[test]
    fn fixture_reads_delayed_fragmented_requests_from_nonblocking_streams() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        // Reproduce Windows accepted-socket inheritance on every test platform.
        stream.set_nonblocking(true).unwrap();
        let (finished, completion) = mpsc::channel();
        let server = thread::spawn(move || {
            let request = read_request(&mut stream);
            finished.send(()).unwrap();
            request
        });

        // No bytes are available yet. The parser must wait, not panic with
        // WouldBlock. A dropped sender also fails this assertion.
        assert!(matches!(
            completion.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        client
            .write_all(b"PATCH /configs HTTP/1.1\r\nAuthorization: Bearer fixture-secret\r\nContent-Length: 4\r\n\r\nab")
            .unwrap();
        // The complete header must not make an incomplete body look complete.
        assert!(matches!(
            completion.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        client.write_all(b"cd").unwrap();
        completion.recv_timeout(Duration::from_secs(3)).unwrap();
        let request = server.join().unwrap();
        assert_eq!(request.method, "PATCH");
        assert_eq!(request.target, "/configs");
        assert_eq!(request.authorization, "Bearer fixture-secret");
        assert_eq!(request.body, b"abcd");
    }

    #[test]
    fn proxy_names_are_one_encoded_path_segment() {
        let controller = Controller::new(19090, "hidden".into()).unwrap();
        let url = controller.url(&["proxies", "HK / fast?#", "delay"]);
        assert_eq!(url.path(), "/proxies/HK%20%2F%20fast%3F%23/delay");
        assert!(url.query().is_none());
        assert!(check_name(".").is_err());
        assert!(check_name("..").is_err());
    }

    #[tokio::test]
    async fn compact_controls_use_authenticated_encoded_endpoints() {
        let (controller, server) = fixture(vec![
            Response::new("DELETE", "/connections", 204, Value::Null),
            Response::new(
                "GET",
                "/providers/proxies/HK%20%2F%20fast%3F%23/healthcheck",
                204,
                Value::Null,
            ),
            Response::new(
                "GET",
                "/group/HK%20%2F%20fast%3F%23/delay",
                200,
                json!({"Node A": 42, "Node B": 0}),
            ),
            Response::new("PATCH", "/configs", 204, Value::Null),
        ]);
        controller.close_all_connections().await.unwrap();
        controller
            .provider_healthcheck("HK / fast?#")
            .await
            .unwrap();
        assert_eq!(
            controller.test_group_delay("HK / fast?#").await.unwrap(),
            json!({"Node A": 42, "Node B": 0})
        );
        controller.set_log_level("debug").await.unwrap();
        assert_eq!(
            controller.set_log_level("trace").await.unwrap_err(),
            "Unknown log level."
        );
        let requests = server.join().unwrap();
        assert!(requests
            .iter()
            .all(|request| request.authorization == "Bearer fixture-secret"));
        let group = requests
            .iter()
            .find(|request| request.target.starts_with("/group/"))
            .unwrap();
        let group_url = Url::parse(&format!("http://127.0.0.1{}", group.target)).unwrap();
        let query: std::collections::BTreeMap<_, _> = group_url.query_pairs().collect();
        assert_eq!(
            query.get("timeout").map(|value| value.as_ref()),
            Some("5000")
        );
        assert_eq!(
            query.get("url").map(|value| value.as_ref()),
            Some("https://www.gstatic.com/generate_204")
        );
        let patch = requests
            .iter()
            .find(|request| request.method == "PATCH")
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&patch.body).unwrap(),
            json!({"log-level": "debug"})
        );
    }

    #[tokio::test]
    async fn snapshot_uses_connection_memory_and_only_exposes_allowed_configs() {
        let (controller, server) = fixture(snapshot_responses(Some(123456)));
        let snapshot = controller.snapshot().await.unwrap();
        assert_eq!(snapshot.memory, Some(123456));
        assert_eq!(
            snapshot.configs,
            json!({"mode": "rule", "log-level": "info", "allow-lan": false, "ipv6": true, "tcp-concurrent": false})
        );
        assert!(!serde_json::to_string(&snapshot)
            .unwrap()
            .contains("must-not-cross-ipc"));
        assert_eq!(
            snapshot.rule_providers["providers"]["test"]["ruleCount"],
            42
        );
        assert_eq!(server.join().unwrap().len(), 6);
    }

    #[tokio::test]
    async fn snapshot_accepts_empty_go_collections_without_accepting_missing_fields() {
        let mut responses = snapshot_responses(Some(0));
        for response in &mut responses {
            response.body = match response.path {
                "/rules" => json!({"rules": null}).to_string(),
                "/connections" => json!({"connections": null, "memory": 0}).to_string(),
                "/providers/proxies" => json!({"providers": null}).to_string(),
                _ => response.body.clone(),
            };
        }
        let (controller, server) = fixture(responses);
        let snapshot = controller.snapshot().await.unwrap();
        assert_eq!(snapshot.rules["rules"], json!([]));
        assert_eq!(snapshot.connections["connections"], json!([]));
        assert_eq!(snapshot.providers["providers"], json!({}));
        server.join().unwrap();

        let mut responses = snapshot_responses(Some(0));
        responses
            .iter_mut()
            .find(|response| response.path == "/connections")
            .unwrap()
            .body = "{}".into();
        let (controller, server) = fixture(responses);
        assert!(controller.snapshot().await.is_err());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn unavailable_memory_does_not_fail_snapshot() {
        let mut responses = snapshot_responses(None);
        responses.push(Response::new(
            "GET",
            "/memory",
            404,
            json!({"message": "unsupported"}),
        ));
        let (controller, server) = fixture(responses);
        let snapshot = controller.snapshot().await.unwrap();
        assert_eq!(snapshot.memory, None);
        assert_eq!(snapshot.configs["mode"], "rule");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn memory_reads_only_first_stream_sample() {
        let response = Response {
            method: "GET",
            path: "/memory",
            status: 200,
            body: "{\"inuse\": 123, \"oslimit\": 0}\n{\"inuse\": 456}\n".into(),
        };
        let (controller, server) = fixture(vec![response]);
        assert_eq!(controller.memory_usage().await, Some(123));
        let requests = server.join().unwrap();
        assert_eq!(requests[0].authorization, "Bearer fixture-secret");
    }

    #[tokio::test]
    async fn remote_controller_authentication_and_empty_secret_use_the_selected_origin() {
        let (local, server) = fixture(vec![Response::new(
            "GET",
            "/version",
            200,
            json!({"version":"remote"}),
        )]);
        let machine = crate::remote::RemoteMachine::from_input(
            crate::remote::RemoteInput {
                id: None,
                name: "remote".into(),
                host: "127.0.0.1".into(),
                port: local.base.port().unwrap(),
                use_https: false,
                secret: Some("remote-fixture-token".into()),
            },
            None,
        )
        .unwrap();
        assert_eq!(
            Controller::remote(&machine)
                .unwrap()
                .version()
                .await
                .unwrap(),
            "remote"
        );
        assert_eq!(
            server.join().unwrap()[0].authorization,
            "Bearer remote-fixture-token"
        );
        let (local, server) = fixture(vec![Response::new(
            "GET",
            "/version",
            200,
            json!({"version":"no-auth"}),
        )]);
        let mut machine = machine;
        machine.port = local.base.port().unwrap();
        machine.secret.clear();
        Controller::remote(&machine)
            .unwrap()
            .version()
            .await
            .unwrap();
        assert!(server.join().unwrap()[0].authorization.is_empty());
    }

    #[tokio::test]
    async fn log_stream_preserves_level_and_redacts_credentials_in_bounded_records() {
        let body = (0..505)
            .map(|index| {
                json!({"type":"warning","payload":format!("record {index} fixture-secret")})
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let (controller, server) = fixture(vec![Response {
            method: "GET",
            path: "/logs",
            status: 200,
            body,
        }]);
        let logs = crate::process::logs();
        assert!(controller
            .receive_logs(&logs, "warning", || {})
            .await
            .unwrap_err()
            .contains("已结束"));
        let entries = logs.lock().unwrap();
        assert_eq!(entries.len(), 500);
        assert!(entries.front().unwrap().message.contains("record 5"));
        assert!(entries
            .back()
            .unwrap()
            .message
            .contains("warning record 504"));
        assert!(entries
            .iter()
            .all(|entry| !entry.message.contains("fixture-secret")));
        assert!(server.join().unwrap()[0].target.contains("level=warning"));
    }

    #[tokio::test]
    // tungstenite 的握手回调固定使用未装箱的 HTTP 错误响应。
    #[allow(clippy::result_large_err)]
    async fn websocket_logs_use_header_auth_and_accept_binary_wrapped_and_plain_messages() {
        use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (captured, request) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut captured = Some(captured);
            let mut socket = tokio_tungstenite::accept_hdr_async(
                stream,
                move |request: &Request, response: Response| {
                    captured
                        .take()
                        .unwrap()
                        .send((
                            request.uri().to_string(),
                            request.headers()[AUTHORIZATION]
                                .to_str()
                                .unwrap()
                                .to_owned(),
                        ))
                        .unwrap();
                    Ok(response)
                },
            )
            .await
            .unwrap();
            for message in [
                Message::Text(
                    json!({"type":"warning", "payload":"first fixture-secret"})
                        .to_string()
                        .into(),
                ),
                Message::Binary(
                    json!({"logs":[{"type":"error", "payload":"wrapped fixture-secret"}]})
                        .to_string()
                        .into_bytes()
                        .into(),
                ),
                Message::Text("plain fixture-secret".into()),
                Message::Ping(vec![1, 2, 3].into()),
            ] {
                socket.send(message).await.unwrap();
            }
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Pong(vec![1, 2, 3].into())
            );
            socket.close(None).await.unwrap();
        });
        let logs = crate::process::logs();
        let controller = Controller::new(port, "fixture-secret".into()).unwrap();
        let mut ready = false;
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            controller.receive_log_events(&logs, "debug", || ready = true),
        )
        .await
        .unwrap();
        assert!(result.unwrap_err().contains("已结束"));
        assert!(ready);
        server.await.unwrap();
        let (path, authorization) = request.await.unwrap();
        assert_eq!(path, "/logs?level=debug");
        assert_eq!(authorization, "Bearer fixture-secret");
        assert!(!path.contains("fixture-secret"));
        let entries = logs.lock().unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries[0].message.contains("warning first"));
        assert!(entries[1].message.contains("error wrapped"));
        assert!(entries[2].message.contains("info plain"));
        assert!(entries
            .iter()
            .all(|entry| !entry.message.contains("fixture-secret")));
    }

    #[tokio::test]
    async fn websocket_unavailable_cores_fall_back_to_authenticated_json_lines() {
        let (controller, server) = fixture(vec![
            Response::new("GET", "/traffic", 400, Value::Null),
            Response {
                method: "GET",
                path: "/traffic",
                status: 200,
                body: "{\"up\":1,\"down\":2}\n{\"up\":3,\"down\":4}\n".into(),
            },
        ]);
        let mut samples = Vec::new();
        let result = controller
            .receive_events(
                "traffic",
                &[],
                4096,
                || {},
                |bytes| {
                    samples.push(serde_json::from_slice::<Value>(bytes).unwrap());
                    Ok(())
                },
            )
            .await;
        assert!(result.unwrap_err().contains("已结束"));
        assert_eq!(
            samples,
            vec![json!({"up":1,"down":2}), json!({"up":3,"down":4})]
        );
        assert!(server
            .join()
            .unwrap()
            .iter()
            .all(|request| request.authorization == "Bearer fixture-secret"));
    }

    #[tokio::test]
    async fn remote_boolean_setting_preserves_remote_listener_binding() {
        let (controller, server) =
            fixture(vec![Response::new("PATCH", "/configs", 204, Value::Null)]);
        controller
            .set_remote_boolean_setting(CoreBooleanSetting::AllowLan, true)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&server.join().unwrap()[0].body).unwrap(),
            json!({"allow-lan":true})
        );
    }

    #[tokio::test]
    async fn boolean_settings_keep_controller_private_and_limit_writable_keys() {
        let (controller, server) = fixture(vec![
            Response::new(
                "GET",
                "/configs",
                200,
                json!({"allow-lan": false, "secret": "private"}),
            ),
            Response::new("PATCH", "/configs", 204, Value::Null),
            Response::new("PATCH", "/configs", 204, Value::Null),
        ]);
        assert!(!controller
            .boolean_setting(CoreBooleanSetting::AllowLan)
            .await
            .unwrap());
        controller
            .set_boolean_setting(CoreBooleanSetting::AllowLan, true)
            .await
            .unwrap();
        controller
            .set_boolean_setting(CoreBooleanSetting::AllowLan, false)
            .await
            .unwrap();
        assert!(serde_json::from_str::<CoreBooleanSetting>("\"external-controller\"").is_err());
        let requests = server.join().unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&requests[1].body).unwrap(),
            json!({"allow-lan": true, "bind-address": "*"})
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&requests[2].body).unwrap(),
            json!({"allow-lan": false, "bind-address": "127.0.0.1"})
        );
    }

    #[tokio::test]
    async fn rule_refresh_continues_after_failure_and_encodes_names() {
        let (controller, server) = fixture(vec![
            Response::new(
                "GET",
                "/providers/rules",
                200,
                json!({"providers": {
                    "A /?#": {}, "B": {}, "C": {}
                }}),
            ),
            Response::new(
                "PUT",
                "/providers/rules/A%20%2F%3F%23",
                500,
                json!({"message": "private detail"}),
            ),
            Response::new("PUT", "/providers/rules/B", 204, Value::Null),
            Response::new("PUT", "/providers/rules/C", 204, Value::Null),
        ]);
        let error = controller.refresh_rule_providers().await.unwrap_err();
        assert!(error.contains("2/3"));
        assert!(error.contains("A /?#"));
        assert!(!error.contains("private detail"));
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[3].target, "/providers/rules/C");
        assert!(requests
            .iter()
            .all(|request| request.authorization == "Bearer fixture-secret"));
    }

    #[tokio::test]
    async fn empty_and_invalid_rule_provider_inventories_are_distinguished() {
        let (controller, server) = fixture(vec![
            Response::new("GET", "/providers/rules", 200, json!({"providers": {}})),
            Response::new("GET", "/providers/rules", 200, json!({"providers": []})),
        ]);
        controller.refresh_rule_providers().await.unwrap();
        assert!(controller.rule_providers().await.is_err());
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn maintenance_uses_source_methods_and_reports_core_errors() {
        let (controller, server) = fixture(vec![
            Response::new("POST", "/cache/fakeip/flush", 204, Value::Null),
            Response::new(
                "POST",
                "/cache/dns/flush",
                500,
                json!({"message": "private detail"}),
            ),
            Response::new("POST", "/upgrade/geo", 204, Value::Null),
        ]);
        controller.flush_fakeip_cache().await.unwrap();
        assert_eq!(
            controller.flush_dns_cache().await.unwrap_err(),
            "控制器返回 HTTP 500。"
        );
        controller.upgrade_geo().await.unwrap();
        let requests = server.join().unwrap();
        assert!(requests
            .iter()
            .all(|request| request.authorization == "Bearer fixture-secret"));
        assert!(requests[0].body.is_empty());
        assert_eq!(
            serde_json::from_slice::<Value>(&requests[2].body).unwrap(),
            json!({"path": "", "payload": ""})
        );
    }

    #[tokio::test]
    async fn invalid_group_latency_payload_is_rejected() {
        let (controller, server) = fixture(vec![Response::new(
            "GET",
            "/group/Auto/delay",
            200,
            json!({"Node A": "not-a-delay"}),
        )]);
        assert!(controller.test_group_delay("Auto").await.is_err());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn core_upgrade_posts_legacy_body_and_accepts_success_or_latest() {
        let (controller, server) = fixture(vec![
            Response::new("POST", "/upgrade", 200, json!({"status": "ok"})),
            Response::new(
                "POST",
                "/upgrade",
                400,
                json!({
                    "message": "Already using latest version v1.19.15"
                }),
            ),
            Response::new(
                "POST",
                "/upgrade",
                200,
                json!({
                    "message": "already using latest version"
                }),
            ),
        ]);
        assert_eq!(
            controller.upgrade_core().await.unwrap(),
            CoreUpgradeResult::Succeeded
        );
        assert_eq!(
            controller.upgrade_core().await.unwrap(),
            CoreUpgradeResult::AlreadyLatest {
                version: Some("1.19.15".into())
            }
        );
        assert_eq!(
            controller.upgrade_core().await.unwrap(),
            CoreUpgradeResult::AlreadyLatest { version: None }
        );
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests
            .iter()
            .all(|request| request.authorization == "Bearer fixture-secret"));
        for request in requests {
            assert_eq!(
                serde_json::from_slice::<Value>(&request.body).unwrap(),
                json!({"path": "", "payload": ""})
            );
        }
    }

    #[tokio::test]
    async fn core_upgrade_redacts_controller_secret_and_reports_real_error() {
        let (controller, server) = fixture(vec![
            Response::new(
                "POST",
                "/upgrade",
                500,
                json!({
                    "message": "download failed for fixture-secret"
                }),
            ),
            Response::new("POST", "/upgrade", 200, json!({})),
            Response::new("POST", "/upgrade", 204, Value::Null),
        ]);
        let error = controller.upgrade_core().await.unwrap_err();
        assert!(error.contains("HTTP 500"));
        assert!(error.contains("download failed"));
        assert!(error.contains("[已隐藏]"));
        assert!(!error.contains("fixture-secret"));
        assert!(controller.upgrade_core().await.is_err());
        assert!(controller.upgrade_core().await.is_err());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn core_upgrade_recognizes_plain_text_latest_response() {
        let (controller, server) = fixture(vec![Response {
            method: "POST",
            path: "/upgrade",
            status: 400,
            body: "Already using latest version v1.20.0".into(),
        }]);
        assert_eq!(
            controller.upgrade_core().await.unwrap(),
            CoreUpgradeResult::AlreadyLatest {
                version: Some("1.20.0".into())
            }
        );
        server.join().unwrap();
    }

    #[test]
    fn latest_version_uses_last_numeric_version_without_exposing_secret() {
        let controller = Controller::new(9090, "9.99.99".into()).unwrap();
        let response = controller
            .parse_upgrade_response(
                400,
                br#"{"message":"already using latest version v1.18.0; token=9.99.99"}"#,
            )
            .unwrap();
        assert_eq!(
            response,
            CoreUpgradeResult::AlreadyLatest {
                version: Some("1.18.0".into())
            }
        );
        assert_eq!(
            latest_core_version("from v1.18.0 to v1.19.2"),
            Some("1.19.2".into())
        );
        assert_eq!(latest_core_version("no version 2026"), None);
    }

    #[tokio::test]
    async fn web_ui_metadata_reads_only_known_fields_from_controller() {
        let (controller, server) = fixture(vec![Response::new(
            "GET",
            "/configs",
            200,
            json!({
                "external-ui-url": "https://example.test/dashboard.zip",
                "external-ui-name": " /dashboard/ ",
                "secret": "must-not-cross-ipc",
                "external-controller": "malicious.invalid:1234",
            }),
        )]);
        let metadata = controller.web_ui_metadata().await.unwrap();
        assert_eq!(
            metadata,
            WebUiMetadata {
                has_configured_external_ui: true,
                external_ui_name: Some("dashboard".into()),
            }
        );
        assert!(!serde_json::to_string(&metadata)
            .unwrap()
            .contains("must-not-cross-ipc"));
        let web_ui = controller.web_ui_url(&metadata).unwrap();
        assert_eq!(web_ui.host_str(), Some("127.0.0.1"));
        assert_eq!(web_ui.port(), controller.base.port());
        assert_eq!(web_ui.path(), "/ui/dashboard");
        assert!(web_ui.query().is_none());
        assert!(web_ui.fragment().is_none());
        assert_eq!(
            server.join().unwrap()[0].authorization,
            "Bearer fixture-secret"
        );
    }

    #[tokio::test]
    async fn web_ui_metadata_without_url_uses_setup_and_ignores_orphan_name() {
        let (controller, server) = fixture(vec![Response::new(
            "GET",
            "/configs",
            200,
            json!({
                "external-ui-url": "  ", "external-ui-name": "unused"
            }),
        )]);
        let metadata = controller.web_ui_metadata().await.unwrap();
        assert_eq!(metadata, WebUiMetadata::default());
        let url = controller.web_ui_url(&metadata).unwrap();
        assert_eq!(url.host_str(), Some("metacubex.github.io"));
        assert!(url.query().is_none());
        assert!(url
            .fragment()
            .unwrap()
            .starts_with("/setup?http=true&hostname=127.0.0.1&port="));
        assert!(url.fragment().unwrap().ends_with("&secret=fixture-secret"));
        server.join().unwrap();
    }

    #[test]
    fn web_ui_keeps_name_in_one_path_segment_and_secret_in_fragment() {
        let controller = Controller::with_endpoint(
            Url::parse("https://[::1]:9443/").unwrap(),
            " &private?#=/中文 ".into(),
        )
        .unwrap();
        let url = controller
            .web_ui_url(&WebUiMetadata {
                has_configured_external_ui: true,
                external_ui_name: Some("/dash /?#/".into()),
            })
            .unwrap();
        assert_eq!(url.path(), "/ui/dash%20%2F%3F%23");
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
        let setup = controller.web_ui_url(&WebUiMetadata::default()).unwrap();
        assert!(setup.query().is_none());
        let query: std::collections::BTreeMap<_, _> = url::form_urlencoded::parse(
            setup
                .fragment()
                .unwrap()
                .strip_prefix("/setup?")
                .unwrap()
                .as_bytes(),
        )
        .collect();
        assert_eq!(query.get("http").unwrap(), "false");
        assert_eq!(query.get("hostname").unwrap(), "::1");
        assert_eq!(query.get("port").unwrap(), "9443");
        assert_eq!(query.get("secret").unwrap(), "&private?#=/中文");
        for name in [".", "..", "with\nnewline"] {
            assert!(controller
                .web_ui_url(&WebUiMetadata {
                    has_configured_external_ui: true,
                    external_ui_name: Some(name.into())
                })
                .is_err());
        }
    }

    #[tokio::test]
    async fn malformed_web_ui_configs_are_not_reported_as_valid_metadata() {
        let (controller, server) = fixture(vec![Response::new("GET", "/configs", 200, json!([]))]);
        assert!(controller.web_ui_metadata().await.is_err());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn tun_patch_verifies_runtime_state_and_only_sends_tun_fields() {
        let (controller, server) = fixture(vec![
            Response::new("PATCH", "/configs", 204, Value::Null),
            Response::new(
                "GET",
                "/configs",
                200,
                json!({
                    "secret": "must-not-cross-ipc",
                    "tun": {"enable": true, "stack": "Mixed", "device": "private-device"}
                }),
            ),
            Response::new("PATCH", "/configs", 204, Value::Null),
            Response::new(
                "GET",
                "/configs",
                200,
                json!({"tun": {"enable": false, "stack": "mixed"}}),
            ),
        ]);
        let enabled = controller
            .set_tun_settings(true, Some("MiXeD"))
            .await
            .unwrap();
        assert_eq!(
            enabled,
            TunSettings {
                enable: true,
                stack: Some("mixed".into())
            }
        );
        let disabled = controller.set_tun_settings(false, None).await.unwrap();
        assert!(!disabled.enable);
        let requests = server.join().unwrap();
        assert_eq!(
            requests
                .iter()
                .map(|request| request.method.as_str())
                .collect::<Vec<_>>(),
            ["PATCH", "GET", "PATCH", "GET"]
        );
        assert!(requests
            .iter()
            .all(|request| request.authorization == "Bearer fixture-secret"));
        assert_eq!(
            serde_json::from_slice::<Value>(&requests[0].body).unwrap(),
            json!({"tun": {"enable": true, "stack": "mixed"}})
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&requests[2].body).unwrap(),
            json!({"tun": {"enable": false}})
        );
        assert_eq!(
            serde_json::to_value(enabled).unwrap(),
            json!({"enable": true, "stack": "mixed"})
        );
    }

    #[tokio::test]
    async fn tun_patch_rejects_state_or_stack_mismatch() {
        let (controller, server) = fixture(vec![
            Response::new("PATCH", "/configs", 204, Value::Null),
            Response::new(
                "GET",
                "/configs",
                200,
                json!({"tun": {"enable": false, "stack": "mixed"}}),
            ),
            Response::new("PATCH", "/configs", 204, Value::Null),
            Response::new(
                "GET",
                "/configs",
                200,
                json!({"tun": {"enable": true, "stack": "gvisor"}}),
            ),
            Response::new(
                "PATCH",
                "/configs",
                403,
                json!({"message": "permission denied"}),
            ),
        ]);
        assert!(controller
            .set_tun_settings(true, Some("mixed"))
            .await
            .unwrap_err()
            .contains("运行状态"));
        assert!(controller
            .set_tun_settings(true, Some("system"))
            .await
            .unwrap_err()
            .contains("协议栈"));
        assert!(controller
            .set_tun_settings(true, None)
            .await
            .unwrap_err()
            .contains("403"));
        assert_eq!(server.join().unwrap().len(), 5);
    }

    #[tokio::test]
    async fn tun_mips_gate_checks_selected_core_before_sending_patch() {
        let (controller, server) = fixture(vec![
            Response::new("GET", "/version", 200, json!({"version": "v1.19.30"})),
            Response::new("GET", "/version", 200, json!({"version": "v1.19.31"})),
            Response::new("PATCH", "/configs", 204, Value::Null),
            Response::new(
                "GET",
                "/configs",
                200,
                json!({"tun": {"enable": true, "stack": "mips"}}),
            ),
        ]);
        assert!(controller
            .set_tun_settings(true, Some("unknown"))
            .await
            .is_err());
        assert!(controller
            .set_tun_settings(true, Some("mips"))
            .await
            .unwrap_err()
            .contains(MIPS_MINIMUM_CORE_VERSION));
        let settings = controller
            .set_tun_settings(true, Some("MIPS"))
            .await
            .unwrap();
        assert_eq!(settings.stack.as_deref(), Some("mips"));
        let requests = server.join().unwrap();
        assert_eq!(
            requests
                .iter()
                .map(|request| request.target.as_str())
                .collect::<Vec<_>>(),
            ["/version", "/version", "/configs", "/configs"]
        );
    }

    #[tokio::test]
    async fn tun_patch_uses_remote_endpoint_and_keeps_its_listener_unchanged() {
        let (fixture_controller, server) = fixture(vec![
            Response::new("PATCH", "/configs", 204, Value::Null),
            Response::new(
                "GET",
                "/configs",
                200,
                json!({"tun": {"enable": true, "stack": "system"}}),
            ),
        ]);
        let remote = crate::remote::RemoteMachine::from_input(
            crate::remote::RemoteInput {
                id: None,
                name: "TUN fixture".into(),
                host: "127.0.0.1".into(),
                port: fixture_controller.base.port().unwrap(),
                use_https: false,
                secret: Some("selected-remote-secret".into()),
            },
            None,
        )
        .unwrap();
        let controller = Controller::remote(&remote).unwrap();
        controller
            .set_tun_settings(true, Some("system"))
            .await
            .unwrap();
        let requests = server.join().unwrap();
        assert!(requests
            .iter()
            .all(|request| request.authorization == "Bearer selected-remote-secret"));
        let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body, json!({"tun": {"enable": true, "stack": "system"}}));
        assert!(body.get("bind-address").is_none());
        assert!(body.get("external-controller").is_none());
    }

    #[tokio::test]
    async fn snapshot_exposes_tun_controls_without_private_tun_options() {
        let mut responses = snapshot_responses(Some(1));
        let configs = responses
            .iter_mut()
            .find(|response| response.path == "/configs")
            .unwrap();
        let mut body: Value = serde_json::from_str(&configs.body).unwrap();
        body["tun"] = json!({"enable": true, "stack": "gvisor", "dns-hijack": ["any:53"], "device": "private-device"});
        configs.body = body.to_string();
        let (controller, server) = fixture(responses);
        let snapshot = controller.snapshot().await.unwrap();
        assert_eq!(
            snapshot.configs["tun"],
            json!({"enable": true, "stack": "gvisor"})
        );
        assert!(!serde_json::to_string(&snapshot)
            .unwrap()
            .contains("private-device"));
        server.join().unwrap();
    }

    #[test]
    fn tun_parser_preserves_legacy_boolean_forms_and_rejects_missing_state() {
        for value in [
            json!(true),
            json!(1),
            json!(-1),
            json!(" true "),
            json!("YES"),
            json!("on"),
        ] {
            assert!(
                tun_settings_from_configs(&json!({"tun": {"enable": value, "stack": "system"}}))
                    .unwrap()
                    .enable
            );
        }
        for value in [
            json!(false),
            json!(0),
            json!("false"),
            json!("No"),
            json!("off"),
        ] {
            assert!(
                !tun_settings_from_configs(&json!({"tun": {"enable": value}}))
                    .unwrap()
                    .enable
            );
        }
        for value in [
            json!({}),
            json!({"tun": {}}),
            json!({"tun": {"enable": "unknown"}}),
        ] {
            assert!(tun_settings_from_configs(&value).is_err());
        }
        assert!(!supports_mips_stack("1.19.30"));
        assert!(supports_mips_stack("v1.19.31"));
        assert!(supports_mips_stack("v1.20.0"));
        assert!(!supports_mips_stack("unknown"));
    }
}
