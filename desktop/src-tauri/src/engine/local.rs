use super::*;
use crate::ssid::{self, SsidResolution, SsidSnapshot, SsidStatus};

impl Engine {
    pub fn set_ui_language(&mut self, language: String) -> Result<Status, String> {
        if !matches!(language.as_str(), "zh-CN" | "en") {
            return Err("界面语言必须为 zh-CN 或 en。".into());
        }
        let mut settings = self.settings.clone();
        settings.ui_language = language;
        settings.save(&self.directory)?;
        self.settings = settings;
        Ok(self.status())
    }

    pub async fn set_mode(&mut self, mode: &str) -> Result<(), String> {
        if self.active_remote().is_some() {
            return self.controller()?.set_mode(mode).await;
        }
        self.persist_core_text("mode", mode).await
    }

    pub(super) async fn persist_core_text(&mut self, key: &str, value: &str) -> Result<(), String> {
        let controller = self.controller()?;
        let snapshot = controller.snapshot().await?;
        let previous = snapshot
            .configs
            .get(key)
            .and_then(serde_json::Value::as_str)
            .ok_or("内核没有返回当前设置值。")?;
        let mut settings = self.settings.clone();
        let patch = if key == "mode" {
            settings.core_preferences.mode = Some(value.into());
            controller.set_mode(value).await
        } else {
            settings.core_preferences.log_level = Some(value.into());
            controller.set_log_level(value).await
        };
        let result = patch.and_then(|_| settings.save(&self.directory));
        if let Err(error) = result {
            let restored = if key == "mode" {
                controller.set_mode(previous).await
            } else {
                controller.set_log_level(previous).await
            };
            return match restored {
                Ok(_) => Err(error),
                Err(restore) => Err(format!("{error} 恢复内核设置失败：{restore}")),
            };
        }
        self.settings = settings;
        Ok(())
    }

    pub fn tray_traffic(&mut self) -> Option<(u64, u64)> {
        if self.telemetry.is_none() {
            self.telemetry = Some(crate::streams::TelemetryStreams::start(
                self.controller().ok()?,
            ));
        }
        self.telemetry
            .as_ref()?
            .traffic()
            .map(|traffic| (traffic.up, traffic.down))
    }

    async fn activate_settings(&mut self, settings: Settings) -> Result<Status, String> {
        self.refresh();
        if self.child.is_none() {
            settings.save(&self.directory)?;
            self.settings = settings;
            return Ok(self.status());
        }
        let raw = read_profile(&self.active_profile_path()?)?;
        self.validate_candidate(&raw, &settings).await?;
        let previous = self.settings.clone();
        let proxy_enabled = self.owned_proxy_enabled()?;
        if proxy_enabled && settings.proxy_ports().system_ports() == (None, None) {
            return Err("请先关闭系统代理，再关闭全部 HTTP、SOCKS 和混合代理端口。".into());
        }
        self.stop()?;
        self.settings = settings;
        let result = match self.resume(proxy_enabled).await {
            Ok(_) => self.settings.save(&self.directory),
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            self.stop()
                .map_err(|cleanup| format!("设置应用失败：{error} 停止候选内核失败：{cleanup}"))?;
            self.settings = previous;
            return match self.resume(proxy_enabled).await {
                Ok(_) => Err(format!("设置应用失败，已恢复原设置与内核：{error}")),
                Err(restore) => Err(format!("设置应用失败：{error} 恢复原内核失败：{restore}")),
            };
        }
        Ok(self.status())
    }

    pub async fn save_local_ports(
        &mut self,
        ports: config::ProxyPorts,
        controller_port: u16,
    ) -> Result<Status, String> {
        self.require_local_target()?;
        let mut settings = self.settings.clone();
        settings.set_proxy_ports(&ports);
        settings.controller_port = controller_port;
        settings.validate_local_ports()?;
        self.activate_settings(settings).await
    }

