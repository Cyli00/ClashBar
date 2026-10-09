use clashbar_desktop::{config::ProxyPorts, engine::Engine};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener},
    path::PathBuf,
    time::Duration,
};

#[tokio::test]
#[ignore = "需要 CLASHBAR_TEST_MIHOMO 指向可信的真实内核；仅访问临时本机 HTTP 服务"]
async fn real_mihomo_lifecycle_proxy_and_streams() {
    let core = PathBuf::from(std::env::var_os("CLASHBAR_TEST_MIHOMO").expect("请设置测试内核路径"));
    let directory = tempfile::tempdir().unwrap();
    let proxy_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let api_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mixed_port = proxy_listener.local_addr().unwrap().port();
    let controller_port = api_listener.local_addr().unwrap().port();
    drop((proxy_listener, api_listener));
    let mut engine = Engine::new(directory.path().to_owned()).unwrap();
    engine.choose_core(&core).unwrap();
    engine
        .save_local_ports(
            ProxyPorts {
                port: 0,
                socks_port: 0,
                mixed_port,
                redir_port: 0,
                tproxy_port: 0,
            },
            controller_port,
        )
        .await
        .unwrap();
    engine.import_profile(b"proxies: []\nproxy-groups: [{name: Manual, type: select, proxies: [DIRECT, REJECT]}]\nrules: ['MATCH,DIRECT']\ndns: {enable: false}\n", "local-test.yaml").unwrap();
    assert!(engine.start().await.unwrap().running);
    engine.set_mode("rule").await.unwrap();
    engine.set_log_level("debug").await.unwrap();
    let controller = engine.controller().unwrap();
    controller.select_proxy("Manual", "DIRECT").await.unwrap();
    assert_eq!(
        engine.target_snapshot().await.unwrap().configs["mode"],
        "rule"
    );

    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = upstream.local_addr().unwrap();
    upstream.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match upstream.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("本地转发连接失败：{error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = [0; 8192];
        assert!(stream.read(&mut request).unwrap() > 0);
        let body = "clashbar-local-fixture";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::http(format!("http://127.0.0.1:{mixed_port}")).unwrap())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    assert_eq!(
        client
            .get(format!("http://{address}/local-test"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "clashbar-local-fixture"
    );
    server.join().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while engine.tray_traffic().is_none() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        engine.tray_traffic().is_some(),
        "真实 WebSocket 流量流未建立"
    );
    let snapshot = engine.target_snapshot().await.unwrap();
    assert!(snapshot.memory.is_some());
    assert!(snapshot.traffic.is_some());
    assert!(engine.restart().await.unwrap().running);
    assert_eq!(
        engine.target_snapshot().await.unwrap().configs["log-level"],
        "debug"
    );
    assert!(!engine.stop().unwrap().running);
    assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, mixed_port)).is_ok());
    assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, controller_port)).is_ok());
}
