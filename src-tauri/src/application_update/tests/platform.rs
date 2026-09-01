use super::*;
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;

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
