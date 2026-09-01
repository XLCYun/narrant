use super::{
    InstallError, InstallIntent, InstallKind, InstallOutcome, UpdateInstaller,
    VerifiedStagedPackage,
};
use std::path::Path;

#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
pub(crate) const MACOS_TARGET: &str = "darwin-aarch64";
#[cfg(any(all(target_os = "windows", target_arch = "x86_64"), test))]
pub(crate) const WINDOWS_TARGET: &str = "windows-x86_64";

/// 返回当前构建唯一支持的 Stable target 与持久化安装类型。
pub(crate) fn current_install_plan() -> Result<(&'static str, InstallKind), InstallError> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Ok((MACOS_TARGET, InstallKind::MacosAppArchive));
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Ok((WINDOWS_TARGET, InstallKind::WindowsNsis));
    #[allow(unreachable_code)]
    Err(InstallError::new("当前平台不支持应用内更新"))
}

/// 核对持久化 target/install kind 与当前进程唯一安装计划完全一致。
fn validate_install_plan(
    expected_target: &str,
    expected_kind: InstallKind,
    actual_target: &str,
    actual_kind: InstallKind,
) -> Result<(), InstallError> {
    if expected_target == actual_target && expected_kind == actual_kind {
        Ok(())
    } else {
        Err(InstallError::new("应用更新包与当前平台不匹配"))
    }
}

/// 直接消费仓储复验句柄的平台安装器，不接受 caller 路径或安装类型。
pub(crate) struct PlatformUpdateInstaller {
    #[cfg(target_os = "windows")]
    app_name: String,
}

impl PlatformUpdateInstaller {
    /// 从 Tauri 包元数据创建当前平台安装器。
    pub(crate) fn new<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Self {
        #[cfg(not(target_os = "windows"))]
        let _ = app;
        Self {
            #[cfg(target_os = "windows")]
            app_name: app.package_info().name.clone(),
        }
    }
}

impl UpdateInstaller for PlatformUpdateInstaller {
    /// 严格核对 target/install kind 后执行原路径替换或 current-user NSIS handoff。
    fn install(
        &self,
        package: &VerifiedStagedPackage,
        _intent: InstallIntent,
    ) -> Result<InstallOutcome, InstallError> {
        let (target, install_kind) = current_install_plan()?;
        validate_install_plan(
            target,
            install_kind,
            package.target(),
            package.install_kind(),
        )?;
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            install_macos_archive(package.path())?;
            return Ok(InstallOutcome::Installed);
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            launch_windows_nsis(
                package.path(),
                &self.app_name,
                _intent == InstallIntent::UpdateRestart,
            )?;
            return Ok(InstallOutcome::HandedOff);
        }
        #[allow(unreachable_code)]
        Err(InstallError::new("当前平台不支持应用内更新"))
    }
}

/// 把候选目录替换到原路径，并在替换或后处理失败时恢复旧目录。
fn replace_with_rollback(
    rename: &mut impl RenameBoundary,
    destination: &Path,
    candidate: &Path,
    backup: &Path,
    failed_candidate: &Path,
    post_replace: impl FnOnce() -> std::io::Result<()>,
) -> Result<(), ReplaceTransactionError> {
    rename
        .rename(destination, backup)
        .map_err(|_| ReplaceTransactionError::OriginalMoveFailed)?;
    if rename.rename(candidate, destination).is_err() {
        return match rename.rename(backup, destination) {
            Ok(()) => Err(ReplaceTransactionError::RolledBack),
            Err(_) => Err(ReplaceTransactionError::RollbackFailed),
        };
    }
    if post_replace().is_err() {
        if rename.rename(destination, failed_candidate).is_err() {
            return Err(ReplaceTransactionError::RollbackFailed);
        }
        return match rename.rename(backup, destination) {
            Ok(()) => Err(ReplaceTransactionError::RolledBack),
            Err(_) => Err(ReplaceTransactionError::RollbackFailed),
        };
    }
    Ok(())
}

/// 文件/目录移动的可测试系统边界。
trait RenameBoundary {
    /// 把 source 原子移动到 destination。
    fn rename(&mut self, source: &Path, destination: &Path) -> std::io::Result<()>;
}

/// 生产环境使用同文件系统 rename 保持替换原子性。
struct FileSystemRename;

