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
    post_replace: impl FnOnce() -> std::io::Result<()>,
) -> Result<(), ReplaceTransactionError> {
    rename
        .rename(destination, backup)
        .map_err(|_| ReplaceTransactionError::OriginalMoveFailed)?;
    if rename.rename(candidate, destination).is_err() || post_replace().is_err() {
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
    use flate2::read::GzDecoder;
    use std::fs::{self, File};

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
    let file = File::open(package).map_err(|_| InstallError::new("无法读取更新包"))?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    archive
        .unpack(&extracted_root)
        .map_err(|_| InstallError::new("应用更新归档无效"))?;
    let mut apps = fs::read_dir(&extracted_root)
        .map_err(|_| InstallError::new("应用更新归档无效"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "app"));
    let candidate = apps
        .next()
        .map(|entry| entry.path())
        .ok_or_else(|| InstallError::new("应用更新归档无效"))?;
    if apps.next().is_some() || !candidate.is_dir() {
        return Err(InstallError::new("应用更新归档无效"));
    }
    let backup = work.path().join("previous.app");
    let result = replace_with_rollback(
        &mut FileSystemRename,
        &destination,
        &candidate,
        &backup,
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

/// 为 current-user NSIS 区分正常退出与显式重启安装参数。
#[cfg(any(target_os = "windows", test))]
fn windows_nsis_args(relaunch: bool) -> &'static [&'static str] {
    if relaunch {
        &["/P", "/UPDATE", "/R"]
    } else {
        &["/P", "/UPDATE"]
    }
}

/// 复制已复验 EXE 到独立临时目录并以结构化 argv 启动 NSIS installer。
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn launch_windows_nsis(package: &Path, app_name: &str, relaunch: bool) -> Result<(), InstallError> {
    use std::fs;
    use std::io::Read;

    let mut input =
        std::fs::File::open(package).map_err(|_| InstallError::new("无法读取应用更新包"))?;
    let mut magic = [0_u8; 2];
    input
        .read_exact(&mut magic)
        .map_err(|_| InstallError::new("应用更新包无效"))?;
    if magic != [b'M', b'Z'] {
        return Err(InstallError::new("应用更新包不是 NSIS 可执行文件"));
    }
    let directory = tempfile::Builder::new()
        .prefix(&format!("{app_name}-update-"))
        .tempdir()
        .map_err(|_| InstallError::new("无法创建更新临时目录"))?
        .keep();
    let installer = directory.join("installer.exe");
    fs::copy(package, &installer).map_err(|_| InstallError::new("无法准备应用安装器"))?;
    std::process::Command::new(installer)
        .args(windows_nsis_args(relaunch))
        .spawn()
        .map_err(|_| InstallError::new("无法启动应用安装器"))?;
    Ok(())
}

#[cfg(test)]
#[path = "tests/platform.rs"]
mod tests;
