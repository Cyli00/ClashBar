use super::{Logs, ManagedChild};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::{
    ffi::OsStr,
    fs, mem,
    os::windows::{ffi::OsStrExt, process::ExitStatusExt},
    path::{Path, PathBuf},
    ptr,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
    System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, OpenProcess, OpenProcessToken, WaitForSingleObject,
        PROCESS_SYNCHRONIZE,
    },
    UI::{
        Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW},
        WindowsAndMessaging::SW_HIDE,
    },
};

const HELPER_FLAG: &str = "--clashbar-core-helper";

struct Handle(HANDLE);
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub(super) fn is_elevated() -> Result<bool, String> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(format!(
            "无法检查内核权限：{}",
            std::io::Error::last_os_error()
        ));
    }
    let token = Handle(token);
    let mut elevation: TOKEN_ELEVATION = unsafe { mem::zeroed() };
    let mut length = 0;
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        )
    } == 0
    {
        return Err(format!(
            "无法读取内核权限：{}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(elevation.TokenIsElevated != 0)
}

#[derive(Serialize, Deserialize)]
struct Request {
    executable: PathBuf,
    directory: PathBuf,
    config: PathBuf,
    parent_pid: u32,
    secret: String,
    executable_sha256: String,
    config_sha256: String,
}

fn file_digest(file: &mut fs::File) -> Result<String, String> {
    let mut digest = Sha256::new();
    let mut bytes = [0u8; 65536];
    loop {
        let length = file
            .read(&mut bytes)
            .map_err(|_| "无法核对内核权限请求文件。")?;
        if length == 0 {
            break;
        }
        digest.update(&bytes[..length]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn path_digest(path: &Path) -> Result<String, String> {
    file_digest(&mut fs::File::open(path).map_err(|_| "无法读取内核权限请求文件。")?)
}

#[derive(Serialize, Deserialize)]
struct HelperLine {
    sequence: u64,
    timestamp: u64,
    source: String,
    message: String,
}

pub(super) struct ElevatedChild {
    process: Handle,
    directory: PathBuf,
    logs: Logs,
    last_line: u64,
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn quoted_path(path: &Path) -> Result<String, String> {
    let path = path.to_str().ok_or("提权助手路径不是有效 Unicode。")?;
    if path.contains(['"', '\0']) {
        return Err("提权助手路径包含无效字符。".into());
    }
    Ok(format!("\"{path}\""))
}

impl ElevatedChild {
    pub(super) fn spawn(
        executable: &Path,
        data_dir: &Path,
        config: &Path,
        logs: &Logs,
        secret: &str,
    ) -> Result<Self, String> {
        let executable_sha256 = path_digest(executable)?;
        let config_sha256 = path_digest(config)?;
        let directory = data_dir.join(format!("elevated-{}", crate::config::secret()));
        fs::create_dir(&directory).map_err(|error| format!("无法准备内核权限请求：{error}"))?;
        let request = Request {
            executable: executable.into(),
            directory: data_dir.into(),
            config: config.into(),
            parent_pid: std::process::id(),
            secret: secret.into(),
            executable_sha256,
            config_sha256,
        };
        let result = (|| {
            let bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
            let digest = crate::config::profile_id(&bytes);
            crate::config::atomic_write(&directory.join("request.json"), &bytes)?;
            let app = wide(std::env::current_exe().map_err(|error| error.to_string())?);
            let parameters = wide(format!(
                "{HELPER_FLAG} {} {digest}",
                quoted_path(&directory)?
            ));
            let verb = wide("runas");
            let mut info: SHELLEXECUTEINFOW = unsafe { mem::zeroed() };
            info.cbSize = mem::size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
            info.lpVerb = verb.as_ptr();
            info.lpFile = app.as_ptr();
            info.lpParameters = parameters.as_ptr();
            info.nShow = SW_HIDE;
            if unsafe { ShellExecuteExW(&mut info) } == 0 {
                let error = std::io::Error::last_os_error();
                return Err(if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
                    "已取消 TUN 管理员授权。".into()
                } else {
                    format!("无法授权 TUN 内核：{error}")
                });
            }
            if info.hProcess.is_null() {
                return Err("Windows 未返回提权内核助手的进程句柄。".into());
            }
            Ok(Handle(info.hProcess))
        })();
        match result {
            Ok(process) => Ok(Self {
                process,
                directory,
                logs: logs.clone(),
                last_line: 0,
            }),
            Err(error) => {
                let _ = fs::remove_dir_all(&directory);
                Err(error)
            }
        }
    }

    fn collect_logs(&mut self) {
        let Ok(bytes) = fs::read(self.directory.join("logs.json")) else {
            return;
        };
        let Ok(lines) = serde_json::from_slice::<Vec<HelperLine>>(&bytes) else {
            return;
        };
        for line in lines {
            if line.sequence <= self.last_line {
                continue;
            }
            self.last_line = line.sequence;
            super::append_entry(
                &self.logs,
                super::LogEntry {
                    timestamp: line.timestamp,
                    source: if line.source == "Mihomo" {
                        "Mihomo"
                    } else {
                        "ClashBar"
                    },
                    message: line.message,
                },
            );
        }
    }

    pub(super) fn exited(&mut self) -> Result<Option<std::process::ExitStatus>, String> {
        self.collect_logs();
        match unsafe { WaitForSingleObject(self.process.0, 0) } {
            WAIT_TIMEOUT => return Ok(None),
            WAIT_OBJECT_0 => {}
            _ => {
                return Err(format!(
                    "无法等待提权内核助手：{}",
                    std::io::Error::last_os_error()
                ))
            }
        }
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.process.0, &mut code) } == 0 {
            return Err(format!(
                "无法检查提权内核状态：{}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Some(std::process::ExitStatus::from_raw(code)))
    }

    pub(super) fn kill(&mut self) -> Result<(), String> {
        if self.exited()?.is_some() {
            return Ok(());
        }
        crate::config::atomic_write(&self.directory.join("stop"), b"stop")?;
        if unsafe { WaitForSingleObject(self.process.0, 8_000) } != WAIT_OBJECT_0 {
            return Err("提权内核助手未在 8 秒内停止；保留恢复文件以便重试。".into());
        }
        self.collect_logs();
        Ok(())
    }
}

impl Drop for ElevatedChild {
    fn drop(&mut self) {
        if self.kill().is_ok() {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

fn read_request(directory: &Path, digest: &str) -> Result<Request, String> {
    let mut bytes = Vec::new();
    fs::File::open(directory.join("request.json"))
        .map_err(|_| "无法读取内核权限请求。")?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取内核权限请求。")?;
    if bytes.len() > 64 * 1024 {
        return Err("内核权限请求过大。".into());
    }
    if crate::config::profile_id(&bytes) != digest {
        return Err("内核权限请求已发生变化，取消提权。".into());
    }
    let request: Request = serde_json::from_slice(&bytes).map_err(|_| "内核权限请求格式无效。")?;
    let runtime = directory
        .parent()
        .ok_or("内核权限请求目录无效。")?
        .canonicalize()
        .map_err(|_| "内核权限请求目录无效。")?;
    if request.directory.canonicalize().ok().as_ref() != Some(&runtime)
        || request.config.canonicalize().ok().as_ref() != Some(&runtime.join("config.yaml"))
    {
        return Err("内核权限请求必须使用当前运行目录中的配置。".into());
    }
    Ok(request)
}

fn run_helper(directory: &Path, digest: &str) -> Result<(), String> {
    let request = read_request(directory, digest)?;
    fs::remove_file(directory.join("request.json"))
        .map_err(|error| format!("无法清理内核权限请求：{error}"))?;
    if !is_elevated()? {
        return Err("TUN 内核助手没有管理员权限。".into());
    }
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    let mut executable = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&request.executable)
        .map_err(|_| "无法锁定已授权的内核文件。")?;
    let mut config = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&request.config)
        .map_err(|_| "无法锁定已授权的内核配置。")?;
    if file_digest(&mut executable)? != request.executable_sha256
        || file_digest(&mut config)? != request.config_sha256
    {
        return Err("内核或配置在授权期间发生变化，取消提权。".into());
    }
    // 校验期间禁止替换文件；启动后内核映像由 Windows 持有，运行配置锁保持到助手退出。
    supervise_core(directory, request, Some(executable))
}

fn supervise_core(
    directory: &Path,
    request: Request,
    locked_executable: Option<fs::File>,
) -> Result<(), String> {
    let parent = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, request.parent_pid) };
    if parent.is_null() {
        return Err("主程序已经退出，取消启动 TUN 内核。".into());
    }
    let parent = Handle(parent);
    let logs = super::logs();
    let mut child = ManagedChild::spawn(
        &request.executable,
        &request.directory,
        &request.config,
        false,
        &logs,
        &request.secret,
    )?;
    drop(locked_executable);
    let mut retained = std::collections::VecDeque::new();
    let mut sequence = 0;
    let mut published = 0;
    loop {
        let exited = child.exited()?;
        if let Ok(mut entries) = logs.lock() {
            for entry in entries.drain(..) {
                sequence += 1;
                retained.push_back(HelperLine {
                    sequence,
                    timestamp: entry.timestamp,
                    source: entry.source.into(),
                    message: entry.message,
                });
                if retained.len() > 500 {
                    retained.pop_front();
                }
            }
        }
        if sequence != published {
            crate::config::atomic_write(
                &directory.join("logs.json"),
                &serde_json::to_vec(&retained).map_err(|error| error.to_string())?,
            )?;
            published = sequence;
        }
        if let Some(status) = exited {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("TUN 内核已退出：{status}"))
            };
        }
        // 主进程崩溃与正常关闭都终止助手持有的 Job，避免残留 TUN 路由。
        if directory.join("stop").exists()
            || unsafe { WaitForSingleObject(parent.0, 0) } == WAIT_OBJECT_0
        {
            return child.kill();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub(super) fn run_if_requested() -> bool {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new(HELPER_FLAG)) {
        return false;
    }
    let Some(directory) = args.next().map(PathBuf::from) else {
        std::process::exit(2);
    };
    let Some(digest) = args.next().and_then(|value| value.into_string().ok()) else {
        std::process::exit(2);
    };
    let valid_directory = directory
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| {
            name.strip_prefix("elevated-").is_some_and(|nonce| {
                nonce.len() == 48 && nonce.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        });
    if !valid_directory
        || digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        std::process::exit(2);
    }
    let result = run_helper(&directory, &digest);
    if let Err(error) = &result {
        let line = HelperLine {
            sequence: u64::MAX,
            timestamp: 0,
            source: "ClashBar".into(),
            message: error.clone(),
        };
        if let Ok(bytes) = serde_json::to_vec(&vec![line]) {
            let _ = crate::config::atomic_write(&directory.join("logs.json"), &bytes);
        }
    }
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, TcpListener};

    #[test]
    fn helper_rejects_manifest_tampering_and_configuration_outside_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let helper_directory = directory.path().join("elevated-test");
        fs::create_dir(&helper_directory).unwrap();
        let config = directory.path().join("config.yaml");
        fs::write(&config, b"proxies: []").unwrap();
        let mut request = Request {
            executable: directory.path().join("mihomo.exe"),
            directory: directory.path().into(),
            config,
            parent_pid: std::process::id(),
            secret: "fixture".into(),
            executable_sha256: String::new(),
            config_sha256: String::new(),
        };
        let bytes = serde_json::to_vec(&request).unwrap();
        let digest = crate::config::profile_id(&bytes);
        fs::write(helper_directory.join("request.json"), &bytes).unwrap();
        assert!(read_request(&helper_directory, &digest).is_ok());
        request.parent_pid += 1;
        fs::write(
            helper_directory.join("request.json"),
            serde_json::to_vec(&request).unwrap(),
        )
        .unwrap();
        assert!(read_request(&helper_directory, &digest)
            .err()
            .unwrap()
            .contains("发生变化"));
        let outside = tempfile::tempdir().unwrap();
        request.config = outside.path().join("config.yaml");
        fs::write(&request.config, b"proxies: []").unwrap();
        let bytes = serde_json::to_vec(&request).unwrap();
        fs::write(helper_directory.join("request.json"), &bytes).unwrap();
        assert!(
            read_request(&helper_directory, &crate::config::profile_id(&bytes))
                .err()
                .unwrap()
                .contains("当前运行目录")
        );
    }

    fn wait_until_listening(port: u16) {
        let started = std::time::Instant::now();
        while TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok() {
            assert!(started.elapsed() < Duration::from_secs(8));
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    #[test]
    fn helper_stop_signal_ends_owned_core_without_elevating_test_process() {
        let directory = tempfile::tempdir().unwrap();
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let config = directory.path().join("config.yaml");
        fs::write(
            &config,
            format!("proxies: []\nexternal-controller: 127.0.0.1:{port}\nsecret: test-secret\n"),
        )
        .unwrap();
        let request = Request {
            executable: crate::engine::tests::fixture().join("mihomo.exe"),
            directory: directory.path().into(),
            config,
            parent_pid: std::process::id(),
            secret: "test-secret".into(),
            executable_sha256: String::new(),
            config_sha256: String::new(),
        };
        let helper_directory = directory.path().to_owned();
        let helper = std::thread::spawn(move || supervise_core(&helper_directory, request, None));
        wait_until_listening(port);
        crate::config::atomic_write(&directory.path().join("stop"), b"stop").unwrap();
        helper.join().unwrap().unwrap();
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok());
    }

    #[test]
    fn helper_exits_when_parent_dies_and_releases_core_listener() {
        let directory = tempfile::tempdir().unwrap();
        let parent_port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let parent_config = directory.path().join("parent.yaml");
        fs::write(
            &parent_config,
            format!("external-controller: 127.0.0.1:{parent_port}\n"),
        )
        .unwrap();
        let executable = crate::engine::tests::fixture().join("mihomo.exe");
        let mut parent = std::process::Command::new(&executable)
            .arg("-f")
            .arg(parent_config)
            .spawn()
            .unwrap();
        wait_until_listening(parent_port);
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let config = directory.path().join("core.yaml");
        fs::write(&config, format!("external-controller: 127.0.0.1:{port}\n")).unwrap();
        let request = Request {
            executable,
            directory: directory.path().into(),
            config,
            parent_pid: parent.id(),
            secret: "test-secret".into(),
            executable_sha256: String::new(),
            config_sha256: String::new(),
        };
        let helper_directory = directory.path().to_owned();
        let helper = std::thread::spawn(move || supervise_core(&helper_directory, request, None));
        wait_until_listening(port);
        parent.kill().unwrap();
        parent.wait().unwrap();
        helper.join().unwrap().unwrap();
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok());
    }
}
