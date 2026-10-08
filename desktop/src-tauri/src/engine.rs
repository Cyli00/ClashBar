use crate::{
    config::{self, Profile, Settings},
    controller::Controller,
    process::{self, Logs, ManagedChild},
    system_proxy::SystemProxy,
};
use serde::Serialize;
use std::{
    fs,
    net::{Ipv4Addr, TcpListener, UdpSocket},
    path::{Path, PathBuf},
    time::Duration,
};

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
        Ok(Self {
            directory,
            settings,
            child: None,
            controller: None,
            system_proxy,
            logs: process::logs(),
            version: None,
            last_error,
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
        Status {
            running: self.child.is_some(),
            core_path: self
                .settings
                .core_path
                .as_ref()
                .map(|path| path.display().to_string()),
            config_name: self.settings.config_name.clone(),
            mixed_port: self.settings.mixed_port,
            controller_port: self.settings.controller_port,
            system_proxy,
            version: self.version.clone(),
            last_error: self.last_error.clone(),
            profiles: self.settings.profiles.clone(),
            active_profile_id: self.settings.active_profile_id.clone(),
        }
    }

    pub fn refresh(&mut self) {
        let exited = self.child.as_mut().and_then(|child| match child.exited() {
            Ok(Some(status)) => Some(format!("The core exited unexpectedly ({status}).")),
            Ok(None) => None,
            Err(error) => Some(error),
        });
        if let Some(error) = exited {
            #[cfg(windows)]
            let error = match self.system_proxy.restore() {
                Ok(()) => error,
                Err(restore) => format!("{error} Proxy recovery failed: {restore}"),
            };
            self.child = None;
            self.controller = None;
            self.version = None;
            let _ = fs::remove_file(self.directory.join("runtime/config.yaml"));
            process::append(&self.logs, &error);
            self.last_error = Some(error);
        }
    }

    fn require_stopped(&mut self) -> Result<(), String> {
        self.refresh();
        if self.child.is_some() {
            return Err("Stop the core before changing its executable, profile, or ports.".into());
        }
        Ok(())
    }

    pub fn choose_core(&mut self, path: &Path) -> Result<Status, String> {
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
        let id = config::profile_id(bytes);
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
        let id = self.register_profile(bytes, name)?;
        self.select_profile(&id).await
    }

    pub async fn select_profile(&mut self, id: &str) -> Result<Status, String> {
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

    async fn validate_candidate(&self, raw: &[u8], settings: &Settings) -> Result<(), String> {
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
        let result = async {
            ManagedChild::spawn(executable, &directory, &path, true, &self.logs, &secret)?
                .validate()
                .await
        }
        .await;
        let _ = fs::remove_file(path);
        result
    }

    fn owned_proxy_enabled(&self) -> Result<bool, String> {
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
        self.logs
            .lock()
            .map_err(|_| "Cannot clear core logs.")?
            .clear();
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
        settings.save(&self.directory)?;
        self.settings = settings;
        self.last_error = None;
        Ok(self.status())
    }

    pub async fn start(&mut self) -> Result<Status, String> {
        self.refresh();
        if self.child.is_some() {
            return Ok(self.status());
        }
        let result = self.start_inner().await;
        if let Err(error) = &result {
            self.last_error = Some(error.clone());
            process::append(&self.logs, error);
            let _ = fs::remove_file(self.directory.join("runtime/config.yaml"));
        }
        result
    }

    async fn start_inner(&mut self) -> Result<Status, String> {
        #[cfg(windows)]
        self.system_proxy.recover()?;
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
        ensure_ports_available(self.settings.mixed_port, self.settings.controller_port)?;
        let controller = Controller::new(self.settings.controller_port, secret.clone())?;
        let mut child =
            ManagedChild::spawn(&executable, &runtime_dir, &path, false, &self.logs, &secret)?;
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
        self.child = Some(child);
        self.controller = Some(controller);
        self.version = Some(version);
        self.last_error = None;
        process::append(
            &self.logs,
            "The core is ready. System proxy remains off until you enable it.",
        );
        Ok(self.status())
    }

    pub fn stop(&mut self) -> Result<Status, String> {
        // Keep a working core alive if restoration fails; otherwise Windows could point at a dead proxy.
        #[cfg(windows)]
        if let Err(error) = self.system_proxy.restore() {
            self.last_error = Some(error.clone());
            return Err(error);
        }
        if let Some(child) = self.child.as_mut() {
            child.kill()?;
        }
        self.child = None;
        self.controller = None;
        self.version = None;
        self.last_error = None;
        let _ = fs::remove_file(self.directory.join("runtime/config.yaml"));
        process::append(
            &self.logs,
            "Core stopped; ClashBar no longer owns the system proxy.",
        );
        Ok(self.status())
    }

    pub fn set_system_proxy(&mut self, enabled: bool) -> Result<Status, String> {
        self.refresh();
        if enabled {
            if self.child.is_none() {
                return Err("Start the core before enabling the system proxy.".into());
            }
            self.system_proxy.enable(self.settings.mixed_port)?;
        } else {
            self.system_proxy.restore()?;
        }
        self.last_error = None;
        Ok(self.status())
    }

    pub fn controller(&mut self) -> Result<Controller, String> {
        self.refresh();
        self.controller
            .clone()
            .ok_or("Start the core to use the dashboard.".into())
    }

    pub fn log_lines(&self) -> Vec<String> {
        self.logs
            .lock()
            .map(|lines| lines.iter().cloned().collect())
            .unwrap_or_default()
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

#[cfg(test)]
mod tests {
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

    fn fixture() -> &'static Path {
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

    fn configured_engine(profile: &[u8]) -> (tempfile::TempDir, Engine) {
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