    pub async fn save_proxy_exceptions(
        &mut self,
        exceptions: Vec<String>,
    ) -> Result<Status, String> {
        crate::system_proxy::bypass_list(&exceptions)?;
        let mut settings = self.settings.clone();
        settings.system_proxy_exceptions = crate::system_proxy::normalize_exceptions(&exceptions)?;
        let enabled = self.system_proxy.is_enabled()?;
        let remote_target = if enabled {
            self.proxy_remote_id
                .as_ref()
                .map(|id| self.remote_machine(id))
                .transpose()?
        } else {
            None
        };
        let previous = self.settings.clone();
        settings.save(&self.directory)?;
        self.settings = settings;
        if enabled {
            let result = if let Some(machine) = remote_target {
                let controller = Controller::remote(&machine)?;
                match controller.proxy_ports().await {
                    Ok((http, socks)) => self.system_proxy.enable_with_exceptions(
                        &machine.host,
                        http,
                        socks,
                        &self.settings.system_proxy_exceptions,
                    ),
                    Err(error) => Err(error),
                }
            } else {
                self.set_system_proxy(true).map(|_| ())
            };
            if let Err(error) = result {
                self.settings = previous;
                self.settings.save(&self.directory).map_err(|restore| {
                    format!("绕过设置应用失败：{error} 恢复保存值失败：{restore}")
                })?;
                return Err(error);
            }
        }
        Ok(self.status())
    }

    pub async fn set_tun(
        &mut self,
        enabled: bool,
        stack: Option<String>,
    ) -> Result<Status, String> {
        let stack = stack
            .as_deref()
            .map(crate::controller::normalize_tun_stack)
            .transpose()?;
        let controller = self.controller()?;
        if self.active_remote().is_some() {
            controller
                .set_tun_settings(enabled, stack.as_deref())
                .await?;
            return Ok(self.status());
        }
        if stack.as_deref() == Some("mips")
            && !crate::controller::supports_mips_stack(&controller.version().await?)
        {
            return Err("mips 协议栈需要 mihomo 1.19.31 或更新版本。".into());
        }
        let previous = controller.tun_settings().await?;
        let mut settings = self.settings.clone();
        settings.core_preferences.tun_enabled = enabled;
        if let Some(stack) = stack {
            settings.core_preferences.tun_stack = Some(stack);
        }
        if enabled
            && !self
                .child
                .as_ref()
                .is_some_and(ManagedChild::has_tun_permissions)
        {
            // 提权只发生在专用内核助手；候选失败时恢复原来的普通权限内核。
            return self.activate_settings(settings).await;
        }
        let desired_stack = settings.core_preferences.tun_stack.as_deref();
        let applied = controller.set_tun_settings(enabled, desired_stack).await;
        if let Err(error) = applied {
            return match controller
                .set_tun_settings(previous.enable, previous.stack.as_deref())
                .await
            {
                Ok(_) => Err(error),
                Err(restore) => Err(format!("{error} 恢复 TUN 状态失败：{restore}")),
            };
        }
        if let Err(error) = settings.save(&self.directory) {
            return match controller
                .set_tun_settings(previous.enable, previous.stack.as_deref())
                .await
            {
                Ok(_) => Err(format!("保存 TUN 设置失败，已恢复运行状态：{error}")),
                Err(restore) => Err(format!(
                    "保存 TUN 设置失败：{error} 恢复运行状态失败：{restore}"
                )),
            };
        }
        self.settings = settings;
        Ok(self.status())
    }

    pub async fn set_ssid_enabled(&mut self, enabled: bool) -> Result<Status, String> {
        let mut settings = self.settings.clone();
        settings.ssid_enabled = enabled;
        settings.save(&self.directory)?;
        self.settings = settings;
        if enabled {
            let previous = self.ssid_snapshot.clone();
            self.refresh_ssid().await?;
            if previous == self.ssid_snapshot {
                self.apply_ssid_strategy().await;
            }
        } else {
            self.ssid_error = None;
        }
        Ok(self.status())
    }

    pub async fn refresh_ssid(&mut self) -> Result<Status, String> {
        let snapshot = tokio::task::spawn_blocking(ssid::query_current_ssid)
            .await
            .map_err(|_| "读取 Wi-Fi 状态任务失败。")?;
        self.ssid_checked = Some(std::time::Instant::now());
        self.observe_ssid(snapshot).await;
        Ok(self.status())
    }

    pub async fn poll_ssid(&mut self) {
        if !self.settings.ssid_enabled || self.active_remote().is_some() {
            return;
        }
        if self
            .ssid_checked
            .is_some_and(|checked| checked.elapsed() < Duration::from_secs(5))
        {
            return;
        }
        if self.ssid_checked.is_some() && !self.ssid_snapshot.can_retry_automatically() {
            return;
        }
        if let Err(error) = self.refresh_ssid().await {
            self.ssid_error = Some(error);
        }
    }

    async fn observe_ssid(&mut self, snapshot: SsidSnapshot) {
        let changed = self.ssid_snapshot != snapshot;
        self.ssid_snapshot = snapshot;
        if changed {
            self.apply_ssid_strategy().await;
        }
    }

