use super::*;
use crate::{
    app_logs::LogArchive,
    group_icons::IconCache,
    network::NetworkStatus,
    providers::{ProviderRefresh, ProviderRefreshStatus},
};
use std::time::Instant;

struct NetworkSuspension {
    profile_id: Option<String>,
    core_path: Option<PathBuf>,
    proxy_enabled: bool,
    attempts: usize,
    retry_after: Instant,
}

pub(super) struct BackgroundServices {
    archive: LogArchive,
    icons: IconCache,
    provider_refresh: Option<ProviderRefresh>,
    provider_revision: Option<u64>,
    pub(super) network_status: NetworkStatus,
    pub(super) local_lan_address: Option<String>,
    suspended: Option<NetworkSuspension>,
}

impl BackgroundServices {
    pub(super) fn new(directory: &Path) -> Result<Self, String> {
        Ok(Self {
            archive: LogArchive::new(&directory.join("logs"))?,
            icons: IconCache::new(&directory.join("icons")),
            provider_refresh: None,
            provider_revision: None,
            network_status: NetworkStatus::Unknown,
            local_lan_address: crate::network::local_lan_address(),
            suspended: None,
        })
    }

    pub(super) fn restore_logs(&mut self, logs: &Logs) -> Result<(), String> {
        for entry in self.archive.load()? {
            process::append_entry(logs, entry);
        }
        Ok(())
    }
}

impl Engine {
    pub fn install_bundled_core(&mut self, resource_directory: &Path) -> Result<bool, String> {
        if self.settings.core_path.is_some() {
            return Ok(false);
        }
        let Some(path) = crate::bundled_core::install(resource_directory, &self.directory)? else {
            return Ok(false);
        };
        let mut settings = self.settings.clone();
        settings.core_path = Some(path);
        settings.save(&self.directory)?;
        self.settings = settings;
        process::append(&self.logs, "已准备随包 mihomo 内核。");
        Ok(true)
    }

    pub fn provider_refresh_status(&self) -> ProviderRefreshStatus {
        self.background
            .provider_refresh
            .as_ref()
            .map(ProviderRefresh::status)
            .unwrap_or_default()
    }

    pub fn start_provider_refresh(&mut self, controller: Controller) {
        self.background.provider_refresh =
            Some(ProviderRefresh::start(controller, self.logs.clone()));
        self.background.provider_revision = Some(self.target_revision);
    }

    pub fn cancel_background_intent(&mut self) {
        self.background.suspended = None;
        self.background.provider_refresh = None;
        self.background.provider_revision = None;
    }

    pub fn clear_log_archive(&mut self) -> Result<(), String> {
        self.background.archive.clear()
    }

    pub fn flush_log_archive(&mut self) -> Result<(), String> {
        let mut entries: Vec<_> = self
            .logs
            .lock()
            .map_err(|_| "应用日志不可用。")?
            .iter()
            .filter(|entry| entry.source == "ClashBar")
            .cloned()
            .collect();
        entries.sort_by_key(|entry| entry.timestamp);
        self.background.archive.save(&entries)
    }

    pub fn record_app_action(&mut self, message: &str) -> Result<process::LogEntry, String> {
        if message.len() > 16 * 1024 {
            return Err("应用日志消息过长。".into());
        }
        let entry = process::LogEntry {
            timestamp: crate::subscriptions::now_ms(),
            source: "ClashBar",
            message: crate::app_logs::sanitize_message(message),
        };
        process::append_entry(&self.logs, entry.clone());
        self.flush_log_archive()?;
        Ok(entry)
    }

    pub async fn proxy_group_icon(&mut self, name: &str) -> Result<String, String> {
        let url = self
            .controller()?
            .proxy_group_icon(name)
            .await?
            .ok_or("代理组未配置图标。")?;
        self.background.icons.get(&url).await
    }

    pub fn proxy_group_icon_context(&mut self) -> Result<(Controller, IconCache), String> {
        Ok((self.controller()?, self.background.icons.clone()))
    }

