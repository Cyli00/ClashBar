use crate::config::MAX_CONFIG_BYTES;
use std::{collections::HashSet, time::Duration};
use url::Url;

const MAX_REDIRECTS: usize = 10;
const DEFAULT_MIHOMO_VERSION: &str = "v1.19.27";

pub fn validate_url(input: &str) -> Result<Url, String> {
    if input.len() > 8192 {
        return Err("订阅地址过长。".into());
    }
    let url = Url::parse(input.trim()).map_err(|_| "请输入有效的 HTTP 或 HTTPS 订阅地址。")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("请输入有效的 HTTP 或 HTTPS 订阅地址。".into());
    }
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("订阅地址不能包含用户名、密码或片段。".into());
    }
    if url.host_str().is_none() || url.port_or_known_default().is_none_or(|port| port == 0) {
        return Err("订阅地址需要有效的主机名和端口。".into());
    }
    Ok(url)
}

pub fn user_agent(version: Option<&str>) -> String {
    let version = version
        .map(str::trim)
        .filter(|version| {
            !version.is_empty()
                && *version != "-"
                && version.len() <= 128
                && version.bytes().all(|byte| (32..=126).contains(&byte))
        })
        .unwrap_or(DEFAULT_MIHOMO_VERSION);
    format!("clash.meta/{version}")
}

pub async fn download(input: &str) -> Result<Vec<u8>, String> {
    download_with_version(input, None).await
}

pub async fn download_with_version(input: &str, version: Option<&str>) -> Result<Vec<u8>, String> {
    let url = validate_url(input)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(30))
        .user_agent(user_agent(version))
        .build()
        .map_err(|_| "无法创建订阅下载连接。")?;
    // 整条重定向链共用超时；每一步重新校验协议和凭据，不向下一来源转发鉴权。
    tokio::time::timeout(Duration::from_secs(30), download_from(&client, url))
        .await
        .map_err(|_| "订阅下载超时。".to_owned())?
}

