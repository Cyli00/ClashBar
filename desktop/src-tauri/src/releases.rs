use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, time::Duration};
use url::Url;

pub const RELEASE_INDEX_URL: &str = "https://github.com/Cyli00/ClashBar/releases";
const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/Cyli00/ClashBar/releases/latest";
const MAX_RELEASE_RESPONSE: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppReleaseInfo {
    pub current_version: String,
    pub tag_name: String,
    pub display_version: String,
    pub name: Option<String>,
    pub release_url: String,
    pub is_draft: bool,
    pub is_prerelease: bool,
    pub update_available: bool,
}

#[derive(Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    name: Option<String>,
    html_url: String,
    draft: bool,
    prerelease: bool,
}

pub fn trusted_release_url(input: &str) -> Result<Url, String> {
    if input.len() > 2048 || input.chars().any(char::is_control) {
        return Err("发布页地址无效。".into());
    }
    let url = Url::parse(input).map_err(|_| "发布页地址无效。")?;
    let path = url.path();
    let release_path = "/Cyli00/ClashBar/releases";
    let tag_path = format!("{release_path}/tag/");
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !(path == release_path
            || path == format!("{release_path}/")
            || path
                .strip_prefix(&tag_path)
                .is_some_and(|tag| !tag.is_empty()))
    {
        return Err("只允许打开 ClashBar 官方 GitHub 发布页。".into());
    }
    Ok(url)
}

pub async fn fetch_latest_release(current_version: &str) -> Result<AppReleaseInfo, String> {
    fetch_latest_release_at(
        current_version,
        Url::parse(LATEST_RELEASE_URL).expect("固定发布接口地址"),
    )
    .await
}

