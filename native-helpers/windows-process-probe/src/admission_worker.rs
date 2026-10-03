//! 直接保留 CreateProcess 的原始错误码，独立证明零进程上限拒绝新成员。

use std::{
    fs, io,
    os::windows::io::{AsHandle, AsRawHandle},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{ERROR_NOT_ENOUGH_QUOTA, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::WaitForSingleObject,
};

use super::{
    belongs, observe, state, verify_admission_gate, wait_file,
    windows_process::NativeWindowsJob as WindowsJob,
};

/// 子模式必须在主入口检查 Node 环境之前分派；所有子进程均来自当前探针可执行文件。
pub(crate) fn dispatch() -> Option<io::Result<()>> {
    let mut arguments = std::env::args_os().skip(1);
    let mode = arguments.next()?;
    if mode != "--admission-worker" && mode != "--admission-leaf" {
        return None;
    }
    Some((|| {
        let location = PathBuf::from(arguments.next().ok_or_else(|| {
            io::Error::other("admission child mode requires an owned marker location")
        })?);
        if mode == "--admission-worker" {
            worker(&location)
        } else {
            publish(&location, &std::process::id().to_string())?;
            remain_alive()
        }
    })())
}

/// 原子发布标记，避免观察方将半写入的 PID 或错误码解释为完整结果。
fn publish(marker: &Path, value: &str) -> io::Result<()> {
    let temporary = marker.with_extension("tmp");
    fs::write(&temporary, value)?;
    fs::rename(temporary, marker)
}

/// 已入组夹具保持存活，由主探针拥有的 Job 在成功或失败路径统一回收。
fn remain_alive() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// 固定可执行文件和标准流，门禁前后的创建请求只改变发布标记的目标。
fn leaf_command(marker: &Path) -> io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--admission-leaf")
        .arg(marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}

/// 先创建一个正常后代，再等待准确 Job 的门禁就绪，直接记录 Rust 返回的系统错误码。
fn worker(directory: &Path) -> io::Result<()> {
    let _existing_child = leaf_command(&directory.join("descendant"))?.spawn()?;
    wait_file(&directory.join("descendant"))?;
    publish(&directory.join("ready"), "ready")?;
    wait_file(&directory.join("attempt-spawn"))?;
    let outcome = match leaf_command(&directory.join("unexpected-descendant"))?.spawn() {
        Err(error) => format!("rejected:{:?}", error.raw_os_error()),
        Ok(mut unexpected) => {
            let _ = unexpected.kill();
            unexpected.wait()?;
            "accepted".to_owned()
        }
    };
    publish(&directory.join("attempt-result"), &outcome)?;
    remain_alive()
}

/// 保留原父子句柄，证明门禁拒绝新成员而没有杀死现有成员；不改变生产清理实现。
pub(crate) fn run(directory: &Path) -> io::Result<()> {
    fs::create_dir(directory)?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--admission-worker")
        .arg(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let (mut parent, job) = WindowsJob::spawn(&mut command)?;
    wait_file(&directory.join("ready"))?;
    let pid: u32 = fs::read_to_string(directory.join("descendant"))?
        .parse()
        .map_err(io::Error::other)?;
    let descendant = observe(pid)?;
    for (name, handle) in [
        ("parent", parent.as_raw_handle()),
        ("descendant", descendant.0.as_raw_handle()),
    ] {
        let member = belongs(handle, job.raw())?;
        println!("[windows-process-probe] admission_rust {name} in_exact_job={member}");
        if !member {
            return Err(io::Error::other("admission fixture escaped the exact Job"));
        }
        // SAFETY: 父子句柄在本作用域一直由 Child 和 ObservedProcess 持有。
        assert_eq!(unsafe { WaitForSingleObject(handle, 0) }, WAIT_TIMEOUT);
    }
    verify_admission_gate(&job, directory)?;
    let outcome = fs::read_to_string(directory.join("attempt-result"))?;
    // 1816 是 Windows 的 ERROR_NOT_ENOUGH_QUOTA；未知映射、无错误码或其他失败不能冒充门禁成功。
    let expected = format!("rejected:Some({ERROR_NOT_ENOUGH_QUOTA})");
    println!(
        "[windows-process-probe] admission_rust raw_spawn_outcome={outcome} expected={expected}"
    );
    if outcome != expected {
        return Err(io::Error::other(format!(
            "admission gate requires exact quota rejection: {outcome}"
        )));
    }
    assert_eq!(
        state("admission_rust_parent", parent.id(), parent.as_raw_handle()),
        WAIT_TIMEOUT
    );
    assert_eq!(
        state(
            "admission_rust_descendant",
            pid,
            descendant.0.as_raw_handle()
        ),
        WAIT_TIMEOUT
    );
    let retained_descendant = descendant.0.as_handle().try_clone_to_owned()?;
    let cleanup = job.terminate_and_wait();
    println!("[windows-process-probe] admission_rust fixture_cleanup={cleanup:?}; gate evidence does not prove complete historical cleanup");
    // 门禁故意拒绝一个新进程，可能留下恰好 2 个保留对象 / 3 次累计关联的保守错误。
    let expected_gap = cleanup.as_ref().err().is_some_and(|error| {
        error.kind() == io::ErrorKind::Other
            && error.to_string() == "Windows Job 完成覆盖不足：保留 2，累计 3，活跃 0"
    });
    // 无论完整清理是否成功，都对已持有的直接父进程执行兜底；只有真实句柄完成才调用 wait。
    let parent_kill = parent.kill();
    // SAFETY: Child 一直持有原父进程句柄，等待不依赖 PID 重查。
    let parent_done = unsafe { WaitForSingleObject(parent.as_raw_handle(), 2000) };
    drop(descendant);
    // SAFETY: 克隆句柄仍指向同一叶进程对象，guard 兜底后必须确认其完成。
    let descendant_done = unsafe { WaitForSingleObject(retained_descendant.as_raw_handle(), 2000) };
    println!("[windows-process-probe] admission_rust fixture_disposal parent_kill={parent_kill:?} parent_wait={parent_done} descendant_wait={descendant_done} expected_rejected_association_gap={expected_gap}");
    assert_eq!(
        parent_done, WAIT_OBJECT_0,
        "admission parent disposal failed"
    );
    assert_eq!(
        descendant_done, WAIT_OBJECT_0,
        "admission descendant disposal failed"
    );
    parent.wait()?;
    drop(job);
    fs::remove_dir_all(directory)?;
    if !expected_gap {
        cleanup?;
    }
    println!("[windows-process-probe] PASS: Rust admission gate only; complete historical cleanup is not asserted in this deliberate rejection scenario");
    Ok(())
}
