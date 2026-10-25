use super::super::*;
use std::{fs, time::Instant};

/// 伪 CLI 使用测试 Node 启动，所有消息仍经真实 stdio 与进程所有权边界。
pub(in crate::structure_review::codegraph_client) fn fixture(
    scenario: &str,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    CodeGraphAnalyzer,
    AnalysisInput,
) {
    let pack = tempfile::tempdir().unwrap();
    fs::create_dir(pack.path().join("control")).unwrap();
    fs::create_dir(pack.path().join("bin")).unwrap();
    fs::write(pack.path().join("control/codegraph.json"), r#"{"schema_version":1,"pack_id":"typescript-javascript","pack_version":"0.1.1","engine_version":"0.1.1","protocol":"codegraph.analyzer/1"}"#).unwrap();
    fs::write(
        pack.path().join(if cfg!(windows) {
            "bin/codegraph-typescript-javascript.exe"
        } else {
            "bin/codegraph-typescript-javascript"
        }),
        "fixture",
    )
    .unwrap();
    let mut analyzer = CodeGraphAnalyzer::from_pack("typescript-javascript", pack.path()).unwrap();
    analyzer.executable = crate::child_process::test_support::node_executable();
    analyzer.test_prefix_args = vec![Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/structure_review/codegraph_client/tests/fake-cli.mjs")
        .into_os_string()];
    analyzer
        .test_environment
        .insert("CODEGRAPH_TEST_SCENARIO".into(), scenario.into());
    analyzer.limits = AnalysisLimits {
        analysis_timeout: Duration::from_secs(2),
        exit_timeout: Duration::from_secs(1),
        max_result_bytes: 2048,
        max_diagnostic_bytes: 2048,
    };
    let repository = tempfile::tempdir().unwrap();
    fs::write(
        repository.path().join("main.ts"),
        "function run(){return 1;}",
    )
    .unwrap();
    let input = AnalysisInput {
        repository_root: repository.path().into(),
        files: vec!["main.ts".into()],
        options: Default::default(),
    };
    (pack, repository, analyzer, input)
}

/// 公共消息必须保持精确身份、严格图字段、完整终结行及成功退出码。
#[test]
fn strict_public_protocol_rejects_invalid_messages_and_exits() {
    for scenario in [
        "protocol",
        "version",
        "pack",
        "id",
        "graph",
        "missing",
        "unexpected",
        "stderr",
        "oversize",
        "truncated",
        "duplicate",
        "extra",
        "exit",
        "cancelled",
        "error",
        "ready",
        "empty",
        "crash",
        "utf8",
        "nested-duplicate",
    ] {
        let (_pack, _repository, analyzer, input) = fixture(scenario);
        let error = analyzer.analyze(&input).expect_err(scenario);
        assert!(!error.is_cancelled(), "{scenario}: {error}");
    }
}

/// 错误封套的身份与 schema 先于后端失败消息校验，不能把拒收退化为普通分析错误。
#[test]
fn error_envelopes_reject_protocol_and_identity_before_backend_failure() {
    for (scenario, rejection) in [
        (
            "error-analysis-null",
            "CodeGraph 错误码、消息或请求 ID 无效",
        ),
        ("error-id", "completed 固定身份、请求 ID 或封套无效"),
        ("error-graph", "unknown field `graph`"),
        ("error-version", "completed 固定身份、请求 ID 或封套无效"),
    ] {
        let (_pack, _repository, analyzer, input) = fixture(scenario);
        let error = analyzer.analyze(&input).expect_err(scenario);
        assert!(!error.is_cancelled(), "{scenario}: {error}");
        assert!(error.message.contains(rejection), "{scenario}: {error}");
        assert!(
            !error.message.contains("fixture failure"),
            "{scenario}: {error}"
        );
    }
    let (_pack, _repository, analyzer, input) = fixture("error");
    let error = analyzer.analyze(&input).unwrap_err();
    assert!(
        error.message.contains("analysis_failed：fixture failure"),
        "{error}"
    );
    assert!(!error.is_cancelled());
}

/// 有效结果保留扩展与原文，普通空图和失败不能混淆。
#[test]
fn returns_validated_graph_without_losing_semantic_extensions() {
    let (_pack, _repository, analyzer, input) = fixture("valid");
    let graph = analyzer.analyze(&input).unwrap();
    let node = graph.node(&graph.root().contains[0]).unwrap().data();
    assert_eq!(node.source.as_deref(), Some("function run(){return 1;}"));
    assert_eq!(node.extensions["typescript.custom"]["preserved"], true);
}

/// 未读取输入与已收到正常 EOF 的阻塞进程都由调用方立即取消，不等待协作消息。
#[test]
fn cancellation_and_analysis_timeout_are_bounded() {
    for scenario in ["silent", "hang-after-input"] {
        let (_pack, _repository, analyzer, input) = fixture(scenario);
        let started = Instant::now();
        let error = analyzer
            .with_cancellation(Arc::new(move || {
                started.elapsed() > Duration::from_millis(200)
            }))
            .analyze(&input)
            .unwrap_err();
        assert!(error.is_cancelled(), "{scenario}: {error}");
        assert!(started.elapsed() < Duration::from_secs(3));
    }
    let (_pack, _repository, analyzer, input) = fixture("silent");
    assert!(analyzer
        .analyze(&input)
        .unwrap_err()
        .message
        .contains("分析超时"));
}

/// 测试持有真实进程身份；Windows 在调用返回前固定 HANDLE，避免 PID 重用误判。
#[cfg(unix)]
type Observed = i32;
#[cfg(windows)]
type Observed = crate::child_process::test_support::ObservedProcess;

/// 先保存仍存活的两层后代，再允许根退出；回调本身不提供进程清理。
fn observe_descendants(marker: &Path, saved: &Path, observed: &std::sync::Mutex<Vec<Observed>>) {
    let mut observed = observed.lock().unwrap();
    if !observed.is_empty() {
        return;
    }
    let Ok(values) = fs::read_to_string(marker) else {
        return;
    };
    for pid in values.lines().map(|value| value.parse::<u32>().unwrap()) {
        #[cfg(unix)]
        {
            // SAFETY: 标记来自正在等待本测试放行的真实夹具进程。
            assert_eq!(unsafe { libc::kill(pid as i32, 0) }, 0);
            observed.push(pid as i32);
        }
        #[cfg(windows)]
        {
            let process = Observed::open(pid).unwrap();
            assert_eq!(
                process.status(),
                windows_sys::Win32::Foundation::WAIT_TIMEOUT
            );
            observed.push(process);
        }
    }
    assert_eq!(observed.len(), 2);
    fs::write(saved, "observed").unwrap();
}

/// 正常根退出、继承管道、异常退出、取消和期限均先等待后代及管道，再清除 scratch。
#[test]
fn nested_descendants_are_reaped_before_return_on_cancel_and_outer_death() {
    for (scenario, cancel, success) in [
        ("nested", true, false),
        ("nested", false, false),
        ("outer-death", false, false),
        ("normal-orphan", false, true),
        ("normal-held-pipe", false, false),
        ("exit-held-pipe", false, false),
        ("abnormal-held-pipe", false, false),
    ] {
        let (pack, _repository, mut analyzer, input) = fixture(scenario);
        let marker = pack.path().join("pids");
        let saved = pack.path().join("saved");
        let scratch = pack.path().join("scratch");
        for (key, path) in [
            ("CODEGRAPH_TEST_PIDS", &marker),
            ("CODEGRAPH_TEST_SAVED", &saved),
            ("CODEGRAPH_TEST_SCRATCH", &scratch),
        ] {
            analyzer
                .test_environment
                .insert(key.into(), path.clone().into_os_string());
        }
        analyzer.limits.analysis_timeout = Duration::from_secs(2);
        analyzer.limits.exit_timeout = Duration::from_millis(200);
        let observed = Arc::new(std::sync::Mutex::new(Vec::<Observed>::new()));
        let shared = observed.clone();
        let started = Instant::now();
        analyzer = analyzer.with_cancellation(Arc::new(move || {
            observe_descendants(&marker, &saved, &shared);
            cancel && saved.is_file() && started.elapsed() > Duration::from_millis(200)
        }));
        let result = analyzer.analyze(&input);
        // 返回后的第一项进程观察不补等，不允许断言或清理守卫帮助后代退出。
        #[cfg(unix)]
        let completion: Vec<_> = observed
            .lock()
            .unwrap()
            .iter()
            .map(|pid| {
                // SAFETY: 仅查询本次已观察的自有后代；不向裸 PID 发送终止信号。
                unsafe { libc::kill(*pid, 0) }
            })
            .collect();
        #[cfg(windows)]
        let completion: Vec<_> = observed
            .lock()
            .unwrap()
            .iter()
            .map(|process| process.status())
            .collect();
        #[cfg(unix)]
        assert_eq!(completion, vec![-1, -1], "{scenario}: {result:?}");
        #[cfg(windows)]
        assert_eq!(
            completion,
            vec![windows_sys::Win32::Foundation::WAIT_OBJECT_0; 2],
            "{scenario}"
        );
        assert_eq!(result.is_ok(), success, "{scenario}: {result:?}");
        if let Err(error) = result {
            if scenario == "abnormal-held-pipe" {
                assert!(error.message.contains("异常退出"), "{error}");
            }
            assert_eq!(error.is_cancelled(), cancel, "{scenario}: {error}");
        }
        assert!(started.elapsed() < Duration::from_secs(3), "{scenario}");
        assert!(
            !PathBuf::from(fs::read_to_string(scratch).unwrap()).exists(),
            "scratch 必须在进程和管道退出后清除"
        );
    }
}

/// 重复或不同来源的包身份不能绕过公共描述符检查。
#[test]
fn descriptor_and_input_boundaries_reject_identity_and_path_ambiguity() {
    let (pack, _repository, analyzer, mut input) = fixture("valid");
    input.files.push("main.ts".into());
    assert!(analyzer
        .analyze(&input)
        .unwrap_err()
        .message
        .contains("重复"));
    input.files = vec!["main.ts".into(); 100_001];
    assert!(analyzer
        .analyze(&input)
        .unwrap_err()
        .message
        .contains("文件数量上限"));
    fs::write(pack.path().join("control/codegraph.json"), r#"{"schema_version":1,"pack_id":"typescript-javascript","pack_id":"typescript-javascript","pack_version":"0.1.1","engine_version":"0.1.1","protocol":"codegraph.analyzer/1"}"#).unwrap();
    assert!(CodeGraphAnalyzer::from_pack("typescript-javascript", pack.path()).is_err());
}

/// 接收方停止读取时，大请求也不能阻塞取消与退出屏障。
#[test]
fn unread_stdin_remains_cancellable() {
    let (_pack, _repository, analyzer, mut input) = fixture("unread-input");
    input.files = vec!["x".repeat(512 * 1024).into()];
    let started = Instant::now();
    let error = analyzer
        .with_cancellation(Arc::new(move || {
            started.elapsed() > Duration::from_millis(200)
        }))
        .analyze(&input)
        .unwrap_err();
    assert!(error.is_cancelled(), "{error}");
    assert!(started.elapsed() < Duration::from_secs(3));
}

/// 未接受请求的 null ID 和后续取消都不应抹掉真实公共错误。
#[test]
fn backend_error_survives_null_identity_and_later_cancellation() {
    for (scenario, code) in [
        ("error-null", "invalid_request"),
        ("error-request-id", "invalid_request"),
        ("error-protocol-null", "incompatible_protocol"),
        ("error-cancel", "analysis_failed"),
    ] {
        let (_pack, _repository, analyzer, input) = fixture(scenario);
        let started = Instant::now();
        let error = analyzer
            .with_cancellation(Arc::new(move || {
                started.elapsed() > Duration::from_millis(200)
            }))
            .analyze(&input)
            .unwrap_err();
        assert!(!error.is_cancelled(), "{error}");
        assert!(
            error.message.contains(&format!("{code}：fixture failure")),
            "{error}"
        );
    }
}

/// 诊断先于无结果 stdout EOF 时，也保留真正的启动失败原因。
#[test]
fn empty_stdout_preserves_diagnostics_received_before_eof() {
    let (_pack, _repository, analyzer, input) = fixture("empty-diagnostic");
    let error = analyzer.analyze(&input).unwrap_err();
    assert!(error.message.contains("fixture diagnostic"), "{error}");
    assert!(!error.is_cancelled());
}

/// 真实 Transport 的清理错误也必须先等待根 HANDLE，不能由随后 Drop 帮助断言通过。
#[cfg(windows)]
#[test]
fn cleanup_failure_retains_error_and_waits_root_before_return() {
    use crate::child_process::windows_validation::{
        with_validation, CleanupFault, ValidationOptions,
    };
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    let (_pack, _repository, analyzer, input) = fixture("silent");
    with_validation(
        ValidationOptions {
            fault: CleanupFault::BeforeTerminationOnce,
        },
        |observations| {
            let result = analyzer.analyze(&input);
            let immediate = observations.root_statuses();
            assert_eq!(immediate, vec![WAIT_OBJECT_0]);
            let error = result.unwrap_err();
            assert!(!error.is_cancelled());
            assert!(error.message.contains("清理失败"), "{error}");
        },
    );
}
