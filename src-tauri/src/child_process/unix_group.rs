//! 自有 Unix 进程组的快照与同步退出屏障；不解释调用方协议。

use std::{
    io,
    time::{Duration, Instant},
};

/// 保存优雅退出前的组成员，macOS 僵尸即使离组仍须等待系统回收。
pub(super) struct ProcessGroup {
    id: i32,
    members: std::collections::BTreeMap<i32, (u64, u64)>,
}

impl ProcessGroup {
    /// 只接管 spawn 或 current() 已建立/验证的专属进程组。
    pub(super) fn new(root: u32) -> Self {
        Self {
            id: root as i32,
            members: Default::default(),
        }
    }

    /// 一次性 helper 只能拥有以自身 PID 为组号的域；直接启动时先脱离 shell 的组。
    pub(super) fn current() -> io::Result<Self> {
        // SAFETY: 只查询/设置调用进程自身，不修改父进程或其他会话。
        let pid = unsafe { libc::getpid() };
        if unsafe { libc::getpgrp() } != pid && unsafe { libc::setpgid(0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::getpgrp() } != pid {
            return Err(io::Error::other("helper 未能建立自身专属进程组"));
        }
        Ok(Self::new(pid as u32))
    }

    /// 必须先快照再等待根进程；重试时保留上次尚未确认消失的后代。
    pub(super) fn snapshot(&mut self) {
        let mut pending = vec![self.id];
        pending.extend(
            self.members
                .iter()
                .filter_map(|(&pid, &stamp)| (identity(pid) == Some(stamp)).then_some(pid)),
        );
        pending.extend(group_members(self.id));
        let mut seen = std::collections::BTreeSet::new();
        while let Some(pid) = pending.pop() {
            if !seen.insert(pid) {
                continue;
            }
            if let Some(stamp) = identity(pid) {
                self.members.insert(pid, stamp);
                pending.extend(children(pid));
            }
        }
    }

    /// 只终止自有进程组，直接 Child 的 kill/wait 由共享所有者始终执行。
    pub(super) fn terminate(&self) -> io::Result<()> {
        let mut failure = None;
        // 先终止已观察到的后代；其进程组可以因嵌套 owner 或 setsid 而改变。
        for (&pid, &stamp) in &self.members {
            if pid == self.id || identity(pid) != Some(stamp) {
                continue;
            }
            // SAFETY: PID 和出生时间共同确认它仍是当前所有者记录的进程。
            if unsafe { libc::kill(pid, libc::SIGKILL) } != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    failure = Some(error);
                }
            }
        }
        // SAFETY: 负 PID 只终止本次 spawn/current 创建的专属进程组。
        if unsafe { libc::kill(-self.id, libc::SIGKILL) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                failure = Some(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// 根已回收后继续终止残留成员，覆盖首轮信号与 fork 交错时刚出现的后代。
    pub(super) fn wait(&self) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            // SAFETY: 信号 0 仅查询自有组及此前记录的成员，不发送终止信号。
            let probe = if unsafe { libc::kill(-self.id, 0) } == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            };
            let mut alive = group_probe_alive(probe)?;
            for (&pid, &stamp) in &self.members {
                alive |= recorded_member_alive(pid, stamp)?;
            }
            if !alive {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "子进程树回收超时"));
            }
            if let Err(error) = self.terminate() {
                // macOS 的纯 zombie 组可能拒绝信号；仍须等到组和已记录成员都消失。
                if error.raw_os_error() != Some(libc::EPERM) {
                    return Err(error);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// 出生信息消失可能只是 zombie 过渡；仅查询 PID 是否仍存在，不对未知身份发送信号。
fn recorded_member_alive(pid: i32, stamp: (u64, u64)) -> io::Result<bool> {
    match identity(pid) {
        Some(current) => Ok(current == stamp),
        None => {
            // SAFETY: 信号 0 只观察；PID 已复用时也不会终止不属于当前树的对象。
            let probe = if unsafe { libc::kill(pid, 0) } == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            };
            group_probe_alive(probe)
        }
    }
}

/// macOS 原生进程枚举保留关闭前的组成员，避免启动 ps 辅助进程参与回收。
#[cfg(target_os = "macos")]
fn group_members(group: i32) -> Vec<i32> {
    let mut capacity = 64usize;
    loop {
        let mut pids = vec![0i32; capacity];
        // SAFETY: PROC_PGRP_ONLY=2 来自系统 proc_info.h；缓冲区按 PID 宽度分配。
        let bytes = unsafe {
            libc::proc_listpids(
                2,
                group as u32,
                pids.as_mut_ptr().cast(),
                (pids.len() * std::mem::size_of::<i32>()) as i32,
            )
        };
        if bytes <= 0 {
            return Vec::new();
        }
        let count = bytes as usize / std::mem::size_of::<i32>();
        if count < capacity {
            pids.truncate(count);
            pids.retain(|pid| *pid > 0);
            return pids;
        }
        capacity *= 2;
    }
}

/// Linux 从 procfs 查询同组 PID，不读取任何被分析项目数据。
#[cfg(target_os = "linux")]
fn group_members(group: i32) -> Vec<i32> {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<i32>().ok()?;
            // SAFETY: getpgid 只读取候选 PID 的进程组。
            (unsafe { libc::getpgid(pid) } == group).then_some(pid)
        })
        .collect()
}