    async fn apply_ssid_strategy(&mut self) {
        if !self.settings.ssid_enabled || self.active_remote().is_some() {
            return;
        }
        if self.ssid_snapshot.status != SsidStatus::Available {
            self.ssid_error = self.ssid_snapshot.error.clone();
            return;
        }
        let names = self
            .settings
            .profiles
            .iter()
            .map(|profile| profile.name.clone())
            .collect::<Vec<_>>();
        self.ssid_error = match ssid::resolve_config(
            self.ssid_snapshot.current_ssid.as_deref(),
            self.settings.config_name.as_deref(),
            &self.settings.ssid_rules,
            &names,
        ) {
            SsidResolution::NoAction => None,
            SsidResolution::MissingConfig(name) => {
                Some(format!("当前 Wi-Fi 绑定的配置“{name}”已不存在。"))
            }
            SsidResolution::SwitchToConfig(name) => {
                let id = self
                    .settings
                    .profiles
                    .iter()
                    .find(|profile| profile.name == name)
                    .map(|profile| profile.id.clone());
                if let Some(id) = id {
                    match self.select_profile(&id).await {
                        Ok(_) => {
                            process::append(
                                &self.logs,
                                format!("Wi-Fi 已切换，自动应用配置“{name}”。"),
                            );
                            None
                        }
                        Err(error) => Some(error),
                    }
                } else {
                    Some("Wi-Fi 绑定的配置不存在。".into())
                }
            }
        };
    }

    pub async fn bind_ssid(&mut self, profile_id: &str) -> Result<Status, String> {
        self.require_local_target()?;
        if !self.settings.ssid_enabled {
            return Err("请先启用 SSID 自动切换。".into());
        }
        let name = self
            .settings
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .map(|profile| profile.name.clone())
            .ok_or("配置不存在。")?;
        self.refresh_ssid().await?;
        let ssid = self
            .ssid_snapshot
            .current_ssid
            .as_deref()
            .ok_or("当前 Wi-Fi 名称不可用，请检查定位权限与网络连接。")?;
        let mut settings = self.settings.clone();
        settings.ssid_rules = if settings
            .ssid_rules
            .iter()
            .any(|rule| rule.ssid == ssid && rule.config_file_name == name)
        {
            ssid::remove_rule(&settings.ssid_rules, ssid)
        } else {
            ssid::upsert_rule(&settings.ssid_rules, ssid, &name)
        };
        settings.save(&self.directory)?;
        self.settings = settings;
        self.apply_ssid_strategy().await;
        Ok(self.status())
    }

