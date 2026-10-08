pub mod config;
pub mod controller;
pub mod engine;
pub mod process;
pub mod subscription;
pub mod system_proxy;

#[cfg(feature = "desktop")]
mod desktop {
    use crate::{
        controller::Snapshot,
        engine::{self, Engine, Status},
        subscription,
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
    use tauri::{AppHandle, Manager, State};
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
        let (sender, receiver) = tokio::sync::oneshot::channel();
        app.dialog()
            .file()
            .set_title(title)
            .add_filter(title, extensions)
            .pick_file(move |file| {
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
            engine.import_profile(&bytes, name)
        } else {
            Ok(engine.status())
        }
    }

    #[tauri::command]
    async fn import_subscription(url: String, state: State<'_, Shared>) -> Result<Status, String> {
        let mut engine = state.lock().await;
        if engine.status().running {
            return Err("Stop the core before importing a subscription.".into());
        }
        let bytes = subscription::download(&url).await?;
        // The full subscription URL often contains credentials and is never persisted or logged.
        let name = subscription::validate_url(&url)?
            .host_str()
            .unwrap_or("HTTPS subscription")
            .to_owned();
        engine.import_profile(&bytes, &name)
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
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
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
                    app.dialog().message(format!("ClashBar could not restore the system proxy and will remain open.\n\n{error}\n\nCorrect the Windows proxy settings, then try Quit again."))
                        .title("Cannot safely quit ClashBar").kind(tauri_plugin_dialog::MessageDialogKind::Error).show(|_| {});
                }
            }
        });
    }

    pub fn run() {
        tauri::Builder::default()
            .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
            .plugin(tauri_plugin_dialog::init())
            .manage(QuitState {
                finished: AtomicBool::new(false),
                pending: AtomicBool::new(false),
            })
            .setup(|app| {
                let directory = app.path().app_data_dir()?;
                let engine = Engine::new(directory).map_err(std::io::Error::other)?;
                let shared = Arc::new(Mutex::new(engine));
                app.manage(shared.clone());
                let show_item = tauri::menu::MenuItem::with_id(
                    app,
                    "show",
                    "Open ClashBar",
                    true,
                    None::<&str>,
                )?;
                let quit_item =
                    tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
                let menu = tauri::menu::Menu::with_items(app, &[&show_item, &quit_item])?;
                let mut tray = tauri::tray::TrayIconBuilder::new()
                    .tooltip("ClashBar")
                    .menu(&menu)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => show(app),
                        "quit" => quit(app),
                        _ => {}
                    });
                if let Some(icon) = app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }
                tray.build(app)?;
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        shared.lock().await.refresh();
                    }
                });
                Ok(())
            })
            .on_window_event(|window, event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
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
                get_logs
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
