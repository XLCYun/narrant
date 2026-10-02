use super::model::*;
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

pub(super) const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
const MAX_UNPACKED: u64 = 1024 * 1024 * 1024;

/// 应用私有存储：每个版本有跨进程锁，暂存和正式目录位于同一文件系统。
#[derive(Clone)]
pub(super) struct Storage {
    pub root: PathBuf,
    pub target: String,
}

impl Storage {
    /// 定位已安装版本，调用方只传入经过校验的版本和固定包标识。
    pub fn installed(&self, key: &PackKey) -> PathBuf {
        self.root
            .join("installed")
            .join(key.id.slug())
            .join(&key.version)
            .join(&self.target)
    }

    /// 每个包版本独占一个暂存父目录，方便在锁保护下回收崩溃残片。
    pub fn staging(&self, key: &PackKey) -> PathBuf {
        self.root
            .join("staging")
            .join(key.id.slug())
            .join(&key.version)
            .join(&self.target)
    }

    /// 锁文件常驻独立目录，安装、分析使用和删除均经同一锁协调。
    pub fn lock_file(&self, key: &PackKey) -> Result<File, String> {
        let locks = self.root.join("locks");
        fs::create_dir_all(&locks).map_err(error)?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(locks.join(format!(
                "{}-{}-{}.lock",
                key.id.slug(),
                key.version,
                self.target
            )))
            .map_err(error)
    }

    /// 启动只扫描应用拥有的固定层级，不触碰正在安装或使用的版本。
    pub fn recover(&self) -> Result<(), String> {
        for key in self.keys("staging")? {
            let lock = self.lock_file(&key)?;
            if lock.try_lock_exclusive().is_ok() {
                fs::remove_dir_all(self.staging(&key)).map_err(error)?;
            }
        }
        Ok(())
    }

    /// 发现当前平台的历史版本，拒绝链接目录及任意目录名。
    pub fn keys(&self, area: &str) -> Result<Vec<PackKey>, String> {
        let mut keys = vec![];
        for id in [
            PackId::Rust,
            PackId::TypeScriptJavascript,
            PackId::Python,
            PackId::Java,
            PackId::Go,
            PackId::Csharp,
            PackId::CCpp,
        ] {
            let base = self.root.join(area).join(id.slug());
            if !base.exists() {
                continue;
            }
            if fs::symlink_metadata(&base)
                .map_err(error)?
                .file_type()
                .is_symlink()
            {
                return Err("语言支持包目录不能是链接".into());
            }
            for entry in fs::read_dir(base).map_err(error)? {
                let entry = entry.map_err(error)?;
                let version = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type().map_err(error)?.is_dir() && valid_version(&version) {
                    let target = entry.path().join(&self.target);
                    if fs::symlink_metadata(target).is_ok_and(|m| m.file_type().is_dir()) {
                        keys.push(PackKey { id, version });
                    }
                }
            }
        }
        Ok(keys)
    }

