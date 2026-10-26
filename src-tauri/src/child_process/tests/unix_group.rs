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

/// 首轮信号与 fork 交错后出现的同组成员也必须被终止，根退出不能提前结束屏障。
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn exit_barrier_reaps_group_member_born_after_initial_signal() {
    use super::ProcessGroup;
    use std::{
        io::Read,
        os::unix::process::{CommandExt, ExitStatusExt},
        process::{Command, Stdio},
    };

    let mut root = Command::new("/bin/sleep")
        .arg("60")
        .process_group(0)
        .spawn()
        .unwrap();
    let mut group = ProcessGroup::new(root.id());
    group.snapshot();
    group.terminate().unwrap();

    // 根保留为 zombie，使组号不会复用；在首轮信号之后加入尚未被快照的新成员。
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: 仅等待本测试的直接 Child，并保留其 PID 和进程组。
    assert_eq!(
        unsafe {
            libc::waitid(
                libc::P_PID,
                root.id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        },
        0
    );
    let mut late = Command::new("/bin/sleep")
        .arg("60")
        .process_group(root.id() as i32)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let late_pid = late.id() as i32;
    let mut stdout = late.stdout.take().unwrap();
    let mut stderr = late.stderr.take().unwrap();
    root.wait().unwrap();
    assert!(!group.members.contains_key(&late_pid));

    // 独立等待真正的直接 Child，生产屏障负责发信号，避免由测试兜底提前制造成功。
    let reaper = std::thread::spawn(move || late.wait().unwrap());
    let result = group.wait();
    if result.is_err() {
        // SAFETY: 失败路径清理仍被 reaper 持有的本测试 Child，不改变已采集的屏障结果。
        unsafe { libc::kill(late_pid, libc::SIGKILL) };
    }
    let status = reaper.join().unwrap();
    let mut output = Vec::new();
    stdout.read_to_end(&mut output).unwrap();
    stderr.read_to_end(&mut output).unwrap();
    assert_eq!(status.signal(), Some(libc::SIGKILL));
    assert!(output.is_empty());
    result.expect("首轮信号后出现的同组后代及其输出管道必须在退出屏障内回收");
}

/// macOS 的已退出对象会先失去 BSDINFO，但裸 PID 在父进程 wait 前仍存在。
#[cfg(target_os = "macos")]
#[test]
fn recorded_detached_zombie_cannot_satisfy_exit_barrier() {
    use super::{identity, ProcessGroup};
    use std::{os::unix::process::CommandExt, process::Command};
    let mut root = Command::new("/usr/bin/true")
        .process_group(0)
        .spawn()
        .unwrap();
    let mut group = ProcessGroup::new(root.id());
    root.wait().unwrap();
    let mut child = Command::new("/bin/sleep")
        .arg("60")
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = child.id() as i32;
    let stamp = identity(pid).expect("存活对象必须有出生身份");
    group.members.insert(pid, stamp);
    child.kill().unwrap();
    // SAFETY: 只等待本测试准确的直接 Child；WNOWAIT 保留确定的退出中对象。
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        },
        0
    );
    assert_eq!(identity(pid), None, "内核已不返回 zombie 的 BSDINFO");
    let result = group.wait();
    // SAFETY: 返回屏障后立即采样，不先 wait 或释放对象。
    let immediate = unsafe { libc::kill(pid, 0) };
    child.wait().unwrap();
    assert_eq!(immediate, 0, "测试必须确实保留未回收对象");
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    group.wait().unwrap();
}

/// 出生身份匹配的活进程仍阻止屏障，身份不匹配则不得等待或终止复用后的对象。
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn recorded_member_requires_matching_birth_identity() {
    let pid = std::process::id() as i32;
    let stamp = super::identity(pid).unwrap();
    assert!(super::recorded_member_alive(pid, stamp).unwrap());
    assert!(!super::recorded_member_alive(pid, (stamp.0.wrapping_add(1), stamp.1)).unwrap());
}
