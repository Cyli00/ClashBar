use crate::{
    controller::Controller,
    process::{self, Logs},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const SAMPLE_MAX_AGE: Duration = Duration::from_secs(5);

pub struct LogStream {
    task: tokio::task::JoinHandle<()>,
    pub logs: Logs,
    pub level: String,
    error: Arc<Mutex<Option<String>>>,
}

impl LogStream {
    pub fn start(controller: Controller, level: &str, retained: Option<Logs>) -> Self {
        let logs = retained.unwrap_or_else(process::logs);
        let error = Arc::new(Mutex::new(None));
        let output = logs.clone();
        let failure = error.clone();
        let selected_level = level.to_owned();
        let task = tokio::spawn(async move {
            if selected_level == "silent" {
                std::future::pending::<()>().await;
            }
            let mut delay = 1;
            loop {
                let mut connected = false;
                let result = controller
                    .receive_log_events(&output, &selected_level, || {
                        connected = true;
                        if let Ok(mut error) = failure.lock() {
                            *error = None;
                        }
                    })
                    .await;
                if let Err(message) = result {
                    if let Ok(mut error) = failure.lock() {
                        *error = Some(message);
                    }
                }
                if connected {
                    delay = 1;
                }
                tokio::time::sleep(Duration::from_secs(delay)).await;
                delay = (delay * 2).min(30);
            }
        });
        Self {
            task,
            logs,
            error,
            level: level.to_owned(),
        }
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|error| error.clone())
    }
}

impl Drop for LogStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrafficSample {
    #[serde(default)]
    pub up: u64,
    #[serde(default)]
    pub down: u64,
    #[serde(default, alias = "uptotal", alias = "uploadTotal")]
    pub up_total: Option<u64>,
    #[serde(default, alias = "downtotal", alias = "downloadTotal")]
    pub down_total: Option<u64>,
}

#[derive(Default)]
struct TelemetryState {
    traffic: Option<(Instant, TrafficSample)>,
    memory: Option<(Instant, u64)>,
    connections: Option<(Instant, Value)>,
    errors: BTreeMap<&'static str, String>,
}

impl TelemetryState {
    fn record(&mut self, endpoint: &'static str, bytes: &[u8]) -> Result<(), String> {
        let value: Value = serde_json::from_slice(bytes).map_err(|_| "监控流消息格式无效。")?;
        let now = Instant::now();
        match endpoint {
            "traffic" => {
                if value.get("up").is_none() && value.get("down").is_none() {
                    return Err("流量监控消息缺少速率。".into());
                }
                let sample = serde_json::from_value(value).map_err(|_| "流量监控消息格式无效。")?;
                self.traffic = Some((now, sample));
            }
            "memory" => {
                let memory = value
                    .get("inuse")
                    .and_then(Value::as_u64)
                    .ok_or("内存监控消息格式无效。")?;
                self.memory = Some((now, memory));
            }
            "connections" => {
                if !value.get("connections").is_some_and(Value::is_array)
                    || !value.get("uploadTotal").is_some_and(Value::is_u64)
                    || !value.get("downloadTotal").is_some_and(Value::is_u64)
                {
                    return Err("连接监控消息格式无效。".into());
                }
                self.connections = Some((now, value));
            }
            _ => return Err("监控流类型无效。".into()),
        }
        self.errors.remove(endpoint);
        Ok(())
    }
}

pub struct TelemetryStreams {
    tasks: Vec<tokio::task::JoinHandle<()>>,
    state: Arc<Mutex<TelemetryState>>,
}

impl TelemetryStreams {
    pub fn start(controller: Controller) -> Self {
        let state = Arc::new(Mutex::new(TelemetryState::default()));
        let mut tasks = Vec::new();
        for endpoint in ["traffic", "memory", "connections"] {
            let controller = controller.clone();
            let output = state.clone();
            tasks.push(tokio::spawn(async move {
                let mut delay = 1;
                loop {
                    let mut connected = false;
                    let query: &[(&str, &str)] = if endpoint == "connections" {
                        &[("interval", "1000")]
                    } else {
                        &[]
                    };
                    let maximum = if endpoint == "connections" {
                        16 * 1024 * 1024
                    } else {
                        4096
                    };
                    let result = controller
                        .receive_events(
                            endpoint,
                            query,
                            maximum,
                            || {
                                connected = true;
                            },
                            |bytes| {
                                output
                                    .lock()
                                    .map_err(|_| "监控状态不可用。")?
                                    .record(endpoint, bytes)
                            },
                        )
                        .await;
                    if let Err(error) = result {
                        if let Ok(mut state) = output.lock() {
                            state.errors.insert(endpoint, error);
                        }
                    }
                    if connected {
                        delay = 1;
                    }
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    delay = (delay * 2).min(30);
                }
            }));
        }
        Self { tasks, state }
    }

