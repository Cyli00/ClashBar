use crate::process::LogEntry;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
const BACKUP_COUNT: usize = 5;

#[derive(Deserialize)]
struct StoredEntry {
    timestamp: u64,
    source: String,
    message: String,
}

pub struct LogArchive {
    path: PathBuf,
    seen: HashMap<String, usize>,
    limit: u64,
}

impl LogArchive {
    pub fn new(directory: &Path) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(|_| "无法创建日志目录。")?;
        Ok(Self {
            path: directory.join("clashbar.jsonl"),
            seen: HashMap::new(),
            limit: MAX_FILE_SIZE,
        })
    }

    pub fn load(&mut self) -> Result<Vec<LogEntry>, String> {
        let mut bytes = Vec::new();
        match fs::File::open(&self.path) {
            Ok(file) => {
                file.take(MAX_FILE_SIZE + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "无法读取应用日志。")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(_) => return Err("无法读取应用日志。".into()),
        }
        if bytes.len() > MAX_FILE_SIZE as usize {
            return Err("应用日志超过 10 MiB，未加载。".into());
        }
        let mut entries: Vec<_> = bytes
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice::<StoredEntry>(line).ok())
            .filter(|entry| entry.source == "ClashBar")
            .map(|entry| LogEntry {
                timestamp: entry.timestamp,
                source: if entry.source == "Mihomo" {
                    "Mihomo"
                } else {
                    "ClashBar"
                },
                message: sanitize_message(&entry.message),
            })
            .collect();
        if entries.len() > 500 {
            entries.drain(..entries.len() - 500);
        }
        self.seen = entry_counts(&entries);
        Ok(entries)
    }

    pub fn save(&mut self, entries: &[LogEntry]) -> Result<(), String> {
        let mut counts = HashMap::new();
        let mut bytes = Vec::new();
        for entry in entries {
            if entry.source != "ClashBar" {
                continue;
            }
            let key = entry_key(entry);
            let count = counts.entry(key.clone()).or_insert(0usize);
            *count += 1;
            if *count <= self.seen.get(&key).copied().unwrap_or_default() {
                continue;
            }
            let safe = LogEntry {
                timestamp: entry.timestamp,
                source: entry.source,
                message: sanitize_message(&entry.message),
            };
            serde_json::to_writer(&mut bytes, &safe).map_err(|_| "无法编码应用日志。")?;
            bytes.push(b'\n');
        }
        if !bytes.is_empty() {
            let length = fs::metadata(&self.path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            if length > 0 && length.saturating_add(bytes.len() as u64) > self.limit {
                self.rotate()?;
            }
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .map_err(|_| "无法写入应用日志。")?;
            file.write_all(&bytes).map_err(|_| "无法写入应用日志。")?;
            file.sync_data().map_err(|_| "无法保存应用日志。")?;
        }
        self.seen = counts;
        Ok(())
    }

    pub fn clear(&mut self) -> Result<(), String> {
        crate::config::atomic_write(&self.path, &[]).map_err(|_| "无法清空应用日志。")?;
        self.seen.clear();
        Ok(())
    }

    fn rotate(&self) -> Result<(), String> {
        for index in (1..=BACKUP_COUNT).rev() {
            let source = if index == 1 {
                self.path.clone()
            } else {
                self.backup(index - 1)
            };
            let destination = self.backup(index);
            if !source.exists() {
                continue;
            }
            if destination.exists() {
                fs::remove_file(&destination).map_err(|_| "无法轮换应用日志。")?;
            }
            fs::rename(source, destination).map_err(|_| "无法轮换应用日志。")?;
        }
        Ok(())
    }

    fn backup(&self, index: usize) -> PathBuf {
        self.path.with_extension(format!("jsonl.{index}"))
    }
}

fn entry_key(entry: &LogEntry) -> String {
    format!("{}:{}:{}", entry.timestamp, entry.source, entry.message)
}
fn entry_counts(entries: &[LogEntry]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for entry in entries {
        *counts.entry(entry_key(entry)).or_insert(0) += 1;
    }
    counts
}

pub fn sanitize_message(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
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
        "[已隐藏可能包含凭据的日志]".into()
    } else {
        message.chars().take(4096).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(timestamp: u64, message: &str) -> LogEntry {
        LogEntry {
            timestamp,
            source: "ClashBar",
            message: message.into(),
        }
    }

    #[test]
    fn archive_preserves_timestamps_deduplicates_ticks_and_redacts_sensitive_records() {
        let dir = tempfile::tempdir().unwrap();
        let mut archive = LogArchive::new(dir.path()).unwrap();
        let entries = vec![
            entry(1, "启动成功"),
            entry(1, "启动成功"),
            entry(2, "https://host?token=private"),
        ];
        archive.save(&entries).unwrap();
        archive.save(&entries).unwrap();
        let bytes = fs::read_to_string(&archive.path).unwrap();
        assert_eq!(bytes.lines().count(), 3);
        assert!(!bytes.contains("private"));
        let mut restored = LogArchive::new(dir.path()).unwrap();
        let loaded = restored.load().unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].timestamp, 1);
        restored.save(&loaded).unwrap();
        assert_eq!(
            fs::read_to_string(&archive.path).unwrap().lines().count(),
            3
        );
        restored.clear().unwrap();
        assert!(restored.load().unwrap().is_empty());
    }

    #[test]
    fn archive_rotates_at_limit_and_keeps_five_backups() {
        let dir = tempfile::tempdir().unwrap();
        let mut archive = LogArchive::new(dir.path()).unwrap();
        archive.limit = 1;
        for timestamp in 0..8 {
            archive.save(&[entry(timestamp, "record")]).unwrap();
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 6);
        let newest: StoredEntry =
            serde_json::from_str(fs::read_to_string(archive.backup(1)).unwrap().trim()).unwrap();
        assert_eq!(newest.timestamp, 6);
    }
}
