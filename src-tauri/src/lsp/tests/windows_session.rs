//! Windows 上经过真实 LspClient/Transport 的协议与进程回收回归测试。

use crate::lsp::{
    transport::Transport,
    windows_validation::{with_validation, CleanupFault, ValidationOptions},
    LspClient, LspOptions,
};
use serde_json::json;
use std::{
    fs, io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    },
};

const FIXTURE_SOURCE: &str = include_str!("fixtures/windows-session.cjs");

/// 将同源夹具写入本测试目录，独立 harness 不依赖宿主 Cargo manifest 的位置。
fn write_fixture(directory: &Path) -> PathBuf {
    let fixture = directory.join("windows-session.cjs");
    fs::write(&fixture, FIXTURE_SOURCE).expect("写入本测试的 Windows LSP 夹具");
    fixture
}

/// 只持有本次夹具的准确内核对象；断言失败时同步清理，避免测试遗留进程。
struct ObservedProcess(OwnedHandle);

impl ObservedProcess {
    /// PID 由原子发布的夹具标记提供，且服务器在保存句柄前不能退出。
    fn open(pid: u32) -> io::Result<Self> {
        // SAFETY: 只打开尚未放行的自有进程；查询权限仅检查 Job 归属，终止权限仅供兜底。
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
        // SAFETY: 成功的 OpenProcess 返回唯一所有权，后续不重新按 PID 查询。
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle) }))
    }

    /// 查询同一个 HANDLE，超时 0 不给清理路径增加额外退出时间。
    fn status(&self) -> u32 {
        // SAFETY: 同步句柄由当前对象持有，调用期间不会关闭。
        unsafe { WaitForSingleObject(self.0.as_raw_handle(), 0) }
    }
}

impl Drop for ObservedProcess {
    /// 兜底只在状态快照之后执行，不能让失败清理伪装成产品路径已完成。
    fn drop(&mut self) {
        if self.status() == WAIT_TIMEOUT {
            // SAFETY: 准确句柄仅指向本次自有夹具，不涉及 PID 重用或其他进程。
            unsafe {
                TerminateProcess(self.0.as_raw_handle(), 1);
                WaitForSingleObject(self.0.as_raw_handle(), 5_000);
            }
        }
    }
}

/// 在放行协议前一次性发布根进程及后代句柄，观察线程结束不影响句柄寿命。
struct ObservedProcesses {
    descendant: ObservedProcess,
    root: ObservedProcess,
}

/// 保存紧随清理返回的查询值，之后 join 或报错都无法掩盖 WAIT_TIMEOUT。
struct Completion {
    descendant: u32,
    root: u32,
}

/// 夹具控制目录和观察线程；字段顺序保证失败兜底先于临时目录释放。
struct SessionFixture {
    observed: Arc<OnceLock<ObservedProcesses>>,
    stop: Arc<AtomicBool>,
    observer: Option<JoinHandle<io::Result<()>>>,
    directory: tempfile::TempDir,
    fixture: PathBuf,
    node: PathBuf,
    scenario: &'static str,
}

impl SessionFixture {
    /// 先启动观察者；它必须把准确句柄发布到共享槽，才写 handles-ready 放行。
    fn new(scenario: &'static str, cancelled: Option<Arc<AtomicBool>>) -> Self {
        let node = node_executable();
        let directory = tempfile::tempdir().unwrap();
        let fixture = write_fixture(directory.path());
        let observed = Arc::new(OnceLock::new());
        let published = Arc::clone(&observed);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let control = directory.path().to_owned();
        let observer = std::thread::spawn(move || {
            let root = observe_process(&control.join("root-pid"), &stopping)?;
            let descendant = observe_process(&control.join("descendant-pid"), &stopping)?;
            published
                .set(ObservedProcesses { descendant, root })
                .map_err(|_| io::Error::other("夹具进程句柄只能发布一次"))?;
            publish(&control.join("handles-ready"), "saved")?;
            if let Some(cancelled) = cancelled {
                wait_for_marker(&control.join("request-pending"), &stopping)?;
                if scenario == "pending-parent-exits" {
                    let processes = published.get().expect("句柄已发布");
                    // SAFETY: 取消前证明根对象已经结束，后代仍持有真实输出管道。
                    let status =
                        unsafe { WaitForSingleObject(processes.root.0.as_raw_handle(), 5_000) };
                    if status != WAIT_OBJECT_0 {
                        return Err(io::Error::other("取消前父进程未按夹具约定退出"));
                    }
                }
                cancelled.store(true, Ordering::Release);
            }
            Ok(())
        });
        Self {
            observed,
            stop,
            observer: Some(observer),
            directory,
            fixture,
            node,
            scenario,
        }
    }

