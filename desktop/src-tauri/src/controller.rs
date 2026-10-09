use reqwest::{Client, Method};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
use url::Url;

const MAX_RESPONSE: usize = 16 * 1024 * 1024;
const MAX_MEMORY_SAMPLE: usize = 4096;
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
    /// Bytes used by the core. Older cores may not expose this metric.
    pub memory: Option<u64>,
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
        Ok(Snapshot {
            proxies,
            configs: json!({"mode": mode, "log-level": log_level}),
            rules,
            connections,
            providers,
            memory,
        })
    }

    async fn memory_usage(&self) -> Option<u64> {
        // /memory is newline-delimited JSON that normally never reaches EOF.
        // Read one bounded sample and drop the stream; unsupported/slow/malformed
        // telemetry must not break the otherwise usable dashboard snapshot.
        let mut response = self
            .client
            .get(self.url(&["memory"]))
            .bearer_auth(&self.secret)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .ok()?;
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
                    "external-controller": "127.0.0.1:9090"
                }),
            ),
            Response::new("GET", "/rules", 200, json!({"rules": []})),
            Response::new("GET", "/connections", 200, connections),
            Response::new("GET", "/providers/proxies", 200, json!({"providers": {}})),
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
            json!({"mode": "rule", "log-level": "info"})
        );
        assert!(!serde_json::to_string(&snapshot)
            .unwrap()
            .contains("must-not-cross-ipc"));
        assert_eq!(server.join().unwrap().len(), 5); // No /memory stream request.
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
}