    pub fn apply_to_snapshot(&self, snapshot: &mut crate::controller::Snapshot) {
        let Ok(state) = self.state.lock() else {
            return;
        };
        // 断流时最多使用 5 秒内的观测，随后回到 HTTP 快照，避免显示长期过期数值。
        let current = |sampled: &Instant| sampled.elapsed() <= SAMPLE_MAX_AGE;
        if let Some((_, traffic)) = state.traffic.as_ref().filter(|(at, _)| current(at)) {
            snapshot.traffic = Some(traffic.clone());
        }
        if let Some((_, memory)) = state.memory.as_ref().filter(|(at, _)| current(at)) {
            snapshot.memory = Some(*memory);
        }
        if let Some((_, connections)) = state.connections.as_ref().filter(|(at, _)| current(at)) {
            snapshot.connections = connections.clone();
        }
    }

    pub fn traffic(&self) -> Option<TrafficSample> {
        self.state
            .lock()
            .ok()?
            .traffic
            .as_ref()
            .filter(|(at, _)| at.elapsed() <= SAMPLE_MAX_AGE)
            .map(|(_, sample)| sample.clone())
    }

    pub fn errors(&self) -> BTreeMap<String, String> {
        self.state
            .lock()
            .map(|state| {
                state
                    .errors
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Drop for TelemetryStreams {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use tokio_tungstenite::tungstenite::{
        handshake::server::{Request, Response},
        Message,
    };

    fn empty_snapshot() -> crate::controller::Snapshot {
        crate::controller::Snapshot {
            proxies: json!({}),
            configs: json!({}),
            rules: json!({}),
            connections: json!({"connections":[],"uploadTotal":1,"downloadTotal":2}),
            providers: json!({}),
            rule_providers: json!({}),
            memory: Some(7),
            traffic: None,
        }
    }

    #[test]
    fn telemetry_rejects_invalid_payloads_and_does_not_overlay_stale_values() {
        let mut state = TelemetryState::default();
        for (endpoint, body) in [
            ("traffic", "{}"),
            ("traffic", r#"{"up":-1}"#),
            ("memory", r#"{"inuse":"bad"}"#),
            ("connections", r#"{"connections":null}"#),
        ] {
            assert!(state.record(endpoint, body.as_bytes()).is_err());
        }
        state
            .record(
                "traffic",
                br#"{"up":123,"down":456,"uptotal":1000,"downloadTotal":2000}"#,
            )
            .unwrap();
        state.record("memory", br#"{"inuse":9000}"#).unwrap();
        let streams = TelemetryStreams {
            tasks: vec![],
            state: Arc::new(Mutex::new(state)),
        };
        let mut snapshot = empty_snapshot();
        streams.apply_to_snapshot(&mut snapshot);
        assert_eq!(snapshot.traffic.as_ref().unwrap().up_total, Some(1000));
        assert_eq!(snapshot.traffic.as_ref().unwrap().down_total, Some(2000));
        assert_eq!(snapshot.memory, Some(9000));
        {
            let mut state = streams.state.lock().unwrap();
            state.traffic.as_mut().unwrap().0 = Instant::now() - Duration::from_secs(6);
            state.memory.as_mut().unwrap().0 = Instant::now() - Duration::from_secs(6);
        }
        let mut snapshot = empty_snapshot();
        streams.apply_to_snapshot(&mut snapshot);
        assert!(snapshot.traffic.is_none());
        assert_eq!(snapshot.memory, Some(7));
        assert!(streams.traffic().is_none());
    }

    #[tokio::test]
    // tungstenite 的握手回调固定使用未装箱的 HTTP 错误响应。
    #[allow(clippy::result_large_err)]
    async fn three_websocket_streams_publish_telemetry_and_drop_closes_every_connection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let controller = Controller::new(
            listener.local_addr().unwrap().port(),
            "stream-secret".into(),
        )
        .unwrap();
        let server = tokio::spawn(async move {
            let mut clients = Vec::new();
            for _ in 0..3 {
                let (socket, _) = listener.accept().await.unwrap();
                clients.push(tokio::spawn(async move {
                    let mut path = String::new();
                    let mut socket = tokio_tungstenite::accept_hdr_async(socket, |request: &Request, response: Response| {
                        assert_eq!(request.headers()["authorization"], "Bearer stream-secret");
                        path = request.uri().to_string();
                        Ok(response)
                    }).await.unwrap();
                    let payload = match path.as_str() {
                        "/traffic?" | "/traffic" => json!({"up":123,"down":456}),
                        "/memory?" | "/memory" => json!({"inuse":789}),
                        "/connections?interval=1000" => json!({"connections":[{"id":"live"}],"uploadTotal":1000,"downloadTotal":2000}),
                        other => panic!("未知监控路径：{other}"),
                    };
                    socket.send(Message::Text(payload.to_string().into())).await.unwrap();
                    let result = socket.next().await;
                    assert!(result.is_none() || result.is_some_and(|result| result.is_err() || matches!(result, Ok(Message::Close(_)))));
                }));
            }
            for client in clients {
                client.await.unwrap();
            }
        });
        let streams = TelemetryStreams::start(controller);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let mut snapshot = empty_snapshot();
                streams.apply_to_snapshot(&mut snapshot);
                if let Some(traffic) = snapshot.traffic.filter(|_| {
                    snapshot.memory == Some(789)
                        && snapshot.connections["connections"][0]["id"] == "live"
                }) {
                    assert_eq!(traffic.up, 123);
                    assert_eq!(snapshot.connections["uploadTotal"], 1000);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        drop(streams);
        tokio::time::timeout(Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
    }
}