    /// Node 为仓库开发依赖；命令仅运行受控源码，stdio 完整交由真实 Transport 接管。
    fn command(&self) -> Command {
        let mut command = Command::new(&self.node);
        command
            .arg(&self.fixture)
            .env("NARRANT_LSP_WINDOWS_SESSION_DIR", self.directory.path())
            .env("NARRANT_LSP_WINDOWS_SESSION_SCENARIO", self.scenario);
        command
    }

    /// 初始化失败也可能已触发析构，因此调用方返回后首先采样，绝不先 join。
    fn completion(&self) -> Completion {
        let observed = self.observed.get().expect("夹具放行前必须保存进程句柄");
        Completion {
            descendant: observed.descendant.status(),
            root: observed.root.status(),
        }
    }

    /// 清理前检查测试时序：后代必须活着，父是否已退出由场景明确约定。
    fn assert_before_cleanup(&self, parent_exited: bool) {
        let observed = self.observed.get().expect("初始化前已保存句柄");
        assert_eq!(observed.descendant.status(), WAIT_TIMEOUT);
        assert_eq!(
            observed.root.status(),
            if parent_exited {
                WAIT_OBJECT_0
            } else {
                WAIT_TIMEOUT
            }
        );
    }

    /// 借用仍存活的真实 owner 验证准确归属，detached 叶进程不得逃出 Narrant Job。
    fn assert_owner_membership(&self, client: &LspClient) {
        let observed = self.observed.get().expect("初始化前已保存准确句柄");
        assert!(
            client.windows_owner_contains(&observed.root.0).unwrap(),
            "根进程必须属于当前会话的准确 Job"
        );
        assert!(
            client
                .windows_owner_contains(&observed.descendant.0)
                .unwrap(),
            "detached 叶进程必须仍属于当前会话的准确 Job"
        );
    }

    /// 只在完成状态已被同步采样后等待观察者，不让线程调度延迟帮助退出断言。
    fn join_observer(&mut self) {
        self.observer
            .take()
            .expect("观察线程只回收一次")
            .join()
            .expect("夹具观察线程不应 panic")
            .expect("夹具必须先发布准确句柄并完成同步");
    }
}

impl Drop for SessionFixture {
    /// 测试提前失败时停止文件等待，再交给 retained HANDLE 守卫回收自有进程。
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(observer) = self.observer.take() {
            let _ = observer.join();
        }
    }
}

/// CI 可固定 NARRANT_TEST_NODE；普通开发测试解析 PATH，缺少 Node 时明确失败。
fn node_executable() -> PathBuf {
    let executable = match std::env::var_os("NARRANT_TEST_NODE") {
        Some(path) => PathBuf::from(path),
        None => {
            let output = Command::new("node")
                .args(["--print", "process.execPath"])
                .output()
                .expect("Windows LSP 测试需要 Node：请运行 pnpm 安装开发环境，或设置 NARRANT_TEST_NODE 绝对路径");
            assert!(output.status.success(), "无法从 PATH 解析 Node 可执行文件");
            PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
        }
    };
    assert!(
        executable.is_absolute() && executable.is_file(),
        "NARRANT_TEST_NODE 必须指向 Node 可执行文件的绝对路径：{}",
        executable.display()
    );
    executable
}

/// 文件握手只负责确定先后关系，绝不作为进程已退出的证据。
fn wait_for_marker(path: &Path, stop: &AtomicBool) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if stop.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "夹具观察已停止"));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("等待夹具标记超时：{}", path.display()),
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

/// 只消费原子 rename 后的完整 PID 文件，且父、后代都在 handles-ready 前保活。
fn observe_process(path: &Path, stop: &AtomicBool) -> io::Result<ObservedProcess> {
    wait_for_marker(path, stop)?;
    let pid = fs::read_to_string(path)?
        .parse::<u32>()
        .map_err(io::Error::other)?;
    ObservedProcess::open(pid)
}

/// 写入私有临时文件再原子发布，避免夹具把半写入标记误认为放行许可。
fn publish(path: &Path, value: &str) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, value)?;
    fs::rename(temporary, path)
}

/// 普通握手为 Windows 冷启动预留余量，请求超时场景在握手后另行缩短。
fn options() -> LspOptions {
    LspOptions {
        request_timeout: Duration::from_secs(15),
        ..LspOptions::default()
    }
}