async fn fetch_latest_release_at(
    current_version: &str,
    endpoint: Url,
) -> Result<AppReleaseInfo, String> {
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "无法创建版本检查连接。")?;
    let mut response = client
        .get(endpoint)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", format!("ClashBar/{current_version}"))
        .send()
        .await
        .map_err(|_| "无法检查 ClashBar 更新，请检查网络连接。")?;
    if !response.status().is_success() {
        return Err(format!(
            "版本检查返回 HTTP {}。",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RELEASE_RESPONSE as u64)
    {
        return Err("版本检查响应超过 1 MiB。".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "版本检查响应中断。")? {
        if bytes.len() + chunk.len() > MAX_RELEASE_RESPONSE {
            return Err("版本检查响应超过 1 MiB。".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let release: ReleaseResponse =
        serde_json::from_slice(&bytes).map_err(|_| "版本检查返回了无效的发布信息。")?;
    if release.tag_name.trim().is_empty() || release.tag_name.len() > 200 {
        return Err("发布版本号无效。".into());
    }
    let release_url = trusted_release_url(&release.html_url)?;
    let update_available = !release.draft
        && !release.prerelease
        && is_newer_release(&release.tag_name, current_version);
    Ok(AppReleaseInfo {
        current_version: current_version.to_owned(),
        display_version: normalized_display_version(&release.tag_name),
        tag_name: release.tag_name,
        name: release.name,
        release_url: release_url.to_string(),
        is_draft: release.draft,
        is_prerelease: release.prerelease,
        update_available,
    })
}

fn version_components(input: &str) -> Option<Vec<u64>> {
    let input = input.trim().split('+').next()?.split('-').next()?;
    let index = input.find(|character: char| character.is_ascii_digit())?;
    let parts: Vec<u64> = input[index..]
        .split('.')
        .filter_map(|part| {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect();
    (!parts.is_empty()).then_some(parts)
}

pub fn normalized_display_version(input: &str) -> String {
    version_components(input)
        .map(|parts| {
            parts
                .into_iter()
                .map(|part| part.to_string())
                .collect::<Vec<_>>()
                .join(".")
        })
        .unwrap_or_else(|| input.trim().to_owned())
}

pub fn is_newer_release(release: &str, current: &str) -> bool {
    if let (Some(release), Some(current)) =
        (version_components(release), version_components(current))
    {
        for index in 0..release.len().max(current.len()) {
            match release
                .get(index)
                .unwrap_or(&0)
                .cmp(current.get(index).unwrap_or(&0))
            {
                Ordering::Equal => {}
                ordering => return ordering == Ordering::Greater,
            }
        }
        return false;
    }
    numeric_compare(
        &normalized_display_version(release),
        &normalized_display_version(current),
    ) == Ordering::Greater
}

fn numeric_compare(left: &str, right: &str) -> Ordering {
    let mut left = left.chars().peekable();
    let mut right = right.chars().peekable();
    loop {
        let ordering = match (left.peek(), right.peek()) {
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let a: String =
                    std::iter::from_fn(|| left.next_if(|character| character.is_ascii_digit()))
                        .collect();
                let b: String =
                    std::iter::from_fn(|| right.next_if(|character| character.is_ascii_digit()))
                        .collect();
                let a = a.trim_start_matches('0');
                let b = b.trim_start_matches('0');
                a.len().cmp(&b.len()).then_with(|| a.cmp(b))
            }
            (Some(_), Some(_)) => left.next().cmp(&right.next()),
            (a, b) => return a.cmp(&b),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread::{self, JoinHandle},
        time::Instant,
    };

    fn release_fixture(
        status: u16,
        body: String,
        extra_headers: &str,
    ) -> (Url, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint =
            Url::parse(&format!("http://{}/latest", listener.local_addr().unwrap())).unwrap();
        listener.set_nonblocking(true).unwrap();
        let extra_headers = extra_headers.to_owned();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "未收到版本检查请求");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 1024];
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0 && bytes.len() < 8192);
                bytes.extend_from_slice(&chunk[..count]);
                if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                    break;
                }
            }
            let header = format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).unwrap();
            // 拒绝非成功状态时，客户端允许在响应体传输完成前断开。
            let _ = stream.write_all(body.as_bytes());
            String::from_utf8(bytes).unwrap()
        });
        (endpoint, server)
    }

    fn release_response(draft: bool, prerelease: bool) -> String {
        json!({
            "tag_name": "v1.10.2", "name": "ClashBar 1.10.2",
            "html_url": "https://github.com/Cyli00/ClashBar/releases/tag/v1.10.2",
            "draft": draft, "prerelease": prerelease
        })
        .to_string()
    }

    #[test]
    fn version_comparison_matches_legacy_numeric_components() {
        assert!(is_newer_release("v1.10.2", "1.9.9"));
        assert!(is_newer_release("v1.2.1", "1.2"));
        assert!(!is_newer_release("v1.2.0", "1.2"));
        assert!(!is_newer_release("v1.2.0-beta+build.7", "1.2.0"));
        assert!(!is_newer_release("v1.1.9", "1.2"));
        assert_eq!(normalized_display_version("  v01.02.3+abc  "), "1.2.3");
        assert_eq!(normalized_display_version("development"), "development");
        assert!(is_newer_release("zzz", "development"));
        assert_eq!(numeric_compare("build10", "build2"), Ordering::Greater);
    }

    #[test]
    fn release_links_are_limited_to_the_official_repository() {
        assert!(trusted_release_url(RELEASE_INDEX_URL).is_ok());
        assert!(
            trusted_release_url("https://github.com/Cyli00/ClashBar/releases/tag/v1.2.3").is_ok()
        );
        for input in [
            "http://github.com/Cyli00/ClashBar/releases",
            "https://github.com.evil.test/Cyli00/ClashBar/releases",
            "https://github.com:8443/Cyli00/ClashBar/releases",
            "https://user:password@github.com/Cyli00/ClashBar/releases",
            "https://github.com/another/project/releases",
            "https://github.com/Cyli00/ClashBar/releases/tag/",
            "https://github.com/Cyli00/ClashBar/releases?redirect=elsewhere",
            "https://github.com/Cyli00/ClashBar/releases#secret",
            "file:///C:/Windows/System32/cmd.exe",
        ] {
            assert!(trusted_release_url(input).is_err(), "{input}");
        }
    }

    #[tokio::test]
    async fn release_check_uses_public_headers_and_reports_new_stable_version() {
        let (endpoint, server) = release_fixture(200, release_response(false, false), "");
        let release = fetch_latest_release_at("1.9.0", endpoint).await.unwrap();
        assert!(release.update_available);
        assert_eq!(release.display_version, "1.10.2");
        assert_eq!(release.current_version, "1.9.0");
        let request = server.join().unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /latest http/1.1\r\n"));
        assert!(request.contains("accept: application/vnd.github+json\r\n"));
        assert!(request.contains("x-github-api-version: 2022-11-28\r\n"));
        assert!(request.contains("user-agent: clashbar/1.9.0\r\n"));
        assert!(!request.contains("authorization:"));
        assert!(!request.contains("cookie:"));
    }

    #[tokio::test]
    async fn draft_prerelease_and_existing_versions_do_not_show_update() {
        for (draft, prerelease, current) in [
            (true, false, "1.0.0"),
            (false, true, "1.0.0"),
            (false, false, "1.10.2"),
            (false, false, "2.0.0"),
        ] {
            let (endpoint, server) = release_fixture(200, release_response(draft, prerelease), "");
            let release = fetch_latest_release_at(current, endpoint).await.unwrap();
            assert!(!release.update_available);
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn release_errors_do_not_return_server_body_or_follow_redirects() {
        for (status, body, headers) in [
            (403, "fixture-private-server-details".to_owned(), ""),
            (
                302,
                String::new(),
                "Location: http://127.0.0.1:1/must-not-follow\r\n",
            ),
            (200, "invalid JSON".to_owned(), ""),
            (
                200,
                release_response(false, false).replace("github.com", "evil.test"),
                "",
            ),
        ] {
            let (endpoint, server) = release_fixture(status, body, headers);
            let error = fetch_latest_release_at("1.0.0", endpoint)
                .await
                .unwrap_err();
            assert!(!error.contains("fixture-private-server-details"));
            if status != 200 {
                assert!(error.contains(&status.to_string()));
            }
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn oversized_release_response_is_rejected_before_decoding() {
        let (endpoint, server) = release_fixture(200, " ".repeat(MAX_RELEASE_RESPONSE + 1), "");
        let error = fetch_latest_release_at("1.0.0", endpoint)
            .await
            .unwrap_err();
        assert_eq!(error, "版本检查响应超过 1 MiB。");
        server.join().unwrap();
    }
}
