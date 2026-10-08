use reqwest::{Client, Method};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
use url::Url;

const MAX_RESPONSE: usize = 16 * 1024 * 1024;

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
}

impl Controller {
    pub fn new(port: u16, secret: String) -> Result<Self, String> {
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            base: Url::parse(&format!("http://127.0.0.1:{port}/")).map_err(|e| e.to_string())?,
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
            .bearer_auth(&self.secret)
            .query(query);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| "The local core is not responding.")?;
        if !response.status().is_success() {
            return Err(format!(
                "The local core returned HTTP {}.",
                response.status().as_u16()
            ));
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE as u64)
        {
            return Err("The core response is too large.".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "The local core response was interrupted.")?
        {
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
            .map(str::to_owned)
            .ok_or("The executable did not expose a compatible mihomo controller.".into())
    }

    pub async fn snapshot(&self) -> Result<Snapshot, String> {
        let (proxies, configs, rules, connections, providers) = tokio::try_join!(
            self.request(Method::GET, &["proxies"], None, &[]),
            self.request(Method::GET, &["configs"], None, &[]),
            self.request(Method::GET, &["rules"], None, &[]),
            self.request(Method::GET, &["connections"], None, &[]),
            self.request(Method::GET, &["providers", "proxies"], None, &[])
        )?;
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
        // /configs may contain the API secret. Return only the field used by the UI.
        Ok(Snapshot {
            proxies,
            configs: json!({"mode": mode}),
            rules,
            connections,
            providers,
        })
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
    pub async fn test_delay(&self, name: &str) -> Result<Value, String> {
        check_name(name)?;
        let result = self
            .request(
                Method::GET,
                &["proxies", name, "delay"],
                None,
                &[
                    ("timeout", "5000"),
                    ("url", "https://www.gstatic.com/generate_204"),
                ],
            )
            .await?;
        let delay = result
            .get("delay")
            .and_then(Value::as_u64)
            .ok_or("The proxy latency test did not return a delay.")?;
        Ok(json!({"delay": delay}))
    }
    pub async fn close_connection(&self, id: &str) -> Result<(), String> {
        check_name(id)?;
        self.request(Method::DELETE, &["connections", id], None, &[])
            .await
            .map(|_| ())
    }
    pub async fn update_provider(&self, name: &str) -> Result<(), String> {
        check_name(name)?;
        self.request(Method::PUT, &["providers", "proxies", name], None, &[])
            .await
            .map(|_| ())
    }
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
    #[test]
    fn proxy_names_are_one_encoded_path_segment() {
        let controller = Controller::new(19090, "hidden".into()).unwrap();
        let url = controller.url(&["proxies", "HK / fast?#", "delay"]);
        assert_eq!(url.path(), "/proxies/HK%20%2F%20fast%3F%23/delay");
        assert!(url.query().is_none());
        assert!(check_name(".").is_err());
        assert!(check_name("..").is_err());
    }
}
