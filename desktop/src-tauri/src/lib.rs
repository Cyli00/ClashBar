pub mod app_logs;
pub mod bundled_core;
pub mod config;
pub mod controller;
pub mod engine;
pub mod group_icons;
pub mod network;
pub mod panel_geometry;
pub mod popup_state;
pub mod process;
pub mod providers;
pub mod releases;
pub mod remote;
pub mod ssid;
pub mod streams;
pub mod subscription;
pub mod subscriptions;
pub mod system_proxy;

#[cfg(feature = "desktop")]
pub mod popup;

#[cfg(feature = "desktop")]
mod tray_icons;

#[cfg(feature = "desktop")]
mod desktop {
    use crate::{
        controller::Snapshot,
        engine::{self, Engine, Status},
        popup, tray_icons,
    };
    use serde_json::Value;
    use std::{
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::Duration,
    };
    use tauri::{AppHandle, Emitter, Manager, State};
    use tauri_plugin_autostart::ManagerExt;
    use tauri_plugin_clipboard_manager::ClipboardExt;
    use tauri_plugin_dialog::DialogExt;
    use tokio::sync::Mutex;

    type Shared = Arc<Mutex<Engine>>;
    struct PendingConfigImport(Mutex<Option<PathBuf>>);
    struct QuitState {
        finished: AtomicBool,
        pending: AtomicBool,
    }
    struct NativeLanguage {
        english: AtomicBool,
        show: tauri::menu::MenuItem<tauri::Wry>,
        settings: tauri::menu::MenuItem<tauri::Wry>,
        quit: tauri::menu::MenuItem<tauri::Wry>,
    }

    fn native_text<'a>(app: &AppHandle, chinese: &'a str, english: &'a str) -> &'a str {
        if app
            .state::<NativeLanguage>()
            .english
            .load(Ordering::Relaxed)
        {
            english
        } else {
            chinese
        }
    }

    #[tauri::command]
    async fn set_ui_language(
        app: AppHandle,
        language: String,
        state: State<'_, Shared>,
    ) -> Result<Status, String> {
        let english = language == "en";
        let status = state.lock().await.set_ui_language(language)?;
        let native = app.state::<NativeLanguage>();
        native.english.store(english, Ordering::Relaxed);
        native
            .show
            .set_text(if english {
                "Open ClashBar"
            } else {
                "打开 ClashBar"
            })
            .map_err(|_| "无法更新托盘语言。")?;
        native
            .settings
            .set_text(if english { "Settings" } else { "设置" })
            .map_err(|_| "无法更新托盘语言。")?;
        native
            .quit
            .set_text(if english { "Quit" } else { "退出" })
            .map_err(|_| "无法更新托盘语言。")?;
        Ok(status)
    }

    #[tauri::command]
    async fn get_status(app: AppHandle, state: State<'_, Shared>) -> Result<Status, String> {
        let mut status = state.lock().await.target_status().await;
        attach_launch_status(&app, &mut status);
        Ok(status)
    }

    fn attach_launch_status(app: &AppHandle, status: &mut Status) {
        match app.autolaunch().is_enabled() {
            Ok(enabled) => status.launch_at_login = Some(enabled),
            Err(_) => status.launch_at_login_error = Some("无法读取系统登录启动项。".into()),
        }
    }

    #[tauri::command]
    async fn set_launch_at_login(
        app: AppHandle,
        enabled: bool,
        state: State<'_, Shared>,
    ) -> Result<Status, String> {
        let mut engine = state.lock().await;
        let manager = app.autolaunch();
        if enabled {
            manager.enable()
        } else {
            manager.disable()
        }
        .map_err(|error| format!("无法更改开机自启：{error}"))?;
        let actual = manager
            .is_enabled()
            .map_err(|_| "无法确认系统登录启动项状态。")?;
        if actual != enabled {
            return Err("系统登录启动项未生效，请检查系统设置后重试。".into());
        }
        let mut status = engine.status();
        status.launch_at_login = Some(actual);
        Ok(status)
    }

