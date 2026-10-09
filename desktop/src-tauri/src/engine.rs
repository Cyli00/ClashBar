use crate::{
    config::{self, CoreBooleanSetting, Profile, Settings},
    controller::Controller,
    process::{self, Logs, ManagedChild},
    remote::{self, RemoteInput, RemoteMachine, RemoteSummary},
    streams::LogStream,
    system_proxy::SystemProxy,
};
use serde::Serialize;
use std::{
    fs,
    net::{Ipv4Addr, TcpListener, UdpSocket},
    path::{Path, PathBuf},
    time::Duration,
};

mod services;
pub use services::ProfileMonitor;
mod background;
mod local;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    pub core_path: Option<String>,
    pub config_name: Option<String>,
    pub mixed_port: u16,
    pub controller_port: u16,
    pub system_proxy: bool,
    pub version: Option<String>,
    pub last_error: Option<String>,
    pub profiles: Vec<Profile>,
    pub active_profile_id: Option<String>,
    pub active_remote_id: Option<String>,
    pub remote_machines: Vec<RemoteSummary>,
    pub controller_address: String,
    pub target_name: String,
    pub local_running: bool,
    pub local_mixed_port: u16,
    pub log_stream_error: Option<String>,
    pub system_proxy_target: Option<String>,
    pub system_proxy_remote: bool,
    pub target_revision: u64,
    pub auto_start_core: bool,
    pub launch_at_login: Option<bool>,
    pub launch_at_login_error: Option<String>,
    pub local_proxy_ports: config::ProxyPorts,
    pub system_proxy_exceptions: Vec<String>,
    pub tun_enabled: bool,
    pub tun_stack: Option<String>,
    pub ssid_enabled: bool,
    pub ssid_rules: Vec<crate::ssid::SsidRule>,
    pub ssid_snapshot: crate::ssid::SsidSnapshot,
    pub ssid_error: Option<String>,
    pub subscriptions: Vec<crate::subscriptions::SubscriptionSummary>,
    pub status_bar_style: String,
    pub ui_language: String,
    pub provider_refresh: crate::providers::ProviderRefreshStatus,
    pub network_status: crate::network::NetworkStatus,
    pub local_lan_address: Option<String>,
}

pub struct Engine {
    directory: PathBuf,
    settings: Settings,
    child: Option<ManagedChild>,
    controller: Option<Controller>,
    system_proxy: SystemProxy,
    pub logs: Logs,
    version: Option<String>,
    pub last_error: Option<String>,
    remote_controller: Option<Controller>,
    remote_version: Option<String>,
    remote_error: Option<String>,
    remote_logs: Option<LogStream>,
    proxy_uses_local: bool,
    proxy_target: Option<String>,
    proxy_remote_id: Option<String>,
    target_revision: u64,
    ssid_snapshot: crate::ssid::SsidSnapshot,
    ssid_error: Option<String>,
    ssid_checked: Option<std::time::Instant>,
    telemetry: Option<crate::streams::TelemetryStreams>,
    background: background::BackgroundServices,
}

impl Engine {
    pub fn new(directory: PathBuf) -> Result<Self, String> {
        let system_proxy = SystemProxy::new(directory.clone());
        // Recover connectivity even when settings or runtime storage are damaged.
        #[cfg(windows)]
        let mut last_error = system_proxy.recover().err();
        #[cfg(not(windows))]
        let mut last_error = None;
        fs::create_dir_all(directory.join("runtime/providers/proxy-providers"))
            .map_err(|e| e.to_string())?;
        fs::create_dir_all(directory.join("runtime/providers/rule-providers"))
            .map_err(|e| e.to_string())?;
        let mut settings = Settings::load(&directory)?;
        fs::create_dir_all(directory.join("profiles")).map_err(|e| e.to_string())?;
        // Upgrade the first preview's single slot without losing its source YAML.
        if settings.active_profile_id.is_none() && settings.profiles.is_empty() {
            if let Some(name) = settings.config_name.clone() {
                let migrated = (|| -> Result<Settings, String> {
                    let bytes = read_profile(&directory.join("profile.yaml"))?;
                    config::runtime(&bytes, &settings, "migration-validation")?;
                    let id = config::profile_id(&bytes);
                    config::atomic_write(
                        &directory.join("profiles").join(format!("{id}.yaml")),
                        &bytes,
                    )?;
                    let mut upgraded = settings.clone();
                    upgraded.profiles.push(Profile {
                        id: id.clone(),
                        name,
                    });
                    upgraded.active_profile_id = Some(id);
                    upgraded.save(&directory)?;
                    Ok(upgraded)
                })();
                match migrated {
                    Ok(upgraded) => settings = upgraded,
                    Err(error) => {
                        // Keep the UI available so the user can re-import a missing/damaged slot.
                        settings.config_name = None;
                        let message = format!(
                            "Could not recover the previous profile: {error} Import it again."
                        );
                        last_error = Some(match last_error {
                            Some(previous) => format!("{previous} {message}"),
                            None => message,
                        });
                    }
                }
            }
        }
        let logs = process::logs();
        let mut background = background::BackgroundServices::new(&directory)?;
        if let Err(error) = background.restore_logs(&logs) {
            last_error = Some(error);
        }
        Ok(Self {
            directory,
            settings,
            child: None,
            controller: None,
            system_proxy,
            logs,
            version: None,
            last_error,
            remote_controller: None,
            remote_version: None,
            remote_error: None,
            remote_logs: None,
            proxy_uses_local: true,
            proxy_target: None,
            proxy_remote_id: None,
            target_revision: 0,
            ssid_snapshot: crate::ssid::SsidSnapshot::default(),
            ssid_error: None,
            ssid_checked: None,
            telemetry: None,
            background,
        })
    }

    pub fn status(&mut self) -> Status {
        self.refresh();
        #[cfg(windows)]
        let system_proxy = match self.system_proxy.is_enabled() {
            Ok(enabled) => enabled,
            Err(error) => {
                self.last_error = Some(error);
                false
            }
        };
        #[cfg(not(windows))]
        let system_proxy = false;
        let remote = self.active_remote();
        Status {
            running: if remote.is_some() {
                self.remote_version.is_some()
            } else {
                self.child.is_some()
            },
            core_path: self
                .settings
                .core_path
                .as_ref()
                .map(|path| path.display().to_string()),
            config_name: self.settings.config_name.clone(),
            mixed_port: self.settings.mixed_port,
            controller_port: self.settings.controller_port,
            system_proxy,
            version: if remote.is_some() {
                self.remote_version.clone()
            } else {
                self.version.clone()
            },
            last_error: if remote.is_some() {
                self.remote_error.clone()
            } else {
                self.last_error.clone()
            },
            profiles: self.settings.profiles.clone(),
            active_profile_id: self.settings.active_profile_id.clone(),
            active_remote_id: self.settings.active_remote_id.clone(),
            remote_machines: self
                .settings
                .remote_machines
                .iter()
                .map(RemoteMachine::summary)
                .collect(),
            controller_address: remote
                .map(|machine| machine.summary().address)
                .unwrap_or_else(|| format!("127.0.0.1:{}", self.settings.controller_port)),
            target_name: remote
                .map(|machine| machine.name.clone())
                .unwrap_or_else(|| "本机".into()),
            local_running: self.child.is_some(),
            local_mixed_port: self.settings.mixed_port,
            log_stream_error: self.remote_logs.as_ref().and_then(LogStream::error),
            system_proxy_target: if system_proxy {
                self.proxy_target.clone()
            } else {
                None
            },
            system_proxy_remote: system_proxy && !self.proxy_uses_local,
            target_revision: self.target_revision,
            auto_start_core: self.settings.auto_start_core,
            launch_at_login: None,
            launch_at_login_error: None,
            local_proxy_ports: self.settings.proxy_ports(),
            system_proxy_exceptions: self.settings.system_proxy_exceptions.clone(),
            tun_enabled: self.settings.core_preferences.tun_enabled,
            tun_stack: self.settings.core_preferences.tun_stack.clone(),
            ssid_enabled: self.settings.ssid_enabled,
            ssid_rules: self.settings.ssid_rules.clone(),
            ssid_snapshot: self.ssid_snapshot.clone(),
            ssid_error: self.ssid_error.clone(),
            subscriptions: self
                .settings
                .subscriptions
                .iter()
                .map(crate::subscriptions::RemoteSubscription::summary)
                .collect(),
            status_bar_style: self.settings.status_bar_style.clone(),
            ui_language: self.settings.ui_language.clone(),
            provider_refresh: self.provider_refresh_status(),
            network_status: self.background.network_status,
            local_lan_address: self.background.local_lan_address.clone(),
        }
    }

