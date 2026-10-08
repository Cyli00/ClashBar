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
    for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0u8; 4096];
        let _ = stream.read(&mut request);
        let body = r#"{"version":"fixture"}"#;
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        if config.contains("fixture-exit-after-request: true") {
            return;
        }
    }
}
