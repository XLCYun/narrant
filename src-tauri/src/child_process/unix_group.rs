//! 自有 Unix 进程组的快照与同步退出屏障；不解释调用方协议。

use std::{
    io,
    time::{Duration, Instant},
};

/// 保存优雅退出前的组成员，macOS 僵尸即使离组仍须等待系统回收。
pub(super) struct ProcessGroup {
    id: i32,
    members: Vec<i32>,
}

impl ProcessGroup {
    /// 仅接管 spawn 时以 process_group(0) 建立的独立组。
    pub(super) fn new(root: u32) -> Self {
        Self {
            id: root as i32,
            members: Vec::new(),
        }
    }

    /// 必须先快照再等待根进程；重试时保留上次尚未确认消失的后代。
    pub(super) fn snapshot(&mut self) {
        self.members.extend(group_members(self.id));
        self.members.sort_unstable();
        self.members.dedup();
    }

    /// 只终止自有进程组，直接 Child 的 kill/wait 由共享所有者始终执行。
    pub(super) fn terminate(&self) {
        // SAFETY: 本组由 process_group(0) 创建，负 PID 只指向当前会话。
        unsafe {
            libc::kill(-self.id, libc::SIGKILL);
        }
    }

    /// 根 Child 已回收后检查组与快照；不能把父退出当作整棵树完成。
    pub(super) fn wait(&self) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            // SAFETY: 信号 0 仅查询自有组及此前记录的成员，不发送终止信号。
            let alive = unsafe { libc::kill(-self.id, 0) } == 0
                || self
                    .members
                    .iter()
                    .any(|pid| unsafe { libc::kill(*pid, 0) } == 0);
            if !alive {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "子进程树回收超时"));
            }
            std::thread::sleep(Duration::from_millis(10));
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
