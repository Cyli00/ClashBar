//! Current-user Windows LAN/default-connection proxy ownership.
//!
//! The caller must serialize operations and enforce a single application instance.
//! Named dial-up/VPN connections, WinHTTP services, and TUN routing are out of scope.
//! WinINet has no compare-and-swap API: ownership is checked immediately before a
//! write, but another process can still race the native call.

use std::path::PathBuf;

pub struct SystemProxy {
    #[cfg(windows)]
    snapshot_path: PathBuf,
}

impl SystemProxy {
    pub fn new(data_dir: PathBuf) -> Self {
        #[cfg(not(windows))]
        let _ = data_dir;
        Self {
            #[cfg(windows)]
            snapshot_path: data_dir.join("system-proxy-backup.json"),
        }
    }

    /// Restore an interrupted session before starting a new core process.
    pub fn recover(&self) -> Result<(), String> {
        self.restore()
    }

    #[cfg(windows)]
    pub fn enable(&self, port: u16) -> Result<(), String> {
        transaction::enable(&self.snapshot_path, &native::WinInet, port)
    }

    #[cfg(windows)]
    pub fn restore(&self) -> Result<(), String> {
        transaction::restore(&self.snapshot_path, &native::WinInet)
    }

    #[cfg(windows)]
    pub fn is_enabled(&self) -> Result<bool, String> {
        transaction::is_enabled(&self.snapshot_path, &native::WinInet)
    }

    #[cfg(not(windows))]
    pub fn enable(&self, _port: u16) -> Result<(), String> {
        Err("System proxy management is currently supported on Windows only".into())
    }

    #[cfg(not(windows))]
    pub fn restore(&self) -> Result<(), String> {
        Err("System proxy management is currently supported on Windows only".into())
    }

    #[cfg(not(windows))]
    pub fn is_enabled(&self) -> Result<bool, String> {
        Err("System proxy management is currently supported on Windows only".into())
    }
}

#[cfg(any(windows, test))]
mod transaction {
    use serde::{Deserialize, Serialize};
    use std::{
        fs::{self, OpenOptions},
        io::{ErrorKind, Write},
        path::Path,
    };

    // WinINet PROXY_TYPE_DIRECT | PROXY_TYPE_PROXY. PAC/auto-detect are disabled
    // while the local proxy is selected and restored from the original flags.
    const MANUAL_PROXY_FLAGS: u32 = 1 | 2;

