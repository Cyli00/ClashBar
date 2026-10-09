use crate::config::MAX_CONFIG_BYTES;
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use url::{Host, Url};

pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
                || a == 0
                || a >= 240
                || (a == 100 && (64..=127).contains(&b))
                || (a == 198 && (b == 18 || b == 19))
                || (a == 192 && b == 0 && c == 0))
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(mapped));
            }
            let parts = ip.segments();
            // Only global unicast; exclude documentation, tunnelling and special-use space.
            (parts[0] & 0xe000) == 0x2000
                && parts[0] != 0x2002
                && !(parts[0] == 0x2001 && parts[1] < 0x0200)
                && !(parts[0] == 0x2001 && parts[1] == 0x0db8)
                && !(parts[0] == 0x3fff && parts[1] < 0x1000)
        }
    }
}

pub fn validate_url(input: &str) -> Result<Url, String> {
    if input.len() > 8192 {
        return Err("The subscription URL is too long.".into());
    }
    let url = Url::parse(input.trim()).map_err(|_| "Enter a valid HTTPS subscription URL.")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
    {
        return Err(
            "Subscriptions require HTTPS on port 443, without embedded credentials or fragments."
                .into(),
        );
    }
    match url.host().ok_or("The subscription URL needs a hostname.")? {
        Host::Ipv4(ip) if !public_ip(IpAddr::V4(ip)) => {
            return Err("Local and private subscription addresses are not allowed.".into())
        }
        Host::Ipv6(ip) if !public_ip(IpAddr::V6(ip)) => {
            return Err("Local and private subscription addresses are not allowed.".into())
        }
        Host::Domain(host)
            if !host.contains('.')
                || host.ends_with(".localhost")
                || host.ends_with(".local")
                || host.ends_with(".internal") =>
        {
            return Err("Use a public subscription hostname.".into())
        }
        _ => {}
    }
    Ok(url)
}

pub async fn download(input: &str) -> Result<Vec<u8>, String> {
    let url = validate_url(input)?;
    let hostname = url
        .host_str()
        .ok_or("Missing subscription hostname.")?
        .to_owned();
    let addresses: Vec<SocketAddr> = match url.host().unwrap() {
        Host::Ipv4(ip) => vec![SocketAddr::new(IpAddr::V4(ip), 443)],
        Host::Ipv6(ip) => vec![SocketAddr::new(IpAddr::V6(ip), 443)],
        Host::Domain(host) => {
            tokio::time::timeout(Duration::from_secs(8), tokio::net::lookup_host((host, 443)))
                .await
                .map_err(|_| "Subscription DNS lookup timed out.")?
                .map_err(|_| "Cannot resolve subscription hostname.")?
                .collect()
        }
    };
    if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
        return Err(
            "The subscription hostname resolves to a local, private, or reserved address.".into(),
        );
    }
    // Pin vetted DNS answers; do not let a second lookup bypass the address checks.
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(&hostname, &addresses)
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .get(url)
        .header("User-Agent", "ClashBar/0.1.0 mihomo")
        .send()
        .await
        .map_err(|_| "Cannot download the subscription. Check the URL and network connection.")?;
    if response.status().is_redirection() {
        return Err(
            "Subscription redirects are disabled. Enter the final HTTPS subscription URL.".into(),
        );
    }
    if !response.status().is_success() {
        return Err(format!(
            "Subscription server returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_CONFIG_BYTES as u64)
    {
        return Err("The subscription exceeds the 8 MiB limit.".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Subscription download was interrupted.")?
    {
        if bytes.len() + chunk.len() > MAX_CONFIG_BYTES {
            return Err("The subscription exceeds the 8 MiB limit.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsafe_urls_without_network() {
        for url in [
            "http://example.com/config",
            "https://127.0.0.1/x",
            "https://10.1.2.3/x",
            "https://[::1]/x",
            "https://user:pass@example.com/x",
            "https://example.com:8443/x",
            "https://router.local/x",
            "https://example.com/x#token",
        ] {
            assert!(validate_url(url).is_err(), "{url}");
        }
        assert!(validate_url("https://example.com/config?token=secret").is_ok());
    }
    #[test]
    fn rejects_reserved_and_mapped_addresses() {
        for ip in [
            "0.0.0.0",
            "100.64.0.1",
            "169.254.169.254",
            "192.0.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "2002:7f00:1::",
            "2001:db8::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        assert!(public_ip("1.1.1.1".parse().unwrap()));
        assert!(public_ip("2606:4700:4700::1111".parse().unwrap()));
    }
}
