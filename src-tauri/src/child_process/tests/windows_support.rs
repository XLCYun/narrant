//! Windows 测试在清理前保留准确 HANDLE，返回后先 Wait(0)，再断言或兜底。

use std::{
    io,
    os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
    process::Child,
};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    },
};

/// 零超时读取仍被保留的原始对象，不补等也不按裸 PID 重查。
pub(crate) fn retained_status(process: &impl AsRawHandle) -> u32 {
    // SAFETY: 对象在调用期间被借用，句柄由调用者持有。
    unsafe { WaitForSingleObject(process.as_raw_handle(), 0) }
}

/// 准确自有对象的失败清理守卫；所有断言必须先保存状态再允许它析构。
pub(crate) struct ObservedProcess(pub(crate) OwnedHandle);

impl ObservedProcess {
    /// PID 来自仍等待握手放行的夹具，此后身份只由 HANDLE 决定。
    pub(crate) fn open(pid: u32) -> io::Result<Self> {
        // SAFETY: 仅打开本次保活夹具，终止权限用于测试失败时的兜底。
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: 成功 OpenProcess 的所有权只接管一次。
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle) }))
    }

    /// 根对象直接从 Child 克隆，完全不经过 PID 查找。
    pub(crate) fn child(child: &Child) -> Self {
        Self(child.as_handle().try_clone_to_owned().unwrap())
    }

    /// 清理返回后的第一项观察是准确对象的零超时状态。
    pub(crate) fn status(&self) -> u32 {
        retained_status(&self.0)
    }
}

impl Drop for ObservedProcess {
    /// 只在快照之后清理存活的自有对象，避免失败测试影响后续测试。
    fn drop(&mut self) {
        if self.status() == WAIT_TIMEOUT {
            // SAFETY: HANDLE 固定本次对象，不会终止 PID 重用后的其他进程。
            unsafe {
                TerminateProcess(self.0.as_raw_handle(), 1);
                WaitForSingleObject(self.0.as_raw_handle(), 5_000);
            }
        }
    }
}

/// 夹具根与后代的准确句柄保持到所有立即状态完成采样。
pub(crate) struct ObservedProcesses {
    pub(crate) descendant: ObservedProcess,
    pub(crate) root: ObservedProcess,
}

/// 清理返回后立即保存的查询值，后续日志或析构不能改变结果。
pub(crate) struct Completion {
    descendant: u32,
    root: u32,
}

impl ObservedProcesses {
    /// 先采样所有句柄再断言，失败输出也不能增加第二个对象的退出时间。
    pub(crate) fn completion(&self) -> Completion {
        Completion {
            descendant: self.descendant.status(),
            root: self.root.status(),
        }
    }
}

/// 仅检查已保存的立即状态；不重新查询或等待进程。
pub(crate) fn assert_completed(completion: Completion) {
    assert_eq!(completion.descendant, WAIT_OBJECT_0, "后代必须同步退出");
    assert_eq!(completion.root, WAIT_OBJECT_0, "根进程必须同步退出");
}