    pub async fn remove_ssid_binding(&mut self, ssid: &str) -> Result<Status, String> {
        let mut settings = self.settings.clone();
        settings.ssid_rules = ssid::remove_rule(&settings.ssid_rules, ssid);
        settings.save(&self.directory)?;
        self.settings = settings;
        self.apply_ssid_strategy().await;
        Ok(self.status())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::configured_engine;
    use super::*;

    #[cfg(windows)]
    #[tokio::test]
    async fn system_proxy_preference_survives_shutdown_and_explicit_disable_survives_reload() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        engine.system_proxy = SystemProxy::memory(directory.path().into());
        engine.set_core_autostart(true).unwrap();
        engine.start().await.unwrap();
        assert!(!engine.settings.desired_system_proxy);
        engine.set_selected_system_proxy(true).await.unwrap();
        assert!(engine.system_proxy.is_enabled().unwrap());
        engine.shutdown().unwrap();
        assert!(!engine.system_proxy.is_enabled().unwrap());
        assert!(
            Settings::load(directory.path())
                .unwrap()
                .desired_system_proxy
        );
        let mut reloaded = Engine::new(directory.path().into()).unwrap();
        reloaded.system_proxy = SystemProxy::memory(directory.path().into());
        reloaded.auto_start_if_configured().await.unwrap();
        assert!(reloaded.system_proxy.is_enabled().unwrap());
        reloaded.set_selected_system_proxy(false).await.unwrap();
        reloaded.shutdown().unwrap();
        let mut disabled = Engine::new(directory.path().into()).unwrap();
        disabled.system_proxy = SystemProxy::memory(directory.path().into());
        disabled.auto_start_if_configured().await.unwrap();
        assert!(!disabled.system_proxy.is_enabled().unwrap());
        assert!(!disabled.settings.desired_system_proxy);
        disabled.stop().unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn failed_proxy_preference_save_keeps_actual_state_and_failed_apply_restores_preference()
    {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        engine.system_proxy = SystemProxy::memory(directory.path().into());
        engine.start().await.unwrap();
        engine.set_selected_system_proxy(true).await.unwrap();
        fs::create_dir(directory.path().join("settings.tmp")).unwrap();
        assert!(engine.set_selected_system_proxy(false).await.is_err());
        assert!(engine.system_proxy.is_enabled().unwrap());
        assert!(
            Settings::load(directory.path())
                .unwrap()
                .desired_system_proxy
        );
        fs::remove_dir(directory.path().join("settings.tmp")).unwrap();
        engine.set_selected_system_proxy(false).await.unwrap();
        engine.system_proxy.fail_next_enable();
        assert!(engine.set_selected_system_proxy(true).await.is_err());
        assert!(!engine.system_proxy.is_enabled().unwrap());
        assert!(!engine.settings.desired_system_proxy);
        assert!(
            !Settings::load(directory.path())
                .unwrap()
                .desired_system_proxy
        );
        engine.stop().unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn startup_proxy_restore_failure_keeps_ready_core_and_its_runtime_configuration() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        engine.system_proxy = SystemProxy::memory(directory.path().into());
        engine.settings.desired_system_proxy = true;
        engine.settings.save(directory.path()).unwrap();
        engine.system_proxy.fail_next_enable();
        assert!(engine.start().await.is_err());
        assert!(engine.status().running);
        assert!(directory.path().join("runtime/config.yaml").is_file());
        assert!(!engine.system_proxy.is_enabled().unwrap());
        engine.stop().unwrap();
    }

    fn available_port() -> u16 {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[tokio::test]
    async fn separate_local_ports_survive_restart_and_disk_reload() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        engine.start().await.unwrap();
        let mut ports = engine.settings.proxy_ports();
        ports.port = available_port();
        ports.socks_port = available_port();
        while ports.socks_port == ports.port {
            ports.socks_port = available_port();
        }
        ports.mixed_port = 0;
        let controller_port = engine.settings.controller_port;
        let status = engine
            .save_local_ports(ports.clone(), controller_port)
            .await
            .unwrap();
        assert!(status.running);
        assert_eq!(status.local_proxy_ports, ports);
        assert_eq!(
            engine
                .controller()
                .unwrap()
                .proxy_port_settings()
                .await
                .unwrap(),
            ports
        );
        assert_eq!(
            Settings::load(directory.path()).unwrap().proxy_ports(),
            ports
        );
        engine.restart().await.unwrap();
        assert_eq!(
            engine
                .controller()
                .unwrap()
                .proxy_port_settings()
                .await
                .unwrap(),
            ports
        );
        engine.stop().unwrap();
    }

    #[tokio::test]
    async fn occupied_new_port_restores_running_core_and_saved_ports() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        engine.start().await.unwrap();
        let original = engine.settings.proxy_ports();
        let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut ports = original.clone();
        ports.port = occupied.local_addr().unwrap().port();
        let controller_port = engine.settings.controller_port;
        let error = engine
            .save_local_ports(ports, controller_port)
            .await
            .unwrap_err();
        assert!(error.contains("已恢复原设置与内核"));
        assert!(engine.status().running);
        assert_eq!(
            Settings::load(directory.path()).unwrap().proxy_ports(),
            original
        );
        assert_eq!(
            engine
                .controller()
                .unwrap()
                .proxy_port_settings()
                .await
                .unwrap(),
            original
        );
        engine.stop().unwrap();
    }

