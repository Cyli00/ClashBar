use super::*;
use crate::engine::tests::configured_engine;
use std::io::{Read, Write};

#[tokio::test]
async fn confirmed_same_name_file_import_keeps_identity_selection_and_ssid_and_removes_subscription(
) {
    let original = b"proxies: []\nrules: []\n";
    let (directory, mut engine) = configured_engine(original);
    let id = engine.settings.active_profile_id.clone().unwrap();
    engine.settings.ssid_rules.push(crate::ssid::SsidRule {
        ssid: "Office".into(),
        config_file_name: "fixture.yaml".into(),
    });
    engine.settings.subscriptions.push(
        SubscriptionInput {
            url: "http://127.0.0.1:1/old".into(),
            name: None,
            auto_update_enabled: false,
            auto_update_interval_hours: 6,
            overwrite: false,
        }
        .prepare()
        .unwrap()
        .bind(id.clone(), 1)
        .unwrap(),
    );
    engine.settings.save(directory.path()).unwrap();
    engine.start().await.unwrap();
    let source = tempfile::tempdir().unwrap();
    let path = source.path().join("fixture.yaml");
    fs::write(&path, b"proxies: []\nmode: global\n").unwrap();
    assert_eq!(
        engine.profile_import_collision(&path).unwrap().as_deref(),
        Some("fixture.yaml")
    );
    assert!(engine
        .import_file(&path)
        .await
        .unwrap_err()
        .contains("确认覆盖"));
    assert_eq!(
        fs::read(engine.active_profile_path().unwrap()).unwrap(),
        original
    );
    let status = engine.import_file_confirmed(&path, true).await.unwrap();
    assert_eq!(status.active_profile_id.as_deref(), Some(id.as_str()));
    assert_eq!(status.profiles.len(), 1);
    assert!(status.subscriptions.is_empty());
    assert_eq!(status.ssid_rules[0].config_file_name, "fixture.yaml");
    assert!(status.running);
    assert_eq!(
        engine
            .controller()
            .unwrap()
            .snapshot()
            .await
            .unwrap()
            .configs["mode"],
        "global"
    );
    engine.stop().unwrap();
}

#[tokio::test]
async fn new_file_import_preserves_current_selection_and_identical_content_has_distinct_name() {
    let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
    let id = engine.settings.active_profile_id.clone().unwrap();
    engine.start().await.unwrap();
    let runtime = fs::read(directory.path().join("runtime/config.yaml")).unwrap();
    let source = tempfile::tempdir().unwrap();
    for name in ["second.yaml", "third.yaml"] {
        let path = source.path().join(name);
        fs::write(&path, b"proxies: []\nmode: global\n").unwrap();
        engine.import_file(&path).await.unwrap();
    }
    assert_eq!(
        engine.settings.active_profile_id.as_deref(),
        Some(id.as_str())
    );
    assert_eq!(engine.settings.profiles.len(), 3);
    assert!(runtime == fs::read(directory.path().join("runtime/config.yaml")).unwrap());
    engine.stop().unwrap();
}

#[tokio::test]
async fn failed_confirmed_file_overwrite_restores_running_configuration() {
    let original = b"proxies: []\nrules: []\n";
    let (_directory, mut engine) = configured_engine(original);
    let id = engine.settings.active_profile_id.clone();
    engine.start().await.unwrap();
    let source = tempfile::tempdir().unwrap();
    let path = source.path().join("fixture.yaml");
    fs::write(&path, b"proxies: []\nfixture-startup-exit: true\n").unwrap();
    assert!(engine.import_file_confirmed(&path, true).await.is_err());
    assert_eq!(engine.settings.active_profile_id, id);
    assert_eq!(
        fs::read(engine.active_profile_path().unwrap()).unwrap(),
        original
    );
    assert!(engine.status().running);
    engine.stop().unwrap();
}