    fn active_remote(&self) -> Option<&RemoteMachine> {
        self.settings.active_remote_id.as_ref().and_then(|id| {
            self.settings
                .remote_machines
                .iter()
                .find(|machine| &machine.id == id)
        })
    }

    pub fn require_local_target(&self) -> Result<(), String> {
        if self.active_remote().is_some() {
            Err("请先切换到本机，再执行本地内核或配置操作。".into())
        } else {
            Ok(())
        }
    }

    pub fn set_core_autostart(&mut self, enabled: bool) -> Result<Status, String> {
        let mut settings = self.settings.clone();
        settings.auto_start_core = enabled;
        settings.save(&self.directory)?;
        self.settings = settings;
        Ok(self.status())
    }

    pub async fn auto_start_if_configured(&mut self) -> Result<bool, String> {
        if !self.settings.auto_start_core
            || self.active_remote().is_some()
            || self.settings.core_path.is_none()
            || self.settings.active_profile_id.is_none()
        {
            return Ok(false);
        }
        self.start().await?;
        Ok(true)
    }

    pub fn remote_machine(&self, id: &str) -> Result<RemoteMachine, String> {
        self.settings
            .remote_machines
            .iter()
            .find(|machine| machine.id == id)
            .cloned()
            .ok_or_else(|| "远程机器不存在。".into())
    }

    pub async fn target_status(&mut self) -> Status {
        if let Some(machine) = self.active_remote().cloned() {
            let connectivity = remote::probe(&machine).await;
            self.remote_version = connectivity.version;
            self.remote_error = connectivity.error;
            if connectivity.connected && self.remote_logs.is_none() {
                if let Ok(controller) = self.controller() {
                    self.remote_logs = Some(LogStream::start(controller, "info", None));
                }
            }
        }
        self.status()
    }

    fn reset_target_session(&mut self) {
        self.cancel_background_intent();
        self.telemetry = None;
        self.ssid_checked = None;
        self.ssid_snapshot = crate::ssid::SsidSnapshot::default();
        self.target_revision = self.target_revision.wrapping_add(1);
        self.remote_controller = None;
        self.remote_version = None;
        self.remote_error = None;
        self.remote_logs = None;
        if let Ok(mut logs) = self.logs.lock() {
            logs.clear();
        }
    }

    pub async fn save_remote_machine(&mut self, input: RemoteInput) -> Result<Status, String> {
        let previous = input
            .id
            .as_ref()
            .map(|id| self.remote_machine(id))
            .transpose()?;
        let candidate = RemoteMachine::from_input(input, previous.as_ref())?;
        let active = self.settings.active_remote_id.as_deref() == Some(candidate.id.as_str());
        if active {
            let connectivity = remote::probe(&candidate).await;
            if !connectivity.connected {
                return Err(connectivity
                    .error
                    .unwrap_or_else(|| "无法连接远程机器。".into()));
            }
        }
        let mut settings = self.settings.clone();
        if let Some(machine) = settings
            .remote_machines
            .iter_mut()
            .find(|machine| machine.id == candidate.id)
        {
            *machine = candidate;
        } else {
            if settings.remote_machines.len() >= 64 {
                return Err("最多保存 64 台远程机器。".into());
            }
            settings.remote_machines.push(candidate);
        }
        settings.save(&self.directory)?;
        self.settings = settings;
        if active {
            self.reset_target_session();
        }
        Ok(self.status())
    }

    pub async fn select_machine(&mut self, id: Option<String>) -> Result<Status, String> {
        if self.settings.active_remote_id == id {
            return Ok(self.target_status().await);
        }
        let connectivity = if let Some(id) = &id {
            let result = remote::probe(&self.remote_machine(id)?).await;
            if !result.connected {
                return Err(result.error.unwrap_or_else(|| "无法连接远程机器。".into()));
            }
            Some(result)
        } else {
            None
        };
        let mut settings = self.settings.clone();
        settings.active_remote_id = id;
        settings.save(&self.directory)?;
        self.settings = settings;
        self.reset_target_session();
        self.remote_version = connectivity.and_then(|status| status.version);
        if self.active_remote().is_some() {
            self.remote_logs = Some(LogStream::start(self.controller()?, "info", None));
        }
        Ok(self.status())
    }

    pub fn delete_remote_machine(&mut self, id: &str) -> Result<Status, String> {
        self.remote_machine(id)?;
        let active = self.settings.active_remote_id.as_deref() == Some(id);
        let mut settings = self.settings.clone();
        settings.remote_machines.retain(|machine| machine.id != id);
        if active {
            settings.active_remote_id = None;
        }
        settings.save(&self.directory)?;
        self.settings = settings;
        if active {
            self.reset_target_session();
        }
        Ok(self.status())
    }

    pub async fn target_snapshot(&mut self) -> Result<crate::controller::Snapshot, String> {
        let controller = self.controller()?;
        let mut snapshot = controller.snapshot().await?;
        if self.telemetry.is_none() {
            self.telemetry = Some(crate::streams::TelemetryStreams::start(controller.clone()));
        }
        if let Some(streams) = &self.telemetry {
            streams.apply_to_snapshot(&mut snapshot);
        }
        if self.active_remote().is_some() {
            let level = snapshot
                .configs
                .get("log-level")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("info");
            self.update_remote_log_level(controller, level);
        }
        Ok(snapshot)
    }

    fn update_remote_log_level(&mut self, controller: Controller, level: &str) {
        if self
            .remote_logs
            .as_ref()
            .is_some_and(|stream| stream.level == level)
        {
            return;
        }
        let retained = self.remote_logs.take().map(|stream| stream.logs.clone());
        self.remote_logs = Some(LogStream::start(controller, level, retained));
    }

    pub async fn set_log_level(&mut self, level: &str) -> Result<(), String> {
        let controller = self.controller()?;
        if self.active_remote().is_some() {
            controller.set_log_level(level).await?;
            self.update_remote_log_level(controller, level);
            return Ok(());
        }
        self.persist_core_text("log-level", level).await
    }