    pub async fn background_services_tick(&mut self) -> Result<(), String> {
        self.refresh();
        self.background.local_lan_address = crate::network::local_lan_address();
        self.observe_network(crate::network::query()).await?;
        if self.active_remote().is_some()
            && self.background.provider_revision != Some(self.target_revision)
        {
            if let Ok(controller) = self.controller() {
                self.start_provider_refresh(controller);
            }
        }
        self.flush_log_archive()
    }

    pub async fn observe_network(&mut self, status: NetworkStatus) -> Result<(), String> {
        self.background.network_status = status;
        if self.active_remote().is_some() || status == NetworkStatus::Unknown {
            return Ok(());
        }
        if status == NetworkStatus::Offline && self.child.is_some() {
            let suspension = NetworkSuspension {
                profile_id: self.settings.active_profile_id.clone(),
                core_path: self.settings.core_path.clone(),
                proxy_enabled: self.status().system_proxy && self.proxy_uses_local,
                attempts: 0,
                retry_after: Instant::now(),
            };
            process::append(&self.logs, "网络已断开，暂停本机内核；网络恢复后重启。");
            self.stop()?;
            self.background.suspended = Some(suspension);
        } else if status == NetworkStatus::Online
            && self
                .background
                .suspended
                .as_ref()
                .is_some_and(|state| Instant::now() >= state.retry_after)
        {
            let mut suspension = self
                .background
                .suspended
                .take()
                .expect("checked suspension");
            if suspension.profile_id != self.settings.active_profile_id
                || suspension.core_path != self.settings.core_path
            {
                return Ok(());
            }
            process::append(&self.logs, "网络已恢复，正在恢复本机内核。");
            match self.start().await {
                Ok(_) => {
                    if suspension.proxy_enabled {
                        self.set_system_proxy(true)?;
                    }
                }
                Err(error) => {
                    suspension.attempts += 1;
                    suspension.retry_after = Instant::now() + Duration::from_secs(5);
                    if suspension.attempts < 30 {
                        self.background.suspended = Some(suspension);
                    }
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_install_persists_core_path_and_keeps_existing_user_selection() {
        let directory = tempfile::tempdir().unwrap();
        let resources = tempfile::tempdir().unwrap();
        fs::create_dir(resources.path().join("bin")).unwrap();
        fs::write(
            resources.path().join("bin/mihomo.exe"),
            crate::bundled_core::tests::executable(),
        )
        .unwrap();
        let mut engine = Engine::new(directory.path().to_path_buf()).unwrap();
        assert!(engine.install_bundled_core(resources.path()).unwrap());
        let persisted = Settings::load(directory.path()).unwrap().core_path.unwrap();
        assert!(persisted.ends_with("core/mihomo.exe"));
        assert!(!engine.install_bundled_core(resources.path()).unwrap());
        let manual = directory.path().join("user-selected.exe");
        engine.choose_core(&persisted).unwrap();
        engine.settings.core_path = Some(manual.clone());
        assert!(!engine.install_bundled_core(resources.path()).unwrap());
        assert_eq!(engine.settings.core_path, Some(manual));
    }

    #[test]
    fn archive_survives_engine_restart_and_clear_removes_persisted_entries() {
        let directory = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(directory.path().to_path_buf()).unwrap();
        process::append_entry(
            &engine.logs,
            process::LogEntry {
                timestamp: 123,
                source: "ClashBar",
                message: "persisted entry".into(),
            },
        );
        engine.flush_log_archive().unwrap();
        let mut restored = Engine::new(directory.path().to_path_buf()).unwrap();
        assert!(restored
            .log_entries()
            .iter()
            .any(|entry| entry.timestamp == 123 && entry.message == "persisted entry"));
        restored.clear_logs().unwrap();
        let reopened = Engine::new(directory.path().to_path_buf()).unwrap();
        assert!(!reopened
            .log_lines()
            .iter()
            .any(|line| line == "persisted entry"));
    }

    #[test]
    fn app_action_archive_excludes_core_output_and_returns_sanitized_entry() {
        let directory = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(directory.path().to_path_buf()).unwrap();
        process::append_entry(
            &engine.logs,
            process::LogEntry {
                timestamp: 1,
                source: "Mihomo",
                message: "local core output".into(),
            },
        );
        let entry = engine
            .record_app_action("更新订阅 https://example.com?private-token=abc")
            .unwrap();
        assert_eq!(entry.source, "ClashBar");
        assert!(!entry.message.contains("private-token"));
        let restored = Engine::new(directory.path().to_path_buf()).unwrap();
        assert!(restored
            .log_entries()
            .iter()
            .all(|entry| entry.source == "ClashBar"));
        assert!(!restored
            .log_lines()
            .iter()
            .any(|line| line == "local core output"));
        assert!(restored
            .log_entries()
            .iter()
            .any(|stored| stored.timestamp == entry.timestamp && stored.message == entry.message));
    }

    #[tokio::test]
    async fn remote_log_view_contains_app_actions_without_local_core_output() {
        let directory = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(directory.path().to_path_buf()).unwrap();
        let machine = RemoteMachine::from_input(
            RemoteInput {
                id: None,
                name: "remote".into(),
                host: "127.0.0.1".into(),
                port: 1,
                use_https: false,
                secret: None,
            },
            None,
        )
        .unwrap();
        engine.settings.active_remote_id = Some(machine.id.clone());
        engine.settings.remote_machines.push(machine.clone());
        let stream = LogStream::start(Controller::remote(&machine).unwrap(), "silent", None);
        process::append_entry(
            &engine.logs,
            process::LogEntry {
                timestamp: 1,
                source: "Mihomo",
                message: "local core output".into(),
            },
        );
        process::append_entry(
            &stream.logs,
            process::LogEntry {
                timestamp: 2,
                source: "Mihomo",
                message: "remote core output".into(),
            },
        );
        engine.remote_logs = Some(stream);
        engine.record_app_action("用户切换了远程目标").unwrap();
        let messages = engine.log_lines();
        assert!(!messages.iter().any(|line| line == "local core output"));
        assert!(messages.iter().any(|line| line == "remote core output"));
        assert!(messages.iter().any(|line| line == "用户切换了远程目标"));
    }

    #[tokio::test]
    async fn manual_stop_cancels_network_resume_intent() {
        let directory = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(directory.path().to_path_buf()).unwrap();
        engine.background.suspended = Some(NetworkSuspension {
            profile_id: None,
            core_path: None,
            proxy_enabled: false,
            attempts: 0,
            retry_after: Instant::now(),
        });
        engine.stop().unwrap();
        engine.observe_network(NetworkStatus::Online).await.unwrap();
        assert!(engine.background.suspended.is_none());
        assert!(engine.last_error.is_none());
    }

    #[tokio::test]
    async fn network_loss_stops_managed_core_and_recovery_resumes_same_profile() {
        let (_directory, mut engine) =
            crate::engine::tests::configured_engine(b"proxies: []\nrules: []\n");
        engine.start().await.unwrap();
        let selected = engine.settings.active_profile_id.clone();
        engine
            .observe_network(NetworkStatus::Unknown)
            .await
            .unwrap();
        assert!(engine.status().running);
        engine
            .observe_network(NetworkStatus::Offline)
            .await
            .unwrap();
        assert!(!engine.status().running);
        assert!(engine.background.suspended.is_some());
        engine.observe_network(NetworkStatus::Online).await.unwrap();
        assert!(engine.status().running);
        assert_eq!(engine.settings.active_profile_id, selected);
        assert!(engine.background.suspended.is_none());
        engine.stop().unwrap();
    }

    #[tokio::test]
    async fn changing_profile_while_offline_cancels_previous_resume() {
        let (_directory, mut engine) =
            crate::engine::tests::configured_engine(b"proxies: []\nrules: []\n");
        engine.start().await.unwrap();
        engine
            .observe_network(NetworkStatus::Offline)
            .await
            .unwrap();
        let next = engine
            .import_profile(b"proxies: []\nmode: direct\n", "next.yaml")
            .unwrap()
            .active_profile_id
            .unwrap();
        engine.select_profile(&next).await.unwrap();
        engine.observe_network(NetworkStatus::Online).await.unwrap();
        assert!(!engine.status().running);
        assert!(engine.background.suspended.is_none());
    }
}
