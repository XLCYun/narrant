use super::{
    InstallError, InstallIntent, InstallKind, InstallOutcome, UpdateInstaller,
    VerifiedStagedPackage,
};
#[cfg(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "windows", target_arch = "x86_64"),
    test
))]
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
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
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
        .map_err(|error| ReplaceTransactionError::OriginalMoveFailed(error.kind()))?;
    if let Err(error) = rename.rename(candidate, destination) {
        return match rename.rename(backup, destination) {
            Ok(()) => Err(ReplaceTransactionError::CandidateMoveRolledBack(
                error.kind(),
            )),
            Err(_) => Err(ReplaceTransactionError::RollbackFailed),
        };
    }
    if post_replace().is_err() {
        if rename.rename(destination, failed_candidate).is_err() {
            return Err(ReplaceTransactionError::RollbackFailed);
        }
        return match rename.rename(backup, destination) {
            Ok(()) => Err(ReplaceTransactionError::PostReplaceRolledBack),
            Err(_) => Err(ReplaceTransactionError::RollbackFailed),
        };
    }
    Ok(())
}

/// 文件/目录移动的可测试系统边界。
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
trait RenameBoundary {
    /// 把 source 原子移动到 destination。
    fn rename(&mut self, source: &Path, destination: &Path) -> std::io::Result<()>;
}

/// 生产环境使用同文件系统 rename 保持替换原子性。
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
struct FileSystemRename;

#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
impl RenameBoundary for FileSystemRename {
    /// 委托标准库执行原子移动。
    fn rename(&mut self, source: &Path, destination: &Path) -> std::io::Result<()> {
        std::fs::rename(source, destination)
    }
}

/// 描述原路径替换失败及旧版本能否恢复。
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplaceTransactionError {
    OriginalMoveFailed(std::io::ErrorKind),
    CandidateMoveRolledBack(std::io::ErrorKind),
    PostReplaceRolledBack,
    RollbackFailed,
}

/// 仅在直接移动因权限不足且旧应用仍安全可用时请求系统管理员授权。
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
fn macos_authorization_required(error: &ReplaceTransactionError) -> bool {
    matches!(
        error,
        ReplaceTransactionError::OriginalMoveFailed(std::io::ErrorKind::PermissionDenied)
            | ReplaceTransactionError::CandidateMoveRolledBack(
                std::io::ErrorKind::PermissionDenied
            )
    )
}

/// 只有相邻工作目录因权限不足无法创建时才升级为管理员授权路径。
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
fn macos_work_creation_requires_authorization(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::PermissionDenied
}

/// 表示系统授权对话框与提权替换事务的脱敏结果。
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MacosAuthorizationOutcome {
    Succeeded,
    Denied,
    FailedBeforeReplacement,
    FailedAndRolledBack,
    FailedBackupPreserved,
    UnknownFailure,
}

/// 把 AppleScript 管理员授权收敛为可注入的单次命令边界。
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
trait MacosAuthorizationBoundary {
    /// 使用固定 AppleScript 包装执行已经安全引用的本地替换命令。
    fn run(&mut self, command: &str) -> std::io::Result<MacosAuthorizationOutcome>;
}

/// 生产环境通过系统 `osascript` 展示标准管理员授权对话框。
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
struct SystemMacosAuthorization;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
impl MacosAuthorizationBoundary for SystemMacosAuthorization {
    /// shell command 作为 AppleScript argv 传入，避免再经过 AppleScript 字符串插值。
    fn run(&mut self, command: &str) -> std::io::Result<MacosAuthorizationOutcome> {
        const APPLE_SCRIPT: &str = r#"on run argv
try
  do shell script (item 1 of argv) with administrator privileges
  return "NARRANT_UPDATE_OK"
on error errorMessage number errorNumber
  return "NARRANT_UPDATE_ERROR:" & (errorNumber as text)
end try
end run"#;
        let output = std::process::Command::new("/usr/bin/osascript")
            .args(["-e", APPLE_SCRIPT, command])
            .output()?;
        if !output.status.success() {
            return Ok(MacosAuthorizationOutcome::UnknownFailure);
        }
        let marker = String::from_utf8_lossy(&output.stdout);
        Ok(parse_macos_authorization_marker(marker.trim()))
    }
}

/// 把 osascript 的固定结果 marker 映射为不暴露系统输出的安装结果。
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
fn parse_macos_authorization_marker(marker: &str) -> MacosAuthorizationOutcome {
    match marker {
        "NARRANT_UPDATE_OK" => MacosAuthorizationOutcome::Succeeded,
        "NARRANT_UPDATE_ERROR:-128" => MacosAuthorizationOutcome::Denied,
        "NARRANT_UPDATE_ERROR:40" | "NARRANT_UPDATE_ERROR:41" | "NARRANT_UPDATE_ERROR:42" => {
            MacosAuthorizationOutcome::FailedBeforeReplacement
        }
        "NARRANT_UPDATE_ERROR:43" => MacosAuthorizationOutcome::FailedAndRolledBack,
        "NARRANT_UPDATE_ERROR:44" => MacosAuthorizationOutcome::FailedBackupPreserved,
        _ => MacosAuthorizationOutcome::UnknownFailure,
    }
}

