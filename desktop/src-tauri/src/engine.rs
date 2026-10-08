use crate::{
    config::{self, Settings},
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
        let last_error = system_proxy.recover().err();
        #[cfg(not(windows))]
        let last_error = None;
        fs::create_dir_all(directory.join("runtime/providers/proxy-providers"))
            .map_err(|e| e.to_string())?;
        fs::create_dir_all(directory.join("runtime/providers/rule-providers"))
            .map_err(|e| e.to_string())?;
        let settings = Settings::load(&directory)?;
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

    pub fn import_profile(&mut self, bytes: &[u8], name: &str) -> Result<Status, String> {
        self.require_stopped()?;
        config::runtime(bytes, &self.settings, "validation-placeholder")?;
        let path = self.directory.join("profile.yaml");
        let previous = fs::read(&path).ok();
        let mut settings = self.settings.clone();
        settings.config_name = Some(name.chars().filter(|c| !c.is_control()).take(180).collect());
        config::atomic_write(&path, bytes)?;
        if let Err(error) = settings.save(&self.directory) {
            if let Some(previous) = previous {
                let _ = config::atomic_write(&path, &previous);
            } else {
                let _ = fs::remove_file(path);
            }
            return Err(error);
        }
        self.settings = settings;
        self.last_error = None;
        Ok(self.status())
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
        let raw = read_profile(&self.directory.join("profile.yaml"))?;
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
            String::from_utf8(fs::read(dir.path().join("profile.yaml")).unwrap())
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
}
