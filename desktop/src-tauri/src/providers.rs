use crate::{
    controller::Controller,
    process::{self, Logs},
};
use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRefreshStatus {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub failed: usize,
    pub error: Option<String>,
}

pub struct ProviderRefresh {
    task: tokio::task::JoinHandle<()>,
    status: Arc<Mutex<ProviderRefreshStatus>>,
}

impl ProviderRefresh {
    pub fn start(controller: Controller, logs: Logs) -> Self {
        let status = Arc::new(Mutex::new(ProviderRefreshStatus {
            running: true,
            ..Default::default()
        }));
        let progress = status.clone();
        let task = tokio::spawn(async move {
            let (proxies, rules) =
                tokio::join!(controller.proxy_providers(), controller.rule_providers());
            let mut inventory_errors = 0;
            let mut resources = Vec::new();
            for (is_rule, result) in [(false, proxies), (true, rules)] {
                match result {
                    Ok(value) => {
                        let mut names: Vec<_> = value["providers"]
                            .as_object()
                            .expect("validated inventory")
                            .keys()
                            .cloned()
                            .collect();
                        names.sort();
                        resources.extend(names.into_iter().map(|name| (is_rule, name)));
                    }
                    Err(_) => {
                        inventory_errors += 1;
                    }
                }
            }
            if let Ok(mut status) = progress.lock() {
                status.total = resources.len();
                if inventory_errors > 0 {
                    status.error = Some(format!("有 {inventory_errors} 类提供者列表读取失败。"));
                }
            }
            for (is_rule, name) in resources {
                let result = if is_rule {
                    controller.update_rule_provider(&name).await
                } else {
                    controller.update_provider(&name).await
                };
                if let Ok(mut status) = progress.lock() {
                    status.done += 1;
                    if result.is_err() {
                        status.failed += 1;
                    }
                }
            }
            if let Ok(mut status) = progress.lock() {
                status.running = false;
                if status.failed > 0 {
                    status.error = Some(format!(
                        "提供者更新完成 {}/{}；{} 项失败，可手动重试。",
                        status.done - status.failed,
                        status.total,
                        status.failed
                    ));
                }
                if let Some(error) = &status.error {
                    process::append(&logs, error);
                } else if status.total > 0 {
                    process::append(
                        &logs,
                        format!("提供者更新完成 {}/{}。", status.done, status.total),
                    );
                }
            }
        });
        Self { task, status }
    }

    pub fn status(&self) -> ProviderRefreshStatus {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default()
    }
}

impl Drop for ProviderRefresh {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    #[tokio::test]
    async fn refresh_updates_every_kind_and_continues_after_one_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let controller =
            Controller::new(listener.local_addr().unwrap().port(), "test".into()).unwrap();
        let server = thread::spawn(move || {
            let mut updates = Vec::new();
            for _ in 0..5 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                    let mut chunk = [0; 2048];
                    let count = socket.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                }
                let request = String::from_utf8(bytes).unwrap();
                let line = request.lines().next().unwrap();
                let (status, body) = if line == "GET /providers/proxies HTTP/1.1" {
                    (200, r#"{"providers":{"a":{},"b":{}}}"#)
                } else if line == "GET /providers/rules HTTP/1.1" {
                    (200, r#"{"providers":{"rules":{}}}"#)
                } else {
                    updates.push(line.to_owned());
                    (if line.contains("/a ") { 500 } else { 204 }, "")
                };
                write!(socket, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            updates
        });
        let refresh = ProviderRefresh::start(controller, process::logs());
        tokio::time::timeout(Duration::from_secs(4), async {
            while refresh.status().running {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let status = refresh.status();
        assert_eq!((status.done, status.total, status.failed), (3, 3, 1));
        assert!(status.error.unwrap().contains("2/3"));
        assert_eq!(
            server.join().unwrap(),
            [
                "PUT /providers/proxies/a HTTP/1.1",
                "PUT /providers/proxies/b HTTP/1.1",
                "PUT /providers/rules/rules HTTP/1.1"
            ]
        );
    }
}
