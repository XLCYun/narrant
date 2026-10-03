//! 固定 Windows ty 的真实 Session 生命周期；准确进程对象与目录清理分别验收。

use super::*;
use crate::{
    lsp::{
        file_uri,
        windows_validation::{with_owner, OwnerKind, ValidationOptions},
        Position,
    },
    structure_review::{
        code_graph::{AnalysisInput, CodeGraph, CodeNode, LanguageAnalyzer, ReferenceTarget},
        python_analyzer::PythonLanguageAnalyzer,
    },
};
use std::{
    os::windows::io::AsRawHandle,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::WaitForSingleObject,
};

const SOURCE: &str = "def service():\n    return 1\n\ndef run():\n    return service()\n";

/// 临时仓库只提供源码；服务配置、空 Python 环境与缓存仍由真实 Session 创建。
struct NativeSessionFixture {
    _repository: tempfile::TempDir,
    repository: PathBuf,
    executable: PathBuf,
    project: Project,
    source: PathBuf,
    uri: String,
}

impl NativeSessionFixture {
    /// 仅接受调用方显式提供的固定 ty 0.0.84，不搜索宿主 PATH 或安装解释器。
    fn new() -> Self {
        let executable = PathBuf::from(
            std::env::var_os("NARRANT_TY_PATH")
                .expect("Windows Session 原生验收需要固定 NARRANT_TY_PATH"),
        )
        .canonicalize()
        .unwrap();
        let version = Command::new(&executable).arg("--version").output().unwrap();
        assert!(version.status.success(), "固定 ty 无法报告版本");
        assert_eq!(
            String::from_utf8(version.stdout)
                .unwrap()
                .split_whitespace()
                .take(2)
                .collect::<Vec<_>>(),
            ["ty", "0.0.84"],
            "生命周期验收必须使用固定支持包版本"
        );
        let temporary = tempfile::tempdir().unwrap();
        let repository = temporary.path().canonicalize().unwrap();
        let source = repository.join("main.py");
        fs::write(&source, SOURCE).unwrap();
        let project = Project::for_file(&repository, &source, SOURCE).unwrap();
        let uri = file_uri(&source).unwrap();
        Self {
            _repository: temporary,
            repository,
            executable,
            project,
            source,
            uri,
        }
    }

    /// 启动真实服务并完成握手；取消标记在此期间保持 false，避免预检查短路。
    fn start(&self, cancelled: Arc<AtomicBool>) -> Session {
        let observed = cancelled.clone();
        let mut session = Session::start(
            &self.executable,
            &self.repository,
            &self.project,
            Duration::from_secs(30),
            Arc::new(move || observed.load(Ordering::Acquire)),
        )
        .unwrap();
        assert!(!cancelled.load(Ordering::Acquire));
        session
            .client()
            .open_document(&self.uri, "python", 1, SOURCE)
            .unwrap();
        session
    }

    /// 实际定义结果必须仍指向仓库内 service，不能把清理成功当作产品协议成功。
    fn assert_definition(&self, session: &mut Session) {
        let definitions = session
            .client()
            .definitions(&self.uri, call_position())
            .unwrap();
        assert_eq!(definitions.len(), 1, "{definitions:?}");
        assert_eq!(
            url::Url::parse(&definitions[0].uri)
                .unwrap()
                .to_file_path()
                .unwrap()
                .canonicalize()
                .unwrap(),
            self.source,
            "定义查询必须指回本次仓库源码"
        );
        assert_eq!(
            definitions[0].position,
            Position {
                line: 0,
                character: 4,
            }
        );
    }

    /// 控制目录释放后仓库仍只有未改写的原始源码，防止测试掩盖产品写入。
    fn assert_repository_unchanged(&self) {
        assert_eq!(fs::read_to_string(&self.source).unwrap(), SOURCE);
        let entries = fs::read_dir(&self.repository)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, [std::ffi::OsString::from("main.py")]);
    }
}

/// 使用调用表达式中的 UTF-16 列触发真实 definition 请求。
fn call_position() -> Position {
    Position {
        line: 4,
        character: 12,
    }
}

