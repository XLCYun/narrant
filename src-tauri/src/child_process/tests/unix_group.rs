use super::group_probe_alive;
use std::io;

/// 权限拒绝不是退出证据；ESRCH 以外的真实探测错误也不能成为成功屏障。
#[test]
fn permission_denied_never_means_group_disappeared() {
    assert!(group_probe_alive(Ok(())).unwrap());
    assert!(group_probe_alive(Err(io::Error::from_raw_os_error(libc::EPERM))).unwrap());
    assert!(!group_probe_alive(Err(io::Error::from_raw_os_error(libc::ESRCH))).unwrap());
    assert_eq!(
        group_probe_alive(Err(io::Error::from_raw_os_error(libc::EINVAL)))
            .unwrap_err()
            .raw_os_error(),
        Some(libc::EINVAL)
    );
}