#[tokio::test]
async fn subscription_name_collision_requires_confirmation_and_keeps_original_identity() {
    let (_directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
    let id = engine.settings.active_profile_id.clone().unwrap();
    let mut input = SubscriptionInput {
        url: "http://127.0.0.1:1/profile".into(),
        name: Some(" fixture ".into()),
        auto_update_enabled: true,
        auto_update_interval_hours: 6,
        overwrite: false,
    };
    assert!(engine
        .add_subscription(input.clone())
        .await
        .unwrap_err()
        .contains("确认覆盖"));
    let (url, server) = one_response(b"proxies: []\nmode: global\n");
    input.url = url.clone();
    input.overwrite = true;
    let status = engine.add_subscription(input).await.unwrap();
    assert_eq!(status.profiles.len(), 1);
    assert_eq!(status.active_profile_id.as_deref(), Some(id.as_str()));
    assert_eq!(status.subscriptions[0].profile_id, id);
    assert_eq!(engine.subscription_url(&id).unwrap(), url);
    server.join().unwrap();
}

fn one_response(body: &'static [u8]) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let url = format!("http://{}/subscription", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let worker = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("订阅测试服务器没有收到请求：{error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = [0u8; 4096];
        assert!(stream.read(&mut bytes).unwrap() > 0);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(body).unwrap();
    });
    (url, worker)
}

#[tokio::test]
async fn source_edits_keep_identity_and_manual_delete_stays_deleted_after_reload() {
    let directory = tempfile::tempdir().unwrap();
    let mut engine = Engine::new(directory.path().into()).unwrap();
    let source = engine.profiles_directory().join("office.yaml");
    fs::write(&source, b"proxies: []\nmode: rule\n").unwrap();
    let mut monitor = ProfileMonitor::default();
    engine.monitor_profiles(&mut monitor).await.unwrap();
    let id = engine.settings.profiles[0].id.clone();
    fs::write(&source, b"proxies: []\nmode: global\n").unwrap();
    engine.monitor_profiles(&mut monitor).await.unwrap();
    assert_eq!(engine.settings.profiles.len(), 1);
    assert_eq!(engine.settings.profiles[0].id, id);
    assert!(
        fs::read_to_string(engine.profiles_directory().join(format!("{id}.yaml")))
            .unwrap()
            .contains("global")
    );
    engine.delete_profile(&id).await.unwrap();
    drop(engine);
    let mut engine = Engine::new(directory.path().into()).unwrap();
    engine
        .monitor_profiles(&mut ProfileMonitor::default())
        .await
        .unwrap();
    assert!(engine.settings.profiles.is_empty());
}

#[tokio::test]
async fn failed_external_edit_restores_live_core_and_archives_attempt() {
    let original = b"proxies: []\nrules: []\n";
    let (directory, mut engine) = configured_engine(original);
    engine.start().await.unwrap();
    let mut monitor = ProfileMonitor::default();
    engine.monitor_profiles(&mut monitor).await.unwrap();
    let path = engine.active_profile_path().unwrap();
    for candidate in [
        b"proxies: []\nfixture-startup-exit: true\n".as_slice(),
        b"[invalid".as_slice(),
    ] {
        fs::write(&path, candidate).unwrap();
        let error = engine.monitor_profiles(&mut monitor).await.unwrap_err();
        assert!(error.contains("rejected-profile-edits"));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(engine.status().running);
        let recovered = fs::read_dir(directory.path().join("rejected-profile-edits"))
            .unwrap()
            .any(|entry| fs::read(entry.unwrap().path()).unwrap() == candidate);
        assert!(recovered);
        engine.monitor_profiles(&mut monitor).await.unwrap();
    }
    engine.stop().unwrap();
}