async fn download_from(client: &reqwest::Client, mut url: Url) -> Result<Vec<u8>, String> {
    let mut visited = HashSet::new();
    for redirects in 0..=MAX_REDIRECTS {
        if !visited.insert(url.clone()) {
            return Err("订阅重定向次数过多。".into());
        }
        let mut response = client.get(url.clone()).send().await.map_err(|error| {
            if error.is_timeout() {
                "订阅下载超时。"
            } else {
                "无法下载订阅，请检查地址和网络连接。"
            }
        })?;
        if matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
            if redirects == MAX_REDIRECTS {
                return Err("订阅重定向次数过多。".into());
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or("订阅服务器返回无效重定向地址。")?;
            let target = url
                .join(location)
                .map_err(|_| "订阅服务器返回无效重定向地址。")?;
            url = validate_url(target.as_str()).map_err(|_| "订阅服务器返回无效重定向地址。")?;
            continue;
        }
        if !response.status().is_success() {
            return Err(format!(
                "订阅服务器返回 HTTP {}。",
                response.status().as_u16()
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_CONFIG_BYTES as u64)
        {
            return Err("订阅超过 8 MiB 大小限制。".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "订阅下载已中断。")? {
            if bytes.len().saturating_add(chunk.len()) > MAX_CONFIG_BYTES {
                return Err("订阅超过 8 MiB 大小限制。".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(bytes);
    }
    Err("订阅重定向次数过多。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
    };

    fn server(responses: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    let mut buffer = [0; 2048];
                    let count = stream.read(&mut buffer).unwrap();
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                }
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes).unwrap());
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (format!("http://{address}"), requests, task)
    }

    #[test]
    fn accepts_desktop_subscription_sources_and_rejects_credentials() {
        for url in [
            "http://example.com/config",
            "https://example.com:8443/x",
            "http://127.0.0.1:8090/x",
            "http://10.1.2.3/x",
            "https://[::1]:8080/x",
            "http://router.local/x",
            "https://example.com/config?token=private-value",
        ] {
            assert!(validate_url(url).is_ok());
        }
        for url in [
            "file:///tmp/config.yaml",
            "https://user:pass@example.com/x",
            "https://example.com/x#token",
            "http://example.com:0/x",
        ] {
            assert!(validate_url(url).is_err());
        }
    }

    #[test]
    fn user_agent_matches_mihomo_subscription_formats() {
        assert_eq!(user_agent(None), "clash.meta/v1.19.27");
        assert_eq!(user_agent(Some("-")), "clash.meta/v1.19.27");
        assert_eq!(user_agent(Some(" v1.20.0 ")), "clash.meta/v1.20.0");
        assert_eq!(
            user_agent(Some("v1\r\nAuthorization: secret")),
            "clash.meta/v1.19.27"
        );
    }

    #[tokio::test]
    async fn downloads_local_subscriptions_with_relative_redirect_and_core_user_agent() {
        let body = "proxies: []\nrules: []\n";
        let (url, requests, task) = server(vec![
            "HTTP/1.1 302 Found\r\nLocation: /config.yaml\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()),
        ]);
        let bytes = download_with_version(
            &format!("{url}/initial?token=private-value"),
            Some("v1.20.0"),
        )
        .await
        .unwrap();
        task.join().unwrap();
        assert_eq!(bytes, body.as_bytes());
        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("GET /initial?token=private-value "));
        assert!(requests[1].starts_with("GET /config.yaml "));
        assert!(requests.iter().all(|request| request
            .to_lowercase()
            .contains("user-agent: clash.meta/v1.20.0")));
        assert!(!requests[1].contains("private-value"));
    }

    #[tokio::test]
    async fn redirect_credentials_and_non_http_schemes_are_rejected_without_request() {
        for target in [
            "file:///tmp/config.yaml",
            "http://user:private-value@127.0.0.1/config",
        ] {
            let (url, _, task) = server(vec![format!("HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")]);
            let error = download(&url).await.unwrap_err();
            task.join().unwrap();
            assert_eq!(error, "订阅服务器返回无效重定向地址。");
            assert!(!error.contains("private-value"));
        }
    }

    #[tokio::test]
    async fn status_errors_and_content_size_limits_do_not_expose_request_tokens() {
        for (response, expected) in [
            (
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .into(),
                "订阅服务器返回 HTTP 401。",
            ),
            (
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    MAX_CONFIG_BYTES + 1
                ),
                "订阅超过 8 MiB 大小限制。",
            ),
        ] {
            let (url, _, task) = server(vec![response]);
            let error = download(&format!("{url}/private-value?token=private-value"))
                .await
                .unwrap_err();
            task.join().unwrap();
            assert_eq!(error, expected);
        }
    }

    #[tokio::test]
    async fn cross_origin_redirect_does_not_forward_source_tokens_or_authentication() {
        let (target, target_requests, target_task) = server(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\nproxies: []\n"
                .into(),
        ]);
        let (source, _, source_task) = server(vec![format!(
            "HTTP/1.1 302 Found\r\nLocation: {target}/config\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )]);
        assert_eq!(
            download(&format!("{source}/source?token=private-value"))
                .await
                .unwrap(),
            b"proxies: []\n"
        );
        source_task.join().unwrap();
        target_task.join().unwrap();
        let request = target_requests.lock().unwrap()[0].to_lowercase();
        assert!(!request.contains("private-value"));
        assert!(!request.contains("authorization:"));
        assert!(!request.contains("referer:"));
    }

    #[tokio::test]
    async fn chunked_body_without_content_length_is_still_bounded() {
        let payload = "a".repeat(MAX_CONFIG_BYTES + 1);
        let (url, _, task) = server(vec![format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{payload}\r\n0\r\n\r\n", payload.len()
        )]);
        assert_eq!(
            download(&url).await.unwrap_err(),
            "订阅超过 8 MiB 大小限制。"
        );
        task.join().unwrap();
    }

    #[tokio::test]
    async fn redirect_loops_stop_before_sending_the_same_url_again() {
        let (url, requests, task) = server(vec![
            "HTTP/1.1 302 Found\r\nLocation: /\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .into(),
        ]);
        assert_eq!(download(&url).await.unwrap_err(), "订阅重定向次数过多。");
        task.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 1);
    }
}