    #[tauri::command]
    async fn set_core_autostart(enabled: bool, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.set_core_autostart(enabled)
    }

    #[tauri::command]
    async fn set_status_bar_style(
        style: String,
        state: State<'_, Shared>,
    ) -> Result<Status, String> {
        state.lock().await.set_status_bar_style(style)
    }

    #[tauri::command]
    async fn save_remote_machine(
        input: crate::remote::RemoteInput,
        state: State<'_, Shared>,
    ) -> Result<Status, String> {
        state.lock().await.save_remote_machine(input).await
    }

    #[tauri::command]
    async fn delete_remote_machine(id: String, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.delete_remote_machine(&id)
    }

    #[tauri::command]
    async fn select_machine(
        id: Option<String>,
        state: State<'_, Shared>,
    ) -> Result<Status, String> {
        state.lock().await.select_machine(id).await
    }

    #[tauri::command]
    async fn check_remote_machine(
        id: String,
        state: State<'_, Shared>,
    ) -> Result<crate::remote::Connectivity, String> {
        let machine = state.lock().await.remote_machine(&id)?;
        Ok(crate::remote::probe(&machine).await)
    }

    async fn pick(
        app: &AppHandle,
        title: &str,
        extensions: &[&str],
    ) -> Result<Option<PathBuf>, String> {
        let _dialog_guard = popup::DialogGuard::new(app);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let mut dialog = app
            .dialog()
            .file()
            .set_title(title)
            .add_filter(title, extensions);
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        dialog.pick_file(move |file| {
            let _ = sender.send(file);
        });
        receiver
            .await
            .map_err(|_| "The file picker was closed unexpectedly.".to_string())?
            .map(|file| file.into_path().map_err(|_| "Select a local file.".into()))
            .transpose()
    }

    #[tauri::command]
    async fn choose_core(app: AppHandle, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.require_local_target()?;
        let path = pick(
            &app,
            native_text(
                &app,
                "选择可信的 mihomo 可执行文件",
                "Select a trusted mihomo executable",
            ),
            &["exe"],
        )
        .await?;
        let mut engine = state.lock().await;
        match path {
            Some(path) => engine.choose_core(&path),
            None => Ok(engine.status()),
        }
    }

    #[tauri::command]
    async fn import_config(app: AppHandle, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.require_local_target()?;
        let path = pick(
            &app,
            native_text(
                &app,
                "导入 mihomo YAML 配置",
                "Import a mihomo YAML profile",
            ),
            &["yaml", "yml"],
        )
        .await?;
        let mut engine = state.lock().await;
        if let Some(path) = path {
            engine.import_file(&path).await
        } else {
            Ok(engine.status())
        }
    }

    #[tauri::command]
    async fn prepare_config_import(
        app: AppHandle,
        state: State<'_, Shared>,
    ) -> Result<Option<Value>, String> {
        state.lock().await.require_local_target()?;
        let selected = pick(
            &app,
            native_text(
                &app,
                "导入 mihomo YAML 配置",
                "Import a mihomo YAML profile",
            ),
            &["yaml", "yml"],
        )
        .await?;
        let Some(path) = selected else {
            return Ok(None);
        };
        let collision = state.lock().await.profile_import_collision(&path)?;
        let name = crate::subscriptions::normalized_name(
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Imported.yaml"),
            "Imported.yaml",
        )?;
        *app.state::<PendingConfigImport>().0.lock().await = Some(path);
        Ok(Some(
            serde_json::json!({ "name": name, "overwriteRequired": collision.is_some() }),
        ))
    }

    #[tauri::command]
    async fn finish_config_import(
        app: AppHandle,
        overwrite: bool,
        state: State<'_, Shared>,
    ) -> Result<Status, String> {
        let path = app
            .state::<PendingConfigImport>()
            .0
            .lock()
            .await
            .clone()
            .ok_or("请先选择配置文件。")?;
        let result = state
            .lock()
            .await
            .import_file_confirmed(&path, overwrite)
            .await?;
        *app.state::<PendingConfigImport>().0.lock().await = None;
        Ok(result)
    }