    /// 已安装标记只在提交后出现；缺文件、链接和尺寸不匹配都会使该版本不可用。
    pub fn inspect(&self, key: &PackKey) -> Result<Manifest, String> {
        let root = self.installed(key);
        let bytes = read_limited(&root.join("installed.json"), 2 * 1024 * 1024)?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(error)?;
        validate_manifest(&manifest, key, &self.target)?;
        for file in &manifest.files {
            let metadata = fs::symlink_metadata(root.join(&file.path)).map_err(error)?;
            if !metadata.file_type().is_file() || metadata.len() != file.size {
                return Err("已安装语言支持包文件损坏，请重试安装".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if file.executable && metadata.permissions().mode() & 0o111 == 0 {
                    return Err("语言支持包工具不可执行，请重试安装".into());
                }
            }
            let canonical = root.join(&file.path).canonicalize().map_err(error)?;
            if !canonical.starts_with(root.canonicalize().map_err(error)?) {
                return Err("语言支持包文件路径越界".into());
            }
        }
        Ok(manifest)
    }

    /// 校验过的归档只解出清单中的普通文件，完整后整体提交，不覆盖在用目录。
    pub fn unpack(
        &self,
        key: &PackKey,
        manifest: &Manifest,
        archive: &Path,
        temporary: &Path,
        stop: &CancellationToken,
    ) -> Result<u64, String> {
        let payload = temporary.join("payload");
        fs::create_dir(&payload).map_err(error)?;
        let expected: BTreeMap<_, _> = manifest
            .files
            .iter()
            .map(|f| (f.path.as_str(), f))
            .collect();
        let mut seen = BTreeSet::new();
        let decoder = flate2::read::GzDecoder::new(File::open(archive).map_err(error)?)
            .take(MAX_UNPACKED + 32 * 1024 * 1024);
        let mut tar = tar::Archive::new(decoder);
        for entry in tar.entries().map_err(error)? {
            if stop.is_cancelled() {
                return Err("应用正在退出，语言支持包准备已停止".into());
            }
            let mut entry = entry.map_err(error)?;
            let entry_path = entry.path().map_err(error)?.into_owned();
            let relative = entry_path.strip_prefix(".").unwrap_or(&entry_path);
            if relative.as_os_str().is_empty() && entry.header().entry_type().is_dir() {
                continue;
            }
            let name = relative.to_str().ok_or("归档路径不是 UTF-8")?;
            if !safe_path(name) {
                return Err(format!(
                    "语言支持包归档路径不安全：{}",
                    name.chars().take(200).collect::<String>()
                ));
            }
            if entry.header().entry_type().is_dir() {
                continue;
            }
            if !entry.header().entry_type().is_file() || !seen.insert(name.to_owned()) {
                return Err("语言支持包含链接、特殊文件或重复路径".into());
            }
            let file = expected.get(name).ok_or("归档包含清单之外的文件")?;
            if entry.size() != file.size {
                return Err("归档文件大小不匹配".into());
            }
            let path = payload.join(relative);
            fs::create_dir_all(path.parent().ok_or("归档路径无效")?).map_err(error)?;
            let mut output = File::create(&path).map_err(error)?;
            let mut digest = Sha256::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                if stop.is_cancelled() {
                    return Err("应用正在退出，语言支持包准备已停止".into());
                }
                let count = entry.read(&mut buffer).map_err(error)?;
                if count == 0 {
                    break;
                }
                output.write_all(&buffer[..count]).map_err(error)?;
                digest.update(&buffer[..count]);
            }
            if format!("{:x}", digest.finalize()) != file.sha256 {
                return Err("语言支持包文件摘要不匹配".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                output
                    .set_permissions(fs::Permissions::from_mode(if file.executable {
                        0o755
                    } else {
                        0o644
                    }))
                    .map_err(error)?;
            }
            output.sync_all().map_err(error)?;
        }
        if seen.len() != expected.len() {
            return Err("语言支持包归档缺少文件".into());
        }
        let mut marker = File::create(payload.join("installed.json")).map_err(error)?;
        marker
            .write_all(&serde_json::to_vec(manifest).map_err(error)?)
            .map_err(error)?;
        marker.sync_all().map_err(error)?;
        drop(marker);
        let destination = self.installed(key);
        fs::create_dir_all(destination.parent().ok_or("安装路径无效")?).map_err(error)?;
        if destination.exists() {
            fs::remove_dir_all(&destination).map_err(error)?;
        }
        fs::rename(payload, destination).map_err(error)?;
        Ok(manifest.files.iter().map(|f| f.size).sum())
    }
}

/// 清单受固定包身份和资源上限约束，不能指定下载域名或安装位置。
pub(super) fn validate_manifest(
    manifest: &Manifest,
    key: &PackKey,
    target: &str,
) -> Result<(), String> {
    let mut names = BTreeSet::new();
    let mut folded_names = BTreeSet::new();
    let mut size = 0u64;
    if manifest.schema_version != 1
        || manifest.id != key.id
        || manifest.version != key.version
        || manifest.target != target
        || manifest.archive_size == 0
        || manifest.archive_size > MAX_ARCHIVE
        || !valid_hash(&manifest.archive_sha256)
        || manifest.files.is_empty()
        || manifest.files.len() > 20_000
    {
        return Err("语言支持包清单身份或大小无效".into());
    }
    for file in &manifest.files {
        size = size.checked_add(file.size).ok_or("语言支持包大小越界")?;
        if !safe_path(&file.path)
            || !["bin/", "runtime/", "licenses/", "control/"]
                .iter()
                .any(|p| file.path.starts_with(p))
            || !valid_hash(&file.sha256)
            || !names.insert(file.path.as_str())
            || !folded_names.insert(file.path.to_lowercase())
            || size > MAX_UNPACKED
        {
            return Err("语言支持包文件清单无效".into());
        }
    }
    if key
        .id
        .required(target)
        .iter()
        .any(|path| !names.contains(path))
    {
        return Err("语言支持包缺少必需入口".into());
    }
    if manifest.files.iter().any(|file| {
        (file.path.starts_with("bin/")
            || (key.id == PackId::Java
                && (file.path == "runtime/jdk/bin/java"
                    || file.path == "runtime/jdk/bin/java.exe")))
            && !file.executable
    }) || (key.id == PackId::Python
        && manifest
            .files
            .iter()
            .any(|file| file.path == "control/ty.toml" && (file.size != 0 || file.executable)))
    {
        return Err("语言支持包工具权限或固定控制文件无效".into());
    }
    Ok(())
}

/// 统一拒绝 Windows 与 Unix 上可产生路径穿越或设备文件的名字。
fn safe_path(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['\\', ':'])
        && !name.chars().any(char::is_control)
        && Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && name.split('/').all(|p| {
            !p.is_empty()
                && !p.ends_with(['.', ' '])
                && !matches!(
                    p.split('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase()
                        .as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
}

/// SHA-256 清单只接受规范的小写十六进制。
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 限制读取元数据大小，防止损坏文件造成无界分配。
fn read_limited(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let mut bytes = vec![];
    File::open(path)
        .map_err(error)?
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() as u64 > max {
        return Err("语言支持包清单过大".into());
    }
    Ok(bytes)
}

/// I/O 诊断保留原因，供界面提供明确重试信息。
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