impl RenameBoundary for FileSystemRename {
    /// 委托标准库执行原子移动。
    fn rename(&mut self, source: &Path, destination: &Path) -> std::io::Result<()> {
        std::fs::rename(source, destination)
    }
}

/// 描述原路径替换失败及旧版本能否恢复。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplaceTransactionError {
    OriginalMoveFailed,
    RolledBack,
    RollbackFailed,
}

/// 从签名后的 `.app.tar.gz` 解包唯一应用，再执行 rollback-safe 原路径替换。
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_macos_archive(package: &Path) -> Result<(), InstallError> {
    use std::fs;

    let executable = std::env::current_exe().map_err(|_| InstallError::new("无法定位当前应用"))?;
    let destination = tauri_plugin_updater::extract_path_from_executable(&executable)
        .map_err(|_| InstallError::new("无法定位当前应用"))?;
    let parent = destination
        .parent()
        .ok_or_else(|| InstallError::new("无法定位当前应用"))?;
    let work = tempfile::Builder::new()
        .prefix("narrant-update-")
        .tempdir_in(parent)
        .map_err(|_| InstallError::new("无法创建更新临时目录"))?;
    let extracted_root = work.path().join("extracted");
    fs::create_dir(&extracted_root).map_err(|_| InstallError::new("无法创建更新临时目录"))?;
    let candidate = extract_macos_archive(package, &extracted_root)?;
    let backup = work.path().join("previous.app");
    let failed_candidate = work.path().join("failed-candidate.app");
    let result = replace_with_rollback(
        &mut FileSystemRename,
        &destination,
        &candidate,
        &backup,
        &failed_candidate,
        || {
            std::process::Command::new("/usr/bin/touch")
                .arg(&destination)
                .status()
                .and_then(|status| {
                    if status.success() {
                        Ok(())
                    } else {
                        Err(std::io::Error::other("touch failed"))
                    }
                })
        },
    );
    match result {
        Ok(()) => Ok(()),
        Err(ReplaceTransactionError::RollbackFailed) => {
            let _preserved = work.keep();
            Err(InstallError::new("应用更新回滚失败，已保留恢复副本"))
        }
        Err(_) => Err(InstallError::new("应用更新替换失败，已恢复旧版本")),
    }
}

/// 逐 entry 验证并解包唯一、无链接且不能逃逸的 `.app` 目录树。
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
fn extract_macos_archive(
    package: &Path,
    output: &Path,
) -> Result<std::path::PathBuf, InstallError> {
    use flate2::read::GzDecoder;
    use std::fs::{self, File};
    use std::path::Component;

    let file = File::open(package).map_err(|_| InstallError::new("无法读取更新包"))?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let mut root_name = None::<std::ffi::OsString>;
    let mut seen = std::collections::HashSet::new();
    let entries = archive
        .entries()
        .map_err(|_| InstallError::new("应用更新归档无效"))?;
    for entry in entries {
        let mut entry = entry.map_err(|_| InstallError::new("应用更新归档无效"))?;
        let path = entry
            .path()
            .map_err(|_| InstallError::new("应用更新归档无效"))?;
        if path.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        }) {
            return Err(InstallError::new("应用更新归档路径无效"));
        }
        if !seen.insert(path.to_path_buf()) {
            return Err(InstallError::new("应用更新归档包含重复路径"));
        }
        let first = path
            .components()
            .next()
            .and_then(|component| match component {
                Component::Normal(value) => Some(value.to_owned()),
                _ => None,
            })
            .ok_or_else(|| InstallError::new("应用更新归档无效"))?;
        if std::path::Path::new(&first)
            .extension()
            .is_none_or(|ext| ext != "app")
            || root_name.as_ref().is_some_and(|root| root != &first)
        {
            return Err(InstallError::new("应用更新归档必须只包含一个应用"));
        }
        root_name.get_or_insert(first);
        let entry_type = entry.header().entry_type();
        if !(entry_type.is_dir() || entry_type.is_file()) {
            return Err(InstallError::new("应用更新归档不允许链接或特殊文件"));
        }
        let destination = output.join(path.as_ref());
        if entry_type.is_dir() {
            fs::create_dir_all(&destination).map_err(|_| InstallError::new("无法解包应用更新"))?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|_| InstallError::new("无法解包应用更新"))?;
            }
            entry
                .unpack(&destination)
                .map_err(|_| InstallError::new("无法解包应用更新"))?;
        }
    }
    let root = output.join(root_name.ok_or_else(|| InstallError::new("应用更新归档为空"))?);
    let metadata =
        fs::symlink_metadata(&root).map_err(|_| InstallError::new("应用更新归档缺少应用目录"))?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(InstallError::new("应用更新根目录无效"));
    }
    Ok(root)
}