    #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct ProxySettings {
        pub flags: u32,
        pub server: String,
        pub bypass: String,
        pub pac_url: String,
    }

    impl ProxySettings {
        fn for_port(original: &Self, port: u16) -> Self {
            Self {
                flags: MANUAL_PROXY_FLAGS,
                server: format!("http=127.0.0.1:{port};https=127.0.0.1:{port}"),
                bypass: "<local>;localhost;127.*;[::1]".into(),
                // Keep the URL stored but inactive; restore includes its flags.
                pac_url: original.pac_url.clone(),
            }
        }

        fn validate(&self) -> Result<(), String> {
            if self.flags & !0x0f != 0
                || [&self.server, &self.bypass, &self.pac_url]
                    .iter()
                    .any(|value| value.contains('\0'))
            {
                return Err("Proxy snapshot contains unsupported settings".into());
            }
            Ok(())
        }
    }

    #[derive(Debug, Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Snapshot {
        version: u32,
        pub original: ProxySettings,
        pub applied: ProxySettings,
    }

    pub(super) trait Platform {
        fn read(&self) -> Result<ProxySettings, String>;
        /// Must set all four values, then notify WinINet consumers.
        fn apply(&self, settings: &ProxySettings) -> Result<(), String>;
        fn notify(&self) -> Result<(), String>;
    }

    pub(super) fn read_snapshot(path: &Path) -> Result<Option<Snapshot>, String> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Cannot read proxy backup: {error}")),
        };
        let snapshot: Snapshot = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Invalid proxy backup; no settings changed: {error}"))?;
        if snapshot.version != 1 {
            return Err("Unsupported proxy backup version; no settings changed".into());
        }
        snapshot.original.validate()?;
        snapshot.applied.validate()?;
        Ok(Some(snapshot))
    }

    fn save_snapshot(path: &Path, snapshot: &Snapshot) -> Result<(), String> {
        let parent = path.parent().ok_or("Proxy backup directory is missing")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let pending = path.with_extension("pending");
        let bytes = serde_json::to_vec_pretty(snapshot).map_err(|error| error.to_string())?;
        {
            // A stale pending file is safe to replace: only the published JSON
            // authorizes changing Windows, and the caller is single-instance.
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&pending)
                .map_err(|error| format!("Cannot create proxy backup: {error}"))?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| format!("Cannot flush proxy backup: {error}"))?;
        }
        #[cfg(windows)]
        super::native::publish_snapshot(&pending, path)?;
        #[cfg(not(windows))]
        {
            fs::rename(&pending, path).map_err(|error| error.to_string())?;
            fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn clear_snapshot(path: &Path) -> Result<(), String> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "Proxy restored but backup cannot be removed: {error}"
            )),
        }
    }

    fn conflict() -> String {
        "Windows proxy settings changed outside ClashBar; left them untouched".into()
    }

    fn archive_snapshot(path: &Path) -> Result<(), String> {
        use std::{
            sync::atomic::{AtomicU64, Ordering},
            time::{SystemTime, UNIX_EPOCH},
        };
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("Cannot timestamp previous proxy backup: {error}"))?
            .as_nanos();
        let archive = path.with_extension(format!(
            "external-change-{timestamp}-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Abandon ownership, not the saved original. Keeping an archive permits
        // inspection without trapping stop/quit or replacing another client's
        // newer settings. A later enable takes those settings as its baseline.
        #[cfg(windows)]
        super::native::publish_snapshot(path, &archive)?;
        #[cfg(not(windows))]
        {
            fs::rename(path, &archive)
                .map_err(|error| format!("Cannot archive previous proxy backup: {error}"))?;
            if let Some(parent) = path.parent() {
                fs::File::open(parent)
                    .and_then(|directory| directory.sync_all())
                    .map_err(|error| error.to_string())?;
            }
        }
        eprintln!(
            "{}; prior settings archived at {}",
            conflict(),
            archive.display()
        );
        Ok(())
    }

    pub(super) fn is_enabled(path: &Path, platform: &impl Platform) -> Result<bool, String> {
        match read_snapshot(path)? {
            Some(snapshot) => Ok(platform.read()? == snapshot.applied),
            None => Ok(false),
        }
    }

    pub(super) fn restore(path: &Path, platform: &impl Platform) -> Result<(), String> {
        let Some(snapshot) = read_snapshot(path)? else {
            return Ok(());
        };
        let current = platform.read()?;
        if current == snapshot.original {
            // Covers a crash before enable or after restore but before deletion.
            // Retry notification if the previous operation failed at that step.
            platform.notify()?;
            return clear_snapshot(path);
        }
        if current != snapshot.applied {
            return archive_snapshot(path);
        }
        // Keep the snapshot on any failure, including notification/verification.
        platform.apply(&snapshot.original)?;
        if platform.read()? != snapshot.original {
            return archive_snapshot(path);
        }
        clear_snapshot(path)
    }

    pub(super) fn enable(path: &Path, platform: &impl Platform, port: u16) -> Result<(), String> {
        if port == 0 {
            return Err("Proxy port must be between 1 and 65535".into());
        }
        if let Some(snapshot) = read_snapshot(path)? {
            let desired = ProxySettings::for_port(&snapshot.original, port);
            if platform.read()? == snapshot.applied && desired == snapshot.applied {
                return Ok(()); // Never replace the original on repeated enable.
            }
            // Port changes restore first. Thus a crash cannot strand the former
            // port while a newly written journal only recognizes the new port.
            restore(path, platform)?;
        }
        let original = platform.read()?;
        original.validate()?;
        let applied = ProxySettings::for_port(&original, port);
        let snapshot = Snapshot {
            version: 1,
            original,
            applied,
        };
        save_snapshot(path, &snapshot)?;
        // Check again after disk I/O before touching the shared Windows setting.
        if platform.read()? != snapshot.original {
            archive_snapshot(path)?;
            return Err(conflict());
        }
        let apply_result = platform.apply(&snapshot.applied).and_then(|_| {
            if platform.read()? == snapshot.applied {
                Ok(())
            } else {
                Err(conflict())
            }
        });
        match apply_result {
            Ok(()) => Ok(()),
            Err(error) => match restore(path, platform) {
                Ok(()) => Err(format!(
                    "Cannot enable proxy: {error}. Proxy recovery completed"
                )),
                Err(rollback_error) => Err(format!(
                    "Cannot enable proxy: {error}. Recovery is still required; \
                     backup retained at {}: {rollback_error}",
                    path.display()
                )),
            },
        }
    }
}