/// 通过公共图的包含关系定位声明，不依赖 Python 私有 AST 或中间态。
fn named<'a>(graph: &'a CodeGraph, name: &str) -> &'a CodeNode {
    let mut pending = graph.root().contains.clone();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap();
        if node.data().name.as_deref() == Some(name) {
            return node;
        }
        pending.extend(node.data().contains.iter().cloned());
    }
    panic!("公共图缺少 {name}")
}

/// 路径快照不持有目录所有权，保证删除只能由 Session 的真实生命周期完成。
struct SessionDirectories {
    control: PathBuf,
    cache: PathBuf,
}

impl SessionDirectories {
    /// 在服务运行时确认隔离目录存在并位于仓库外，保留准确的 cleanup 检查对象。
    fn capture(session: &Session, repository: &Path) -> Self {
        let control = session.control.path().to_owned();
        let cache = session.cache.clone();
        assert!(control.is_dir());
        assert!(cache.is_dir());
        assert!(!control.canonicalize().unwrap().starts_with(repository));
        assert!(cache.starts_with(&control));
        Self { control, cache }
    }

    /// 仅在 retained HANDLE 的零超时断言之后检查目录，避免文件系统调用掩盖退出竞态。
    fn assert_removed(&self) {
        for directory in [&self.control, &self.cache] {
            assert_eq!(
                fs::symlink_metadata(directory).unwrap_err().kind(),
                std::io::ErrorKind::NotFound,
                "Session 未释放目录：{}",
                directory.display()
            );
        }
    }
}

/// 相同真实请求分别走显式 owner，正常 shutdown 返回后第一项观测就是原根对象 Wait(0)。
fn normal_shutdown(owner: OwnerKind) {
    let fixture = NativeSessionFixture::new();
    with_owner(
        ValidationOptions {
            owner,
            ..Default::default()
        },
        |observations| {
            let mut session = fixture.start(Arc::new(AtomicBool::new(false)));
            assert_eq!(observations.spawn_count(), 1);
            assert_eq!(observations.owners(), [owner]);
            let mut roots = observations.roots();
            assert_eq!(roots.len(), 1);
            let root = roots.pop().unwrap();
            let root_raw = root.as_raw_handle();
            // SAFETY: 句柄来自本次成功启动的准确根对象，只进行零超时状态查询。
            assert_eq!(unsafe { WaitForSingleObject(root_raw, 0) }, WAIT_TIMEOUT);
            let directories = SessionDirectories::capture(&session, &fixture.repository);
            fixture.assert_definition(&mut session);

            let started = Instant::now();
            let shutdown = session.shutdown();
            // SAFETY: 保留句柄跨越 shutdown；在任何断言、日志或目录 I/O 前立即取样。
            let completed = unsafe { WaitForSingleObject(root_raw, 0) };
            let elapsed = started.elapsed();
            assert_eq!(
                completed, WAIT_OBJECT_0,
                "{owner:?}：Session shutdown 返回时根进程尚未完成；结果：{shutdown:?}"
            );
            shutdown.unwrap();
            directories.assert_removed();
            fixture.assert_repository_unchanged();
            eprintln!("Python Windows {owner:?} 正常 Session 清理：{elapsed:?}");
        },
    );
}

