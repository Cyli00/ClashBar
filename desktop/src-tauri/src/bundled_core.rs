use flate2::read::MultiGzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_SOURCE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;
const CORE_NAMES: [&str; 3] = ["mihomo.exe", "mihomo.exe.gz", "mihomo.gz"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    file_name: String,
    sha256: String,
    executable_sha256: String,
    executable_size: u64,
}

pub fn install(
    resource_directory: &Path,
    data_directory: &Path,
) -> Result<Option<PathBuf>, String> {
    let managed = data_directory.join("core").join("mihomo.exe");
    if managed.try_exists().map_err(|_| "无法检查已安装内核。")? {
        validate_executable(&read_file(&managed, MAX_EXECUTABLE_BYTES)?)?;
        return managed
            .canonicalize()
            .map(Some)
            .map_err(|_| "无法读取已安装内核路径。".into());
    }
    let resource_bin = resource_directory.join("bin");
    let manifest_path = resource_bin.join("core.json");
    let manifest = if manifest_path
        .try_exists()
        .map_err(|_| "无法检查随包内核清单。")?
    {
        let manifest: Manifest = serde_json::from_slice(&read_file(&manifest_path, 4096)?)
            .map_err(|_| "随包内核清单格式无效。")?;
        if !CORE_NAMES.contains(&manifest.file_name.as_str())
            || !valid_hash(&manifest.sha256)
            || !valid_hash(&manifest.executable_sha256)
            || manifest.executable_size > MAX_EXECUTABLE_BYTES
        {
            return Err("随包内核清单内容无效。".into());
        }
        Some(manifest)
    } else {
        None
    };
    let source = if let Some(manifest) = &manifest {
        resource_bin.join(&manifest.file_name)
    } else {
        let Some(path) = CORE_NAMES
            .iter()
            .map(|name| resource_bin.join(name))
            .find(|path| path.exists())
        else {
            return Ok(None);
        };
        path
    };
    let input = read_file(&source, MAX_SOURCE_BYTES)?;
    if manifest
        .as_ref()
        .is_some_and(|manifest| digest(&input) != manifest.sha256.to_ascii_lowercase())
    {
        return Err("随包内核校验和不匹配。".into());
    }
    let bytes = if source
        .extension()
        .is_some_and(|extension| extension == "gz")
    {
        decompress(&input, MAX_EXECUTABLE_BYTES)?
    } else {
        input
    };
    validate_executable(&bytes)?;
    if manifest.as_ref().is_some_and(|manifest| {
        bytes.len() as u64 != manifest.executable_size
            || digest(&bytes) != manifest.executable_sha256.to_ascii_lowercase()
    }) {
        return Err("随包内核解压结果校验失败。".into());
    }
    let directory = managed.parent().ok_or("内核安装目录无效。")?;
    fs::create_dir_all(directory).map_err(|_| "无法创建内核安装目录。")?;
    let staging = directory.join(format!(".mihomo-{}.tmp", crate::config::secret()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staging)
            .map_err(|_| "无法暂存随包内核。")?;
        file.write_all(&bytes).map_err(|_| "无法写入随包内核。")?;
        file.sync_all().map_err(|_| "无法保存随包内核。")?;
        drop(file);
        // 硬链接发布同卷暂存文件：目标已存在时原子失败，不会覆盖用户新放入的内核。
        match fs::hard_link(&staging, &managed) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                validate_executable(&read_file(&managed, MAX_EXECUTABLE_BYTES)?)?;
            }
            Err(_) => return Err("无法安装随包内核，已有文件未被覆盖。".to_owned()),
        }
        managed
            .canonicalize()
            .map(Some)
            .map_err(|_| "无法读取已安装内核路径。".to_owned())
    })();
    let _ = fs::remove_file(&staging);
    result
}

fn read_file(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "无法读取随包或已安装内核文件。")?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err("内核必须是大小符合限制的普通文件。".into());
    }
    let file = File::open(path).map_err(|_| "无法打开内核文件。")?;
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取内核文件。")?;
    if bytes.len() as u64 > maximum {
        return Err("内核文件超过大小限制。".into());
    }
    Ok(bytes)
}

fn decompress(input: &[u8], maximum: u64) -> Result<Vec<u8>, String> {
    let mut decoder = MultiGzDecoder::new(input);
    let mut bytes = Vec::new();
    (&mut decoder)
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "随包内核 gzip 损坏或不完整。")?;
    if bytes.len() as u64 > maximum {
        return Err("随包内核解压后超过大小限制。".into());
    }
    Ok(bytes)
}