/// 断言的是清理后最先采样的值，期间不能重查句柄或轮询补等。
fn assert_completed(completion: Completion) {
    assert_eq!(completion.descendant, WAIT_OBJECT_0, "后代必须同步退出");
    assert_eq!(completion.root, WAIT_OBJECT_0, "根进程必须同步退出");
}

/// 同一个真实正常关闭场景可验证父主动退出和父仍存活两种完成时序。
fn run_normal_shutdown(scenario: &'static str) {
    let mut fixture = SessionFixture::new(scenario, None);
    let mut client =
        LspClient::start(fixture.command(), fixture.directory.path(), options()).unwrap();
    fixture.assert_owner_membership(&client);
    assert_eq!(client.capabilities(), &json!({}));
    assert_eq!(
        client
            .request("example/echo", json!({"echo":"中文😀"}))
            .unwrap(),
        json!({"echo":"中文😀"})
    );
    fixture.assert_before_cleanup(false);
    let result = client.shutdown();
    let completion = fixture.completion();
    fixture.join_observer();
    assert_completed(completion);
    result.unwrap();
    assert!(fixture.directory.path().join("shutdown-received").exists());
    assert!(fixture.directory.path().join("exit-received").exists());
}

/// 初始化自动失败路径在 start 返回时就必须完成根和后代的清理屏障。
fn run_initialization_failure(scenario: &'static str, expected_error: &str) {
    let mut fixture = SessionFixture::new(scenario, None);
    let result = LspClient::start(fixture.command(), fixture.directory.path(), options());
    let completion = fixture.completion();
    fixture.join_observer();
    assert_completed(completion);
    let error = match result {
        Ok(_) => panic!("无效初始化不应成功"),
        Err(error) => error,
    };
    assert!(!error.is_cancelled());
    assert!(error.message.contains(expected_error), "{error}");
}

/// 取消必须发生在 initialized 后实际请求在途时；超时也不得由 EOF 冒充。
fn run_pending_request(scenario: &'static str, cancel: bool) {
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut fixture = SessionFixture::new(scenario, cancel.then(|| Arc::clone(&cancelled)));
    let mut configured = options();
    let observed_cancel = Arc::clone(&cancelled);
    configured.cancelled = Arc::new(move || observed_cancel.load(Ordering::Acquire));
    let mut client =
        LspClient::start(fixture.command(), fixture.directory.path(), configured).unwrap();
    fixture.assert_owner_membership(&client);
    // 当前模块是 LspClient 的子模块；只缩短已完成握手的测试会话等待预算。
    client.request_timeout = Duration::from_secs(2);
    let started = Instant::now();
    let error = client.request("example/pending", json!({})).unwrap_err();
    assert!(fixture.directory.path().join("initialized").exists());
    assert!(fixture.directory.path().join("request-pending").exists());
    assert_eq!(error.is_cancelled(), cancel, "{error}");
    assert!(
        error.message.contains(if cancel { "取消" } else { "超时" }),
        "{error}"
    );
    if !cancel {
        assert!(error.message.contains("example/pending"));
    }
    fixture.assert_before_cleanup(scenario == "pending-parent-exits");
    drop(client);
    let completion = fixture.completion();
    fixture.join_observer();
    assert_completed(completion);
    assert!(started.elapsed() < Duration::from_secs(7));
}

/// 调用方 panic 经正常 Rust 展开触发 LspClient Drop，句柄快照必须立即完成。
fn run_panic_drop() {
    let mut fixture = SessionFixture::new("pending-parent-lives", None);
    let mut client =
        LspClient::start(fixture.command(), fixture.directory.path(), options()).unwrap();
    fixture.assert_owner_membership(&client);
    assert_eq!(
        client
            .request("example/echo", json!({"echo":"before panic"}))
            .unwrap(),
        json!({"echo":"before panic"})
    );
    fixture.assert_before_cleanup(false);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _client = client;
        panic!("有意测试调用方 panic 时的会话析构");
    }));
    let completion = fixture.completion();
    fixture.join_observer();
    assert_completed(completion);
    assert!(result.is_err());
}

/// 只观察默认生产所有者的实际启动次数，不选择或替换清理实现。
fn with_session_observation(run: impl FnOnce()) {
    with_validation(ValidationOptions::default(), |observations| {
        run();
        assert_eq!(observations.spawn_count(), 1);
    });
}

