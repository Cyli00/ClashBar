//! Compiled by engine unit tests into an isolated, dependency-free lifecycle fixture.
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let path = &arguments[arguments.iter().position(|arg| arg == "-f").unwrap() + 1];
    let config = fs::read_to_string(path).unwrap();
    if arguments.iter().any(|arg| arg == "-t") {
        if config.contains("fixture-invalid: true") {
            std::process::exit(3);
        }
        return;
    }
    if config.contains("fixture-startup-exit: true") {
        std::process::exit(4);
    }
    let address = config
        .lines()
        .find_map(|line| line.strip_prefix("external-controller: "))
        .unwrap()
        .trim_matches(['\'', '"']);
    let listener = TcpListener::bind(address).unwrap();
    let mut settings = ["allow-lan", "ipv6", "tcp-concurrent"].map(|key| {
        let enabled = config.lines().any(|line| line == format!("{key}: true"));
        (key, enabled)
    });
    let mut mode = if config.contains("mode: direct") { "direct" } else if config.contains("mode: global") { "global" } else { "rule" };
    let mut log_level = "info";
    for level in ["debug", "info", "warning", "error", "silent"] {
        if config.lines().any(|line| line == format!("log-level: {level}")) { log_level = level; }
    }
    let mut tun_enabled = config.lines().any(|line| line.trim() == "enable: true");
    let mut tun_stack = "mixed";
    for stack in ["system", "gvisor", "mixed", "mips"] {
        if config.lines().any(|line| line.trim() == format!("stack: {stack}")) { tun_stack = stack; }
    }
    let remote = config.contains("fixture-remote: true");
    let mut ports = ["port", "socks-port", "mixed-port", "redir-port", "tproxy-port"].map(|key| {
        (key, config.lines().find_map(|line| line.strip_prefix(&format!("{key}: ")).and_then(|value| value.parse::<u16>().ok())).unwrap_or(0))
    });
    for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let complete = loop {
            let mut chunk = [0u8; 4096];
            let size = match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break false,
                Ok(size) => size,
            };
            assert!(request.len() + size <= 16384);
            request.extend_from_slice(&chunk[..size]);
            if let Some(index) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&request[..index]);
                let length = header.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().unwrap())
                }).unwrap_or(0);
                if request.len() >= index + 4 + length { break true; }
            }
        };
        if !complete { continue; }
        let request = String::from_utf8(request).unwrap();
        let mut status = 200;
        let body = if request.starts_with("PATCH /configs ") {
            if config.contains("fixture-reject-tcp: true") && request.contains("\"tcp-concurrent\":true") {
                status = 400;
            } else {
                for (key, enabled) in &mut settings {
                    if request.contains(&format!("\"{key}\":true")) { *enabled = true; }
                    if request.contains(&format!("\"{key}\":false")) { *enabled = false; }
                }
                for value in ["rule", "global", "direct"] {
                    if request.contains(&format!("\"mode\":\"{value}\"")) { mode = value; }
                }
                for value in ["debug", "info", "warning", "error", "silent"] {
                    if request.contains(&format!("\"log-level\":\"{value}\"")) { log_level = value; }
                }
                if request.contains("\"tun\":") {
                    if request.contains("\"enable\":true") { tun_enabled = true; }
                    if request.contains("\"enable\":false") { tun_enabled = false; }
                    for stack in ["system", "gvisor", "mixed", "mips"] {
                        if request.contains(&format!("\"stack\":\"{stack}\"")) { tun_stack = stack; }
                    }
                }
                for (key, port) in &mut ports {
                    let pattern = format!("\"{key}\":");
                    if let Some(index) = request.find(&pattern) {
                        let digits: String = request[index + pattern.len()..].chars().take_while(char::is_ascii_digit).collect();
                        if let Ok(value) = digits.parse() { *port = value; }
                    }
                }
            }
            "{}".to_owned()
        } else if request.starts_with("GET /configs ") {
            format!("{{{},{},\"mode\":\"{mode}\",\"log-level\":\"{log_level}\",\"tun\":{{\"enable\":{tun_enabled},\"stack\":\"{tun_stack}\"}}}}", settings.iter().map(|(key, enabled)| format!("\"{key}\":{enabled}")).collect::<Vec<_>>().join(","), ports.iter().map(|(key, port)| format!("\"{key}\":{port}")).collect::<Vec<_>>().join(","))
        } else if request.starts_with("GET /proxies ") {
            r#"{"proxies":{"fixture-group":{"type":"Selector","all":["fixture-node"],"now":"fixture-node"}}}"#.to_owned()
        } else if request.starts_with("GET /rules ") {
            r#"{"rules":[{"type":"MATCH","payload":"fixture","proxy":"DIRECT"}]}"#.to_owned()
        } else if request.starts_with("GET /connections ") {
            r#"{"connections":[],"uploadTotal":1,"downloadTotal":2,"memory":100}"#.to_owned()
        } else if request.starts_with("GET /providers/") {
            r#"{"providers":{}}"#.to_owned()
        } else if request.starts_with("GET /logs?") {
            format!("{{\"type\":\"info\",\"payload\":\"{} log\"}}\n", if remote { "remote" } else { "local" })
        } else {
            format!("{{\"version\":\"{}\"}}", if remote { "remote-fixture" } else { "fixture" })
        };
        // 就绪探测可以取消请求；客户端断开不应结束测试内核。
        let _ = write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        if config.contains("fixture-exit-after-request: true") {
            return;
        }
    }
}