/// 目标请求已真实写入并 flush 后才取消；Drop 完成前不把 request 错误当作生命周期结束。
fn cancellation_after_request_sent(owner: OwnerKind) {
    let fixture = NativeSessionFixture::new();
    let cancelled = Arc::new(AtomicBool::new(false));
    let sent = Arc::new(AtomicUsize::new(0));
    let signal = cancelled.clone();
    let observed_sent = sent.clone();
    let after_request_sent = Arc::new(move |method: &str| {
        if method == "textDocument/definition" {
            observed_sent.fetch_add(1, Ordering::AcqRel);
            signal.store(true, Ordering::Release);
        }
    });
    with_owner(
        ValidationOptions {
            owner,
            after_request_sent: Some(after_request_sent),
            ..Default::default()
        },
        |observations| {
            let mut session = fixture.start(cancelled.clone());
            assert_eq!(observations.spawn_count(), 1);
            assert_eq!(observations.owners(), [owner]);
            let mut roots = observations.roots();
            assert_eq!(roots.len(), 1);
            let root = roots.pop().unwrap();
            let root_raw = root.as_raw_handle();
            // SAFETY: 握手和 didOpen 已完成，原根 HANDLE 必须仍未 signaled。
            assert_eq!(unsafe { WaitForSingleObject(root_raw, 0) }, WAIT_TIMEOUT);
            let directories = SessionDirectories::capture(&session, &fixture.repository);
            assert_eq!(sent.load(Ordering::Acquire), 0);
            assert!(!cancelled.load(Ordering::Acquire));

            let started = Instant::now();
            let result = session
                .client()
                .definitions(&fixture.uri, call_position())
                .map_err(lsp_failure);
            drop(session);
            // SAFETY: Drop 后第一项观测检查同一根对象，不经日志、轮询或目录查询延迟。
            let completed = unsafe { WaitForSingleObject(root_raw, 0) };
            let elapsed = started.elapsed();
            assert_eq!(
                completed, WAIT_OBJECT_0,
                "{owner:?}：取消后 Session Drop 返回时根进程尚未完成；结果：{result:?}"
            );
            assert_eq!(sent.load(Ordering::Acquire), 1);
            assert!(cancelled.load(Ordering::Acquire));
            let error = result.expect_err("实际请求发送后的取消必须返回错误");
            assert!(error.is_cancelled(), "{owner:?}：{error:?}");
            directories.assert_removed();
            fixture.assert_repository_unchanged();
            eprintln!("Python Windows {owner:?} 在途请求取消及清理：{elapsed:?}");
        },
    );
}

/// 旧共享路径的真实 ty 正常退出独立报告，避免其他 owner 的结果掩盖基线。
#[test]
#[ignore = "需要 Windows 固定 ty 0.0.84；显式 NARRANT_TY_PATH"]
fn native_windows_session_baseline_shutdown() {
    normal_shutdown(OwnerKind::Baseline);
}

/// 旧共享路径只在真实请求发送后取消，排除 preflight 的快速返回。
#[test]
#[ignore = "需要 Windows 固定 ty 0.0.84；显式 NARRANT_TY_PATH"]
fn native_windows_session_baseline_cancellation_after_send() {
    cancellation_after_request_sent(OwnerKind::Baseline);
}

/// 候选 owner 必须通过相同真实 ty 请求、返回即完成和目录删除合同。
#[test]
#[ignore = "需要 Windows 固定 ty 0.0.84；显式 NARRANT_TY_PATH"]
fn native_windows_session_candidate_shutdown() {
    normal_shutdown(OwnerKind::NativeCandidate);
}

/// 候选 owner 的取消验证同时保留 Python 错误类别与真实 Session Drop 路径。
#[test]
#[ignore = "需要 Windows 固定 ty 0.0.84；显式 NARRANT_TY_PATH"]
fn native_windows_session_candidate_cancellation_after_send() {
    cancellation_after_request_sent(OwnerKind::NativeCandidate);
}

/// 公共分析入口仍产生正确调用图，且同一线程内启动的真实 Session 确实选择候选 owner。
#[test]
#[ignore = "需要 Windows 固定 ty 0.0.84；显式 NARRANT_TY_PATH"]
fn native_windows_session_candidate_public_graph() {
    let fixture = NativeSessionFixture::new();
    with_owner(
        ValidationOptions {
            owner: OwnerKind::NativeCandidate,
            ..Default::default()
        },
        |observations| {
            let graph = PythonLanguageAnalyzer::new(&fixture.executable)
                .analyze(&AnalysisInput {
                    repository_root: fixture.repository.clone(),
                    files: vec![PathBuf::from("main.py")],
                    options: Default::default(),
                })
                .unwrap();
            assert_eq!(observations.spawn_count(), 1);
            assert_eq!(observations.owners(), [OwnerKind::NativeCandidate]);
            let service = named(&graph, "service");
            let run = named(&graph, "run");
            assert!(run.data().references.iter().any(|reference| {
                reference.semantic == "code.calls"
                    && reference.target == ReferenceTarget::Node(service.data().id.clone())
            }));
            fixture.assert_repository_unchanged();
        },
    );
}