/// 单个真实会话跨八批构建后代仍可继续查询，短命对象不由测试额外保留句柄。
fn run_short_lived_batches(concurrency: u32) {
    let mut fixture = SessionFixture::new("batch-parent-lives", None);
    let startup = Instant::now();
    let mut client =
        LspClient::start(fixture.command(), fixture.directory.path(), options()).unwrap();
    let startup_elapsed = startup.elapsed();
    fixture.assert_owner_membership(&client);
    client.request_timeout = Duration::from_secs(30);
    let initial = client.windows_owner_snapshot().unwrap();
    let mut retained = initial.retained;
    let mut total = initial.total_processes;
    eprintln!(
        "Windows LSP batch initial: concurrency={concurrency}, retained={retained}, total={total}, active={}, known_live=2",
        initial.active
    );
    let batches_started = Instant::now();
    for batch in 1..=8 {
        let response = client
            .request(
                "example/batch",
                json!({"count":32,"concurrency":concurrency}),
            )
            .unwrap();
        assert_eq!(response["batch"], batch);
        assert_eq!(response["completed"], 32);
        assert_eq!(response["total"], batch * 32);
        assert_eq!(response["peakActive"], concurrency);
        assert_eq!(
            client
                .request("example/echo", json!({"echo":batch}))
                .unwrap(),
            json!({"echo":batch})
        );
        let snapshot = client.windows_owner_snapshot().unwrap();
        eprintln!(
            "Windows LSP batch snapshot: concurrency={concurrency}, batch={batch}, successful_short_children={}, retained={:?}, total={:?}, active={}, known_live=2",
            batch * 32, snapshot.retained, snapshot.total_processes, snapshot.active
        );
        total = snapshot.total_processes;
        // 只用实际成功创建的进程数作下界；不猜测额外累计项是否来自嵌套 Job 或其他进程。
        assert!(total >= 2 + 32 * batch, "真实累计数不能少于已知创建数");
        fixture.assert_before_cleanup(false);
        let current = snapshot.retained;
        assert!(current >= retained, "保留对象历史不应随批次结束而丢失");
        assert!(current <= total as usize, "保留对象数不能超过 Job 累计数");
        retained = current;
    }
    let batches_elapsed = batches_started.elapsed();
    fixture.assert_before_cleanup(false);
    let cleanup_started = Instant::now();
    let result = client.shutdown();
    let completion = fixture.completion();
    let cleanup_elapsed = cleanup_started.elapsed();
    fixture.join_observer();
    eprintln!(
        "Windows LSP batches: concurrency={concurrency}, batches=8, successful_short_children=256, known_created_objects=258, observed_total={total}, sampled_retained_peak={retained}, startup={startup_elapsed:?}, workload={batches_elapsed:?}, shutdown={cleanup_elapsed:?}, result={result:?}"
    );
    assert_completed(completion);
    result.unwrap();
}

/// 标准 shutdown/exit 后父先退出，继承管道的后代仍须同步回收。
#[test]
fn normal_shutdown_parent_exits() {
    run_normal_shutdown("normal-parent-exits");
}

/// 收到 exit 仍不主动退出的服务器和后代必须由共享所有者完成回收。
#[test]
fn normal_shutdown_parent_lives() {
    run_normal_shutdown("normal-parent-lives");
}

/// 父进程初始化阶段输出 EOF 时，未继承输出的存活后代同样不得遗留。
#[test]
fn initialization_eof() {
    run_initialization_failure("initialize-eof", "关闭了输出流");
}

/// 无效 JSON 帧使初始化失败时，仍存活且共享管道的父子均须完成清理。
#[test]
fn initialization_malformed() {
    run_initialization_failure("initialize-malformed", "initialize");
}

/// 父和后代都仍存活时，取消真实在途请求后析构会话。
#[test]
fn cancellation_parent_lives() {
    run_pending_request("pending-parent-lives", true);
}

/// 根对象先结束后才触发取消，覆盖后代继续持有 LSP 输出管道的情况。
#[test]
fn cancellation_parent_exits() {
    run_pending_request("pending-parent-exits", true);
}

/// 父和后代都不回应请求时，保留超时类别并在析构返回前回收。
#[test]
fn timeout_parent_lives() {
    run_pending_request("pending-parent-lives", false);
}

/// 父先退出但后代保持管道打开时，请求应真实超时并回收后代。
#[test]
fn timeout_parent_exits() {
    run_pending_request("pending-parent-exits", false);
}

/// 调用方异常展开也覆盖真实会话及继承管道的后代回收。
#[test]
fn panic_drop() {
    run_panic_drop();
}