pub fn validate_executable(bytes: &[u8]) -> Result<(), String> {
    let invalid = || "随包内核不是有效的 Windows x64 PE 可执行文件。".to_owned();
    if bytes.len() < 64 || &bytes[..2] != b"MZ" {
        return Err(invalid());
    }
    let offset = u32::from_le_bytes(bytes[60..64].try_into().expect("checked header")) as usize;
    let header = bytes
        .get(offset..offset.saturating_add(26))
        .ok_or_else(invalid)?;
    if &header[..4] != b"PE\0\0"
        || u16::from_le_bytes([header[4], header[5]]) != 0x8664
        || u16::from_le_bytes([header[22], header[23]]) & 2 == 0
        || u16::from_le_bytes([header[24], header[25]]) != 0x20b
    {
        return Err(invalid());
    }
    Ok(())
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};

    pub(crate) fn executable() -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
        bytes[86..88].copy_from_slice(&2u16.to_le_bytes());
        bytes[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        bytes
    }
    fn compress(bytes: &[u8]) -> Vec<u8> {
        let mut writer = GzEncoder::new(vec![], Compression::default());
        writer.write_all(bytes).unwrap();
        writer.finish().unwrap()
    }

    #[test]
    fn empty_bundle_is_optional_and_plain_or_gzip_install_without_execution() {
        for name in CORE_NAMES {
            let resource = tempfile::tempdir().unwrap();
            let data = tempfile::tempdir().unwrap();
            assert!(install(resource.path(), data.path()).unwrap().is_none());
            fs::create_dir(resource.path().join("bin")).unwrap();
            let bytes = executable();
            fs::write(
                resource.path().join("bin").join(name),
                if name.ends_with("gz") {
                    compress(&bytes)
                } else {
                    bytes.clone()
                },
            )
            .unwrap();
            let installed = install(resource.path(), data.path()).unwrap().unwrap();
            assert_eq!(fs::read(&installed).unwrap(), bytes);
            assert_eq!(installed.file_name().unwrap(), "mihomo.exe");
            assert_eq!(
                fs::read_dir(installed.parent().unwrap()).unwrap().count(),
                1
            );
        }
    }

    #[test]
    fn existing_managed_core_is_preserved_when_bundle_changes() {
        let resource = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        fs::create_dir(resource.path().join("bin")).unwrap();
        fs::create_dir(data.path().join("core")).unwrap();
        let mut bytes = executable();
        bytes[127] = 7;
        fs::write(data.path().join("core/mihomo.exe"), &bytes).unwrap();
        fs::write(
            resource.path().join("bin/mihomo.exe"),
            b"damaged replacement",
        )
        .unwrap();
        let selected = install(resource.path(), data.path()).unwrap().unwrap();
        assert_eq!(fs::read(selected).unwrap(), bytes);
    }

    #[test]
    fn invalid_pe_corrupt_gzip_and_decompression_bombs_are_rejected() {
        assert!(validate_executable(b"not exe").is_err());
        let mut wrong_arch = executable();
        wrong_arch[68] = 0x4c;
        wrong_arch[69] = 1;
        assert!(validate_executable(&wrong_arch).is_err());
        let mut outside = executable();
        outside[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(validate_executable(&outside).is_err());
        assert!(decompress(&compress(&[0; 4096]), 100).is_err());
        let mut truncated = compress(&executable());
        truncated.truncate(truncated.len() - 4);
        assert!(decompress(&truncated, 1024).is_err());
    }

    #[test]
    fn manifest_checksum_is_verified_before_installing() {
        let resource = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        fs::create_dir(resource.path().join("bin")).unwrap();
        let bytes = executable();
        fs::write(resource.path().join("bin/mihomo.exe"), &bytes).unwrap();
        let mut manifest = serde_json::json!({"fileName":"mihomo.exe","sha256":digest(&bytes),"executableSha256":digest(&bytes),"executableSize":bytes.len()});
        manifest["sha256"] = serde_json::json!("0".repeat(64));
        fs::write(
            resource.path().join("bin/core.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(install(resource.path(), data.path())
            .unwrap_err()
            .contains("校验和"));
        assert!(!data.path().join("core/mihomo.exe").exists());
        manifest["sha256"] = serde_json::json!(digest(&bytes));
        fs::write(
            resource.path().join("bin/core.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(install(resource.path(), data.path()).unwrap().is_some());
    }
}
