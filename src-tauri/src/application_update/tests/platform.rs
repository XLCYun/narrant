use super::*;
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

/// 以脚本结果记录每一次目录移动的测试文件系统边界。
struct ScriptedRename {
    results: VecDeque<io::Result<()>>,
    calls: Vec<(PathBuf, PathBuf)>,
}

impl ScriptedRename {
    /// 创建按顺序返回成功/失败的 rename 脚本。
    fn new(results: impl IntoIterator<Item = io::Result<()>>) -> Self {
        Self {
            results: results.into_iter().collect(),
            calls: Vec::new(),
        }
    }
}

impl RenameBoundary for ScriptedRename {
    /// 记录 source/destination 并返回下一个脚本结果。
    fn rename(&mut self, source: &Path, destination: &Path) -> io::Result<()> {
        self.calls.push((source.to_owned(), destination.to_owned()));
        self.results.pop_front().unwrap_or(Ok(()))
    }
}

#[test]
fn target_and_install_kind_must_both_match_the_selected_adapter() {
    assert!(validate_install_plan(
        MACOS_TARGET,
        InstallKind::MacosAppArchive,
        MACOS_TARGET,
        InstallKind::MacosAppArchive,
    )
    .is_ok());
    assert!(validate_install_plan(
        MACOS_TARGET,
        InstallKind::MacosAppArchive,
        WINDOWS_TARGET,
        InstallKind::MacosAppArchive,
    )
    .is_err());
    assert!(validate_install_plan(
        MACOS_TARGET,
        InstallKind::MacosAppArchive,
        MACOS_TARGET,
        InstallKind::WindowsNsis,
    )
    .is_err());
}

#[test]
fn macos_original_path_replacement_rolls_back_when_candidate_move_fails() {
    let mut rename = ScriptedRename::new([
        Ok(()),
        Err(io::Error::other("candidate move failed")),
        Ok(()),
    ]);
    let result = replace_with_rollback(
        &mut rename,
        Path::new("/Applications/Narrant.app"),
        Path::new("/tmp/candidate.app"),
        Path::new("/tmp/previous.app"),
        Path::new("/tmp/failed-candidate.app"),
        || Ok(()),
    );
    assert_eq!(
        result,
        Err(ReplaceTransactionError::CandidateMoveRolledBack(
            io::ErrorKind::Other
        ))
    );
    assert_eq!(rename.calls.len(), 3);
    assert_eq!(
        rename.calls[2].1,
        PathBuf::from("/Applications/Narrant.app")
    );
}

#[test]
fn macos_permission_denied_replacement_selects_authorized_fallback() {
    assert!(macos_authorization_required(
        &ReplaceTransactionError::OriginalMoveFailed(io::ErrorKind::PermissionDenied)
    ));
    assert!(macos_authorization_required(
        &ReplaceTransactionError::CandidateMoveRolledBack(io::ErrorKind::PermissionDenied)
    ));
    assert!(!macos_authorization_required(
        &ReplaceTransactionError::CandidateMoveRolledBack(io::ErrorKind::Other)
    ));
    assert!(!macos_authorization_required(
        &ReplaceTransactionError::PostReplaceRolledBack
    ));
    assert!(macos_work_creation_requires_authorization(&io::Error::new(
        io::ErrorKind::PermissionDenied,
        "parent denied"
    )));
    assert!(!macos_work_creation_requires_authorization(
        &io::Error::new(io::ErrorKind::StorageFull, "disk full")
    ));
}

#[cfg(unix)]
#[test]
fn macos_authorized_script_quotes_paths_and_keeps_old_app_until_commit() {
    let script = build_macos_authorization_script(
        Path::new("/tmp/new candidate's.app"),
        Path::new("/Applications/Narrant user's.app"),
        Path::new("/Applications/.narrant update work"),
    )
    .expect("script");
    assert!(script.contains("'/tmp/new candidate'\\''s.app'"));
    assert!(script.contains("'/Applications/Narrant user'\\''s.app'"));
    assert!(!script.contains("rm -rf '/Applications/Narrant user"));
    let move_old = script
        .find("/bin/mv '/Applications/Narrant user")
        .expect("old app move");
    let delete_backup = script
        .find("/bin/rm -rf '/Applications/.narrant update work/previous.app'")
        .expect("backup cleanup");
    assert!(move_old < delete_backup);
}

#[cfg(unix)]
#[test]
fn macos_authorized_script_rejects_non_utf8_and_control_character_paths() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let non_utf8 = PathBuf::from(OsString::from_vec(b"/tmp/invalid-\xff.app".to_vec()));
    assert!(build_macos_authorization_script(
        &non_utf8,
        Path::new("/Applications/Narrant.app"),
        Path::new("/Applications/.narrant-update")
    )
    .is_err());
    assert!(build_macos_authorization_script(
        Path::new("/tmp/candidate\n.app"),
        Path::new("/Applications/Narrant.app"),
        Path::new("/Applications/.narrant-update")
    )
    .is_err());
}