    pub async fn save_remote_ports(&mut self, ports: config::ProxyPorts) -> Result<(), String> {
        let machine = self.active_remote().cloned().ok_or("请先选择远程机器。")?;
        ports.validate()?;
        let controller = self.controller()?;
        let previous = controller.proxy_port_settings().await?;
        let sync_proxy = self.proxy_remote_id.as_deref() == Some(machine.id.as_str())
            && self.system_proxy.is_enabled()?;
        if sync_proxy && ports.system_ports() == (None, None) {
            return Err("请先关闭本机系统代理，再关闭远程内核的全部代理端口。".into());
        }
        let result = async {
            controller.set_proxy_ports(&ports).await?;
            if sync_proxy {
                let (http, socks) = ports.system_ports();
                self.system_proxy.enable_with_exceptions(
                    &machine.host,
                    http,
                    socks,
                    &self.settings.system_proxy_exceptions,
                )?;
                self.proxy_target =
                    Some(format!("{}:{}", machine.host, http.or(socks).unwrap_or(0)));
            }
            Ok::<_, String>(())
        }
        .await;
        if let Err(error) = result {
            return match controller.set_proxy_ports(&previous).await {
                Ok(()) => Err(format!("远程端口保存失败，已恢复原内核端口：{error}")),
                Err(restore) => Err(format!(
                    "远程端口保存失败：{error} 原端口恢复失败：{restore}"
                )),
            };
        }
        Ok(())
    }

    pub fn refresh(&mut self) {
        let exited = self.child.as_mut().and_then(|child| match child.exited() {
            Ok(Some(status)) => Some(format!("The core exited unexpectedly ({status}).")),
            Ok(None) => None,
            Err(error) => Some(error),
        });
        if let Some(error) = exited {
            #[cfg(windows)]
            let error = match if self.proxy_uses_local {
                self.system_proxy.restore()
            } else {
                Ok(())
            } {
                Ok(()) => error,
                Err(restore) => format!("{error} Proxy recovery failed: {restore}"),
            };
            self.child = None;
            self.controller = None;
            self.telemetry = None;
            self.version = None;
            let _ = fs::remove_file(self.directory.join("runtime/config.yaml"));
            process::append(&self.logs, &error);
            self.last_error = Some(error);
        }
    }

    fn require_stopped(&mut self) -> Result<(), String> {
        self.require_local_target()?;
        self.refresh();
        if self.child.is_some() {
            return Err("Stop the core before changing its executable, profile, or ports.".into());
        }
        Ok(())
    }

