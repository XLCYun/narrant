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
    assert_eq!(result, Err(ReplaceTransactionError::RolledBack));
    assert_eq!(rename.calls.len(), 3);
    assert_eq!(
        rename.calls[2].1,
        PathBuf::from("/Applications/Narrant.app")
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
    assert_eq!(result, Err(ReplaceTransactionError::RolledBack));
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
    assert_eq!(result, Err(ReplaceTransactionError::RolledBack));
    assert_eq!(
        std::fs::read_to_string(destination.join("version")).unwrap(),
        "old"
    );
    assert_eq!(
        std::fs::read_to_string(failed.join("version")).unwrap(),
        "new"
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

/// 写入可被最小 PE contract validator 接受的 AMD64 header。
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
    assert!(validate_windows_pe_x64(write_pe(0x014c).path()).is_err());
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