/// 记录提权脚本并返回预设的系统授权结果。
#[cfg(unix)]
struct ScriptedAuthorization {
    outcome: Option<io::Result<MacosAuthorizationOutcome>>,
    scripts: Vec<String>,
}

#[cfg(unix)]
impl MacosAuthorizationBoundary for ScriptedAuthorization {
    fn run(&mut self, script: &str) -> io::Result<MacosAuthorizationOutcome> {
        self.scripts.push(script.to_owned());
        self.outcome.take().expect("single authorization call")
    }
}

#[cfg(unix)]
#[test]
fn macos_authorization_reports_success_denial_and_preserved_backup() {
    let source = Path::new("/tmp/Narrant.app");
    let destination = Path::new("/Applications/Narrant.app");
    let work = Path::new("/Applications/.narrant-update-test");
    let mut success = ScriptedAuthorization {
        outcome: Some(Ok(MacosAuthorizationOutcome::Succeeded)),
        scripts: Vec::new(),
    };
    assert!(install_macos_with_authorization(source, destination, work, &mut success).is_ok());
    assert_eq!(success.scripts.len(), 1);

    let mut denied = ScriptedAuthorization {
        outcome: Some(Ok(MacosAuthorizationOutcome::Denied)),
        scripts: Vec::new(),
    };
    let error = install_macos_with_authorization(source, destination, work, &mut denied)
        .expect_err("denied");
    assert_eq!(error.message(), "系统管理员授权被拒绝");

    let mut rollback_failed = ScriptedAuthorization {
        outcome: Some(Ok(MacosAuthorizationOutcome::FailedBackupPreserved)),
        scripts: Vec::new(),
    };
    let error = install_macos_with_authorization(source, destination, work, &mut rollback_failed)
        .expect_err("rollback failure");
    assert_eq!(error.message(), "提权安装回滚失败，已保留恢复副本");

    let mut unavailable = ScriptedAuthorization {
        outcome: Some(Err(io::Error::new(io::ErrorKind::NotFound, "osascript"))),
        scripts: Vec::new(),
    };
    let error = install_macos_with_authorization(source, destination, work, &mut unavailable)
        .expect_err("authorization unavailable");
    assert_eq!(error.message(), "无法请求系统管理员授权");
}

#[test]
fn macos_authorization_markers_distinguish_denial_rollback_and_unknown_failure() {
    assert_eq!(
        parse_macos_authorization_marker("NARRANT_UPDATE_OK"),
        MacosAuthorizationOutcome::Succeeded
    );
    assert_eq!(
        parse_macos_authorization_marker("NARRANT_UPDATE_ERROR:-128"),
        MacosAuthorizationOutcome::Denied
    );
    assert_eq!(
        parse_macos_authorization_marker("NARRANT_UPDATE_ERROR:43"),
        MacosAuthorizationOutcome::FailedAndRolledBack
    );
    assert_eq!(
        parse_macos_authorization_marker("NARRANT_UPDATE_ERROR:44"),
        MacosAuthorizationOutcome::FailedBackupPreserved
    );
    assert_eq!(
        parse_macos_authorization_marker("unexpected"),
        MacosAuthorizationOutcome::UnknownFailure
    );
}

#[test]
fn windows_current_user_nsis_args_only_relaunch_for_explicit_restart() {
    assert_eq!(windows_nsis_args(false), ["/P", "/UPDATE"]);
    assert_eq!(windows_nsis_args(true), ["/P", "/UPDATE", "/R"]);
}

#[test]
fn macos_real_filesystem_restores_old_app_after_candidate_or_post_failure() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let destination = temporary.path().join("Narrant.app");
    let candidate = temporary.path().join("candidate.app");
    let backup = temporary.path().join("previous.app");
    let failed = temporary.path().join("failed.app");
    std::fs::create_dir(&destination).expect("old app");
    std::fs::write(destination.join("version"), "old").expect("old marker");
    let result = replace_with_rollback(
        &mut FileSystemRename,
        &destination,
        &candidate,
        &backup,
        &failed,
        || Ok(()),
    );
    assert_eq!(
        result,
        Err(ReplaceTransactionError::CandidateMoveRolledBack(
            io::ErrorKind::NotFound
        ))
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("version")).unwrap(),
        "old"
    );

    std::fs::create_dir(&candidate).expect("candidate app");
    std::fs::write(candidate.join("version"), "new").expect("new marker");
    let result = replace_with_rollback(
        &mut FileSystemRename,
        &destination,
        &candidate,
        &backup,
        &failed,
        || Err(io::Error::other("post replace failed")),
    );
    assert_eq!(result, Err(ReplaceTransactionError::PostReplaceRolledBack));
    assert_eq!(
        std::fs::read_to_string(destination.join("version")).unwrap(),
        "old"
    );
    assert_eq!(
        std::fs::read_to_string(failed.join("version")).unwrap(),
        "new"
    );
}