#[cfg(windows)]
mod native {
    use super::transaction::{Platform, ProxySettings};
    use std::{ffi::c_void, mem::size_of, os::windows::ffi::OsStrExt, path::Path, ptr};
    use windows_sys::Win32::{
        Foundation::GlobalFree,
        Networking::WinInet::{
            InternetQueryOptionW, InternetSetOptionW, INTERNET_OPTION_PER_CONNECTION_OPTION,
            INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
            INTERNET_PER_CONN_AUTOCONFIG_URL, INTERNET_PER_CONN_FLAGS, INTERNET_PER_CONN_FLAGS_UI,
            INTERNET_PER_CONN_OPTIONW, INTERNET_PER_CONN_OPTION_LISTW,
            INTERNET_PER_CONN_PROXY_BYPASS, INTERNET_PER_CONN_PROXY_SERVER,
        },
        Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH},
    };

    pub(super) struct WinInet;

    fn last_error(operation: &str) -> String {
        format!("{operation}: {}", std::io::Error::last_os_error())
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    fn options(flags_option: u32) -> [INTERNET_PER_CONN_OPTIONW; 4] {
        // Zero initializes every union, including string pointers on query error.
        let mut result: [INTERNET_PER_CONN_OPTIONW; 4] = unsafe { std::mem::zeroed() };
        for (entry, option) in result.iter_mut().zip([
            flags_option,
            INTERNET_PER_CONN_PROXY_SERVER,
            INTERNET_PER_CONN_PROXY_BYPASS,
            INTERNET_PER_CONN_AUTOCONFIG_URL,
        ]) {
            entry.dwOption = option;
        }
        result
    }

    fn option_list(options: &mut [INTERNET_PER_CONN_OPTIONW; 4]) -> INTERNET_PER_CONN_OPTION_LISTW {
        INTERNET_PER_CONN_OPTION_LISTW {
            dwSize: size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
            pszConnection: ptr::null_mut(), // NULL selects LAN/default connection.
            dwOptionCount: options.len() as u32,
            dwOptionError: 0,
            pOptions: options.as_mut_ptr(),
        }
    }

    struct QueryOptions([INTERNET_PER_CONN_OPTIONW; 4]);

    impl Drop for QueryOptions {
        fn drop(&mut self) {
            for option in &self.0[1..] {
                // WinINet allocates each queried string with GlobalAlloc, even
                // when another option fails. Never free the flags union.
                unsafe {
                    let value = option.Value.pszValue;
                    if !value.is_null() {
                        GlobalFree(value.cast());
                    }
                }
            }
        }
    }

    unsafe fn queried_string(value: *const u16) -> Result<String, String> {
        if value.is_null() {
            return Ok(String::new());
        }
        let mut len = 0;
        // WinINet promises a NUL-terminated allocation for string options.
        while unsafe { *value.add(len) } != 0 {
            len += 1;
        }
        String::from_utf16(unsafe { std::slice::from_raw_parts(value, len) }).map_err(|_| {
            "Windows proxy settings contain invalid UTF-16; no settings changed".into()
        })
    }

    fn query(flags_option: u32) -> Result<ProxySettings, String> {
        let mut queried = QueryOptions(options(flags_option));
        let mut list = option_list(&mut queried.0);
        let mut size = size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32;
        let result = unsafe {
            InternetQueryOptionW(
                ptr::null_mut(),
                INTERNET_OPTION_PER_CONNECTION_OPTION,
                (&mut list as *mut INTERNET_PER_CONN_OPTION_LISTW).cast::<c_void>(),
                &mut size,
            )
        };
        if result == 0 {
            return Err(last_error("Cannot read current-user proxy settings"));
        }
        // QueryOptions owns and releases all returned allocations on every path.
        unsafe {
            Ok(ProxySettings {
                flags: queried.0[0].Value.dwValue,
                server: queried_string(queried.0[1].Value.pszValue)?,
                bypass: queried_string(queried.0[2].Value.pszValue)?,
                pac_url: queried_string(queried.0[3].Value.pszValue)?,
            })
        }
    }

    impl Platform for WinInet {
        fn read(&self) -> Result<ProxySettings, String> {
            // Microsoft recommends FLAGS_UI for reading and FLAGS for restoring.
            query(INTERNET_PER_CONN_FLAGS_UI).or_else(|_| query(INTERNET_PER_CONN_FLAGS))
        }

        fn apply(&self, settings: &ProxySettings) -> Result<(), String> {
            let mut server = wide(&settings.server);
            let mut bypass = wide(&settings.bypass);
            let mut pac_url = wide(&settings.pac_url);
            let mut values = options(INTERNET_PER_CONN_FLAGS);
            values[0].Value.dwValue = settings.flags;
            values[1].Value.pszValue = server.as_mut_ptr();
            values[2].Value.pszValue = bypass.as_mut_ptr();
            values[3].Value.pszValue = pac_url.as_mut_ptr();
            let mut list = option_list(&mut values);
            let result = unsafe {
                InternetSetOptionW(
                    ptr::null_mut(),
                    INTERNET_OPTION_PER_CONNECTION_OPTION,
                    (&mut list as *mut INTERNET_PER_CONN_OPTION_LISTW).cast::<c_void>(),
                    size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
                )
            };
            if result == 0 {
                return Err(last_error("Cannot set current-user proxy settings"));
            }
            self.notify()
        }

        fn notify(&self) -> Result<(), String> {
            // Attempt both calls even if the first fails; preserve the first error.
            let mut error = None;
            for option in [INTERNET_OPTION_SETTINGS_CHANGED, INTERNET_OPTION_REFRESH] {
                if unsafe { InternetSetOptionW(ptr::null_mut(), option, ptr::null_mut(), 0) } == 0 {
                    let failure = last_error("Cannot notify Windows of proxy changes");
                    error.get_or_insert(failure);
                }
            }
            error.map_or(Ok(()), Err)
        }
    }

    pub(super) fn publish_snapshot(source: &Path, destination: &Path) -> Result<(), String> {
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // A same-directory, write-through rename publishes only a complete,
        // flushed snapshot before any Windows settings mutation may happen.
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(last_error("Cannot publish proxy backup"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::transaction::{self, Platform, ProxySettings};
    use std::{
        cell::{Cell, RefCell},
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "clashbar-proxy-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn snapshot(&self) -> PathBuf {
            self.0.join("proxy.json")
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct FakeWindows {
        current: RefCell<ProxySettings>,
        writes: Cell<usize>,
        fail_before_write: Cell<bool>,
        fail_after_write: Cell<usize>,
        fail_notify: Cell<bool>,
    }

    impl FakeWindows {
        fn new() -> Self {
            Self {
                current: RefCell::new(ProxySettings {
                    flags: 1 | 2 | 4 | 8,
                    server: "http=corporate.example:8080".into(),
                    bypass: "*.internal;localhost".into(),
                    pac_url: "https://corporate.example/proxy.pac".into(),
                }),
                writes: Cell::new(0),
                fail_before_write: Cell::new(false),
                fail_after_write: Cell::new(0),
                fail_notify: Cell::new(false),
            }
        }
    }

    impl Platform for FakeWindows {
        fn read(&self) -> Result<ProxySettings, String> {
            Ok(self.current.borrow().clone())
        }

        fn apply(&self, settings: &ProxySettings) -> Result<(), String> {
            if self.fail_before_write.get() {
                return Err("simulated native write failure".into());
            }
            self.writes.set(self.writes.get() + 1);
            self.current.replace(settings.clone());
            if self.fail_after_write.get() > 0 {
                self.fail_after_write.set(self.fail_after_write.get() - 1);
                Err("simulated failure after native write".into())
            } else {
                self.notify()
            }
        }

        fn notify(&self) -> Result<(), String> {
            if self.fail_notify.get() {
                Err("simulated notification failure".into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn restores_existing_proxy_pac_bypass_and_auto_detection_after_restart() {
        let dir = TestDirectory::new();
        let platform = FakeWindows::new();
        let original = platform.read().unwrap();
        transaction::enable(&dir.snapshot(), &platform, 7890).unwrap();
        assert_eq!(platform.read().unwrap().flags, 3);
        assert!(transaction::is_enabled(&dir.snapshot(), &platform).unwrap());
        // No in-memory session state is needed for recovery.
        transaction::restore(&dir.snapshot(), &platform).unwrap();
        assert_eq!(platform.read().unwrap(), original);
        assert!(!dir.snapshot().exists());
        transaction::restore(&dir.snapshot(), &platform).unwrap();
        assert_eq!(platform.writes.get(), 2);
    }

    #[test]
    fn repeated_enable_never_overwrites_original_backup() {
        let dir = TestDirectory::new();
        let platform = FakeWindows::new();
        transaction::enable(&dir.snapshot(), &platform, 7890).unwrap();
        let backup = fs::read(dir.snapshot()).unwrap();
        transaction::enable(&dir.snapshot(), &platform, 7890).unwrap();
        assert_eq!(backup, fs::read(dir.snapshot()).unwrap());
        assert_eq!(platform.writes.get(), 1);
    }

    #[test]
    fn port_change_keeps_the_true_original_settings() {
        let dir = TestDirectory::new();
        let platform = FakeWindows::new();
        let original = platform.read().unwrap();
        transaction::enable(&dir.snapshot(), &platform, 7890).unwrap();
        transaction::enable(&dir.snapshot(), &platform, 7891).unwrap();
        assert!(platform.read().unwrap().server.contains(":7891"));
        assert_eq!(
            transaction::read_snapshot(&dir.snapshot())
                .unwrap()
                .unwrap()
                .original,
            original
        );
        transaction::restore(&dir.snapshot(), &platform).unwrap();
        assert_eq!(platform.read().unwrap(), original);
    }

    #[test]
    fn foreign_settings_are_never_overwritten_even_if_server_still_matches() {
        let dir = TestDirectory::new();
        let platform = FakeWindows::new();
        transaction::enable(&dir.snapshot(), &platform, 7890).unwrap();
        let backup = fs::read(dir.snapshot()).unwrap();
        platform.current.borrow_mut().pac_url = "https://other.example/proxy.pac".into();
        let foreign = platform.read().unwrap();
        assert!(!transaction::is_enabled(&dir.snapshot(), &platform).unwrap());
        transaction::restore(&dir.snapshot(), &platform).unwrap();
        assert_eq!(platform.read().unwrap(), foreign);
        assert_eq!(platform.writes.get(), 1);
        assert!(!dir.snapshot().exists());
        let archives: Vec<_> = fs::read_dir(&dir.0)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(archives.len(), 1);
        assert_eq!(backup, fs::read(&archives[0]).unwrap());
        transaction::enable(&dir.snapshot(), &platform, 7890).unwrap();
        assert_eq!(
            transaction::read_snapshot(&dir.snapshot())
                .unwrap()
                .unwrap()
                .original,
            foreign
        );
        transaction::restore(&dir.snapshot(), &platform).unwrap();
        assert_eq!(platform.read().unwrap(), foreign);
    }

    #[test]
    fn enable_failure_rolls_back_owned_settings() {
        let dir = TestDirectory::new();
        let platform = FakeWindows::new();
        let original = platform.read().unwrap();
        platform.fail_after_write.set(1);
        assert!(transaction::enable(&dir.snapshot(), &platform, 7890).is_err());
        assert_eq!(platform.read().unwrap(), original);
        assert!(!dir.snapshot().exists());
    }

    #[test]
    fn failed_restore_keeps_backup_until_notification_can_be_retried() {
        let dir = TestDirectory::new();
        let platform = FakeWindows::new();
        transaction::enable(&dir.snapshot(), &platform, 7890).unwrap();
        let backup = fs::read(dir.snapshot()).unwrap();
        platform.fail_notify.set(true);
        assert!(transaction::restore(&dir.snapshot(), &platform).is_err());
        assert_eq!(backup, fs::read(dir.snapshot()).unwrap());
        platform.fail_notify.set(false);
        transaction::restore(&dir.snapshot(), &platform).unwrap();
        assert!(!dir.snapshot().exists());
    }

    #[test]
    fn failed_native_restore_retains_original_backup_and_owned_state() {
        let dir = TestDirectory::new();
        let platform = FakeWindows::new();
        let original = platform.read().unwrap();
        transaction::enable(&dir.snapshot(), &platform, 7890).unwrap();
        let backup = fs::read(dir.snapshot()).unwrap();
        platform.fail_before_write.set(true);
        assert!(transaction::restore(&dir.snapshot(), &platform).is_err());
        assert_eq!(backup, fs::read(dir.snapshot()).unwrap());
        assert!(transaction::is_enabled(&dir.snapshot(), &platform).unwrap());
        platform.fail_before_write.set(false);
        transaction::restore(&dir.snapshot(), &platform).unwrap();
        assert_eq!(platform.read().unwrap(), original);
    }

    #[test]
    fn backup_write_failure_never_mutates_windows_settings() {
        let dir = TestDirectory::new();
        let platform = FakeWindows::new();
        let original = platform.read().unwrap();
        fs::create_dir(dir.snapshot().with_extension("pending")).unwrap();
        assert!(transaction::enable(&dir.snapshot(), &platform, 7890).is_err());
        assert_eq!(platform.writes.get(), 0);
        assert_eq!(platform.read().unwrap(), original);
    }

    #[test]
    fn invalid_backup_fails_closed_without_writing_windows_settings() {
        let dir = TestDirectory::new();
        let platform = FakeWindows::new();
        fs::write(dir.snapshot(), "not json").unwrap();
        assert!(transaction::enable(&dir.snapshot(), &platform, 7890).is_err());
        assert!(transaction::restore(&dir.snapshot(), &platform).is_err());
        assert_eq!(platform.writes.get(), 0);
    }

    /// Run only in a disposable Windows account/runner, without another proxy
    /// manager. Both --ignored and the environment variable are required.
    #[cfg(windows)]
    #[test]
    #[ignore = "Changes real current-user Windows proxy settings; disposable account only"]
    fn windows_wininet_roundtrip_restores_actual_settings() {
        assert_eq!(
            std::env::var("CLASHBAR_PROXY_INTEGRATION_TEST").as_deref(),
            Ok("1"),
            "Set CLASHBAR_PROXY_INTEGRATION_TEST=1 only in a disposable Windows account"
        );
        let dir = TestDirectory::new();
        let data_dir = dir.0.clone();
        // A failing native test must retain its recovery journal on disk.
        std::mem::forget(dir);
        let platform = super::native::WinInet;
        let original = platform.read().unwrap();
        struct RestoreOnDrop(super::SystemProxy);
        impl Drop for RestoreOnDrop {
            fn drop(&mut self) {
                if let Err(error) = self.0.restore() {
                    eprintln!("Native proxy test cleanup failed: {error}");
                }
            }
        }
        let guard = RestoreOnDrop(super::SystemProxy::new(data_dir.clone()));
        guard.0.enable(49199).unwrap();
        assert!(guard.0.is_enabled().unwrap());
        guard.0.enable(49199).unwrap();
        guard.0.restore().unwrap();
        assert_eq!(platform.read().unwrap(), original);
        assert!(!guard.0.is_enabled().unwrap());
        drop(guard);
        fs::remove_dir_all(data_dir).unwrap();
    }
}