    pub fn choose_core(&mut self, path: &Path) -> Result<Status, String> {
        self.cancel_background_intent();
        self.require_stopped()?;
        let path = path
            .canonicalize()
            .map_err(|e| format!("Cannot open the selected executable: {e}"))?;
        if !path.is_file() {
            return Err("Select a mihomo executable file.".into());
        }
        #[cfg(windows)]
        if !path
            .extension()
            .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("exe"))
        {
            return Err("Select a Windows mihomo .exe file.".into());
        }
        let mut settings = self.settings.clone();
        settings.core_path = Some(path);
        settings.save(&self.directory)?;
        self.settings = settings;
        self.last_error = None;
        Ok(self.status())
    }

    fn register_profile(&mut self, bytes: &[u8], name: &str) -> Result<String, String> {
        config::runtime(bytes, &self.settings, "validation-placeholder")?;
        let mut id = config::profile_id(bytes);
        if self
            .settings
            .profiles
            .iter()
            .any(|profile| profile.id == id)
            && fs::read(self.profiles_directory().join(format!("{id}.yaml")))
                .is_ok_and(|existing| existing != bytes)
        {
            id = config::profile_id(format!("{id}:{}", config::secret()).as_bytes());
        }
        let mut settings = self.settings.clone();
        let name: String = name.chars().filter(|c| !c.is_control()).take(180).collect();
        let name = if name.trim().is_empty() {
            "Imported profile".to_owned()
        } else {
            name
        };
        if settings.active_profile_id.as_deref() == Some(id.as_str()) {
            settings.config_name = Some(name.clone());
        }
        if let Some(profile) = settings
            .profiles
            .iter_mut()
            .find(|profile| profile.id == id)
        {
            profile.name = name;
        } else {
            if settings.profiles.len() >= 128 {
                return Err("The profile library is limited to 128 profiles.".into());
            }
            settings.profiles.push(Profile {
                id: id.clone(),
                name,
            });
        }
        config::atomic_write(
            &self.directory.join("profiles").join(format!("{id}.yaml")),
            bytes,
        )?;
        settings.save(&self.directory)?;
        self.settings = settings;
        self.remember_profile_content(&id, bytes)?;
        Ok(id)
    }

    fn profile_settings(&self, id: &str) -> Result<Settings, String> {
        let profile = self
            .settings
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .ok_or("The selected profile no longer exists.")?;
        let mut settings = self.settings.clone();
        settings.active_profile_id = Some(profile.id.clone());
        settings.config_name = Some(profile.name.clone());
        Ok(settings)
    }

    fn active_profile_path(&self) -> Result<PathBuf, String> {
        let id = self
            .settings
            .active_profile_id
            .as_deref()
            .ok_or("Import a mihomo YAML profile first.")?;
        Ok(self.directory.join("profiles").join(format!("{id}.yaml")))
    }

    pub fn import_profile(&mut self, bytes: &[u8], name: &str) -> Result<Status, String> {
        self.require_stopped()?;
        let id = self.register_profile(bytes, name)?;
        let settings = self.profile_settings(&id)?;
        settings.save(&self.directory)?;
        self.settings = settings;
        self.last_error = None;
        Ok(self.status())
    }

    pub async fn import_and_activate(
        &mut self,
        bytes: &[u8],
        name: &str,
    ) -> Result<Status, String> {
        self.require_local_target()?;
        let id = self.register_profile(bytes, name)?;
        self.select_profile(&id).await
    }

    pub async fn select_profile(&mut self, id: &str) -> Result<Status, String> {
        self.cancel_background_intent();
        self.require_local_target()?;
        self.refresh();
        let settings = self.profile_settings(id)?;
        let raw = read_profile(&self.directory.join("profiles").join(format!("{id}.yaml")))?;
        config::runtime(&raw, &settings, "validation-placeholder")?;
        if self.settings.active_profile_id.as_deref() == Some(id) {
            return Ok(self.status());
        }
        let previous = self.settings.clone();
        let was_running = self.child.is_some();
        if !was_running {
            settings.save(&self.directory)?;
            self.settings = settings;
            self.last_error = None;
            return Ok(self.status());
        }
        // Run the candidate's real mihomo -t before disturbing a working connection.
        self.validate_candidate(&raw, &settings).await?;
        let proxy_enabled = self.owned_proxy_enabled()?;
        self.stop()?;
        self.settings = settings;
        let activation = match self.resume(proxy_enabled).await {
            Ok(_) => self.settings.save(&self.directory),
            Err(error) => Err(error),
        };
        if let Err(error) = activation {
            // The persisted pointer still names the old profile until activation succeeds.
            // If safe shutdown fails, keep the new working core alive for proxy recovery.
            if let Err(cleanup) = self.stop() {
                let message = format!("Could not activate the profile: {error} Could not safely stop the candidate: {cleanup}");
                self.last_error = Some(message.clone());
                return Err(message);
            }
            self.settings = previous;
            let error = match self.resume(proxy_enabled).await {
                Ok(_) => format!("Could not activate the profile: {error} The previous profile was restored."),
                Err(recovery) => format!("Could not activate the profile: {error} Previous profile recovery failed: {recovery}"),
            };
            self.last_error = Some(error.clone());
            return Err(error);
        }
        self.last_error = None;
        Ok(self.status())
    }

    pub async fn delete_profile(&mut self, id: &str) -> Result<Status, String> {
        self.require_local_target()?;
        self.refresh();
        self.profile_settings(id)?;
        let previous = self.settings.clone();
        let was_active = previous.active_profile_id.as_deref() == Some(id);
        let was_running = self.child.is_some();
        let proxy_enabled = self.owned_proxy_enabled()?;
        if was_active {
            let next = previous
                .profiles
                .iter()
                .find(|profile| profile.id != id)
                .map(|profile| profile.id.clone());
            if let Some(next) = next {
                self.select_profile(&next).await?;
            } else if was_running {
                self.stop()?;
            }
        }
        let result = (|| -> Result<(), String> {
            let source = self.directory.join("profiles").join(format!("{id}.yaml"));
            let trash = self.directory.join("deleted-profiles");
            fs::create_dir_all(&trash).map_err(|error| format!("无法创建配置回收目录：{error}"))?;
            let archived = trash.join(format!("{id}-{}.yaml", &config::secret()[..12]));
            fs::rename(&source, &archived).map_err(|error| format!("无法移除配置文件：{error}"))?;
            let mut settings = self.settings.clone();
            settings.profiles.retain(|profile| profile.id != id);
            settings
                .subscriptions
                .retain(|subscription| subscription.profile_id != id);
            if settings.profiles.is_empty() {
                settings.active_profile_id = None;
                settings.config_name = None;
            }
            if let Err(error) = settings.save(&self.directory) {
                return match fs::rename(&archived, &source) {
                    Ok(()) => Err(error),
                    Err(restore) => Err(format!("{error} 配置文件恢复失败：{restore}")),
                };
            }
            self.settings = settings;
            Ok(())
        })();
        if let Err(error) = result {
            if was_active {
                // 删除失败时恢复原选择与运行状态，源文件保留在工作目录或回收目录。
                let recovery = if self.settings.active_profile_id != previous.active_profile_id {
                    self.select_profile(id).await.map(|_| ())
                } else if was_running && self.child.is_none() {
                    self.resume(proxy_enabled).await.map(|_| ())
                } else {
                    Ok(())
                };
                if let Err(recovery) = recovery {
                    return Err(format!("{error} 原配置恢复失败：{recovery}"));
                }
            }
            return Err(error);
        }
        self.last_error = None;
        Ok(self.status())
    }

    pub fn core_directory(&self) -> Result<PathBuf, String> {
        self.require_local_target()?;
        self.settings
            .core_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .ok_or_else(|| "请先选择 mihomo 内核。".into())
    }

    pub fn profiles_directory(&self) -> PathBuf {
        self.directory.join("profiles")
    }

    async fn validate_candidate(&mut self, raw: &[u8], settings: &Settings) -> Result<(), String> {
        let executable = settings
            .core_path
            .as_ref()
            .ok_or("Choose a trusted mihomo executable first.")?;
        let secret = config::secret();
        let derived = config::runtime(raw, settings, &secret)?;
        let directory = self.directory.join("runtime");
        for kind in ["proxy-providers", "rule-providers"] {
            fs::create_dir_all(
                directory
                    .join("providers")
                    .join(config::profile_id(raw))
                    .join(kind),
            )
            .map_err(|error| error.to_string())?;
        }
        let path = directory.join("validate-profile.yaml");
        config::atomic_write(&path, &derived)?;
        let candidate =
            ManagedChild::spawn(executable, &directory, &path, true, &self.logs, &secret);
        let result = match candidate {
            Ok(child) => child.validate().await,
            Err(error) => Err(error),
        };
        let _ = fs::remove_file(path);
        result
    }

    fn owned_proxy_enabled(&self) -> Result<bool, String> {
        if !self.proxy_uses_local {
            return Ok(false);
        }
        #[cfg(windows)]
        {
            self.system_proxy.is_enabled()
        }
        #[cfg(not(windows))]
        {
            Ok(false)
        }
    }

    async fn resume(&mut self, proxy_enabled: bool) -> Result<Status, String> {
        self.start().await?;
        if proxy_enabled {
            self.set_system_proxy(true)?;
        }
        Ok(self.status())
    }

    pub async fn restart(&mut self) -> Result<Status, String> {
        let proxy_enabled = self.owned_proxy_enabled()?;
        self.stop()?;
        self.resume(proxy_enabled).await
    }

    pub fn clear_logs(&mut self) -> Result<(), String> {
        self.selected_logs()
            .lock()
            .map_err(|_| "Cannot clear core logs.")?
            .clear();
        self.clear_log_archive()?;
        Ok(())
    }

    pub fn save_settings(
        &mut self,
        mixed_port: u16,
        controller_port: u16,
    ) -> Result<Status, String> {
        self.require_stopped()?;
        config::validate_ports(mixed_port, controller_port)?;
        let mut settings = self.settings.clone();
        settings.mixed_port = mixed_port;
        settings.controller_port = controller_port;
        settings.validate_local_ports()?;
        settings.save(&self.directory)?;
        self.settings = settings;
        self.last_error = None;
        Ok(self.status())
    }

    pub async fn set_core_boolean(
        &mut self,
        setting: CoreBooleanSetting,
        value: bool,
    ) -> Result<(), String> {
        let controller = self.controller()?;
        if self.active_remote().is_some() {
            return controller.set_remote_boolean_setting(setting, value).await;
        }
        let previous = controller.boolean_setting(setting).await?;
        let mut settings = self.settings.clone();
        settings.core_preferences.set(setting, value);
        if let Err(error) = controller.set_boolean_setting(setting, value).await {
            // 超时不证明请求未生效；恢复已读取的旧值，避免误显示保存成功。
            return match controller.set_boolean_setting(setting, previous).await {
                Ok(()) => Err(error),
                Err(restore) => Err(format!("{error} 恢复内核运行设置失败：{restore}")),
            };
        }
        if let Err(error) = settings.save(&self.directory) {
            return match controller.set_boolean_setting(setting, previous).await {
                Ok(()) => Err(format!("保存设置失败，已恢复原值：{error}")),
                Err(restore) => Err(format!(
                    "保存设置失败：{error} 恢复内核运行设置失败：{restore}"
                )),
            };
        }
        self.settings = settings;
        Ok(())
    }

    pub async fn start(&mut self) -> Result<Status, String> {
        self.refresh();
        if self.child.is_some() {
            return Ok(self.status());
        }
        let result = match self.start_inner().await {
            Ok(_)
                if self.settings.desired_system_proxy
                    && self.active_remote().is_none()
                    && self.proxy_uses_local =>
            {
                self.set_system_proxy(true)
            }
            other => other,
        };
        if let Err(error) = &result {
            self.last_error = Some(error.clone());
            process::append(&self.logs, error);
            if self.child.is_none() {
                let _ = fs::remove_file(self.directory.join("runtime/config.yaml"));
            }
        }
        result
    }

    async fn start_inner(&mut self) -> Result<Status, String> {
        #[cfg(windows)]
        if self.proxy_uses_local {
            self.system_proxy.recover()?;
        }
        let executable = self
            .settings
            .core_path
            .clone()
            .ok_or("Choose a trusted mihomo executable first.")?;
        if self.settings.config_name.is_none() {
            return Err("Import a mihomo YAML profile first.".into());
        }
        let raw = read_profile(&self.active_profile_path()?)?;
        let secret = config::secret();
        let derived = config::runtime(&raw, &self.settings, &secret)?;
        let runtime_dir = self.directory.join("runtime");
        let cache = runtime_dir.join("providers").join(config::profile_id(&raw));
        for kind in ["proxy-providers", "rule-providers"] {
            fs::create_dir_all(cache.join(kind)).map_err(|e| e.to_string())?;
        }
        let path = runtime_dir.join("config.yaml");
        config::atomic_write(&path, &derived)?;
        process::append(&self.logs, "Validating the selected mihomo profile…");
        ManagedChild::spawn(&executable, &runtime_dir, &path, true, &self.logs, &secret)?
            .validate()
            .await?;
        ensure_all_ports_available(&self.settings)?;
        let controller = Controller::new(self.settings.controller_port, secret.clone())?;
        let mut child = if self.settings.core_preferences.tun_enabled {
            ManagedChild::spawn_for_tun(&executable, &runtime_dir, &path, &self.logs, &secret)?
        } else {
            ManagedChild::spawn(&executable, &runtime_dir, &path, false, &self.logs, &secret)?
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        let version = loop {
            if let Some(status) = child.exited()? {
                return Err(format!(
                    "The core exited during startup ({status}). See the core logs."
                ));
            }
            match tokio::time::timeout(Duration::from_millis(700), controller.version()).await {
                Ok(Ok(version)) => break version,
                _ if tokio::time::Instant::now() >= deadline => {
                    return Err(
                        "The core did not become ready within 30 seconds. See the core logs."
                            .into(),
                    )
                }
                _ => tokio::time::sleep(Duration::from_millis(150)).await,
            }
        };
        if self.settings.core_preferences.tun_enabled {
            let actual = controller.tun_settings().await?;
            if !actual.enable
                || self
                    .settings
                    .core_preferences
                    .tun_stack
                    .as_ref()
                    .is_some_and(|expected| {
                        actual
                            .stack
                            .as_ref()
                            .is_none_or(|value| !value.eq_ignore_ascii_case(expected))
                    })
            {
                return Err("TUN 内核运行状态与保存设置不一致，请检查权限与日志。".into());
            }
        }
        if let Some(id) = self.settings.active_profile_id.clone() {
            self.remember_profile_content(&id, &raw)?;
        }
        self.child = Some(child);
        self.start_provider_refresh(controller.clone());
        self.controller = Some(controller);
        self.version = Some(version);
        self.last_error = None;
        process::append(&self.logs, "内核已就绪。");
        Ok(self.status())
    }

    pub fn stop(&mut self) -> Result<Status, String> {
        self.cancel_background_intent();
        // Keep a working core alive if restoration fails; otherwise Windows could point at a dead proxy.
        #[cfg(windows)]
        if let Err(error) = if self.proxy_uses_local {
            self.system_proxy.restore()
        } else {
            Ok(())
        } {
            self.last_error = Some(error.clone());
            return Err(error);
        }
        if let Some(child) = self.child.as_mut() {
            child.kill()?;
        }
        self.child = None;
        self.telemetry = None;
        self.controller = None;
        self.version = None;
        self.last_error = None;
        let _ = fs::remove_file(self.directory.join("runtime/config.yaml"));
        process::append(&self.logs, "本机内核已停止。");
        Ok(self.status())
    }

    pub fn set_system_proxy(&mut self, enabled: bool) -> Result<Status, String> {
        self.refresh();
        if enabled {
            if self.child.is_none() {
                return Err("Start the core before enabling the system proxy.".into());
            }
            let (http, socks) = self.settings.proxy_ports().system_ports();
            self.system_proxy.enable_with_exceptions(
                "127.0.0.1",
                http,
                socks,
                &self.settings.system_proxy_exceptions,
            )?;
            self.proxy_uses_local = true;
            self.proxy_remote_id = None;
            self.proxy_target = Some(format!("127.0.0.1:{}", http.or(socks).unwrap_or(0)));
        } else {
            self.system_proxy.restore()?;
            self.proxy_target = None;
            self.proxy_remote_id = None;
        }
        self.last_error = None;
        Ok(self.status())
    }

    pub async fn set_selected_system_proxy(&mut self, enabled: bool) -> Result<Status, String> {
        if self.active_remote().is_none() {
            let previous = self.settings.clone();
            let mut settings = previous.clone();
            settings.desired_system_proxy = enabled;
            // 保存失败时不触碰系统代理；应用失败则恢复旧偏好，不遗留未完成的启用意图。
            settings.save(&self.directory)?;
            if let Err(error) = self.set_system_proxy(enabled) {
                return match previous.save(&self.directory) {
                    Ok(_) => Err(error),
                    Err(restore) => Err(format!("{error} 恢复系统代理偏好失败：{restore}")),
                };
            }
            self.settings = settings;
            return Ok(self.status());
        }
        if !enabled {
            return self.set_system_proxy(false);
        }
        let machine = self.active_remote().expect("remote target").clone();
        let (http, socks) = self.controller()?.proxy_ports().await?;
        self.system_proxy.enable_with_exceptions(
            &machine.host,
            http,
            socks,
            &self.settings.system_proxy_exceptions,
        )?;
        self.proxy_uses_local = false;
        self.proxy_remote_id = Some(machine.id.clone());
        self.proxy_target = Some(format!("{}:{}", machine.host, http.or(socks).unwrap_or(0)));
        self.last_error = None;
        Ok(self.status())
    }

    pub fn shutdown(&mut self) -> Result<Status, String> {
        self.flush_log_archive()?;
        #[cfg(windows)]
        self.system_proxy.restore()?;
        self.proxy_target = None;
        self.proxy_remote_id = None;
        self.remote_logs = None;
        self.stop()
    }

    pub fn controller(&mut self) -> Result<Controller, String> {
        self.refresh();
        if let Some(machine) = self.active_remote() {
            if self.remote_controller.is_none() {
                self.remote_controller = Some(Controller::remote(machine)?);
            }
            return Ok(self
                .remote_controller
                .as_ref()
                .expect("remote controller initialized")
                .clone());
        }
        self.controller
            .clone()
            .ok_or("Start the core to use the dashboard.".into())
    }

    pub fn log_lines(&self) -> Vec<String> {
        self.log_entries()
            .into_iter()
            .map(|entry| entry.message)
            .collect()
    }
    pub fn log_entries(&self) -> Vec<process::LogEntry> {
        let mut entries: Vec<_> = self
            .selected_logs()
            .lock()
            .map(|entries| entries.iter().cloned().collect())
            .unwrap_or_default();
        if self.active_remote().is_some() {
            if let Ok(local) = self.logs.lock() {
                entries.extend(
                    local
                        .iter()
                        .filter(|entry| entry.source == "ClashBar")
                        .cloned(),
                );
            }
        }
        entries.sort_by_key(|entry| entry.timestamp);
        if entries.len() > 500 {
            entries.drain(..entries.len() - 500);
        }
        entries
    }

    fn selected_logs(&self) -> Logs {
        if self.active_remote().is_some() {
            self.remote_logs
                .as_ref()
                .map(|stream| stream.logs.clone())
                .unwrap_or_else(process::logs)
        } else {
            self.logs.clone()
        }
    }
}