/// 为受信本地绝对路径生成无插值的 POSIX shell 单引号参数。
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
fn macos_shell_quote(path: &Path) -> Result<String, InstallError> {
    if !path.is_absolute() {
        return Err(InstallError::new("提权安装路径无效"));
    }
    let value = path
        .to_str()
        .ok_or_else(|| InstallError::new("提权安装路径无法安全表示"))?;
    if value.chars().any(char::is_control) {
        return Err(InstallError::new("提权安装路径无法安全表示"));
    }
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

/// 构造保留旧应用直至新应用完成替换的提权 shell 事务。
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
fn build_macos_authorization_script(
    source: &Path,
    destination: &Path,
    work: &Path,
) -> Result<String, InstallError> {
    let candidate = work.join("candidate.app");
    let backup = work.join("previous.app");
    let failed_candidate = work.join("failed-candidate.app");
    let source = macos_shell_quote(source)?;
    let destination = macos_shell_quote(destination)?;
    let work = macos_shell_quote(work)?;
    let candidate = macos_shell_quote(&candidate)?;
    let backup = macos_shell_quote(&backup)?;
    let failed_candidate = macos_shell_quote(&failed_candidate)?;
    Ok(format!(
        "set -u; umask 077; \
         /bin/mkdir {work} || exit 40; \
         if ! /usr/bin/ditto {source} {candidate}; then /bin/rm -rf {work}; exit 41; fi; \
         if ! /bin/mv {destination} {backup}; then /bin/rm -rf {work}; exit 42; fi; \
         if /bin/mv {candidate} {destination}; then \
           if /usr/bin/touch {destination}; then /bin/rm -rf {backup} {work}; exit 0; fi; \
           if ! /bin/mv {destination} {failed_candidate}; then exit 44; fi; \
         fi; \
         if /bin/mv {backup} {destination}; then /bin/rm -rf {work}; exit 43; fi; \
         exit 44"
    ))
}

/// 请求系统管理员权限并把授权、回滚和未知失败分别投影为清晰错误。
#[cfg(any(all(target_os = "macos", target_arch = "aarch64"), test))]
fn install_macos_with_authorization(
    source: &Path,
    destination: &Path,
    work: &Path,
    authorization: &mut impl MacosAuthorizationBoundary,
) -> Result<(), InstallError> {
    let command = build_macos_authorization_script(source, destination, work)?;
    match authorization.run(&command) {
        Ok(MacosAuthorizationOutcome::Succeeded) => Ok(()),
        Ok(MacosAuthorizationOutcome::Denied) => Err(InstallError::new("系统管理员授权被拒绝")),
        Ok(MacosAuthorizationOutcome::FailedBeforeReplacement) => {
            Err(InstallError::new("提权安装失败，当前应用未被替换"))
        }
        Ok(MacosAuthorizationOutcome::FailedAndRolledBack) => {
            Err(InstallError::new("提权安装失败，已恢复旧版本"))
        }
        Ok(MacosAuthorizationOutcome::FailedBackupPreserved) => {
            Err(InstallError::new("提权安装回滚失败，已保留恢复副本"))
        }
        Ok(MacosAuthorizationOutcome::UnknownFailure) => {
            Err(InstallError::new("提权安装状态无法确认"))
        }
        Err(_) => Err(InstallError::new("无法请求系统管理员授权")),
    }
}

/// 生成由提权事务独占创建的相邻恢复目录路径，避免复用外部输入名称。
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn fresh_macos_authorized_work_path(parent: &Path) -> Result<std::path::PathBuf, InstallError> {
    let name_source = tempfile::Builder::new()
        .prefix(".narrant-update-authorized-")
        .tempdir()
        .map_err(|_| InstallError::new("无法准备提权安装"))?;
    let name = name_source
        .path()
        .file_name()
        .ok_or_else(|| InstallError::new("无法准备提权安装"))?;
    Ok(parent.join(name))
}

/// 从签名后的 `.app.tar.gz` 解包唯一应用，再执行 rollback-safe 原路径替换。
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_macos_archive(package: &Path) -> Result<(), InstallError> {
    let executable = std::env::current_exe().map_err(|_| InstallError::new("无法定位当前应用"))?;
    let destination = tauri_plugin_updater::extract_path_from_executable(&executable)
        .map_err(|_| InstallError::new("无法定位当前应用"))?;
    let parent = destination
        .parent()
        .ok_or_else(|| InstallError::new("无法定位当前应用"))?;
    let work = match tempfile::Builder::new()
        .prefix("narrant-update-")
        .tempdir_in(parent)
    {
        Ok(work) => work,
        Err(error) if macos_work_creation_requires_authorization(&error) => {
            let fallback = tempfile::Builder::new()
                .prefix("narrant-update-extracted-")
                .tempdir()
                .map_err(|_| InstallError::new("无法创建更新临时目录"))?;
            let candidate = extract_macos_candidate(package, fallback.path())?;
            let elevated_work = fresh_macos_authorized_work_path(parent)?;
            return install_macos_with_authorization(
                &candidate,
                &destination,
                &elevated_work,
                &mut SystemMacosAuthorization,
            );
        }
        Err(_) => return Err(InstallError::new("无法创建更新临时目录")),
    };
    let candidate = extract_macos_candidate(package, work.path())?;
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
        Err(error) if macos_authorization_required(&error) => {
            let elevated_work = fresh_macos_authorized_work_path(parent)?;
            install_macos_with_authorization(
                &candidate,
                &destination,
                &elevated_work,
                &mut SystemMacosAuthorization,
            )
        }
        Err(ReplaceTransactionError::RollbackFailed) => {
            let _preserved = work.keep();
            Err(InstallError::new("应用更新回滚失败，已保留恢复副本"))
        }
        Err(_) => Err(InstallError::new("应用更新替换失败，已恢复旧版本")),
    }
}

/// 在指定本地工作目录中安全解包候选应用，并返回唯一 `.app` 根目录。
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn extract_macos_candidate(
    package: &Path,
    work: &Path,
) -> Result<std::path::PathBuf, InstallError> {
    let extracted_root = work.join("extracted");
    std::fs::create_dir(&extracted_root).map_err(|_| InstallError::new("无法创建更新临时目录"))?;
    extract_macos_archive(package, &extracted_root)
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
