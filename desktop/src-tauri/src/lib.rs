pub mod config;
pub mod controller;
pub mod engine;
pub mod panel_geometry;
pub mod popup_state;
pub mod process;
pub mod subscription;
pub mod system_proxy;

#[cfg(feature = "desktop")]
pub mod popup;

#[cfg(feature = "desktop")]
mod desktop {
    use crate::{
        controller::Snapshot,
        engine::{self, Engine, Status},
        popup, subscription,
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
    use tauri_plugin_dialog::DialogExt;
    use tokio::sync::Mutex;

    type Shared = Arc<Mutex<Engine>>;
    struct QuitState {
        finished: AtomicBool,
        pending: AtomicBool,
    }

    #[tauri::command]
    async fn get_status(state: State<'_, Shared>) -> Result<Status, String> {
        Ok(state.lock().await.status())
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
        let path = pick(&app, "Select a trusted mihomo executable", &["exe"]).await?;
        let mut engine = state.lock().await;
        match path {
            Some(path) => engine.choose_core(&path),
            None => Ok(engine.status()),
        }
    }

    #[tauri::command]
    async fn import_config(app: AppHandle, state: State<'_, Shared>) -> Result<Status, String> {
        let path = pick(&app, "Import a mihomo YAML profile", &["yaml", "yml"]).await?;
        let mut engine = state.lock().await;
        if let Some(path) = path {
            let bytes = engine::read_profile(&path)?;
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Imported profile");
            engine.import_and_activate(&bytes, name).await
        } else {
            Ok(engine.status())
        }
    }

    #[tauri::command]
    async fn import_subscription(url: String, state: State<'_, Shared>) -> Result<Status, String> {
        let bytes = subscription::download(&url).await?;
        // The full subscription URL often contains credentials and is never persisted or logged.
        let name = subscription::validate_url(&url)?
            .host_str()
            .unwrap_or("HTTPS subscription")
            .to_owned();
        state.lock().await.import_and_activate(&bytes, &name).await
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
        state.lock().await.start().await
    }
    #[tauri::command]
    async fn stop_core(state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.stop()
    }
    #[tauri::command]
    async fn restart_core(state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.restart().await
    }
    #[tauri::command]
    async fn select_profile(id: String, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.select_profile(&id).await
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
        state.lock().await.controller()?.set_log_level(&level).await
    }
    #[tauri::command]
    async fn set_system_proxy(enabled: bool, state: State<'_, Shared>) -> Result<Status, String> {
        state.lock().await.set_system_proxy(enabled)
    }
    #[tauri::command]
    async fn get_snapshot(state: State<'_, Shared>) -> Result<Snapshot, String> {
        state.lock().await.controller()?.snapshot().await
    }
    #[tauri::command]
    async fn set_mode(mode: String, state: State<'_, Shared>) -> Result<(), String> {
        state.lock().await.controller()?.set_mode(&mode).await
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
    async fn get_logs(state: State<'_, Shared>) -> Result<Vec<String>, String> {
        Ok(state.lock().await.log_lines())
    }

    fn show(app: &AppHandle) {
        let _ = popup::show(app);
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
            let result = app.state::<Shared>().lock().await.stop();
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
                    app.dialog().message(format!("ClashBar could not restore the system proxy and will remain open.\n\n{error}\n\nCorrect the Windows proxy settings, then try Quit again."))
                        .title("Cannot safely quit ClashBar").kind(tauri_plugin_dialog::MessageDialogKind::Error).show(move |_| drop(dialog_guard));
                }
            }
        });
    }

    pub fn run() {
        tauri::Builder::default()
            .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
            .plugin(tauri_plugin_dialog::init())
            .manage(popup::PopupState::default())
            .manage(QuitState {
                finished: AtomicBool::new(false),
                pending: AtomicBool::new(false),
            })
            .setup(|app| {
                let directory = app.path().app_data_dir()?;
                let engine = Engine::new(directory).map_err(std::io::Error::other)?;
                let shared = Arc::new(Mutex::new(engine));
                app.manage(shared.clone());
                popup::configure_windows(app.handle()).map_err(std::io::Error::other)?;
                let show_item = tauri::menu::MenuItem::with_id(
                    app,
                    "show",
                    "Open ClashBar",
                    true,
                    None::<&str>,
                )?;
                let quit_item =
                    tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
                let settings_item = tauri::menu::MenuItem::with_id(
                    app,
                    "settings",
                    "System settings",
                    true,
                    None::<&str>,
                )?;
                let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
                let menu = tauri::menu::Menu::with_items(
                    app,
                    &[&show_item, &settings_item, &separator, &quit_item],
                )?;
                let mut tray = tauri::tray::TrayIconBuilder::with_id(popup::TRAY_ID)
                    .tooltip("ClashBar")
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
                if let Some(icon) = app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }
                tray.build(app)?;
                let monitor_app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        let status = shared.lock().await.status();
                        if let Some(tray) = monitor_app.tray_by_id(popup::TRAY_ID) {
                            let tooltip = format!(
                                "ClashBar · {} · {}",
                                if status.running { "Running" } else { "Stopped" },
                                if status.system_proxy {
                                    "System proxy on"
                                } else {
                                    "System proxy off"
                                }
                            );
                            let _ = tray.set_tooltip(Some(tooltip));
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
                choose_core,
                import_config,
                import_subscription,
                save_settings,
                start_core,
                stop_core,
                set_system_proxy,
                get_snapshot,
                set_mode,
                select_proxy,
                test_delay,
                close_connection,
                update_provider,
                get_logs,
                clear_logs,
                restart_core,
                select_profile,
                close_all_connections,
                provider_healthcheck,
                test_group_delay,
                set_log_level,
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
