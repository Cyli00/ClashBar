use super::*;
use crate::subscriptions::{self, SubscriptionInput};
use std::collections::HashMap;

#[cfg(test)]
#[path = "services_review_tests.rs"]
mod review_tests;

#[derive(Default)]
pub struct ProfileMonitor {
    observed: HashMap<PathBuf, String>,
}

impl Engine {
    pub fn seed_default_profile(&mut self) -> Result<(), String> {
        if !self.settings.profiles.is_empty() {
            return Ok(());
        }
        self.import_profile(
            include_bytes!("../../resources/ClashBar.yaml"),
            "ClashBar.yaml",
        )?;
        Ok(())
    }
    pub async fn import_file(&mut self, path: &Path) -> Result<Status, String> {
        self.import_file_confirmed(path, false).await
    }

    pub fn profile_import_collision(&self, path: &Path) -> Result<Option<String>, String> {
        self.require_local_target()?;
        let name = subscriptions::normalized_name(
            path.file_name()
                .and_then(|name| name.to_str())
                .ok_or("配置文件名无效。")?,
            "Imported.yaml",
        )?;
        Ok(self
            .find_named_profile(&name)
            .map(|profile| profile.name.clone()))
    }

    fn find_named_profile(&self, name: &str) -> Option<&config::Profile> {
        self.settings
            .profiles
            .iter()
            .find(|profile| profile.name.trim().eq_ignore_ascii_case(name))
    }

    pub async fn import_file_confirmed(
        &mut self,
        path: &Path,
        overwrite: bool,
    ) -> Result<Status, String> {
        self.require_local_target()?;
        let name = subscriptions::normalized_name(
            path.file_name()
                .and_then(|name| name.to_str())
                .ok_or("配置文件名无效。")?,
            "Imported.yaml",
        )?;
        if self.find_named_profile(&name).is_some() && !overwrite {
            return Err(format!("配置“{name}”已存在，请确认覆盖。"));
        }
        let bytes = read_profile(path)?;
        self.copy_local_providers(&bytes, path.parent().ok_or("配置目录无效。")?)?;
        self.install_named_profile(&bytes, &name, overwrite, None)
            .await
    }

    async fn install_named_profile(
        &mut self,
        bytes: &[u8],
        name: &str,
        overwrite: bool,
        subscription: Option<&subscriptions::PreparedSubscription>,
    ) -> Result<Status, String> {
        self.require_local_target()?;
        config::runtime(bytes, &self.settings, "import-validation")?;
        let existing = self.find_named_profile(name).cloned();
        if existing.is_some() && !overwrite {
            return Err(format!("配置“{name}”已存在，请确认覆盖。"));
        }
        let mut settings = self.settings.clone();
        let previous = existing
            .as_ref()
            .map(|profile| {
                read_profile(
                    &self
                        .profiles_directory()
                        .join(format!("{}.yaml", profile.id)),
                )
            })
            .transpose()?;
        let id = if let Some(profile) = &existing {
            self.replace_profile_content(&profile.id, bytes).await?;
            profile.id.clone()
        } else {
            if settings.profiles.len() >= 128 {
                return Err("配置库最多保存 128 个配置。".into());
            }
            let mut id = config::profile_id(bytes);
            if settings.profiles.iter().any(|profile| profile.id == id) {
                id = config::profile_id(format!("{id}:{name}:{}", config::secret()).as_bytes());
            }
            config::atomic_write(&self.profiles_directory().join(format!("{id}.yaml")), bytes)?;
            self.remember_profile_content(&id, bytes)?;
            settings.profiles.push(config::Profile {
                id: id.clone(),
                name: name.into(),
            });
            if settings.active_profile_id.is_none() {
                settings.active_profile_id = Some(id.clone());
                settings.config_name = Some(name.into());
            }
            id
        };
        // 文件导入解除此前的订阅来源；确认覆盖保留配置身份，因此 SSID 绑定仍指向原配置。
        settings.subscriptions.retain(|item| item.profile_id != id);
        if let Some(subscription) = subscription {
            settings
                .subscriptions
                .push(subscription.bind(id.clone(), subscriptions::now_ms())?);
        }
        if let Err(error) = settings.save(&self.directory) {
            if let Some(previous) = previous {
                return match self.replace_profile_content(&id, &previous).await {
                    Ok(_) => Err(format!("导入设置保存失败，已恢复原配置：{error}")),
                    Err(restore) => {
                        Err(format!("导入设置保存失败：{error} 恢复配置失败：{restore}"))
                    }
                };
            }
            let archive = self.directory.join("failed-imports");
            fs::create_dir_all(&archive)
                .map_err(|failure| format!("{error} 无法保留失败导入：{failure}"))?;
            fs::rename(
                self.profiles_directory().join(format!("{id}.yaml")),
                archive.join(format!("{id}-{}.yaml", &config::secret()[..12])),
            )
            .map_err(|failure| format!("{error} 无法移出失败导入：{failure}"))?;
            return Err(error);
        }
        self.settings = settings;
        Ok(self.status())
    }