#[test]
fn macos_rollback_failure_leaves_backup_at_recovery_path() {
    let mut rename = ScriptedRename::new([
        Ok(()),
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "candidate")),
        Err(io::Error::other("rollback")),
    ]);
    let backup = Path::new("/Applications/.narrant-update/previous.app");
    let result = replace_with_rollback(
        &mut rename,
        Path::new("/Applications/Narrant.app"),
        Path::new("/Applications/.narrant-update/candidate.app"),
        backup,
        Path::new("/Applications/.narrant-update/failed.app"),
        || Ok(()),
    );
    assert_eq!(result, Err(ReplaceTransactionError::RollbackFailed));
    assert_eq!(rename.calls[0].1, backup);
    assert_eq!(rename.calls.len(), 3);
    assert_eq!(rename.calls[2].0, backup);
    assert_eq!(
        rename.calls[2].1,
        PathBuf::from("/Applications/Narrant.app")
    );
}

/// 创建真实 gzip tar fixture。
fn tar_fixture(entry_type: tar::EntryType, raw_name: &[u8]) -> tempfile::NamedTempFile {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    let file = tempfile::NamedTempFile::new().expect("archive file");
    let encoder = GzEncoder::new(
        file.reopen().expect("archive writer"),
        Compression::default(),
    );
    let mut archive = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(entry_type);
    header.set_size(0);
    header.set_mode(0o755);
    header.set_cksum();
    let bytes = header.as_mut_bytes();
    bytes[..100].fill(0);
    bytes[..raw_name.len()].copy_from_slice(raw_name);
    header.set_cksum();
    archive
        .append(&header, io::empty())
        .expect("append raw entry");
    archive
        .into_inner()
        .expect("finish tar")
        .finish()
        .expect("finish gzip");
    file
}

#[test]
fn macos_archive_rejects_root_symlink_hardlink_and_path_traversal() {
    for (entry_type, name) in [
        (tar::EntryType::Symlink, b"Narrant.app".as_slice()),
        (tar::EntryType::Link, b"Narrant.app".as_slice()),
        (
            tar::EntryType::Regular,
            b"../Narrant.app/payload".as_slice(),
        ),
    ] {
        let fixture = tar_fixture(entry_type, name);
        let output = tempfile::tempdir().expect("output");
        assert!(extract_macos_archive(fixture.path(), output.path()).is_err());
    }
}

/// 记录一次 Windows installer handoff，并在调用时验证临时复制仍存活。
#[derive(Default)]
struct RecordingLauncher {
    calls: Mutex<Vec<(PathBuf, Vec<String>)>>,
}

impl CommandLauncher for RecordingLauncher {
    fn launch(&self, executable: &Path, args: &[&str]) -> io::Result<()> {
        assert!(executable.is_file());
        self.calls.lock().unwrap().push((
            executable.to_owned(),
            args.iter().map(|value| (*value).to_owned()).collect(),
        ));
        Ok(())
    }
}

/// 写入与真实安装器一致的最小 PE machine header。
fn write_pe(machine: u16) -> tempfile::NamedTempFile {
    use std::io::{Seek, SeekFrom, Write};
    let mut file = tempfile::NamedTempFile::new().expect("pe file");
    let mut dos = [0_u8; 64];
    dos[..2].copy_from_slice(b"MZ");
    dos[0x3c..0x40].copy_from_slice(&64_u32.to_le_bytes());
    file.write_all(&dos).expect("dos");
    file.seek(SeekFrom::Start(64)).expect("seek");
    file.write_all(b"PE\0\0").expect("pe signature");
    file.write_all(&machine.to_le_bytes()).expect("machine");
    file.flush().expect("flush");
    file
}

#[test]
fn windows_handoff_validates_x64_pe_copy_lifetime_argv_and_single_launch() {
    assert!(validate_windows_nsis_pe(write_pe(0xaa64).path()).is_err());
    let package = write_pe(0x8664);
    let launcher = RecordingLauncher::default();
    let installer = prepare_and_launch_windows_nsis(package.path(), "Narrant", true, &launcher)
        .expect("handoff");
    assert!(installer.is_file());
    let calls = launcher.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, ["/P", "/UPDATE", "/R"]);
    std::fs::remove_dir_all(installer.parent().unwrap()).expect("cleanup kept tempdir");
}

#[test]
fn windows_handoff_accepts_nsis_x86_bootstrapper_for_x64_application() {
    // 已发布的 x64 NSIS 包使用 I386 loader；应用目标仍由 windows-x86_64 安装计划约束。
    let package = write_pe(0x014c);
    let launcher = RecordingLauncher::default();
    let installer = prepare_and_launch_windows_nsis(package.path(), "Narrant", false, &launcher)
        .expect("x86 NSIS bootstrapper can install the x64 application");
    let calls = launcher.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, ["/P", "/UPDATE"]);
    std::fs::remove_dir_all(installer.parent().unwrap()).expect("cleanup kept tempdir");
}