    #[tauri::command]
    async fn cancel_config_import(app: AppHandle) {
        *app.state::<PendingConfigImport>().0.lock().await = None;
    }

    #[tauri::command]
    async fn prepare_subscription(
        input: crate::subscriptions::SubscriptionInput,
        state: State<'_, Shared>,
    ) -> Result<Value, String> {
        let prepared = input.prepare()?;
        let engine = state.lock().await;
        let collision = engine
            .profile_import_collision(std::path::Path::new(&prepared.name))?
            .is_some();
        Ok(serde_json::json!({ "name": prepared.name, "overwriteRequired": collision }))
    }

    #[tauri::command]
    async fn import_subscription(url: String, state: State<'_, Shared>) -> Result<Status, String> {
        state
            .lock()
            .await
            .add_subscription(crate::subscriptions::SubscriptionInput {
                url,
                name: None,
                auto_update_enabled: true,
                auto_update_interval_hours: 6,
                overwrite: false,
            })
            .await
    }

    #[tauri::command]
    async fn add_subscription(
        input: crate::subscriptions::SubscriptionInput,
        state: State<'_, Shared>,
    ) -> Result<Status, String> {
        state.lock().await.add_subscription(input).await
    }

    #[tauri::command]
    async fn refresh_subscription(id: String, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.refresh_subscription(&id).await
    }

    #[tauri::command]
    async fn refresh_all_subscriptions(state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.refresh_all_subscriptions(false).await
    }

    #[tauri::command]
    async fn save_subscription(
        id: String,
        url: Option<String>,
        auto_update_enabled: bool,
        auto_update_interval_hours: u32,
        state: State<'_, Shared>,
    ) -> Result<Status, String> {
        state.lock().await.save_subscription(
            &id,
            url,
            auto_update_enabled,
            auto_update_interval_hours,
        )
    }

    #[tauri::command]
    async fn copy_subscription_url(
        app: AppHandle,
        id: String,
        state: State<'_, Shared>,
    ) -> Result<(), String> {
        let url = state.lock().await.subscription_url(&id)?;
        app.clipboard()
            .write_text(url)
            .map_err(|_| "无法写入系统剪贴板。".into())
    }

    #[tauri::command]
    async fn save_local_ports(
        ports: crate::config::ProxyPorts,
        controller_port: u16,
        state: State<'_, Shared>,
    ) -> Result<Status, String> {
        state
            .lock()
            .await
            .save_local_ports(ports, controller_port)
            .await
    }

    #[tauri::command]
    async fn save_proxy_exceptions(
        exceptions: Vec<String>,
        state: State<'_, Shared>,
    ) -> Result<Status, String> {
        state.lock().await.save_proxy_exceptions(exceptions).await
    }

    #[tauri::command]
    async fn set_tun(
        enabled: bool,
        stack: String,
        state: State<'_, Shared>,
    ) -> Result<Status, String> {
        state.lock().await.set_tun(enabled, Some(stack)).await
    }

    #[tauri::command]
    async fn set_ssid_enabled(enabled: bool, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.set_ssid_enabled(enabled).await
    }

    #[tauri::command]
    async fn refresh_ssid(state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.refresh_ssid().await
    }

    #[tauri::command]
    fn open_location_settings() -> Result<(), String> {
        #[cfg(windows)]
        {
            open_url(
                url::Url::parse(crate::ssid::LOCATION_SETTINGS_URI)
                    .map_err(|_| "定位设置地址无效。")?,
            )
        }
        #[cfg(not(windows))]
        {
            Err("当前平台不提供 Windows 定位设置。".into())
        }
    }

    #[tauri::command]
    async fn bind_ssid(profile_id: String, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.bind_ssid(&profile_id).await
    }

    #[tauri::command]
    async fn remove_ssid_binding(ssid: String, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.remove_ssid_binding(&ssid).await
    }