/// 同一长期生产会话中连续处理八批串行短命后代。
#[test]
fn short_lived_batches_serial() {
    with_session_observation(|| run_short_lived_batches(1));
}

/// 同一长期生产会话中连续处理八批四路并发短命后代。
#[test]
fn short_lived_batches_parallel() {
    with_session_observation(|| run_short_lived_batches(4));
}

/// 有限比较 libuv 普通和 detached 子进程的累计差异，不推测额外累计项的身份。
#[test]
fn bounded_spawn_accounting_diagnostic() {
    with_session_observation(|| {
        let mut fixture = SessionFixture::new("diagnostic-parent-lives", None);
        let mut client =
            LspClient::start(fixture.command(), fixture.directory.path(), options()).unwrap();
        fixture.assert_owner_membership(&client);
        let initial = client.windows_owner_snapshot().unwrap();
        let mut previous_total = initial.total_processes;
        let mut previous_retained = initial.retained;
        let mut successful = 0;
        eprintln!("Windows LSP bounded diagnostic initial: {initial:?}");
        for (index, (count, detached)) in [(2, false), (2, true), (4, false), (4, true)]
            .into_iter()
            .enumerate()
        {
            let response = client
                .request(
                    "example/diagnostic-batch",
                    json!({"count":count,"detached":detached}),
                )
                .unwrap();
            successful += count;
            assert_eq!(response["batch"], index + 1);
            assert_eq!(response["completed"], count);
            assert_eq!(response["total"], successful);
            assert_eq!(
                client
                    .request("example/echo", json!({"echo":index}))
                    .unwrap(),
                json!({"echo":index})
            );
            let snapshot = client.windows_owner_snapshot().unwrap();
            let total = snapshot.total_processes;
            let retained = snapshot.retained;
            eprintln!(
                "Windows LSP bounded diagnostic: count={count}, detached={detached}, successful_short_children={successful}, retained={retained}, total={total}, active={}, total_delta={}, retained_delta={}, known_live=2",
                snapshot.active,
                i64::from(total) - i64::from(previous_total),
                retained as i64 - previous_retained as i64
            );
            assert!(total >= 2 + successful);
            assert!(total >= previous_total);
            assert!(retained >= previous_retained);
            assert!(retained <= total as usize);
            fixture.assert_before_cleanup(false);
            previous_total = total;
            previous_retained = retained;
        }
        let started = Instant::now();
        let result = client.shutdown();
        let completion = fixture.completion();
        let elapsed = started.elapsed();
        fixture.join_observer();
        eprintln!(
            "Windows LSP bounded diagnostic cleanup: total={previous_total}, sampled_retained_peak={previous_retained}, shutdown={elapsed:?}, result={result:?}"
        );
        assert_completed(completion);
        result.unwrap();
    });
}

/// 清理在终止前报错也必须先回收直接子进程，不能靠随后 Drop 的重试掩盖早退。
#[test]
fn cleanup_error_reaps_direct_child() {
    with_validation(
        ValidationOptions {
            fault: CleanupFault::BeforeTerminationOnce,
            ..ValidationOptions::default()
        },
        |observations| {
            let directory = tempfile::tempdir().unwrap();
            let fixture = write_fixture(directory.path());
            let mut command = Command::new(node_executable());
            command
                .arg(fixture)
                .arg("--root-only")
                .env("NARRANT_LSP_WINDOWS_SESSION_DIR", directory.path())
                .env("NARRANT_LSP_WINDOWS_SESSION_SCENARIO", "root-only");
            let mut transport = Transport::start(command).unwrap();
            let roots = observations.roots();
            assert_eq!(roots.len(), 1);
            wait_for_marker(&directory.path().join("root-pid"), &AtomicBool::new(false)).unwrap();
            let result = transport.finish();
            // SAFETY: 根句柄由 spawn 直接克隆；返回后第一步查询且 transport 尚未 Drop。
            let immediate = unsafe { WaitForSingleObject(roots[0].as_raw_handle(), 0) };
            assert_eq!(
                immediate, WAIT_OBJECT_0,
                "Job 报错不能跳过 direct Child 回收"
            );
            assert!(result.is_err());
            let retried = transport.finish();
            // SAFETY: 重试之后仍查询同一个根对象，不补等也不提前析构 transport。
            let retried_immediate = unsafe { WaitForSingleObject(roots[0].as_raw_handle(), 0) };
            assert_eq!(retried_immediate, WAIT_OBJECT_0);
            assert!(retried.is_err(), "后续重试不能清除先前的清理失败");
            assert_eq!(observations.spawn_count(), 1);
        },
    );
}