#[tokio::test]
async fn internal_profile_update_does_not_trigger_a_second_monitor_restart() {
    let (_directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
    engine.start().await.unwrap();
    let mut monitor = ProfileMonitor::default();
    engine.monitor_profiles(&mut monitor).await.unwrap();
    let id = engine.settings.active_profile_id.clone().unwrap();
    engine
        .replace_profile_content(&id, b"proxies: []\nmode: global\n")
        .await
        .unwrap();
    let runtime = fs::read(engine.directory.join("runtime/config.yaml")).unwrap();
    engine.monitor_profiles(&mut monitor).await.unwrap();
    assert!(runtime == fs::read(engine.directory.join("runtime/config.yaml")).unwrap());
    engine.stop().unwrap();
}

#[tokio::test]
async fn failed_subscription_activation_restores_source_and_running_core() {
    let original = b"proxies: []\nrules: []\n";
    let (directory, mut engine) = configured_engine(original);
    let id = engine.settings.active_profile_id.clone().unwrap();
    let (url, server) = one_response(b"proxies: []\nfixture-startup-exit: true\n");
    engine.settings.subscriptions.push(
        SubscriptionInput {
            url,
            name: None,
            auto_update_enabled: true,
            auto_update_interval_hours: 6,
            overwrite: false,
        }
        .prepare()
        .unwrap()
        .bind(id.clone(), 1)
        .unwrap(),
    );
    engine.settings.save(directory.path()).unwrap();
    engine.start().await.unwrap();
    assert!(engine.refresh_subscription(&id).await.is_err());
    assert_eq!(
        fs::read(engine.active_profile_path().unwrap()).unwrap(),
        original
    );
    assert!(engine.status().running);
    assert!(Settings::load(directory.path()).unwrap().subscriptions[0]
        .last_error
        .is_some());
    engine.stop().unwrap();
    server.join().unwrap();
}

#[tokio::test]
async fn subscription_metadata_save_failure_restores_updated_profile() {
    let original = b"proxies: []\nrules: []\n";
    let (directory, mut engine) = configured_engine(original);
    let id = engine.settings.active_profile_id.clone().unwrap();
    let (url, server) = one_response(b"proxies: []\nmode: global\n");
    engine.settings.subscriptions.push(
        SubscriptionInput {
            url,
            name: None,
            auto_update_enabled: true,
            auto_update_interval_hours: 6,
            overwrite: false,
        }
        .prepare()
        .unwrap()
        .bind(id.clone(), 1)
        .unwrap(),
    );
    engine.settings.save(directory.path()).unwrap();
    engine.start().await.unwrap();
    fs::create_dir(directory.path().join("settings.tmp")).unwrap();
    assert!(engine
        .refresh_subscription(&id)
        .await
        .unwrap_err()
        .contains("已恢复原配置"));
    assert_eq!(
        fs::read(engine.active_profile_path().unwrap()).unwrap(),
        original
    );
    assert!(engine.status().running);
    assert_eq!(
        Settings::load(directory.path()).unwrap().subscriptions[0].last_checked_at,
        Some(1)
    );
    engine.stop().unwrap();
    server.join().unwrap();
}

#[tokio::test]
async fn local_web_ui_is_copied_with_nested_assets_and_cannot_escape_source() {
    let source = tempfile::tempdir().unwrap();
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(source.path().join("ui/assets")).unwrap();
    fs::write(
        source.path().join("ui/index.html"),
        b"<main>dashboard</main>",
    )
    .unwrap();
    fs::write(
        source.path().join("ui/assets/app.js"),
        b"console.info('dashboard')",
    )
    .unwrap();
    let bytes = b"proxies: []\nexternal-ui: ui\n";
    let path = source.path().join("main.yaml");
    fs::write(&path, bytes).unwrap();
    let mut engine = Engine::new(directory.path().into()).unwrap();
    engine.import_file(&path).await.unwrap();
    let cache = directory
        .path()
        .join("runtime/ui")
        .join(config::profile_id(bytes));
    assert_eq!(
        fs::read(cache.join("index.html")).unwrap(),
        b"<main>dashboard</main>"
    );
    assert_eq!(
        fs::read(cache.join("assets/app.js")).unwrap(),
        b"console.info('dashboard')"
    );
    assert!(engine
        .copy_local_providers(b"proxies: []\nexternal-ui: ../\n", source.path())
        .is_err());
}