    fn copy_local_providers(&self, bytes: &[u8], source_directory: &Path) -> Result<(), String> {
        let parsed = config::parse(bytes)?;
        let root = source_directory
            .canonicalize()
            .map_err(|_| "无法读取配置所在目录。")?;
        let cache_id = config::profile_id(bytes);
        for kind in ["proxy-providers", "rule-providers"] {
            let Some(providers) = parsed
                .get(serde_yaml::Value::String(kind.into()))
                .and_then(serde_yaml::Value::as_mapping)
            else {
                continue;
            };
            for (index, (_, provider)) in providers.iter().enumerate() {
                if provider["type"].as_str() != Some("file") {
                    continue;
                }
                let relative = provider["path"]
                    .as_str()
                    .ok_or("文件型 Provider 缺少路径。")?;
                let target = self
                    .directory
                    .join("runtime/providers")
                    .join(&cache_id)
                    .join(kind)
                    .join(format!("{index}.yaml"));
                if target.is_file() {
                    continue;
                }
                let source = root
                    .join(relative)
                    .canonicalize()
                    .map_err(|_| "无法读取文件型 Provider，请将资源与配置一起放入配置目录。")?;
                if !source.starts_with(&root) {
                    return Err("文件型 Provider 必须位于导入配置的目录内。".into());
                }
                let metadata = source.metadata().map_err(|_| "无法读取文件型 Provider。")?;
                if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
                    return Err("文件型 Provider 必须为不超过 64 MiB 的普通文件。".into());
                }
                let content = fs::read(&source).map_err(|_| "无法读取文件型 Provider。")?;
                fs::create_dir_all(target.parent().ok_or("缓存路径无效。")?)
                    .map_err(|_| "无法创建 Provider 目录。")?;
                config::atomic_write(&target, &content)?;
            }
        }
        self.copy_local_web_ui(&parsed, &root, &cache_id)?;
        Ok(())
    }

    fn copy_local_web_ui(
        &self,
        parsed: &serde_yaml::Mapping,
        root: &Path,
        cache_id: &str,
    ) -> Result<(), String> {
        let Some(relative) = parsed
            .get(serde_yaml::Value::String("external-ui".into()))
            .and_then(serde_yaml::Value::as_str)
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(());
        };
        let source = root.join(relative);
        if !source.exists()
            && parsed
                .get(serde_yaml::Value::String("external-ui-url".into()))
                .and_then(serde_yaml::Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        {
            return Ok(());
        }
        let source = source
            .canonicalize()
            .map_err(|_| "本地 Web UI 目录不存在。")?;
        if !source.starts_with(root) || !source.is_dir() {
            return Err("本地 Web UI 必须位于配置所在目录内。".into());
        }
        let target = self.directory.join("runtime/ui").join(cache_id);
        let mut pending = vec![(source.clone(), PathBuf::new())];
        let mut files = Vec::new();
        let mut total = 0u64;
        while let Some((directory, relative)) = pending.pop() {
            if relative.components().count() > 32 {
                return Err("本地 Web UI 目录层数不能超过 32。".into());
            }
            for entry in fs::read_dir(directory).map_err(|_| "无法读取本地 Web UI 目录。")?
            {
                let entry = entry.map_err(|_| "无法读取本地 Web UI 文件。")?;
                let metadata = entry
                    .file_type()
                    .map_err(|_| "无法检查本地 Web UI 文件。")?;
                let resolved = entry
                    .path()
                    .canonicalize()
                    .map_err(|_| "无法解析本地 Web UI 文件。")?;
                if metadata.is_symlink() || !resolved.starts_with(&source) {
                    return Err("本地 Web UI 不能包含符号链接或指向目录外的文件。".into());
                }
                let next = relative.join(entry.file_name());
                if metadata.is_dir() {
                    pending.push((resolved, next));
                } else if metadata.is_file() {
                    let size = entry
                        .metadata()
                        .map_err(|_| "无法读取本地 Web UI 文件信息。")?
                        .len();
                    total = total.checked_add(size).ok_or("本地 Web UI 总大小无效。")?;
                    if total > 128 * 1024 * 1024 || files.len() >= 4096 {
                        return Err(
                            "本地 Web UI 最多包含 4096 个文件且总大小不得超过 128 MiB。".into()
                        );
                    }
                    files.push((resolved, next));
                } else {
                    return Err("本地 Web UI 只能包含普通文件和目录。".into());
                }
            }
        }
        let staging = self
            .directory
            .join("runtime/ui")
            .join(format!(".staging-{}", config::secret()));
        fs::create_dir_all(&staging).map_err(|_| "无法创建 Web UI 缓存目录。")?;
        let copied = (|| -> Result<(), String> {
            for (source, relative) in files {
                let target_file = staging.join(relative);
                fs::create_dir_all(target_file.parent().ok_or("Web UI 缓存目录无效。")?)
                    .map_err(|_| "无法创建 Web UI 缓存目录。")?;
                let bytes = fs::read(source).map_err(|_| "无法读取 Web UI 资源。")?;
                config::atomic_write(&target_file, &bytes)?;
            }
            let archive = self
                .directory
                .join("runtime/ui")
                .join(format!(".previous-{cache_id}-{}", &config::secret()[..12]));
            let had_previous = target.exists();
            if had_previous {
                fs::rename(&target, &archive).map_err(|_| "无法保留旧 Web UI 资源。")?;
            }
            if let Err(error) = fs::rename(&staging, &target) {
                if had_previous {
                    fs::rename(&archive, &target).map_err(|restore| {
                        format!("Web UI 更新失败：{error} 恢复资源失败：{restore}")
                    })?;
                }
                return Err(format!("无法发布 Web UI 资源：{error}"));
            }
            Ok(())
        })();
        if copied.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        copied?;
        Ok(())
    }
    pub fn set_status_bar_style(&mut self, style: String) -> Result<Status, String> {
        if !matches!(style.as_str(), "iconAndSpeed" | "iconOnly" | "speedOnly") {
            return Err("状态栏样式无效。".into());
        }
        let mut settings = self.settings.clone();
        settings.status_bar_style = style;
        settings.save(&self.directory)?;
        self.settings = settings;
        Ok(self.status())
    }
    pub async fn add_subscription(&mut self, input: SubscriptionInput) -> Result<Status, String> {
        self.require_local_target()?;
        let prepared = input.prepare()?;
        if self.find_named_profile(&prepared.name).is_some() && !prepared.overwrite {
            return Err(format!("配置“{}”已存在，请确认覆盖。", prepared.name));
        }
        let bytes = crate::subscription::download(&prepared.url).await?;
        config::runtime(&bytes, &self.settings, "subscription-validation")
            .map_err(|_| "订阅内容不是有效的 mihomo YAML 配置。")?;
        self.install_named_profile(&bytes, &prepared.name, prepared.overwrite, Some(&prepared))
            .await
    }

    pub fn subscription_url(&self, id: &str) -> Result<String, String> {
        self.require_local_target()?;
        self.settings
            .subscriptions
            .iter()
            .find(|item| item.profile_id == id)
            .map(|item| item.url.clone())
            .ok_or_else(|| "该配置没有订阅来源。".into())
    }

    pub fn save_subscription(
        &mut self,
        id: &str,
        url: Option<String>,
        enabled: bool,
        hours: u32,
    ) -> Result<Status, String> {
        self.require_local_target()?;
        if hours == 0 || hours > 8760 {
            return Err("订阅更新间隔需为 1–8760 小时。".into());
        }
        let mut settings = self.settings.clone();
        let item = settings
            .subscriptions
            .iter_mut()
            .find(|item| item.profile_id == id)
            .ok_or("该配置没有订阅来源。")?;
        if let Some(url) = url.filter(|url| !url.trim().is_empty()) {
            item.url = crate::subscription::validate_url(url.trim())?.to_string();
        }
        item.auto_update_enabled = enabled;
        item.auto_update_interval_hours = hours;
        item.validate()?;
        settings.save(&self.directory)?;
        self.settings = settings;
        Ok(self.status())
    }

    pub async fn refresh_subscription(&mut self, id: &str) -> Result<Status, String> {
        let url = self.subscription_url(id)?;
        let previous = read_profile(&self.profiles_directory().join(format!("{id}.yaml")))?;
        let result = async {
            let bytes = crate::subscription::download(&url).await?;
            self.replace_profile_content(id, &bytes).await
        }
        .await;
        let mut settings = self.settings.clone();
        let item = settings
            .subscriptions
            .iter_mut()
            .find(|item| item.profile_id == id)
            .ok_or("该配置没有订阅来源。")?;
        match &result {
            Ok(changed) => item.mark_success(subscriptions::now_ms(), *changed),
            Err(error) => item.mark_failed(subscriptions::now_ms(), error),
        }
        if let Err(error) = settings.save(&self.directory) {
            if result.as_ref().is_ok_and(|changed| *changed) {
                return match self.replace_profile_content(id, &previous).await {
                    Ok(_) => Err(format!("订阅元数据保存失败，已恢复原配置：{error}")),
                    Err(restore) => Err(format!(
                        "订阅元数据保存失败：{error} 恢复配置失败：{restore}"
                    )),
                };
            }
            return Err(error);
        }
        self.settings = settings;
        result.map_err(|error| subscriptions::safe_error(&error))?;
        Ok(self.status())
    }

    pub async fn refresh_all_subscriptions(&mut self, due_only: bool) -> Result<Status, String> {
        self.require_local_target()?;
        let ids: Vec<_> = self
            .settings
            .subscriptions
            .iter()
            .filter(|item| !due_only || item.is_due(subscriptions::now_ms()))
            .map(|item| item.profile_id.clone())
            .collect();
        let total = ids.len();
        let mut failed = 0;
        for id in ids {
            if self.refresh_subscription(&id).await.is_err() {
                failed += 1;
            }
        }
        if failed > 0 {
            return Err(format!(
                "订阅更新完成：成功 {} 项，失败 {failed} 项。请在订阅设置中查看失败原因。",
                total - failed
            ));
        }
        Ok(self.status())
    }

    pub(super) fn remember_profile_content(&self, id: &str, bytes: &[u8]) -> Result<(), String> {
        let directory = self.directory.join("accepted-profiles");
        fs::create_dir_all(&directory).map_err(|error| format!("无法保存配置恢复副本：{error}"))?;
        config::atomic_write(&directory.join(format!("{id}.yaml")), bytes)
    }

    // 更新保留配置 ID，避免 SSID 绑定、当前选择和订阅来源因内容变化失效。
    async fn replace_profile_content(&mut self, id: &str, bytes: &[u8]) -> Result<bool, String> {
        let previous = read_profile(&self.profiles_directory().join(format!("{id}.yaml")))?;
        self.apply_profile_content(id, bytes, &previous).await
    }

    async fn apply_profile_content(
        &mut self,
        id: &str,
        bytes: &[u8],
        previous: &[u8],
    ) -> Result<bool, String> {
        self.require_local_target()?;
        self.profile_settings(id)?;
        config::runtime(bytes, &self.settings, "profile-validation")
            .map_err(|_| "配置内容不是有效的 mihomo YAML。")?;
        if previous == bytes {
            self.remember_profile_content(id, bytes)?;
            return Ok(false);
        }
        let path = self.profiles_directory().join(format!("{id}.yaml"));
        let active_running =
            self.settings.active_profile_id.as_deref() == Some(id) && self.child.is_some();
        let proxy_enabled = active_running && self.owned_proxy_enabled()?;
        if active_running {
            self.validate_candidate(bytes, &self.settings.clone())
                .await?;
        }
        config::atomic_write(&path, bytes)?;
        let activation = if active_running {
            match self.stop() {
                Ok(_) => self.resume(proxy_enabled).await.map(|_| ()),
                Err(error) => Err(error),
            }
        } else {
            Ok(())
        };
        let activation = activation.and_then(|_| self.remember_profile_content(id, bytes));
        if let Err(error) = activation {
            // 停机失败时原内核仍在使用已加载配置，也必须恢复磁盘来源，避免下一次启动误用候选。
            config::atomic_write(&path, previous)
                .map_err(|restore| format!("{error} 配置恢复失败：{restore}"))?;
            self.remember_profile_content(id, previous)?;
            if active_running {
                if let Err(cleanup) = self.stop() {
                    return Err(format!("{error} 无法安全恢复原内核：{cleanup}"));
                }
                if let Err(restore) = self.resume(proxy_enabled).await {
                    let message = format!("{error} 原内核恢复失败：{restore}");
                    self.last_error = Some(message.clone());
                    return Err(message);
                }
            }
            return Err(format!("{error} 已恢复原配置。"));
        }
        Ok(true)
    }

    async fn observe_profile_file(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let known = self
            .settings
            .profiles
            .iter()
            .any(|profile| profile.id == stem);
        if known {
            let accepted_path = self
                .directory
                .join("accepted-profiles")
                .join(format!("{stem}.yaml"));
            let previous = match read_profile(&accepted_path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    config::runtime(bytes, &self.settings, "directory-validation")?;
                    self.remember_profile_content(stem, bytes)?;
                    return Ok(());
                }
            };
            if previous == bytes {
                return Ok(());
            }
            let result = async {
                self.copy_local_providers(bytes, &self.profiles_directory())?;
                self.apply_profile_content(stem, bytes, &previous).await
            }
            .await;
            if let Err(error) = result {
                let rejected = self.directory.join("rejected-profile-edits");
                fs::create_dir_all(&rejected)
                    .map_err(|failure| format!("{error} 无法保留失败编辑：{failure}"))?;
                config::atomic_write(
                    &rejected.join(format!("{stem}-{}.yaml", &config::secret()[..12])),
                    bytes,
                )?;
                config::atomic_write(path, &previous)?;
                return Err(format!(
                    "{error} 失败编辑已保存在 rejected-profile-edits，配置文件已恢复。"
                ));
            }
            return Ok(());
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("配置文件名无效。")?;
        if let Some(id) = self.settings.profile_sources.get(name).cloned() {
            // 手动删除配置后保留来源断开记录，避免目录观察再次导入同一文件。
            if !self
                .settings
                .profiles
                .iter()
                .any(|profile| profile.id == id)
            {
                return Ok(());
            }
            self.copy_local_providers(bytes, &self.profiles_directory())?;
            self.replace_profile_content(&id, bytes).await?;
        } else {
            self.copy_local_providers(bytes, &self.profiles_directory())?;
            let id = if let Some(profile) = self.find_named_profile(name).cloned() {
                self.replace_profile_content(&profile.id, bytes).await?;
                profile.id
            } else {
                self.install_named_profile(bytes, name, false, None).await?;
                self.find_named_profile(name)
                    .ok_or("导入配置未写入配置库。")?
                    .id
                    .clone()
            };
            let mut settings = self.settings.clone();
            settings.profile_sources.insert(name.into(), id);
            settings.save(&self.directory)?;
            self.settings = settings;
        }
        Ok(())
    }

    pub async fn monitor_profiles(&mut self, monitor: &mut ProfileMonitor) -> Result<(), String> {
        if self.active_remote().is_some() {
            return Ok(());
        }
        let directory = self.profiles_directory();
        let entries =
            fs::read_dir(&directory).map_err(|error| format!("无法读取配置目录：{error}"))?;
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            if entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
                && path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml")
                })
            {
                paths.push(path);
            }
        }
        paths.sort();
        let missing: Vec<_> = self
            .settings
            .profiles
            .iter()
            .filter(|profile| !paths.contains(&directory.join(format!("{}.yaml", profile.id))))
            .map(|profile| profile.id.clone())
            .collect();
        if !missing.is_empty() {
            let active_missing = self
                .settings
                .active_profile_id
                .as_ref()
                .is_some_and(|id| missing.contains(id));
            if active_missing {
                if let Some(next) = self
                    .settings
                    .profiles
                    .iter()
                    .find(|profile| !missing.contains(&profile.id))
                    .map(|profile| profile.id.clone())
                {
                    self.select_profile(&next).await?;
                } else {
                    self.stop()?;
                }
            }
            let mut settings = self.settings.clone();
            settings
                .profiles
                .retain(|profile| !missing.contains(&profile.id));
            settings
                .subscriptions
                .retain(|item| !missing.contains(&item.profile_id));
            if settings.profiles.is_empty() {
                settings.active_profile_id = None;
                settings.config_name = None;
            }
            settings.save(&self.directory)?;
            self.settings = settings;
        }
        let mut settings = self.settings.clone();
        settings
            .profile_sources
            .retain(|name, _| directory.join(name).is_file());
        if settings.profile_sources != self.settings.profile_sources {
            settings.save(&self.directory)?;
            self.settings = settings;
        }
        monitor.observed.retain(|path, _| paths.contains(path));
        let mut errors = Vec::new();
        for path in paths {
            let bytes = match read_profile(&path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
            let fingerprint = config::profile_id(&bytes);
            if monitor.observed.get(&path) == Some(&fingerprint) {
                continue;
            }
            if let Err(error) = self.observe_profile_file(&path, &bytes).await {
                errors.push(error);
            }
            // 包含失败版本，只有用户再次编辑时才重试，避免每秒打断当前运行配置。
            monitor.observed.insert(path, fingerprint);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join(" "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn subscription_server(bodies: Vec<&'static str>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let task = std::thread::spawn(move || {
            for body in bodies {
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && std::time::Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(error) => panic!("测试订阅连接失败：{error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = [0; 4096];
                assert!(stream.read(&mut request).unwrap() > 0);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        (
            format!("http://{address}/profile?token=fixture-private"),
            task,
        )
    }

    #[tokio::test]
    async fn subscription_import_refresh_failure_and_schedule_keep_saved_profile_identity() {
        let (url, server) = subscription_server(vec![
            "proxies: []\nmode: rule",
            "proxies: []\nmode: global",
            "[invalid",
        ]);
        let directory = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(directory.path().to_owned()).unwrap();
        let added = engine
            .add_subscription(SubscriptionInput {
                url: url.clone(),
                name: Some("office.yaml".into()),
                auto_update_enabled: true,
                auto_update_interval_hours: 6,
                overwrite: false,
            })
            .await
            .unwrap();
        let id = added.active_profile_id.unwrap();
        let status = engine.refresh_subscription(&id).await.unwrap();
        assert_eq!(status.profiles.len(), 1);
        assert_eq!(status.active_profile_id.as_deref(), Some(id.as_str()));
        assert!(!serde_json::to_string(&status)
            .unwrap()
            .contains("fixture-private"));
        assert_eq!(engine.subscription_url(&id).unwrap(), url);
        let path = engine.profiles_directory().join(format!("{id}.yaml"));
        let valid = fs::read(&path).unwrap();
        assert!(engine.refresh_subscription(&id).await.is_err());
        assert_eq!(fs::read(&path).unwrap(), valid);
        assert!(engine.status().subscriptions[0].last_error.is_some());
        engine.save_subscription(&id, None, false, 12).unwrap();
        let reloaded = Settings::load(directory.path()).unwrap();
        assert!(!reloaded.subscriptions[0].auto_update_enabled);
        assert_eq!(reloaded.subscriptions[0].auto_update_interval_hours, 12);
        assert!(reloaded.subscriptions[0].next_update_at().is_none());
        server.join().unwrap();
    }

    #[test]
    fn original_default_template_is_seeded_once() {
        let directory = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(directory.path().to_owned()).unwrap();
        engine.seed_default_profile().unwrap();
        let initial = engine.status();
        assert_eq!(initial.config_name.as_deref(), Some("ClashBar.yaml"));
        assert_eq!(initial.profiles.len(), 1);
        engine.seed_default_profile().unwrap();
        assert_eq!(engine.status().active_profile_id, initial.active_profile_id);
    }

    #[tokio::test]
    async fn subscription_refresh_keeps_identity_and_rejects_invalid_content() {
        let directory = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(directory.path().to_owned()).unwrap();
        let before = b"proxies: []\nmode: rule\n";
        let status = engine.import_profile(before, "network.yaml").unwrap();
        let id = status.active_profile_id.unwrap();
        let after = b"proxies: []\nmode: global\n";
        assert!(engine.replace_profile_content(&id, after).await.unwrap());
        assert_eq!(
            engine.status().active_profile_id.as_deref(),
            Some(id.as_str())
        );
        assert_eq!(
            read_profile(&engine.profiles_directory().join(format!("{id}.yaml"))).unwrap(),
            after
        );
        assert!(!engine.replace_profile_content(&id, after).await.unwrap());
        assert!(engine
            .replace_profile_content(&id, b"[invalid")
            .await
            .is_err());
        assert_eq!(
            read_profile(&engine.profiles_directory().join(format!("{id}.yaml"))).unwrap(),
            after
        );
    }

    #[tokio::test]
    async fn directory_monitor_imports_external_yaml_once_and_preserves_invalid_edits() {
        let directory = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(directory.path().to_owned()).unwrap();
        let path = engine.profiles_directory().join("office.yaml");
        fs::write(&path, b"proxies: []\nmode: rule\n").unwrap();
        let mut monitor = ProfileMonitor::default();
        engine.monitor_profiles(&mut monitor).await.unwrap();
        engine.monitor_profiles(&mut monitor).await.unwrap();
        assert_eq!(engine.status().profiles.len(), 1);
        fs::write(&path, b"[invalid").unwrap();
        assert!(engine.monitor_profiles(&mut monitor).await.is_err());
        assert_eq!(engine.status().profiles.len(), 1);
        assert_eq!(fs::read(&path).unwrap(), b"[invalid");
    }

    #[tokio::test]
    async fn imported_file_provider_is_copied_without_modifying_source() {
        let source = tempfile::tempdir().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let bytes = b"proxy-providers: {local: {type: file, path: nodes.yaml}}\n";
        fs::write(source.path().join("main.yaml"), bytes).unwrap();
        fs::write(source.path().join("nodes.yaml"), b"proxies: []").unwrap();
        let mut engine = Engine::new(directory.path().to_owned()).unwrap();
        let status = engine
            .import_file(&source.path().join("main.yaml"))
            .await
            .unwrap();
        let id = status.active_profile_id.unwrap();
        let cache = directory
            .path()
            .join("runtime/providers")
            .join(id)
            .join("proxy-providers/0.yaml");
        assert_eq!(fs::read(cache).unwrap(), b"proxies: []");
        assert_eq!(fs::read(source.path().join("main.yaml")).unwrap(), bytes);
        assert!(engine
            .copy_local_providers(
                b"proxy-providers: {local: {type: file, path: ../outside.yaml}}",
                source.path()
            )
            .is_err());
    }

    #[tokio::test]
    async fn directory_monitor_reconciles_external_removal() {
        let directory = tempfile::tempdir().unwrap();
        let mut engine = Engine::new(directory.path().to_owned()).unwrap();
        let status = engine.import_profile(b"proxies: []", "first.yaml").unwrap();
        let path = engine
            .profiles_directory()
            .join(format!("{}.yaml", status.active_profile_id.unwrap()));
        let mut monitor = ProfileMonitor::default();
        engine.monitor_profiles(&mut monitor).await.unwrap();
        fs::rename(path, directory.path().join("removed.yaml")).unwrap();
        engine.monitor_profiles(&mut monitor).await.unwrap();
        let status = engine.status();
        assert!(status.profiles.is_empty());
        assert!(status.active_profile_id.is_none());
    }
}
