use super::*;

/// 写入已满 stdin 管道时仍能响应取消，并在返回前回收输入线程和子进程。
#[tokio::test]
async fn cancellation_reaps_a_blocked_input_writer() {
    let fixture = repository();
    let cancellation = TestCancellation::default();
    let request = cancellation.clone();
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        request.cancel();
    });
    let started = Instant::now();
    let error = CoreGitProcess::default()
        .execute(
            GitInvocation::new(
                fixture.path(),
                ["-c", "alias.block=!sleep 30", "block"],
                GitOperationClass::Query,
            )
            .with_input(vec![b'x'; 1024 * 1024]),
            &cancellation,
        )
        .await
        .unwrap_err();
    canceller.join().unwrap();
    assert_eq!(error.kind(), CoreGitFailureKind::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(2));
}

/// 输出超限时不能等待尚未消费的 stdin 写完；原始超限类别应保留。
#[tokio::test]
async fn output_limit_reaps_a_blocked_input_writer() {
    let fixture = repository();
    let started = Instant::now();
    let error = CoreGitProcess::with_limits(QUERY_TIMEOUT, 1)
        .execute(
            GitInvocation::new(
                fixture.path(),
                ["-c", "alias.overflow=!printf xx; sleep 30", "overflow"],
                GitOperationClass::Query,
            )
            .with_input(vec![b'x'; 1024 * 1024]),
            OperationContext::background().cancellation(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.kind(),
        CoreGitFailureKind::OutputLimitExceeded {
            stream: GitOutputStream::Stdout,
            limit: 1
        }
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

/// 主进程退出而后代保留 stdin 时，输入等待仍受查询超时控制。
#[tokio::test]
async fn timeout_reaps_input_inherited_after_git_exits() {
    let fixture = repository();
    let started = Instant::now();
    let error = CoreGitProcess::with_limits(Duration::from_millis(100), DEFAULT_OUTPUT_LIMIT)
        .execute(
            GitInvocation::new(
                fixture.path(),
                [
                    "-c",
                    "alias.detach=!exec 3<&0; sleep 30 <&3 >/dev/null 2>&1 &",
                    "detach",
                ],
                GitOperationClass::Query,
            )
            .with_input(vec![b'x'; 1024 * 1024]),
            OperationContext::background().cancellation(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), CoreGitFailureKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(2));
}

/// 逃离 Git 进程组的测试后代持有 stdin 时，取消仍能回收自己的输入线程。
#[tokio::test]
async fn cancellation_does_not_wait_for_an_escaped_stdin_holder() {
    let fixture = repository();
    let _cleanup = EscapedInputCleanup(fixture.path().to_owned());
    let executable = std::env::current_exe().unwrap();
    let alias = format!(
        "alias.escape=!NARRANT_ESCAPED_INPUT_FIXTURE={} {} --exact core_git::tests::input::escaped_stdin_holder --nocapture",
        shell_argument(fixture.path().to_str().unwrap()),
        shell_argument(executable.to_str().unwrap()),
    );
    let cancellation = TestCancellation::default();
    let request = cancellation.clone();
    let ready = fixture.path().join("escaped-ready");
    let ready_for_cancel = ready.clone();
    let canceller = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !ready_for_cancel.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let started = Instant::now();
        request.cancel();
        started
    });
    let error = CoreGitProcess::default()
        .execute(
            GitInvocation::new(
                fixture.path(),
                ["-c", alias.as_str(), "escape"],
                GitOperationClass::Query,
            )
            .with_input(vec![b'x'; 1024 * 1024]),
            &cancellation,
        )
        .await
        .unwrap_err();
    let cancelled_at = canceller.join().unwrap();
    assert!(
        ready.exists(),
        "escaped fixture must be running before cancellation"
    );
    assert_eq!(error.kind(), CoreGitFailureKind::Cancelled);
    assert!(cancelled_at.elapsed() < Duration::from_secs(2));
    assert!(
        !fixture.path().join("escaped-stopped").exists(),
        "the escaped holder must still be alive when cancellation returns"
    );
}

/// 同一个测试可执行文件充当受控逃逸后代；正常测试调用没有环境标记便立即返回。
#[test]
fn escaped_stdin_holder() {
    use std::os::fd::AsRawFd;
    let Some(directory) = std::env::var_os("NARRANT_ESCAPED_INPUT_FIXTURE") else {
        return;
    };
    let directory = PathBuf::from(directory);
    assert_ne!(unsafe { libc::setsid() }, -1);
    let null = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .unwrap();
    assert_ne!(
        unsafe { libc::dup2(null.as_raw_fd(), libc::STDOUT_FILENO) },
        -1
    );
    assert_ne!(
        unsafe { libc::dup2(null.as_raw_fd(), libc::STDERR_FILENO) },
        -1
    );
    std::fs::write(
        directory.join("escaped-ready.tmp"),
        std::process::id().to_string(),
    )
    .unwrap();
    std::fs::rename(
        directory.join("escaped-ready.tmp"),
        directory.join("escaped-ready"),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !directory.join("escaped-stop").exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    std::fs::write(directory.join("escaped-stopped"), "done").unwrap();
}

/// 即使断言失败也通知本 fixture 的后代退出，并等待其关闭继承的句柄。
struct EscapedInputCleanup(PathBuf);

impl Drop for EscapedInputCleanup {
    /// 使用专属 stop 文件清理，而不按可能已复用的 PID 发送信号。
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("escaped-stop"), "stop");
        let Ok(pid) = std::fs::read_to_string(self.0.join("escaped-ready")) else {
            return;
        };
        let Ok(pid) = pid.parse::<i32>() else {
            return;
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while unsafe { libc::kill(pid, 0) } == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// 仅 fixture 的 Git alias 需要 shell，路径按单引号规则转义而非插入原文。
fn shell_argument(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
