//! Windows 测试在清理前保留准确 HANDLE，返回后先 Wait(0)，再 join 或兜底。

use super::{publish, wait_for_marker};
use std::{
    fs, io,
    os::windows::io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    process::Child,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    thread::JoinHandle,
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

    /// 标记必须已经完整发布，读取后立即固定准确对象。
    pub(crate) fn marker(path: &Path, stop: &AtomicBool) -> io::Result<Self> {
        wait_for_marker(path, stop)?;
        let pid = fs::read_to_string(path)?
            .parse::<u32>()
            .map_err(io::Error::other)?;
        Self::open(pid)
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

/// 夹具根与后代的准确句柄在放行前一起发布，观察线程结束不会缩短其寿命。
pub(crate) struct ObservedProcesses {
    pub(crate) descendant: ObservedProcess,
    pub(crate) root: ObservedProcess,
}

/// 清理返回后立即保存的查询值，后续日志、join 或析构不能改变结果。
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

/// 文件握手观察线程的测试守卫；协议夹具只指定三个标记与放行后的动作。
pub(crate) struct FixtureObservation {
    observed: Arc<OnceLock<ObservedProcesses>>,
    stop: Arc<AtomicBool>,
    observer: Option<JoinHandle<io::Result<()>>>,
}

impl FixtureObservation {
    /// 根、后代、放行标记按顺序传入；句柄发布一定早于夹具获准继续执行。
    pub(crate) fn start(
        directory: &Path,
        markers: [&'static str; 3],
        after_ready: impl FnOnce(&ObservedProcesses, &AtomicBool) -> io::Result<()> + Send + 'static,
    ) -> Self {
        let observed = Arc::new(OnceLock::new());
        let published = Arc::clone(&observed);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let directory = directory.to_owned();
        let observer = std::thread::spawn(move || {
            // 后代标记必定晚于根启动，兼容先写根 PID 再原子发布后代的 native 夹具。
            let descendant = ObservedProcess::marker(&directory.join(markers[1]), &stopping)?;
            let root = ObservedProcess::marker(&directory.join(markers[0]), &stopping)?;
            published
                .set(ObservedProcesses { descendant, root })
                .map_err(|_| io::Error::other("夹具进程句柄只能发布一次"))?;
            publish(&directory.join(markers[2]), "saved")?;
            after_ready(published.get().expect("句柄已发布"), &stopping)
        });
        Self {
            observed,
            stop,
            observer: Some(observer),
        }
    }

    /// 只有夹具已获放行的调用方才能读取；不在这里等待观察线程。
    pub(crate) fn processes(&self) -> &ObservedProcesses {
        self.observed.get().expect("夹具放行前必须保存进程句柄")
    }

    /// 必须是清理返回后的第一项动作；join 由调用方随后显式执行。
    pub(crate) fn completion(&self) -> Completion {
        self.processes().completion()
    }

    /// 已保存完成状态后才等待观察者，线程调度不能帮助退出断言通过。
    pub(crate) fn join(&mut self) {
        self.observer
            .take()
            .expect("观察线程只回收一次")
            .join()
            .expect("夹具观察线程不应 panic")
            .expect("夹具必须完成准确对象握手");
    }
}

impl Drop for FixtureObservation {
    /// 提前失败先停止文件等待，再由准确对象守卫兜底，避免孤儿观察线程。
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(observer) = self.observer.take() {
            let _ = observer.join();
        }
    }
}
