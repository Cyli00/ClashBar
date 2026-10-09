use base64::{engine::general_purpose::STANDARD, Engine as _};
use quick_xml::{events::Event, Reader};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use url::Url;

const MAX_ICON_BYTES: usize = 2 * 1024 * 1024;
const CACHE_LIFETIME: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone)]
pub struct IconCache {
    directory: PathBuf,
}

impl IconCache {
    pub fn new(directory: &Path) -> Self {
        Self {
            directory: directory.to_owned(),
        }
    }

    pub async fn get(&self, source: &str) -> Result<String, String> {
        let mut url = icon_url(source)?;
        let key = format!("{:x}.icon", Sha256::digest(url.as_str().as_bytes()));
        let path = self.directory.join(key);
        if let Ok(metadata) = fs::metadata(&path) {
            if metadata.len() <= MAX_ICON_BYTES as u64
                && metadata
                    .modified()
                    .ok()
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .is_some_and(|age| age < CACHE_LIFETIME)
            {
                let mut bytes = Vec::new();
                if fs::File::open(&path)
                    .and_then(|file| {
                        file.take((MAX_ICON_BYTES + 1) as u64)
                            .read_to_end(&mut bytes)
                    })
                    .is_ok()
                {
                    if let Ok(uri) = data_uri(&bytes) {
                        return Ok(uri);
                    }
                }
            }
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| "无法创建图标下载连接。")?;
        let bytes = tokio::time::timeout(Duration::from_secs(15), async {
            for redirects in 0..=5 {
                let mut response = client
                    .get(url.clone())
                    .send()
                    .await
                    .map_err(|_| "无法下载代理组图标。")?;
                if response.status().is_redirection() {
                    if redirects == 5 {
                        return Err("代理组图标重定向次数过多。".to_owned());
                    }
                    let location = response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|value| value.to_str().ok())
                        .ok_or("代理组图标重定向无效。")?;
                    let target = url.join(location).map_err(|_| "代理组图标重定向无效。")?;
                    url = icon_url(target.as_str())?;
                    continue;
                }
                if !response.status().is_success() {
                    return Err(format!(
                        "代理组图标返回 HTTP {}。",
                        response.status().as_u16()
                    ));
                }
                if response
                    .content_length()
                    .is_some_and(|length| length > MAX_ICON_BYTES as u64)
                {
                    return Err("代理组图标超过 2 MiB。".into());
                }
                let mut bytes = Vec::new();
                while let Some(chunk) =
                    response.chunk().await.map_err(|_| "代理组图标下载中断。")?
                {
                    if bytes.len() + chunk.len() > MAX_ICON_BYTES {
                        return Err("代理组图标超过 2 MiB。".into());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                return Ok(bytes);
            }
            Err("无法下载代理组图标。".into())
        })
        .await
        .map_err(|_| "代理组图标下载超时。")??;
        let uri = data_uri(&bytes)?;
        if fs::create_dir_all(&self.directory).is_ok() {
            let _ = crate::config::atomic_write(&path, &bytes);
        }
        Ok(uri)
    }
}

fn icon_url(input: &str) -> Result<Url, String> {
    crate::subscription::validate_url(input)
        .map_err(|_| "代理组图标需要有效的 HTTP 或 HTTPS 地址。".into())
}

fn data_uri(bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() || bytes.len() > MAX_ICON_BYTES {
        return Err("代理组图标大小无效。".into());
    }
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        validate_svg(bytes)?;
        "image/svg+xml"
    };
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn validate_svg(bytes: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "代理组图标格式不受支持。")?;
    let mut reader = Reader::from_str(text);
    let mut root = false;
    loop {
        match reader.read_event().map_err(|_| "SVG 图标格式无效。")? {
            Event::Start(element) | Event::Empty(element) => {
                let name =
                    String::from_utf8_lossy(element.local_name().as_ref()).to_ascii_lowercase();
                if !root {
                    if name != "svg" {
                        return Err("代理组图标格式不受支持。".into());
                    }
                    root = true;
                }
                if [
                    "script",
                    "foreignobject",
                    "iframe",
                    "object",
                    "embed",
                    "audio",
                    "video",
                    "image",
                    "animate",
                    "animatetransform",
                    "set",
                ]
                .contains(&name.as_str())
                {
                    return Err("SVG 图标包含不支持的活动内容。".into());
                }
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|_| "SVG 图标属性无效。")?;
                    let key = String::from_utf8_lossy(attribute.key.local_name().as_ref())
                        .to_ascii_lowercase();
                    let value = attribute
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map_err(|_| "SVG 图标属性无效。")?;
                    if key.starts_with("on")
                        || (key == "href" && !value.starts_with('#'))
                        || !safe_css(&value)
                    {
                        return Err("SVG 图标不能执行脚本或引用外部内容。".into());
                    }
                }
            }
            Event::DocType(_) | Event::PI(_) => {
                return Err("SVG 图标不能包含外部文档或处理指令。".into())
            }
            Event::Text(text) => {
                if !safe_css(&text.decode().map_err(|_| "SVG 图标内容无效。")?) {
                    return Err("SVG 图标不能引用外部样式。".into());
                }
            }
            Event::CData(_) => return Err("SVG 图标不接受内嵌活动内容。".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if root {
        Ok(())
    } else {
        Err("代理组图标格式不受支持。".into())
    }
}

fn safe_css(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    if value.contains('@') || value.contains('\\') {
        return false;
    }
    let mut rest = value.as_str();
    while let Some((_, tail)) = rest.split_once("url(") {
        let Some((target, tail)) = tail.split_once(')') else {
            return false;
        };
        if !target.trim().trim_matches(['\'', '"']).starts_with('#') {
            return false;
        }
        rest = tail;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn svg_allows_static_shapes_but_rejects_active_or_external_content() {
        assert!(data_uri(br##"<svg xmlns="http://www.w3.org/2000/svg"><defs><linearGradient id="a"/></defs><path fill="url(#a)" d="M0 0h24v24H0z"/></svg>"##).unwrap().starts_with("data:image/svg+xml;base64,"));
        for svg in [
            r#"<html>not an icon</html>"#,
            r#"<svg onload="alert(1)"/>"#,
            r#"<svg><script>alert(1)</script></svg>"#,
            r#"<svg><use href="https://remote/a"/></svg>"#,
            r#"<svg><style>@import 'https://remote/a';</style></svg>"#,
            r#"<!DOCTYPE svg SYSTEM "file:///a"><svg/>"#,
            r#"<svg><path fill="url(https://remote/a)"/></svg>"#,
        ] {
            assert!(data_uri(svg.as_bytes()).is_err(), "{svg}");
        }
    }

    #[tokio::test]
    async fn icon_download_is_cached_and_never_exposes_source_url() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let source = format!(
            "http://{}/icon?private=token",
            listener.local_addr().unwrap()
        );
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = [0; 4096];
            assert!(stream.read(&mut bytes).unwrap() > 0);
            let body = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><path d="M0 0h16v16z"/></svg>"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let cache = IconCache::new(directory.path());
        let first = cache.get(&source).await.unwrap();
        server.join().unwrap();
        assert_eq!(cache.get(&source).await.unwrap(), first);
        assert!(!first.contains("token"));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        assert!(cache
            .get("file:///secret")
            .await
            .unwrap_err()
            .contains("HTTP"));
    }
}