/// 其他 Unix 平台仍以进程组信号检查作为回收依据。
#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn group_members(_group: i32) -> Vec<i32> {
    Vec::new()
}

/// macOS 按内核记录的出生时间避免 PID 复用造成误杀。
#[cfg(target_os = "macos")]
fn identity(pid: i32) -> Option<(u64, u64)> {
    // SAFETY: BSDINFO 的 POD 大小与内核公开接口一致，只读进程元数据。
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (read == size).then_some((info.pbi_start_tvsec, info.pbi_start_tvusec))
}

/// macOS 查询直接后代，递归遍历由快照负责，包含另建进程组的子进程。
#[cfg(target_os = "macos")]
fn children(pid: i32) -> Vec<i32> {
    let mut capacity = 64;
    loop {
        let mut values = vec![0i32; capacity];
        // SAFETY: PROC_PPID_ONLY=6，缓冲区按 PID 元素分配。
        let bytes = unsafe {
            libc::proc_listpids(
                6,
                pid as u32,
                values.as_mut_ptr().cast(),
                (capacity * 4) as i32,
            )
        };
        if bytes <= 0 {
            return Vec::new();
        }
        let count = bytes as usize / 4;
        if count < capacity {
            values.truncate(count);
            values.retain(|v| *v > 0);
            return values;
        }
        capacity *= 2;
    }
}

/// Linux 的启动时钟 tick 是进程身份的一部分，测试平台保留同样的复用保护。
#[cfg(target_os = "linux")]
fn identity(pid: i32) -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    Some((fields.get(19)?.parse().ok()?, 0))
}

/// Linux 按父身份读取当前后代，不受其进程组变化影响。
#[cfg(target_os = "linux")]
fn children(pid: i32) -> Vec<i32> {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let child: i32 = entry.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            (stat
                .rsplit_once(')')?
                .1
                .split_whitespace()
                .nth(1)?
                .parse::<i32>()
                .ok()?
                == pid)
                .then_some(child)
        })
        .collect()
}

/// 非目标 Unix 平台仅保留组管理，不声明相同的脱组后代保证。
#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn identity(pid: i32) -> Option<(u64, u64)> {
    (unsafe { libc::kill(pid, 0) } == 0).then_some((0, 0))
}

/// 非目标 Unix 平台没有本项目维护的枚举实现。
#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn children(_: i32) -> Vec<i32> {
    Vec::new()
}

/// 只有 ESRCH 能证明组已消失；权限失败仍按存活等待，其他查询错误直接失败。
fn group_probe_alive(probe: io::Result<()>) -> io::Result<bool> {
    match probe {
        Ok(()) => Ok(true),
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(false),
        Err(error) if error.raw_os_error() == Some(libc::EPERM) => Ok(true),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "tests/unix_group.rs"]
mod tests;
