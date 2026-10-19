use super::super::*;
use std::{fs, time::Instant};

/// 伪 CLI 使用测试 Node 启动，所有消息仍经真实 stdio 与进程所有权边界。
fn fixture(
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
    fs::write(pack.path().join("control/codegraph.json"), r#"{"schema_version":1,"pack_id":"typescript-javascript","pack_version":"0.1.0","engine_version":"0.1.0","protocol":"codegraph.analyzer/1"}"#).unwrap();
    fs::write(
        pack.path().join(if cfg!(windows) {
            "bin/codegraph.exe"
        } else {
            "bin/codegraph"
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
        startup_timeout: Duration::from_secs(2),
        analysis_timeout: Duration::from_secs(2),
        cancellation_grace: Duration::from_millis(500),
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
    ] {
        let (_pack, _repository, analyzer, input) = fixture(scenario);
        let error = analyzer.analyze(&input).expect_err(scenario);
        assert!(!error.is_cancelled(), "{scenario}: {error}");
    }
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

/// 未 ready 的阻塞进程和运行中的协作取消都必须及时退出并报告取消。
#[test]
fn cancellation_and_startup_timeout_are_bounded() {
    for scenario in ["silent", "cooperate"] {
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
        .contains("启动超时"));
}

/// 后代即使建立新进程组，父 CLI 异常退出或取消也先完成后代屏障。
#[cfg(unix)]
#[test]
fn nested_descendants_are_reaped_before_return_on_cancel_and_outer_death() {
    for scenario in ["nested", "outer-death"] {
        let (pack, _repository, mut analyzer, input) = fixture(scenario);
        let marker = pack.path().join("pids");
        analyzer.test_environment.insert(
            "CODEGRAPH_TEST_PIDS".into(),
            marker.clone().into_os_string(),
        );
        let observed = marker.clone();
        let started = Instant::now();
        let cancel = scenario == "nested";
        analyzer = analyzer.with_cancellation(Arc::new(move || {
            cancel && observed.is_file() && started.elapsed() > Duration::from_millis(300)
        }));
        let error = analyzer.analyze(&input).unwrap_err();
        assert_eq!(error.is_cancelled(), cancel, "{error}");
        for pid in fs::read_to_string(marker)
            .unwrap()
            .lines()
            .map(|value| value.parse::<i32>().unwrap())
        {
            // SAFETY: 仅检查本次夹具记录的后代 PID，返回前已由 ProcessTree 完成回收。
            assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "后代仍存活：{pid}");
        }
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
    fs::write(pack.path().join("control/codegraph.json"), r#"{"schema_version":1,"pack_id":"typescript-javascript","pack_id":"typescript-javascript","pack_version":"0.1.0","engine_version":"0.1.0","protocol":"codegraph.analyzer/1"}"#).unwrap();
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
    for scenario in ["error-null", "error-cancel"] {
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
            error.message.contains("analysis_failed：fixture failure"),
            "{error}"
        );
    }
}
