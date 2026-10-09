use serde::{Deserialize, Serialize};
use std::{fmt, net::IpAddr};
use url::Url;

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteMachine {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub use_https: bool,
    #[serde(default)]
    pub secret: String,
}

impl fmt::Debug for RemoteMachine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteMachine")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("use_https", &self.use_https)
            .field("has_secret", &!self.secret.is_empty())
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteInput {
    pub id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub use_https: bool,
    /// 编辑时不传表示保留，空字符串表示清除；已保存密钥不回传给前端。
    pub secret: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSummary {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub use_https: bool,
    pub has_secret: bool,
    pub address: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connectivity {
    pub id: String,
    pub connected: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

impl RemoteMachine {
    pub fn from_input(input: RemoteInput, previous: Option<&Self>) -> Result<Self, String> {
        let machine = Self {
            id: previous
                .map(|machine| machine.id.clone())
                .unwrap_or_else(|| crate::config::secret()[..32].to_owned()),
            name: input.name.trim().to_owned(),
            host: normalize_host(&input.host)?,
            port: input.port,
            use_https: input.use_https,
            secret: input
                .secret
                .map(|secret| secret.trim().to_owned())
                .unwrap_or_else(|| {
                    previous
                        .map(|machine| machine.secret.clone())
                        .unwrap_or_default()
                }),
        };
        machine.validate()?;
        Ok(machine)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.len() != 32 || !self.id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err("远程机器标识无效。".into());
        }
        if self.name.trim().is_empty()
            || self.name.chars().count() > 180
            || self.name.chars().any(char::is_control)
        {
            return Err("名称需为 1–180 个字符，且不能包含控制字符。".into());
        }
        if self.secret.len() > 4096 || !self.secret.bytes().all(|byte| (32..=126).contains(&byte)) {
            return Err("密钥需为不超过 4096 字节的可打印 ASCII 字符。".into());
        }
        self.endpoint().map(|_| ())
    }

    pub fn endpoint(&self) -> Result<Url, String> {
        let host = normalize_host(&self.host)?;
        if self.port == 0 {
            return Err("端口需为 1–65535 之间的整数。".into());
        }
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };
        Url::parse(&format!(
            "{}://{host}:{}/",
            if self.use_https { "https" } else { "http" },
            self.port
        ))
        .map_err(|_| "主机地址无效。".into())
    }

    pub fn summary(&self) -> RemoteSummary {
        RemoteSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            host: self.host.clone(),
            port: self.port,
            use_https: self.use_https,
            has_secret: !self.secret.is_empty(),
            address: self
                .endpoint()
                .map(|url| url.as_str().trim_end_matches('/').to_owned())
                .unwrap_or_default(),
        }
    }
}

pub fn normalize_host(raw: &str) -> Result<String, String> {
    let host = raw.trim();
    if host.is_empty()
        || host.len() > 253
        || host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "/\\@?#;%\"'`$".contains(c))
    {
        return Err("请输入主机名或 IP 地址，不要包含协议、端口或路径。".into());
    }
    let ip = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(address) = ip.parse::<IpAddr>() {
        return Ok(address.to_string());
    }
    if host.contains([':', '[', ']']) {
        return Err("IPv6 地址无效；端口请填写在单独的端口字段。".into());
    }
    match url::Host::parse(host).map_err(|_| "主机地址无效。")? {
        url::Host::Domain(host) => Ok(host),
        url::Host::Ipv4(host) => Ok(host.to_string()),
        url::Host::Ipv6(host) => Ok(host.to_string()),
    }
}

pub async fn probe(machine: &RemoteMachine) -> Connectivity {
    let result = async {
        let controller = crate::controller::Controller::remote(machine)?;
        tokio::time::timeout(std::time::Duration::from_secs(2), controller.version())
            .await
            .map_err(|_| "连接超时。".to_owned())?
    }
    .await;
    match result {
        Ok(version) => Connectivity {
            id: machine.id.clone(),
            connected: true,
            version: Some(version),
            error: None,
        },
        Err(error) => Connectivity {
            id: machine.id.clone(),
            connected: false,
            version: None,
            error: Some(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(host: &str) -> RemoteInput {
        RemoteInput {
            id: None,
            name: "测试机器".into(),
            host: host.into(),
            port: 9090,
            use_https: false,
            secret: Some("fixture-secret".into()),
        }
    }

    #[test]
    fn hosts_cannot_inject_urls_headers_or_proxy_commands() {
        for invalid in [
            "",
            "https://host",
            "host:9090",
            "host/path",
            "user@host",
            "host?secret=x",
            "host\nHeader:value",
            "host;proxy",
            "host\\share",
            "[invalid]",
            "host\"",
            "host$env",
        ] {
            assert!(
                RemoteMachine::from_input(input(invalid), None).is_err(),
                "{invalid:?}"
            );
        }
        let ipv6 = RemoteMachine::from_input(input("[2001:db8::1]"), None).unwrap();
        assert_eq!(
            ipv6.endpoint().unwrap().as_str(),
            "http://[2001:db8::1]:9090/"
        );
        assert_eq!(
            RemoteMachine::from_input(input("router.local"), None)
                .unwrap()
                .host,
            "router.local"
        );
    }

    #[test]
    fn summaries_and_debug_do_not_expose_saved_credentials() {
        let machine = RemoteMachine::from_input(input("example.com"), None).unwrap();
        assert!(!serde_json::to_string(&machine.summary())
            .unwrap()
            .contains("fixture-secret"));
        assert!(!format!("{machine:?}").contains("fixture-secret"));
        let mut edit = input("example.com");
        edit.secret = None;
        let saved = RemoteMachine::from_input(edit, Some(&machine)).unwrap();
        assert_eq!(saved.secret, "fixture-secret");
        let mut edit = input("example.com");
        edit.secret = Some(String::new());
        assert!(RemoteMachine::from_input(edit, Some(&saved))
            .unwrap()
            .secret
            .is_empty());
    }
}
