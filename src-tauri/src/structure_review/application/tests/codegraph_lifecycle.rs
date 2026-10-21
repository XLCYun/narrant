use super::*;
use crate::structure_review::{code_graph::LanguageAnalyzer, codegraph_client::CodeGraphAnalyzer};
use std::collections::{BTreeMap, BTreeSet};

/// 查询本次真实 CLI 的后代；记录 PID 后先经过 owner 的轮询快照再触发退出。
fn descendants(root: i32) -> Vec<i32> {
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid="])
        .output()
        .unwrap();
    assert!(output.status.success());
    let entries = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((
                fields.next()?.parse::<i32>().ok()?,
                fields.next()?.parse::<i32>().ok()?,
            ))
        })
        .collect::<Vec<_>>();
    let mut owned = BTreeSet::from([root]);
    loop {
        let next = entries
            .iter()
            .filter_map(|(pid, parent)| owned.contains(parent).then_some(*pid))
            .collect::<Vec<_>>();
        let before = owned.len();
        owned.extend(next);
        if owned.len() == before {
            break;
        }
    }
    owned.into_iter().collect()
}

/// 真实 CLI 取消和异常退出都先回收嵌套 worker/helper，再释放实际 Review 工作区和包租约。
#[tokio::test]
#[ignore = "需要真实 TypeScript CodeGraph 产物，Unix PID 观察只在 macOS 验收"]
async fn real_cli_exit_and_cancellation_preserve_workspace_and_pack_barriers() {
    for terminate_cli in [false, true] {
        let fixture = Fixture::typescript_javascript().await;
        let artifacts = PathBuf::from(
            std::env::var_os("NARRANT_CODEGRAPH_ARTIFACTS").expect("真实 CodeGraph 产物"),
        );
        let server = httpmock::MockServer::start();
        crate::language_support::test_support::serve_artifacts(
            &server,
            &artifacts,
            "aarch64-apple-darwin",
        );
        let packs = crate::language_support::LanguageSupportPacks::with_test_origin(
            fixture._data.path().join("packs"),
            server.base_url(),
            "aarch64-apple-darwin".into(),
        )
        .unwrap();
        let application = StructureReview::new(Arc::new(|_, _, _| panic!("只使用 TypeScript")))
            .with_support_packs(packs.clone());
        let operation = OperationContext::background();
        let (_, lease) = application
            .prepare_analyzers(
                &fixture.lifecycle,
                &operation,
                &fixture.presentation.review_id,
                &fixture.presentation.snapshot_id,
                AnalysisOptions::default(),
            )
            .await
            .unwrap();
        let lease = lease.unwrap();
        let root = lease.path(PackId::TypeScriptJavascript).unwrap().to_owned();
        let marker = fixture._data.path().join("cli.pid");
        let launcher = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/structure_review/codegraph_client/tests/observe-cli.mjs");
        let analyzer = CodeGraphAnalyzer::from_pack("typescript-javascript", &root)
            .unwrap()
            .with_test_launcher(
                crate::child_process::test_support::node_executable(),
                vec![
                    launcher.into_os_string(),
                    root.join("bin/codegraph-typescript-javascript")
                        .into_os_string(),
                ],
                BTreeMap::from([(
                    "CODEGRAPH_OBSERVED_PID".into(),
                    marker.clone().into_os_string(),
                )]),
            );
        let lifecycle = fixture.lifecycle.clone();
        let review = fixture.presentation.review_id.clone();
        let snapshot = fixture.presentation.snapshot_id.clone();
        let worker = tokio::spawn(async move {
            lifecycle
                .run_in_workspace(&operation, &review, &snapshot, move |workspace| {
                    let _lease = lease;
                    let cancellation = workspace.operation.cancellation().clone();
                    analyzer
                        .with_cancellation(Arc::new(move || cancellation.is_cancelled()))
                        .analyze(&AnalysisInput {
                            repository_root: workspace.head_path.clone(),
                            files: vec!["main.ts".into(), "format-review.js".into()],
                            options: Default::default(),
                        })
                        .map_err(analysis_error)
                })
                .await
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let (cli, observed) = loop {
            if let Ok(value) = std::fs::read_to_string(&marker) {
                if let Ok(cli) = value.parse::<i32>() {
                    let owned = descendants(cli);
                    if owned.len() >= 3 {
                        break (cli, owned);
                    }
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "未观察到真实 CLI、worker 与 helper"
            );
            assert!(!worker.is_finished(), "真实分析在生命周期观察前结束");
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        // 留给 ProcessTree 的 10ms 轮询一次快照；仍不以此等待代替最终 PID 退出断言。
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(packs.remove(PackId::TypeScriptJavascript, "0.1.0").is_err());
        assert!(fixture.head.exists());
        if terminate_cli {
            // SAFETY: PID 来自本夹具启动的真实公共 CLI，且其 worker/helper 已被观察。
            assert_eq!(unsafe { libc::kill(cli, libc::SIGKILL) }, 0);
            let error = worker.await.unwrap().unwrap_err();
            assert_eq!(error.code, "analysis-failed", "{error:?}");
            fixture.lifecycle.shutdown().await.unwrap();
        } else {
            // 丢弃 IPC 等待后，实际 workspace worker 仍必须持有 lease 直到 shutdown 排空。
            worker.abort();
            fixture.lifecycle.shutdown().await.unwrap();
            assert!(worker.await.unwrap_err().is_cancelled());
        }
        for pid in observed {
            // SAFETY: 信号 0 只确认当前测试记录的 PID 已消失。
            assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "返回后仍有后代：{pid}");
        }
        assert!(!fixture.head.exists());
        assert!(!packs.snapshot().packs.iter().any(|pack| pack.in_use));
        packs.remove(PackId::TypeScriptJavascript, "0.1.0").unwrap();
        packs.shutdown().await;
    }
}