pub fn read_profile(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file =
        fs::File::open(path).map_err(|e| format!("Cannot read the selected profile: {e}"))?;
    let mut bytes = Vec::new();
    file.take((config::MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > config::MAX_CONFIG_BYTES {
        return Err("The profile exceeds the 8 MiB limit.".into());
    }
    Ok(bytes)
}

#[cfg(test)]
fn ensure_ports_available(mixed: u16, controller: u16) -> Result<(), String> {
    let _mixed = TcpListener::bind((Ipv4Addr::LOCALHOST, mixed))
        .map_err(|_| format!("Proxy port {mixed} is already in use. Change it in Settings."))?;
    let _controller = TcpListener::bind((Ipv4Addr::LOCALHOST, controller)).map_err(|_| {
        format!("Controller port {controller} is already in use. Change it in Settings.")
    })?;
    let _udp = UdpSocket::bind((Ipv4Addr::LOCALHOST, mixed))
        .map_err(|_| format!("UDP proxy port {mixed} is already in use. Change it in Settings."))?;
    Ok(())
}

fn ensure_all_ports_available(settings: &Settings) -> Result<(), String> {
    settings.validate_local_ports()?;
    let ports = settings.proxy_ports();
    let mut tcp = Vec::new();
    let mut udp = Vec::new();
    let bind = if settings.core_preferences.allow_lan {
        Ipv4Addr::UNSPECIFIED
    } else {
        Ipv4Addr::LOCALHOST
    };
    for port in [
        ports.port,
        ports.socks_port,
        ports.mixed_port,
        ports.redir_port,
        ports.tproxy_port,
    ]
    .into_iter()
    .filter(|port| *port > 0)
    {
        tcp.push(
            TcpListener::bind((bind, port))
                .map_err(|_| format!("代理 TCP 端口 {port} 已被占用，请在设置中更换。"))?,
        );
    }
    tcp.push(
        TcpListener::bind((Ipv4Addr::LOCALHOST, settings.controller_port))
            .map_err(|_| format!("控制端口 {} 已被占用。", settings.controller_port))?,
    );
    for port in [ports.socks_port, ports.mixed_port, ports.tproxy_port]
        .into_iter()
        .filter(|port| *port > 0)
    {
        udp.push(
            UdpSocket::bind((bind, port))
                .map_err(|_| format!("代理 UDP 端口 {port} 已被占用，请在设置中更换。"))?,
        );
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    #[test]
    fn importing_invalid_profile_preserves_last_good_profile() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(dir.path().to_path_buf()).unwrap();
        engine
            .import_profile(b"proxies: []\nrules: [MATCH,DIRECT]", "good.yaml")
            .unwrap();
        assert!(engine.import_profile(b"hello", "bad.yaml").is_err());
        assert_eq!(engine.status().config_name.as_deref(), Some("good.yaml"));
        assert!(
            String::from_utf8(fs::read(engine.active_profile_path().unwrap()).unwrap())
                .unwrap()
                .starts_with("proxies:")
        );
    }
    #[test]
    fn catches_port_conflict_without_starting_a_core() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(ensure_ports_available(port, if port == 19090 { 19091 } else { 19090 }).is_err());
    }

    #[test]
    fn upgrades_the_preview_single_profile_without_modifying_its_source() {
        let dir = tempfile::tempdir().unwrap();
        let raw = b"proxies: []\nrules: []\n";
        fs::write(dir.path().join("profile.yaml"), raw).unwrap();
        let settings = Settings {
            config_name: Some("legacy.yaml".into()),
            ..Settings::default()
        };
        settings.save(dir.path()).unwrap();
        let mut engine = Engine::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(engine.status().profiles.len(), 1);
        assert_eq!(
            fs::read(engine.active_profile_path().unwrap()).unwrap(),
            raw
        );
        assert_eq!(fs::read(dir.path().join("profile.yaml")).unwrap(), raw);
        drop(engine);
        assert_eq!(
            Engine::new(dir.path().to_path_buf())
                .unwrap()
                .status()
                .profiles
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn retains_profiles_and_rejects_unknown_ids_without_changing_selection() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(dir.path().to_path_buf()).unwrap();
        let first = engine
            .import_profile(b"proxies: []\nrules: []", "first.yaml")
            .unwrap();
        let first_id = first.active_profile_id.unwrap();
        engine
            .import_profile(b"proxies: []\nrules: [MATCH,DIRECT]", "second.yaml")
            .unwrap();
        assert_eq!(engine.status().profiles.len(), 2);
        let selected = engine.select_profile(&first_id).await.unwrap();
        assert_eq!(selected.config_name.as_deref(), Some("first.yaml"));
        assert!(engine.select_profile("../../settings").await.is_err());
        assert_eq!(
            engine.status().active_profile_id.as_deref(),
            Some(first_id.as_str())
        );
        engine
            .import_profile(b"proxies: []\nrules: []", "renamed.yaml")
            .unwrap();
        assert_eq!(engine.status().profiles.len(), 2);
        assert_eq!(engine.status().config_name.as_deref(), Some("renamed.yaml"));
        assert_eq!(
            Settings::load(dir.path()).unwrap().config_name.as_deref(),
            Some("renamed.yaml")
        );
    }

    pub(crate) fn fixture() -> &'static Path {
        static FIXTURE: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        FIXTURE
            .get_or_init(|| {
                let directory = tempfile::tempdir().unwrap();
                let source = directory.path().join("fake_core.rs");
                fs::write(&source, include_str!("../fixtures/fake_core.rs")).unwrap();
                let output = directory.path().join(if cfg!(windows) {
                    "mihomo.exe"
                } else {
                    "mihomo"
                });
                let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
                assert!(std::process::Command::new(rustc)
                    .arg(source)
                    .arg("-o")
                    .arg(output)
                    .status()
                    .unwrap()
                    .success());
                directory
            })
            .path()
    }

    pub(super) fn configured_engine(profile: &[u8]) -> (tempfile::TempDir, Engine) {
        let directory = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(directory.path().to_path_buf()).unwrap();
        let core = fixture().join(if cfg!(windows) {
            "mihomo.exe"
        } else {
            "mihomo"
        });
        engine.choose_core(&core).unwrap();
        let port = || {
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .unwrap()
                .local_addr()
                .unwrap()
                .port()
        };
        let mixed = port();
        let mut controller = port();
        while mixed == controller {
            controller = port();
        }
        engine.save_settings(mixed, controller).unwrap();
        engine.import_profile(profile, "fixture.yaml").unwrap();
        (directory, engine)
    }

    #[tokio::test]
    async fn core_autostart_is_opt_in_and_survives_app_reload() {
        let (directory, mut engine) = configured_engine(b"proxies: []\n");
        assert!(!engine.auto_start_if_configured().await.unwrap());
        assert!(!engine.status().running);
        engine.set_core_autostart(true).unwrap();
        assert!(!engine.status().running);
        assert!(Settings::load(directory.path()).unwrap().auto_start_core);
        let mut reloaded = Engine::new(directory.path().to_path_buf()).unwrap();
        assert!(reloaded.auto_start_if_configured().await.unwrap());
        assert!(reloaded.status().running);
        reloaded.stop().unwrap();
    }

    #[tokio::test]
    async fn autostart_skips_unconfigured_and_remote_targets() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(dir.path().to_path_buf()).unwrap();
        engine.set_core_autostart(true).unwrap();
        assert!(!engine.auto_start_if_configured().await.unwrap());
        let (_dir, mut engine) = configured_engine(b"proxies: []\n");
        engine.set_core_autostart(true).unwrap();
        let machine = RemoteMachine::from_input(
            RemoteInput {
                id: None,
                name: "远程".into(),
                host: "127.0.0.1".into(),
                port: 1,
                use_https: false,
                secret: None,
            },
            None,
        )
        .unwrap();
        engine.settings.active_remote_id = Some(machine.id.clone());
        engine.settings.remote_machines.push(machine);
        assert!(!engine.auto_start_if_configured().await.unwrap());
        assert!(!engine.status().local_running);
    }

    #[tokio::test]
    async fn remote_target_routes_reads_writes_and_logs_without_mutating_local_preferences() {
        let (_remote_dir, mut remote_engine) =
            configured_engine(b"proxies: []\nmode: direct\nfixture-remote: true\n");
        remote_engine.start().await.unwrap();
        let (directory, mut local) = configured_engine(b"proxies: []\nipv6: true\n");
        local.start().await.unwrap();
        let remote_port = remote_engine.settings.controller_port;
        local
            .save_remote_machine(RemoteInput {
                id: None,
                name: "测试路由器".into(),
                host: "127.0.0.1".into(),
                port: remote_port,
                use_https: false,
                secret: Some("remote-fixture-secret".into()),
            })
            .await
            .unwrap();
        let id = local.status().remote_machines[0].id.clone();
        let status = local.select_machine(Some(id.clone())).await.unwrap();
        assert!(status.local_running && status.running);
        assert_eq!(status.version.as_deref(), Some("remote-fixture"));
        assert!(local.require_local_target().is_err());
        assert!(local.save_settings(7890, 19090).is_err());
        assert!(local
            .import_and_activate(b"proxies: []", "blocked.yaml")
            .await
            .is_err());
        assert_eq!(
            local.target_snapshot().await.unwrap().configs["mode"],
            "direct"
        );
        local
            .controller()
            .unwrap()
            .set_mode("global")
            .await
            .unwrap();
        assert_eq!(
            remote_engine.target_snapshot().await.unwrap().configs["mode"],
            "global"
        );
        assert_eq!(
            local
                .controller
                .as_ref()
                .unwrap()
                .snapshot()
                .await
                .unwrap()
                .configs["mode"],
            "rule"
        );
        local
            .set_core_boolean(CoreBooleanSetting::Ipv6, false)
            .await
            .unwrap();
        local
            .save_remote_ports(config::ProxyPorts {
                port: 8888,
                socks_port: 8889,
                mixed_port: 0,
                redir_port: 0,
                tproxy_port: 0,
            })
            .await
            .unwrap();
        assert_eq!(
            remote_engine
                .controller()
                .unwrap()
                .proxy_ports()
                .await
                .unwrap(),
            (Some(8888), Some(8889))
        );
        assert_ne!(local.settings.mixed_port, 0);
        assert_eq!(
            Settings::load(directory.path())
                .unwrap()
                .core_preferences
                .ipv6,
            None
        );
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if local
                    .log_entries()
                    .iter()
                    .any(|entry| entry.message.contains("remote log"))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!serde_json::to_string(&local.status())
            .unwrap()
            .contains("remote-fixture-secret"));
        let status = local.delete_remote_machine(&id).unwrap();
        assert!(status.running && status.local_running && status.active_remote_id.is_none());
        assert!(local.remote_logs.is_none());
        assert!(local.log_entries().is_empty());
        local.stop().unwrap();
        remote_engine.stop().unwrap();
    }

    #[tokio::test]
    async fn failed_remote_selection_keeps_local_target_and_saved_credentials_are_not_returned() {
        let (directory, mut local) = configured_engine(b"proxies: []\n");
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        local
            .save_remote_machine(RemoteInput {
                id: None,
                name: "离线机器".into(),
                host: "127.0.0.1".into(),
                port,
                use_https: false,
                secret: Some("offline-fixture-secret".into()),
            })
            .await
            .unwrap();
        let id = local.status().remote_machines[0].id.clone();
        assert!(local.select_machine(Some(id.clone())).await.is_err());
        assert!(local.status().active_remote_id.is_none());
        let saved = Settings::load(directory.path()).unwrap();
        assert_eq!(saved.remote_machines[0].secret, "offline-fixture-secret");
        assert!(saved.active_remote_id.is_none());
        local
            .save_remote_machine(RemoteInput {
                id: Some(id.clone()),
                name: "已编辑".into(),
                host: "127.0.0.1".into(),
                port,
                use_https: true,
                secret: None,
            })
            .await
            .unwrap();
        assert_eq!(
            local.remote_machine(&id).unwrap().secret,
            "offline-fixture-secret"
        );
        assert!(!format!("{:?}", local.status()).contains("offline-fixture-secret"));
        local
            .save_remote_machine(RemoteInput {
                id: Some(id.clone()),
                name: "已编辑".into(),
                host: "127.0.0.1".into(),
                port,
                use_https: true,
                secret: Some(String::new()),
            })
            .await
            .unwrap();
        assert!(local.remote_machine(&id).unwrap().secret.is_empty());
    }

    #[tokio::test]
    async fn validates_starts_and_stops_a_managed_process() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        let status = engine.start().await.unwrap();
        assert!(status.running);
        assert_eq!(status.version.as_deref(), Some("fixture"));
        assert!(engine.save_settings(7890, 19090).is_err());
        assert!(engine.import_profile(b"proxies: []", "other").is_err());
        assert!(!engine.stop().unwrap().running);
        assert!(!directory.path().join("runtime/config.yaml").exists());
    }

    #[tokio::test]
    async fn deleting_active_profile_switches_then_archives_and_last_delete_stops() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        let first = engine.status().active_profile_id.unwrap();
        engine
            .import_profile(b"proxies: []\nrules: [MATCH,DIRECT]\n", "second.yaml")
            .unwrap();
        let second = engine.status().active_profile_id.unwrap();
        engine.start().await.unwrap();
        let status = engine.delete_profile(&second).await.unwrap();
        assert!(status.running);
        assert_eq!(status.active_profile_id.as_deref(), Some(first.as_str()));
        assert!(!directory
            .path()
            .join("profiles")
            .join(format!("{second}.yaml"))
            .exists());
        assert_eq!(
            fs::read_dir(directory.path().join("deleted-profiles"))
                .unwrap()
                .count(),
            1
        );
        let status = engine.delete_profile(&first).await.unwrap();
        assert!(!status.running);
        assert!(status.active_profile_id.is_none());
        assert!(status.profiles.is_empty());
        assert!(Settings::load(directory.path())
            .unwrap()
            .config_name
            .is_none());
        assert!(engine.delete_profile("../../outside").await.is_err());
    }

    #[tokio::test]
    async fn failed_profile_delete_restores_original_selection_and_core() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        engine
            .import_profile(b"proxies: []\nrules: [MATCH,DIRECT]\n", "second.yaml")
            .unwrap();
        let active = engine.status().active_profile_id.unwrap();
        engine.start().await.unwrap();
        fs::write(
            directory.path().join("deleted-profiles"),
            "block archive creation",
        )
        .unwrap();
        assert!(engine.delete_profile(&active).await.is_err());
        assert_eq!(
            engine.status().active_profile_id.as_deref(),
            Some(active.as_str())
        );
        assert_eq!(
            Settings::load(directory.path())
                .unwrap()
                .active_profile_id
                .as_deref(),
            Some(active.as_str())
        );
        assert_eq!(engine.status().profiles.len(), 2);
        assert!(engine.status().running);
        engine.stop().unwrap();
    }

    #[tokio::test]
    async fn failed_profile_metadata_save_restores_archived_file() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        let inactive = engine.status().active_profile_id.unwrap();
        engine
            .import_profile(b"proxies: []\nrules: [MATCH,DIRECT]\n", "second.yaml")
            .unwrap();
        fs::create_dir(directory.path().join("settings.tmp")).unwrap();
        assert!(engine.delete_profile(&inactive).await.is_err());
        assert!(directory
            .path()
            .join("profiles")
            .join(format!("{inactive}.yaml"))
            .exists());
        assert_eq!(engine.status().profiles.len(), 2);
        assert_eq!(Settings::load(directory.path()).unwrap().profiles.len(), 2);
    }

    #[tokio::test]
    async fn core_boolean_changes_survive_restart_and_disk_reload() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nipv6: true\n");
        engine.start().await.unwrap();
        engine
            .set_core_boolean(CoreBooleanSetting::Ipv6, false)
            .await
            .unwrap();
        engine
            .set_core_boolean(CoreBooleanSetting::AllowLan, true)
            .await
            .unwrap();
        let saved = Settings::load(directory.path()).unwrap();
        assert_eq!(saved.core_preferences.ipv6, Some(false));
        assert!(saved.core_preferences.allow_lan);
        engine.restart().await.unwrap();
        let controller = engine.controller().unwrap();
        assert!(!controller
            .boolean_setting(CoreBooleanSetting::Ipv6)
            .await
            .unwrap());
        assert!(controller
            .boolean_setting(CoreBooleanSetting::AllowLan)
            .await
            .unwrap());
        engine.stop().unwrap();
    }

    #[tokio::test]
    async fn failed_preference_save_restores_runtime_and_keeps_saved_settings() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nipv6: true\n");
        engine.start().await.unwrap();
        fs::create_dir(directory.path().join("settings.tmp")).unwrap();
        let error = engine
            .set_core_boolean(CoreBooleanSetting::Ipv6, false)
            .await
            .unwrap_err();
        assert!(error.contains("已恢复原值"));
        assert!(engine
            .controller()
            .unwrap()
            .boolean_setting(CoreBooleanSetting::Ipv6)
            .await
            .unwrap());
        assert_eq!(
            Settings::load(directory.path())
                .unwrap()
                .core_preferences
                .ipv6,
            None
        );
        engine.stop().unwrap();
    }

    #[tokio::test]
    async fn rejected_boolean_patch_does_not_persist_or_change_other_settings() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nfixture-reject-tcp: true\n");
        engine.start().await.unwrap();
        assert!(engine
            .set_core_boolean(CoreBooleanSetting::TcpConcurrent, true)
            .await
            .is_err());
        assert!(!engine
            .controller()
            .unwrap()
            .boolean_setting(CoreBooleanSetting::TcpConcurrent)
            .await
            .unwrap());
        assert_eq!(
            Settings::load(directory.path())
                .unwrap()
                .core_preferences
                .tcp_concurrent,
            None
        );
        engine.stop().unwrap();
    }

    #[tokio::test]
    async fn validation_failure_never_marks_core_running() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nfixture-invalid: true\n");
        assert!(engine
            .start()
            .await
            .unwrap_err()
            .contains("configuration check failed"));
        assert!(!engine.status().running);
        assert!(!directory.path().join("runtime/config.yaml").exists());
    }

    #[tokio::test]
    async fn startup_exit_is_detected_and_runtime_secret_removed() {
        let (directory, mut engine) =
            configured_engine(b"proxies: []\nfixture-startup-exit: true\n");
        assert!(engine
            .start()
            .await
            .unwrap_err()
            .contains("exited during startup"));
        assert!(!engine.status().running);
        assert!(!directory.path().join("runtime/config.yaml").exists());
    }

    #[tokio::test]
    async fn detects_unexpected_core_exit() {
        let (_directory, mut engine) =
            configured_engine(b"proxies: []\nfixture-exit-after-request: true\n");
        engine.start().await.unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        let status = engine.status();
        assert!(!status.running);
        assert!(status.last_error.unwrap().contains("exited unexpectedly"));
    }

    #[tokio::test]
    async fn failed_live_profile_switch_restores_previous_running_core() {
        let (_directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        engine.start().await.unwrap();
        let original = engine.status().active_profile_id.unwrap();
        let error = engine
            .import_and_activate(b"proxies: []\nfixture-invalid: true\n", "invalid.yaml")
            .await
            .unwrap_err();
        assert!(error.contains("configuration check failed"));
        let status = engine.status();
        assert!(status.running);
        assert_eq!(status.active_profile_id.as_deref(), Some(original.as_str()));
        assert_eq!(status.profiles.len(), 2);
        engine.stop().unwrap();
    }

    #[tokio::test]
    async fn live_switch_restart_and_log_clear_use_the_managed_core() {
        let (_directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        engine.start().await.unwrap();
        let status = engine
            .import_and_activate(b"proxies: []\nrules: [MATCH,DIRECT]", "second.yaml")
            .await
            .unwrap();
        assert!(status.running);
        assert_eq!(status.config_name.as_deref(), Some("second.yaml"));
        assert!(engine.restart().await.unwrap().running);
        assert!(!engine.log_lines().is_empty());
        engine.clear_logs().unwrap();
        assert!(engine.log_lines().is_empty());
        engine.stop().unwrap();
    }

    #[tokio::test]
    async fn startup_failure_rolls_back_the_active_pointer_on_disk() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        engine.start().await.unwrap();
        let original = engine.status().active_profile_id.unwrap();
        let error = engine
            .import_and_activate(b"proxies: []\nfixture-startup-exit: true\n", "exits.yaml")
            .await
            .unwrap_err();
        assert!(error.contains("previous profile was restored"));
        assert!(engine.status().running);
        assert_eq!(
            Settings::load(directory.path())
                .unwrap()
                .active_profile_id
                .as_deref(),
            Some(original.as_str())
        );
        assert!(!directory
            .path()
            .join("runtime/validate-profile.yaml")
            .exists());
        engine.stop().unwrap();
    }

    #[test]
    fn missing_legacy_profile_does_not_prevent_reimport() {
        let directory = tempfile::tempdir().unwrap();
        Settings {
            config_name: Some("missing.yaml".into()),
            ..Settings::default()
        }
        .save(directory.path())
        .unwrap();
        let mut engine = Engine::new(directory.path().to_path_buf()).unwrap();
        assert!(engine
            .status()
            .last_error
            .unwrap()
            .contains("Import it again"));
        assert!(engine.status().config_name.is_none());
        engine.import_profile(b"proxies: []", "new.yaml").unwrap();
        assert!(engine.status().last_error.is_none());
        assert_eq!(engine.status().config_name.as_deref(), Some("new.yaml"));
    }
}
