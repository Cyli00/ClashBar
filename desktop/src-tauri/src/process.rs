use std::{
    collections::VecDeque,
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
mod elevated;

pub fn run_elevated_helper_if_requested() -> bool {
    #[cfg(windows)]
    {
        elevated::run_if_requested()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct LogEntry {
    pub timestamp: u64,
    pub source: &'static str,
    pub message: String,
}

pub type Logs = Arc<Mutex<VecDeque<LogEntry>>>;
pub fn logs() -> Logs {
    Arc::new(Mutex::new(VecDeque::with_capacity(500)))
}

pub fn append(logs: &Logs, message: impl Into<String>) {
    append_from(logs, "ClashBar", message);
}

fn append_from(logs: &Logs, source: &'static str, message: impl Into<String>) {
    append_entry(
        logs,
        LogEntry {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            source,
            message: message.into().chars().take(4096).collect(),
        },
    );
}

pub fn append_entry(logs: &Logs, entry: LogEntry) {
    if let Ok(mut lines) = logs.lock() {
        if lines.len() >= 500 {
            lines.pop_front();
        }
        lines.push_back(entry);
    }
}

fn capture(reader: impl Read + Send + 'static, logs: Logs, secret: String) {
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = [0u8; 1024];
        let mut line = Vec::with_capacity(4096);
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            for byte in &buffer[..count] {
                if *byte == b'\n' {
                    emit_line(&logs, &line, &secret);
                    line.clear();
                }
                if *byte != b'\n' && *byte != b'\r' && line.len() < 4096 {
                    line.push(*byte);
                }
            }
        }
        if !line.is_empty() {
            emit_line(&logs, &line, &secret);
        }
    });
}

pub(crate) fn emit_line(logs: &Logs, line: &[u8], secret: &str) {
    let value = String::from_utf8_lossy(line);
    let lower = value.to_ascii_lowercase();
    if [
        "authorization",
        "secret:",
        "password:",
        "https://",
        "http://",
    ]
    .iter()
    .any(|pattern| lower.contains(pattern))
    {
        append_from(logs, "Mihomo", "[sensitive core output omitted]");
    } else if secret.is_empty() {
        append_from(logs, "Mihomo", value.into_owned());
    } else {
        append_from(logs, "Mihomo", value.replace(secret, "[redacted]"));
    }
}

pub struct ManagedChild {
    child: Option<Child>,
    #[cfg(windows)]
    _job: Option<Job>,
    #[cfg(windows)]
    elevated: Option<elevated::ElevatedChild>,
}

impl ManagedChild {
    pub fn spawn(
        executable: &Path,
        data_dir: &Path,
        config: &Path,
        validate: bool,
        logs: &Logs,
        secret: &str,
    ) -> Result<Self, String> {
        let mut command = Command::new(executable);
        command
            .args(["-d"])
            .arg(data_dir)
            .arg("-f")
            .arg(config)
            .current_dir(data_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if validate {
            command.arg("-t");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start the selected core: {e}"))?;
        #[cfg(windows)]
        let job = match Job::attach(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        if let Some(stdout) = child.stdout.take() {
            capture(stdout, logs.clone(), secret.to_owned());
        }
        if let Some(stderr) = child.stderr.take() {
            capture(stderr, logs.clone(), secret.to_owned());
        }
        Ok(Self {
            child: Some(child),
            #[cfg(windows)]
            _job: Some(job),
            #[cfg(windows)]
            elevated: None,
        })
    }

    pub fn has_tun_permissions(&self) -> bool {
        #[cfg(windows)]
        {
            self.elevated.is_some() || elevated::is_elevated().unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            true
        }
    }

    pub fn spawn_for_tun(
        executable: &Path,
        data_dir: &Path,
        config: &Path,
        logs: &Logs,
        secret: &str,
    ) -> Result<Self, String> {
        #[cfg(windows)]
        if !elevated::is_elevated()? {
            return Ok(Self {
                child: None,
                _job: None,
                elevated: Some(elevated::ElevatedChild::spawn(
                    executable, data_dir, config, logs, secret,
                )?),
            });
        }
        Self::spawn(executable, data_dir, config, false, logs, secret)
    }

    pub fn exited(&mut self) -> Result<Option<std::process::ExitStatus>, String> {
        #[cfg(windows)]
        if let Some(child) = &mut self.elevated {
            return child.exited();
        }
        self.child
            .as_mut()
            .ok_or("内核进程不存在。")?
            .try_wait()
            .map_err(|e| format!("Cannot inspect the core process: {e}"))
    }

    pub fn kill(&mut self) -> Result<(), String> {
        #[cfg(windows)]
        if let Some(child) = &mut self.elevated {
            return child.kill();
        }
        if self.exited()?.is_none() {
            self.child
                .as_mut()
                .ok_or("内核进程不存在。")?
                .kill()
                .map_err(|e| format!("Cannot stop the core: {e}"))?;
        }
        self.child
            .as_mut()
            .ok_or("内核进程不存在。")?
            .wait()
            .map_err(|e| format!("Cannot reap the core: {e}"))?;
        Ok(())
    }

    pub async fn validate(mut self) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.exited()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err("The mihomo configuration check failed. See the core logs.".into())
                };
            }
            if tokio::time::Instant::now() >= deadline {
                self.kill()?;
                return Err("The mihomo configuration check timed out after 20 seconds.".into());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

#[cfg(windows)]
struct Job(windows_sys::Win32::Foundation::HANDLE);

// A job handle is kernel-owned and may be closed on any thread. Access is serialized by Engine.
#[cfg(windows)]
unsafe impl Send for Job {}

#[cfg(windows)]
impl Job {
    fn attach(child: &Child) -> Result<Self, String> {
        use std::{mem::size_of, os::windows::io::AsRawHandle};
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // Windows copies the limits structure during the call; the owned handle is closed by Drop.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(format!(
                    "Cannot create a core process job: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let job = Self(handle);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle() as _) == 0
            {
                return Err(format!(
                    "Cannot contain the core process in a Windows job: {}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(job)
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logs_are_bounded_and_secret_is_redacted() {
        let logs = logs();
        for _ in 0..600 {
            append(&logs, "line");
        }
        emit_line(&logs, b"token-private", "private");
        let lines = logs.lock().unwrap();
        assert_eq!(lines.len(), 500);
        assert_eq!(lines.back().unwrap().message, "token-[redacted]");
        assert_eq!(lines.back().unwrap().source, "Mihomo");
        assert_eq!(lines.front().unwrap().source, "ClashBar");
        assert!(lines.back().unwrap().timestamp > 0);
    }
    #[test]
    fn subscription_urls_do_not_reach_frontend_logs() {
        let logs = logs();
        emit_line(
            &logs,
            b"download failed: https://example.com/profile?token=credentials",
            "secret",
        );
        assert_eq!(
            logs.lock().unwrap().back().unwrap().message,
            "[sensitive core output omitted]"
        );
    }
}