/// 为 current-user NSIS 区分正常退出与显式重启安装参数。
#[cfg(any(target_os = "windows", test))]
fn windows_nsis_args(relaunch: bool) -> &'static [&'static str] {
    if relaunch {
        &["/P", "/UPDATE", "/R"]
    } else {
        &["/P", "/UPDATE"]
    }
}

/// Windows 子进程启动窄边界，测试可验证复制、argv 与一次 handoff。
#[cfg(any(target_os = "windows", test))]
trait CommandLauncher {
    /// 启动已经复制到独立目录的 installer。
    fn launch(&self, executable: &Path, args: &[&str]) -> std::io::Result<()>;
}

/// 生产环境使用结构化 Command argv，不经过 shell。
#[cfg(target_os = "windows")]
struct SystemCommandLauncher;

#[cfg(target_os = "windows")]
impl CommandLauncher for SystemCommandLauncher {
    /// 启动 NSIS 进程并立即完成 handoff。
    fn launch(&self, executable: &Path, args: &[&str]) -> std::io::Result<()> {
        std::process::Command::new(executable)
            .args(args)
            .spawn()
            .map(|_| ())
    }
}

/// 校验 DOS/PE signature 与 AMD64 machine，拒绝仅伪造 `MZ` 的文件。
#[cfg(any(target_os = "windows", test))]
fn validate_windows_pe_x64(package: &Path) -> Result<(), InstallError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut input =
        std::fs::File::open(package).map_err(|_| InstallError::new("无法读取应用更新包"))?;
    let length = input.metadata().map(|m| m.len()).unwrap_or(0);
    let mut dos = [0_u8; 64];
    input
        .read_exact(&mut dos)
        .map_err(|_| InstallError::new("应用更新包无效"))?;
    if &dos[..2] != b"MZ" {
        return Err(InstallError::new("应用更新包不是 PE 可执行文件"));
    }
    let pe_offset = u32::from_le_bytes(dos[0x3c..0x40].try_into().expect("fixed slice")) as u64;
    if pe_offset < 64 || pe_offset.saturating_add(6) > length || pe_offset > 16 * 1024 * 1024 {
        return Err(InstallError::new("应用更新包 PE header 无效"));
    }
    input
        .seek(SeekFrom::Start(pe_offset))
        .map_err(|_| InstallError::new("应用更新包无效"))?;
    let mut header = [0_u8; 6];
    input
        .read_exact(&mut header)
        .map_err(|_| InstallError::new("应用更新包无效"))?;
    if &header[..4] != b"PE\0\0" || u16::from_le_bytes([header[4], header[5]]) != 0x8664 {
        return Err(InstallError::new("应用更新包不是 Windows x64 PE"));
    }
    Ok(())
}

/// 复制并启动已签名、已复验且通过 PE x64 合约的 installer。
#[cfg(any(target_os = "windows", test))]
fn prepare_and_launch_windows_nsis(
    package: &Path,
    app_name: &str,
    relaunch: bool,
    launcher: &impl CommandLauncher,
) -> Result<std::path::PathBuf, InstallError> {
    validate_windows_pe_x64(package)?;
    let directory = tempfile::Builder::new()
        .prefix(&format!("{app_name}-update-"))
        .tempdir()
        .map_err(|_| InstallError::new("无法创建更新临时目录"))?
        .keep();
    let installer = directory.join("installer.exe");
    std::fs::copy(package, &installer).map_err(|_| InstallError::new("无法准备应用安装器"))?;
    launcher
        .launch(&installer, windows_nsis_args(relaunch))
        .map_err(|_| InstallError::new("无法启动应用安装器"))?;
    Ok(installer)
}

/// 复制已复验 EXE 到独立临时目录并以结构化 argv 启动 NSIS installer。
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn launch_windows_nsis(package: &Path, app_name: &str, relaunch: bool) -> Result<(), InstallError> {
    prepare_and_launch_windows_nsis(package, app_name, relaunch, &SystemCommandLauncher).map(|_| ())
}

#[cfg(test)]
#[path = "tests/platform.rs"]
mod tests;