    #[tokio::test]
    async fn tun_disable_stack_persists_and_rejected_stack_does_not_mutate() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        engine.start().await.unwrap();
        engine.set_tun(false, Some("SYSTEM".into())).await.unwrap();
        let saved = Settings::load(directory.path()).unwrap();
        assert!(!saved.core_preferences.tun_enabled);
        assert_eq!(saved.core_preferences.tun_stack.as_deref(), Some("system"));
        assert!(engine
            .set_tun(true, Some("mips".into()))
            .await
            .unwrap_err()
            .contains("1.19.31"));
        assert!(engine.set_tun(true, Some("invalid".into())).await.is_err());
        assert_eq!(
            engine
                .controller()
                .unwrap()
                .tun_settings()
                .await
                .unwrap()
                .stack
                .as_deref(),
            Some("system")
        );
        engine.restart().await.unwrap();
        assert_eq!(
            engine
                .controller()
                .unwrap()
                .tun_settings()
                .await
                .unwrap()
                .stack
                .as_deref(),
            Some("system")
        );
        engine.stop().unwrap();
    }

    #[tokio::test]
    async fn mode_and_log_level_survive_core_restart_and_failed_save_rolls_back() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        engine.start().await.unwrap();
        engine.set_mode("global").await.unwrap();
        engine.set_log_level("debug").await.unwrap();
        let saved = Settings::load(directory.path()).unwrap();
        assert_eq!(saved.core_preferences.mode.as_deref(), Some("global"));
        assert_eq!(saved.core_preferences.log_level.as_deref(), Some("debug"));
        engine.restart().await.unwrap();
        let snapshot = engine.controller().unwrap().snapshot().await.unwrap();
        assert_eq!(snapshot.configs["mode"], "global");
        assert_eq!(snapshot.configs["log-level"], "debug");
        fs::create_dir(directory.path().join("settings.tmp")).unwrap();
        assert!(engine.set_mode("direct").await.is_err());
        let snapshot = engine.controller().unwrap().snapshot().await.unwrap();
        assert_eq!(snapshot.configs["mode"], "global");
        assert_eq!(
            Settings::load(directory.path())
                .unwrap()
                .core_preferences
                .mode
                .as_deref(),
            Some("global")
        );
        engine.stop().unwrap();
    }

    #[tokio::test]
    async fn ssid_switches_live_profile_once_per_network_change_and_reports_missing_profile() {
        let (directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        let first_id = engine.settings.active_profile_id.clone().unwrap();
        let second = engine
            .import_profile(b"proxies: []\nrules: [MATCH,DIRECT]", "home.yaml")
            .unwrap()
            .active_profile_id
            .unwrap();
        engine.select_profile(&first_id).await.unwrap();
        engine.settings.ssid_enabled = true;
        engine.settings.ssid_rules = vec![ssid::SsidRule {
            ssid: "Home".into(),
            config_file_name: "home.yaml".into(),
        }];
        engine.settings.save(directory.path()).unwrap();
        engine.start().await.unwrap();
        let home = SsidSnapshot {
            current_ssid: Some("Home".into()),
            status: SsidStatus::Available,
            error: None,
        };
        engine.observe_ssid(home.clone()).await;
        assert_eq!(
            engine.settings.active_profile_id.as_deref(),
            Some(second.as_str())
        );
        assert!(engine.status().running);
        engine.select_profile(&first_id).await.unwrap();
        engine.observe_ssid(home.clone()).await;
        assert_eq!(
            engine.settings.active_profile_id.as_deref(),
            Some(first_id.as_str())
        );
        engine
            .observe_ssid(SsidSnapshot {
                status: SsidStatus::Disconnected,
                ..Default::default()
            })
            .await;
        engine.observe_ssid(home).await;
        assert_eq!(
            engine.settings.active_profile_id.as_deref(),
            Some(second.as_str())
        );
        engine.settings.ssid_rules[0].config_file_name = "deleted.yaml".into();
        engine.apply_ssid_strategy().await;
        assert!(engine
            .ssid_error
            .as_deref()
            .unwrap()
            .contains("deleted.yaml"));
        assert_eq!(
            engine.settings.active_profile_id.as_deref(),
            Some(second.as_str())
        );
        engine.remove_ssid_binding("Home").await.unwrap();
        assert!(Settings::load(directory.path())
            .unwrap()
            .ssid_rules
            .is_empty());
        assert!(engine.ssid_error.is_none());
        engine.stop().unwrap();
    }

    #[tokio::test]
    async fn ssid_disabled_or_denied_keeps_selected_profile() {
        let (_directory, mut engine) = configured_engine(b"proxies: []\nrules: []\n");
        let original = engine.settings.active_profile_id.clone();
        engine.settings.ssid_rules = vec![ssid::SsidRule {
            ssid: "Home".into(),
            config_file_name: "missing.yaml".into(),
        }];
        engine
            .observe_ssid(SsidSnapshot {
                current_ssid: Some("Home".into()),
                status: SsidStatus::Available,
                error: None,
            })
            .await;
        assert!(engine.ssid_error.is_none());
        engine.settings.ssid_enabled = true;
        engine
            .observe_ssid(SsidSnapshot {
                current_ssid: None,
                status: SsidStatus::PermissionDenied,
                error: Some("定位访问被拒绝".into()),
            })
            .await;
        assert_eq!(engine.ssid_error.as_deref(), Some("定位访问被拒绝"));
        assert_eq!(engine.settings.active_profile_id, original);
        assert!(!engine.ssid_snapshot.can_retry_automatically());
    }
}