    #[tauri::command]
    async fn check_app_update() -> Result<crate::releases::AppReleaseInfo, String> {
        crate::releases::fetch_latest_release(env!("CARGO_PKG_VERSION")).await
    }

    #[tauri::command]
    fn open_app_release() -> Result<(), String> {
        open_url(crate::releases::trusted_release_url(
            crate::releases::RELEASE_INDEX_URL,
        )?)
    }

    #[tauri::command]
    async fn open_web_ui(url: Option<String>, state: State<'_, Shared>) -> Result<(), String> {
        let Some(url) = url.filter(|url| !url.trim().is_empty()) else {
            let controller = state.lock().await.controller()?;
            let metadata = controller.web_ui_metadata().await?;
            return open_url(controller.web_ui_url(&metadata)?);
        };
        let parsed = url::Url::parse(&url).map_err(|_| "请输入有效的 Web UI 地址。")?;
        if !matches!(parsed.scheme(), "https" | "http")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("Web UI 仅支持不含用户名和密码的 HTTP(S) 地址。".into());
        }
        open_url(parsed)
    }

    fn open_url(url: url::Url) -> Result<(), String> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::Shell::ShellExecuteW;
            let verb: Vec<u16> = "open\0".encode_utf16().collect();
            let value: Vec<u16> = url.as_str().encode_utf16().chain(Some(0)).collect();
            let result = unsafe {
                ShellExecuteW(
                    std::ptr::null_mut(),
                    verb.as_ptr(),
                    value.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    1,
                )
            };
            if result as isize <= 32 {
                return Err("无法打开系统默认浏览器。".into());
            }
        }
        #[cfg(not(windows))]
        {
            let opener = if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };
            let mut child = std::process::Command::new(opener)
                .arg(url.as_str())
                .spawn()
                .map_err(|_| "无法打开系统默认浏览器。")?;
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Ok(())
    }

    #[tauri::command]
    async fn upgrade_core(
        state: State<'_, Shared>,
    ) -> Result<crate::controller::CoreUpgradeResult, String> {
        state.lock().await.controller()?.upgrade_core().await
    }

    #[tauri::command]
    async fn save_settings(
        mixed_port: u16,
        controller_port: u16,
        state: State<'_, Shared>,
    ) -> Result<Status, String> {
        state
            .lock()
            .await
            .save_settings(mixed_port, controller_port)
    }
    #[tauri::command]
    async fn start_core(state: State<'_, Shared>) -> Result<Status, String> {
        let mut engine = state.lock().await;
        engine.require_local_target()?;
        engine.start().await
    }
    #[tauri::command]
    async fn save_remote_ports(
        ports: crate::config::ProxyPorts,
        state: State<'_, Shared>,
    ) -> Result<(), String> {
        state.lock().await.save_remote_ports(ports).await
    }
    #[tauri::command]
    async fn stop_core(state: State<'_, Shared>) -> Result<Status, String> {
        let mut engine = state.lock().await;
        engine.require_local_target()?;
        engine.stop()
    }
    #[tauri::command]
    async fn restart_core(state: State<'_, Shared>) -> Result<Status, String> {
        let mut engine = state.lock().await;
        engine.require_local_target()?;
        engine.restart().await
    }
    #[tauri::command]
    async fn select_profile(id: String, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.select_profile(&id).await
    }
    #[tauri::command]
    async fn delete_profile(id: String, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.delete_profile(&id).await
    }

    fn open_directory(path: PathBuf) -> Result<(), String> {
        if !path.is_dir() {
            return Err("目标目录不存在。".into());
        }
        #[cfg(target_os = "windows")]
        let mut command = {
            use std::os::windows::process::CommandExt;
            let mut command = std::process::Command::new("explorer.exe");
            command.creation_flags(0x08000000);
            command
        };
        #[cfg(target_os = "macos")]
        let mut command = std::process::Command::new("open");
        #[cfg(all(unix, not(target_os = "macos")))]
        let mut command = std::process::Command::new("xdg-open");
        let mut child = command
            .arg(path)
            .spawn()
            .map_err(|error| format!("无法打开目录：{error}"))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    #[tauri::command]
    async fn open_core_directory(state: State<'_, Shared>) -> Result<(), String> {
        let path = state.lock().await.core_directory()?;
        open_directory(path)
    }

    #[tauri::command]
    async fn open_profiles_directory(state: State<'_, Shared>) -> Result<(), String> {
        state.lock().await.require_local_target()?;
        let path = state.lock().await.profiles_directory();
        open_directory(path)
    }
    #[tauri::command]
    async fn open_selected_profile(state: State<'_, Shared>) -> Result<(), String> {
        let path = {
            let mut engine = state.lock().await;
            engine.require_local_target()?;
            let id = engine
                .status()
                .active_profile_id
                .ok_or("请先选择配置文件。")?;
            engine.profiles_directory().join(format!("{id}.yaml"))
        };
        if !path.is_file() {
            return Err("当前配置文件不存在。".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let mut child = std::process::Command::new("explorer.exe")
                .creation_flags(0x08000000)
                .arg("/select,")
                .arg(path)
                .spawn()
                .map_err(|_| "无法在资源管理器中定位配置。")?;
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
        #[cfg(not(windows))]
        {
            open_directory(path.parent().ok_or("配置目录无效。")?.to_owned())
        }
    }
    #[tauri::command]
    async fn clear_logs(state: State<'_, Shared>) -> Result<(), String> {
        state.lock().await.clear_logs()
    }
    #[tauri::command]
    async fn close_all_connections(state: State<'_, Shared>) -> Result<(), String> {
        state
            .lock()
            .await
            .controller()?
            .close_all_connections()
            .await
    }
    #[tauri::command]
    async fn provider_healthcheck(name: String, state: State<'_, Shared>) -> Result<(), String> {
        state
            .lock()
            .await
            .controller()?
            .provider_healthcheck(&name)
            .await
    }
    #[tauri::command]
    async fn test_group_delay(name: String, state: State<'_, Shared>) -> Result<Value, String> {
        state
            .lock()
            .await
            .controller()?
            .test_group_delay(&name)
            .await
    }
    #[tauri::command]
    async fn set_log_level(level: String, state: State<'_, Shared>) -> Result<(), String> {
        state.lock().await.set_log_level(&level).await
    }
    #[tauri::command]
    async fn set_core_boolean(
        setting: crate::config::CoreBooleanSetting,
        value: bool,
        state: State<'_, Shared>,
    ) -> Result<(), String> {
        state.lock().await.set_core_boolean(setting, value).await
    }
    #[tauri::command]
    async fn set_system_proxy(enabled: bool, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.set_selected_system_proxy(enabled).await
    }
    #[tauri::command]
    async fn get_snapshot(state: State<'_, Shared>) -> Result<Snapshot, String> {
        state.lock().await.target_snapshot().await
    }
    #[tauri::command]
    async fn get_proxy_group_icon(
        name: String,
        state: State<'_, Shared>,
    ) -> Result<String, String> {
        let (controller, cache) = state.lock().await.proxy_group_icon_context()?;
        let url = controller
            .proxy_group_icon(&name)
            .await?
            .ok_or("代理组未配置图标。")?;
        cache.get(&url).await
    }
    #[tauri::command]
    async fn set_mode(mode: String, state: State<'_, Shared>) -> Result<(), String> {
        state.lock().await.set_mode(&mode).await
    }
    #[tauri::command]
    async fn select_proxy(
        group: String,
        name: String,
        state: State<'_, Shared>,
    ) -> Result<(), String> {
        state
            .lock()
            .await
            .controller()?
            .select_proxy(&group, &name)
            .await
    }
    #[tauri::command]
    async fn test_delay(name: String, state: State<'_, Shared>) -> Result<Value, String> {
        state.lock().await.controller()?.test_delay(&name).await
    }
    #[tauri::command]
    async fn close_connection(id: String, state: State<'_, Shared>) -> Result<(), String> {
        state.lock().await.controller()?.close_connection(&id).await
    }
    #[tauri::command]
    async fn update_provider(name: String, state: State<'_, Shared>) -> Result<(), String> {
        state
            .lock()
            .await
            .controller()?
            .update_provider(&name)
            .await
    }
    #[tauri::command]
    async fn refresh_rule_providers(state: State<'_, Shared>) -> Result<(), String> {
        state
            .lock()
            .await
            .controller()?
            .refresh_rule_providers()
            .await
    }
    #[tauri::command]
    async fn flush_fakeip_cache(state: State<'_, Shared>) -> Result<(), String> {
        state.lock().await.controller()?.flush_fakeip_cache().await
    }
    #[tauri::command]
    async fn flush_dns_cache(state: State<'_, Shared>) -> Result<(), String> {
        state.lock().await.controller()?.flush_dns_cache().await
    }
    #[tauri::command]
    async fn upgrade_geo(state: State<'_, Shared>) -> Result<(), String> {
        state.lock().await.controller()?.upgrade_geo().await
    }
    #[tauri::command]
    async fn get_logs(state: State<'_, Shared>) -> Result<Vec<String>, String> {
        Ok(state.lock().await.log_lines())
    }

    #[tauri::command]
    async fn get_log_entries(
        state: State<'_, Shared>,
    ) -> Result<Vec<crate::process::LogEntry>, String> {
        Ok(state.lock().await.log_entries())
    }

    #[tauri::command]
    async fn record_app_action(
        message: String,
        state: State<'_, Shared>,
    ) -> Result<crate::process::LogEntry, String> {
        state.lock().await.record_app_action(&message)
    }

    fn show(app: &AppHandle) {
        if let Err(error) = popup::request_show(app) {
            eprintln!("Cannot show ClashBar popup: {error}");
        }
    }

    #[tauri::command]
    fn quit_app(app: AppHandle) {
        quit(&app);
    }

    fn quit(app: &AppHandle) {
        let state = app.state::<QuitState>();
        if state.pending.swap(true, Ordering::SeqCst) {
            return;
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let result = app.state::<Shared>().lock().await.shutdown();
            match result {
                Ok(_) => {
                    app.state::<QuitState>()
                        .finished
                        .store(true, Ordering::SeqCst);
                    app.exit(0);
                }
                Err(error) => {
                    app.state::<QuitState>()
                        .pending
                        .store(false, Ordering::SeqCst);
                    show(&app);
                    let dialog_guard = popup::DialogGuard::new(&app);
                    let message = if app
                        .state::<NativeLanguage>()
                        .english
                        .load(Ordering::Relaxed)
                    {
                        format!("ClashBar could not restore the system proxy and will remain open.\n\n{error}\n\nCorrect the Windows proxy settings, then try Quit again.")
                    } else {
                        format!("ClashBar 无法恢复系统代理，将保持运行。\n\n{error}\n\n请检查 Windows 代理设置，然后重新退出。")
                    };
                    app.dialog()
                        .message(message)
                        .title(native_text(
                            &app,
                            "无法安全退出 ClashBar",
                            "Cannot safely quit ClashBar",
                        ))
                        .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                        .show(move |_| drop(dialog_guard));
                }
            }
        });
    }

    pub fn run() {
        if crate::process::run_elevated_helper_if_requested() {
            return;
        }
        tauri::Builder::default()
            .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_clipboard_manager::init())
            .plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                None,
            ))
            .manage(popup::PopupState::default())
            .manage(PendingConfigImport(Mutex::new(None)))
            .manage(QuitState {
                finished: AtomicBool::new(false),
                pending: AtomicBool::new(false),
            })
            .setup(|app| {
                let directory = app.path().app_data_dir()?;
                let first_launch = !directory.join("settings.json").exists();
                let mut engine = Engine::new(directory).map_err(std::io::Error::other)?;
                if let Err(error) = engine.install_bundled_core(&app.path().resource_dir()?) {
                    engine.last_error = Some(error);
                }
                if first_launch {
                    engine
                        .seed_default_profile()
                        .map_err(std::io::Error::other)?;
                }
                let english = engine.status().ui_language == "en";
                let shared = Arc::new(Mutex::new(engine));
                app.manage(shared.clone());
                popup::configure_windows(app.handle()).map_err(std::io::Error::other)?;
                let show_item = tauri::menu::MenuItem::with_id(
                    app,
                    "show",
                    if english {
                        "Open ClashBar"
                    } else {
                        "打开 ClashBar"
                    },
                    true,
                    None::<&str>,
                )?;
                let quit_item = tauri::menu::MenuItem::with_id(
                    app,
                    "quit",
                    if english { "Quit" } else { "退出" },
                    true,
                    None::<&str>,
                )?;
                let settings_item = tauri::menu::MenuItem::with_id(
                    app,
                    "settings",
                    if english { "Settings" } else { "设置" },
                    true,
                    None::<&str>,
                )?;
                let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
                let menu = tauri::menu::Menu::with_items(
                    app,
                    &[&show_item, &settings_item, &separator, &quit_item],
                )?;
                app.manage(NativeLanguage {
                    english: AtomicBool::new(english),
                    show: show_item,
                    settings: settings_item,
                    quit: quit_item,
                });
                let icons = tray_icons::TrayIcons::load()?;
                let initial_light_taskbar = tray_icons::light_taskbar();
                let tray = tauri::tray::TrayIconBuilder::with_id(popup::TRAY_ID)
                    .tooltip("ClashBar · 已停止 · 未接管系统代理")
                    .icon(icons.image(false, initial_light_taskbar))
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_tray_icon_event(|tray, event| {
                        popup::on_tray_event(tray.app_handle(), event)
                    })
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => show(app),
                        "settings" => {
                            show(app);
                            if let Some(window) = app.get_webview_window("main") {
                                let _ =
                                    window.emit("popup-tab", serde_json::json!({"tab":"system"}));
                            }
                        }
                        "quit" => quit(app),
                        _ => {}
                    });
                tray.build(app)?;
                // WebView2 pumps Win32 messages before Tauri registers each window.
                // A second launch during that interval must wait for this setup.
                if let Err(error) = popup::mark_ready(app.handle()) {
                    eprintln!("Cannot show pending ClashBar popup: {error}");
                }
                let startup_engine = shared.clone();
                let startup_app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if startup_engine
                        .lock()
                        .await
                        .auto_start_if_configured()
                        .await
                        .is_err()
                    {
                        show(&startup_app);
                    }
                });
                let monitor_app = app.handle().clone();
                let background_engine = shared.clone();
                tauri::async_runtime::spawn(async move {
                    let mut profiles = engine::ProfileMonitor::default();
                    let mut ticks = 0u64;
                    loop {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        let mut engine = background_engine.lock().await;
                        if let Err(error) = engine.background_services_tick().await {
                            engine.last_error = Some(error);
                        }
                        engine.poll_ssid().await;
                        if let Err(error) = engine.monitor_profiles(&mut profiles).await {
                            engine.last_error = Some(error);
                        }
                        if ticks.is_multiple_of(30) {
                            let _ = engine.refresh_all_subscriptions(true).await;
                        }
                        ticks = ticks.wrapping_add(1);
                    }
                });
                tauri::async_runtime::spawn(async move {
                    let mut last_running = false;
                    let mut last_light_taskbar = initial_light_taskbar;
                    let mut last_tooltip = String::new();
                    let mut last_style = String::new();
                    let mut last_speed = None;
                    loop {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        let (status, speed) = {
                            let mut engine = shared.lock().await;
                            let status = engine.status();
                            let speed = engine.tray_traffic();
                            (status, speed)
                        };
                        if let Some(tray) = monitor_app.tray_by_id(popup::TRAY_ID) {
                            let mut tooltip = format!(
                                "ClashBar · {} · {}",
                                if status.running {
                                    native_text(&monitor_app, "运行中", "Running")
                                } else {
                                    native_text(&monitor_app, "已停止", "Stopped")
                                },
                                if status.system_proxy {
                                    native_text(
                                        &monitor_app,
                                        "已接管系统代理",
                                        "System proxy enabled",
                                    )
                                } else {
                                    native_text(
                                        &monitor_app,
                                        "未接管系统代理",
                                        "System proxy disabled",
                                    )
                                }
                            );
                            if status.status_bar_style != "iconOnly" {
                                if let Some((up, down)) = speed {
                                    tooltip.push_str(&format!("\n↑ {up} B/s  ↓ {down} B/s"));
                                } else {
                                    tooltip.push_str("\n↑ —  ↓ —");
                                }
                            }
                            if tooltip != last_tooltip {
                                let _ = tray.set_tooltip(Some(&tooltip));
                                last_tooltip = tooltip;
                            }
                            let light_taskbar = tray_icons::light_taskbar();
                            if status.running != last_running
                                || light_taskbar != last_light_taskbar
                                || status.status_bar_style != last_style
                                || speed != last_speed
                            {
                                let _ = tray.set_icon(Some(icons.with_speed(
                                    status.running,
                                    light_taskbar,
                                    &status.status_bar_style,
                                    speed,
                                )));
                                last_running = status.running;
                                last_light_taskbar = light_taskbar;
                                last_style = status.status_bar_style.clone();
                                last_speed = speed;
                            }
                        }
                        popup::refresh_position(&monitor_app);
                    }
                });
                Ok(())
            })
            .on_window_event(|window, event| {
                popup::on_window_event(window.app_handle(), window.label(), event);
            })
            .invoke_handler(tauri::generate_handler![
                get_status,
                set_ui_language,
                set_launch_at_login,
                set_core_autostart,
                set_status_bar_style,
                save_remote_machine,
                delete_remote_machine,
                select_machine,
                check_remote_machine,
                choose_core,
                import_config,
                prepare_config_import,
                finish_config_import,
                cancel_config_import,
                prepare_subscription,
                import_subscription,
                add_subscription,
                refresh_subscription,
                refresh_all_subscriptions,
                save_subscription,
                copy_subscription_url,
                save_local_ports,
                save_proxy_exceptions,
                set_tun,
                set_ssid_enabled,
                refresh_ssid,
                open_location_settings,
                bind_ssid,
                remove_ssid_binding,
                check_app_update,
                open_app_release,
                open_web_ui,
                upgrade_core,
                save_settings,
                save_remote_ports,
                start_core,
                stop_core,
                set_system_proxy,
                get_snapshot,
                get_proxy_group_icon,
                set_mode,
                select_proxy,
                test_delay,
                close_connection,
                update_provider,
                refresh_rule_providers,
                flush_fakeip_cache,
                flush_dns_cache,
                upgrade_geo,
                get_logs,
                get_log_entries,
                record_app_action,
                clear_logs,
                restart_core,
                select_profile,
                delete_profile,
                open_core_directory,
                open_profiles_directory,
                open_selected_profile,
                close_all_connections,
                provider_healthcheck,
                test_group_delay,
                set_log_level,
                set_core_boolean,
                quit_app,
                popup::resize_popup,
                popup::hide_popup,
                popup::set_popup_pinned,
                popup::get_popup_pinned,
                popup::show_attached_menu,
                popup::hide_attached_menu,
                popup::get_attached_menu,
                popup::attached_menu_action,
                popup::attached_menu_hover
            ])
            .build(tauri::generate_context!())
            .expect("Unable to initialize ClashBar")
            .run(|app, event| {
                if let tauri::RunEvent::ExitRequested { api, .. } = event {
                    if !app.state::<QuitState>().finished.load(Ordering::SeqCst) {
                        api.prevent_exit();
                        quit(app);
                    }
                }
            });
    }
}

#[cfg(feature = "desktop")]
pub use desktop::run;
