use super::*;
use crate::child_process::windows_job::tests::{fixture_command, wait_file};
use std::os::windows::io::{AsHandle, AsRawHandle};
use windows_sys::Win32::{Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject};

/// Job 错误不能跳过已持有 direct Child 的回收，也不能把树标记为已确认完成。
#[test]
fn native_windows_tree_error_still_waits_direct_child() {
    let directory = tempfile::tempdir().unwrap();
    let mut tree = ProcessTree::spawn(&mut fixture_command("leaf", directory.path())).unwrap();
    wait_file(&directory.path().join("leaf-ready"));
    let observed = tree.child.as_handle().try_clone_to_owned().unwrap();
    tree.job.fail_next_cleanup_before_termination();
    assert!(tree.reap().is_err());
    let immediate = unsafe { WaitForSingleObject(observed.as_raw_handle(), 0) };
    assert_eq!(immediate, WAIT_OBJECT_0);
    assert!(!tree.reaped);
    assert!(tree.reap().is_err());
}
