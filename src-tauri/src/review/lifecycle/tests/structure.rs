use super::*;
use crate::review::structure::{
    StructureAnalysisStatus, StructureEvent, StructureResult, StructureSide,
    StructureSourceContent, StructureSourceRequest,
};
use std::sync::mpsc;

/// 在公开进度回调中暂停真实分析，验证运行线程与生命周期隔离而不替换分析器。
struct StructureAnalysisPause {
    entered: tokio::sync::oneshot::Receiver<()>,
    release: Option<mpsc::Sender<()>>,
    events: Arc<Mutex<Vec<StructureEvent>>>,
}

impl StructureAnalysisPause {
    /// 等待首次已发布事件；若分析错误或同步运行阻塞 async worker，测试会超时。
    async fn wait(&mut self) {
        tokio::time::timeout(Duration::from_secs(10), &mut self.entered)
            .await
            .expect("结构分析未及时发布进度")
            .expect("结构分析在进度发布前退出");
    }

    /// 放行已经暂停的回调，让真实分析完成或观察取消、Snapshot 失效。
    fn release(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}

impl Drop for StructureAnalysisPause {
    /// 断言失败时也释放 worker，避免测试清理等待一条被遗忘的阻塞线程。
    fn drop(&mut self) {
        self.release();
    }
}

/// 构造含真实源码变化的已打开 Review，保留 README 作为独立普通阅读入口。
async fn open_structure_review() -> (LifecycleFixture, Arc<ReviewLifecycle>, ReviewPresentation) {
    let fixture = LifecycleFixture::new();
    std::fs::write(
        fixture.repository.path().join("example.ts"),
        "export function greet() { return 'hello'; }\n",
    )
    .unwrap();
    run_git(fixture.repository.path(), &["add", "example.ts"]);
    run_git(fixture.repository.path(), &["commit", "-q", "-m", "source"]);
    run_git(
        fixture.repository.path(),
        &["branch", "-f", "topic", "HEAD"],
    );
    let saved = fixture.save_current_review().await;
    fixture.store.activate_initial_snapshot(&saved).unwrap();
    let lifecycle = Arc::new(ReviewLifecycle::new(fixture.store.clone()));
    let presentation = lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: saved.review_id,
            },
        )
        .await
        .unwrap();
    (fixture, lifecycle, presentation)
}

/// 启动真实生命周期分析，并把首个公开事件作为确定性并发测试同步点。
fn pause_structure_analysis(
    lifecycle: Arc<ReviewLifecycle>,
    review: &ReviewPresentation,
    operation: OperationContext,
) -> (
    StructureAnalysisPause,
    tokio::task::JoinHandle<Result<StructureResult, ReviewLifecycleError>>,
) {
    let (entered, wait_entered) = tokio::sync::oneshot::channel();
    let (release, wait_release) = mpsc::channel();
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured_events = events.clone();
    let review_id = review.review_id.clone();
    let snapshot_id = review.snapshot_id.clone();
    let task = tokio::spawn(async move {
        let mut entered = Some(entered);
        lifecycle
            .analyze_structure(&operation, &review_id, &snapshot_id, move |event| {
                captured_events.lock().unwrap().push(event);
                if let Some(entered) = entered.take() {
                    let _ = entered.send(());
                    wait_release
                        .recv_timeout(Duration::from_secs(30))
                        .expect("测试必须释放结构分析回调");
                }
            })
            .await
    });
    (
        StructureAnalysisPause {
            entered: wait_entered,
            release: Some(release),
            events,
        },
        task,
    )
}

/// 单线程 async runtime 仍能在结构 worker 暂停时读取普通 File Change。
#[tokio::test]
async fn structure_analysis_keeps_file_changes_readable_while_worker_runs() {
    let (_fixture, lifecycle, review) = open_structure_review().await;
    let (mut pause, task) =
        pause_structure_analysis(lifecycle.clone(), &review, OperationContext::background());
    pause.wait().await;
    let content = tokio::time::timeout(
        Duration::from_secs(2),
        lifecycle.read_file_change(
            &OperationContext::background(),
            &review.review_id,
            &review.snapshot_id,
            "README.md",
        ),
    )
    .await
    .expect("结构分析不应持有普通文件阅读所需的锁")
    .unwrap();
    assert_eq!(
        content.after.as_ref().and_then(FileContent::text),
        Some("after\n")
    );
    pause.release();
    let result = task.await.unwrap().unwrap();
    assert_eq!(result.status, StructureAnalysisStatus::Completed);
    assert_eq!(result.review_id, review.review_id);
    assert_eq!(result.snapshot_id, review.snapshot_id);
}

/// 关闭后重开同一 Snapshot 不得重新承认旧任务，新的读取仍能使用该 Snapshot。
#[tokio::test]
async fn structure_results_from_a_closed_review_stay_stale_after_same_snapshot_reopen() {
    let (_fixture, lifecycle, review) = open_structure_review().await;
    let (mut pause, task) =
        pause_structure_analysis(lifecycle.clone(), &review, OperationContext::background());
    pause.wait().await;
    lifecycle.close(&review.review_id).unwrap();
    let reopened = lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: review.review_id.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(reopened.snapshot_id, review.snapshot_id);
    pause.release();
    assert!(matches!(
        task.await.unwrap(),
        Err(ReviewLifecycleError::StaleSnapshot)
    ));
    assert_eq!(
        pause.events.lock().unwrap().len(),
        1,
        "失效后不得继续发布旧事件"
    );

    let source = lifecycle
        .read_structure_source(
            &OperationContext::background(),
            &reopened.review_id,
            &reopened.snapshot_id,
            StructureSourceRequest {
                side: StructureSide::After,
                path: "example.ts".to_owned(),
            },
        )
        .await
        .unwrap();
    assert!(
        matches!(source.after, StructureSourceContent::Available { ref text, .. }
        if text == "export function greet() { return 'hello'; }\n")
    );
}

/// 刷新不等待旧结构分析，返回新 Snapshot 后旧进度与结果都不得被接收。
#[tokio::test]
async fn structure_analysis_cannot_publish_into_a_refreshed_snapshot() {
    let (fixture, lifecycle, review) = open_structure_review().await;
    let (mut pause, task) =
        pause_structure_analysis(lifecycle.clone(), &review, OperationContext::background());
    pause.wait().await;
    std::fs::write(
        fixture.repository.path().join("example.ts"),
        "export function greet() { return 'updated'; }\n",
    )
    .unwrap();
    run_git(
        fixture.repository.path(),
        &["commit", "-q", "-am", "update"],
    );
    run_git(
        fixture.repository.path(),
        &["branch", "-f", "topic", "HEAD"],
    );
    let update = lifecycle
        .check_update(&OperationContext::background(), &review.review_id)
        .await
        .unwrap();
    let refreshed = tokio::time::timeout(
        Duration::from_secs(10),
        lifecycle.refresh(
            &OperationContext::background(),
            &review.review_id,
            ConfirmedReviewUpdate {
                current_base_revision: update.current_base_revision,
                current_head_revision: update.current_head_revision,
                available_base_revision: update.available_base_revision,
                available_head_revision: update.available_head_revision,
            },
        ),
    )
    .await
    .expect("刷新不应等待旧结构分析")
    .unwrap();
    assert_ne!(refreshed.snapshot_id, review.snapshot_id);
    pause.release();
    assert!(matches!(
        task.await.unwrap(),
        Err(ReviewLifecycleError::StaleSnapshot)
    ));
    assert_eq!(pause.events.lock().unwrap().len(), 1);
    let fresh = lifecycle
        .analyze_structure(
            &OperationContext::background(),
            &refreshed.review_id,
            &refreshed.snapshot_id,
            |_| {},
        )
        .await
        .unwrap();
    assert_eq!(fresh.snapshot_id, refreshed.snapshot_id);
    assert_eq!(fresh.status, StructureAnalysisStatus::Completed);
}

/// 外部删除持久化 Repository 后，即使内存 runtime 尚在，旧工作也不能继续发布。
#[tokio::test]
async fn structure_analysis_rejects_repository_deleted_outside_the_open_runtime() {
    let (fixture, lifecycle, review) = open_structure_review().await;
    let (mut pause, task) =
        pause_structure_analysis(lifecycle.clone(), &review, OperationContext::background());
    pause.wait().await;
    // 当前应用没有删除入口；通过独立数据库连接模拟存储边界的真实 FK 级联删除。
    let external =
        rusqlite::Connection::open(fixture._application_data.path().join("store.sqlite3")).unwrap();
    external.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    external
        .execute(
            "DELETE FROM repositories WHERE id = ?1",
            [fixture.repository_id],
        )
        .unwrap();
    drop(external);
    pause.release();
    assert!(matches!(
        task.await.unwrap(),
        Err(ReviewLifecycleError::ReviewUnavailable(_))
    ));
    assert_eq!(pause.events.lock().unwrap().len(), 1);
    let source = lifecycle
        .read_structure_source(
            &OperationContext::background(),
            &review.review_id,
            &review.snapshot_id,
            StructureSourceRequest {
                side: StructureSide::After,
                path: "example.ts".to_owned(),
            },
        )
        .await;
    assert!(matches!(
        source,
        Err(ReviewLifecycleError::ReviewUnavailable(_))
    ));
}

/// 取消保留明确标记的部分结果；不再发布进行中进度，并允许同一 Snapshot 显式重试。
#[tokio::test]
async fn structure_cancellation_preserves_partial_status_and_allows_retry() {
    let (_fixture, lifecycle, review) = open_structure_review().await;
    let cancellation = crate::operation::TestCancellation::default();
    let (mut pause, task) = pause_structure_analysis(
        lifecycle.clone(),
        &review,
        OperationContext::from_test_cancellation(&cancellation),
    );
    pause.wait().await;
    cancellation.cancel();
    pause.release();
    let partial = task.await.unwrap().unwrap();
    assert_eq!(partial.status, StructureAnalysisStatus::Cancelled);
    assert!(!partial.summary.complete);
    assert!(pause.events.lock().unwrap()[1..].iter().all(|event| matches!(
        event,
        StructureEvent::Checkpoint { result } if result.status == StructureAnalysisStatus::Cancelled
    )));
    let retry = lifecycle
        .analyze_structure(
            &OperationContext::background(),
            &review.review_id,
            &review.snapshot_id,
            |_| {},
        )
        .await
        .unwrap();
    assert_eq!(retry.status, StructureAnalysisStatus::Completed);
}

/// 取消晚于引擎终态时仍保留已产生的结构，最终接收不得退回无结果的取消错误。
#[tokio::test]
async fn structure_cancellation_at_terminal_checkpoint_preserves_trustworthy_result() {
    let (_fixture, lifecycle, review) = open_structure_review().await;
    let cancellation = crate::operation::TestCancellation::default();
    let operation = OperationContext::from_test_cancellation(&cancellation);
    let completed_nodes = Arc::new(Mutex::new(None));
    let captured_nodes = completed_nodes.clone();
    let result = lifecycle
        .analyze_structure(
            &operation,
            &review.review_id,
            &review.snapshot_id,
            move |event| {
                if let StructureEvent::Checkpoint { result } = event {
                    assert_eq!(result.status, StructureAnalysisStatus::Completed);
                    *captured_nodes.lock().unwrap() = Some(result.nodes);
                    cancellation.cancel();
                }
            },
        )
        .await
        .unwrap();
    let checkpoint_nodes = completed_nodes
        .lock()
        .unwrap()
        .take()
        .expect("必须先收到引擎的真实终态");
    assert!(!checkpoint_nodes.is_empty());
    assert_eq!(result.nodes, checkpoint_nodes);
    assert_eq!(result.status, StructureAnalysisStatus::Cancelled);
    assert!(!result.summary.complete);
}

/// shutdown 尚在等待其他生命周期操作时，结构工作已经失效且不阻止退出收敛。
#[tokio::test]
async fn structure_analysis_stops_when_shutdown_starts_before_runtime_is_cleared() {
    let (_fixture, lifecycle, review) = open_structure_review().await;
    let (mut pause, task) =
        pause_structure_analysis(lifecycle.clone(), &review, OperationContext::background());
    pause.wait().await;
    let gate = Arc::new(LifecycleOperationGateBarrier::new());
    lifecycle
        .install_operation_gate_barrier(gate.clone())
        .unwrap();
    let present_lifecycle = lifecycle.clone();
    let review_id = review.review_id.clone();
    let present = tokio::spawn(async move {
        present_lifecycle
            .present(
                &OperationContext::background(),
                PresentReview::Saved { review_id },
            )
            .await
    });
    gate.wait_until_operation_paused().await;
    let shutdown_lifecycle = lifecycle.clone();
    let shutdown = tokio::spawn(async move { shutdown_lifecycle.shutdown().await });
    while !lifecycle.is_shutting_down() {
        tokio::task::yield_now().await;
    }
    pause.release();
    let result = tokio::time::timeout(Duration::from_secs(10), task).await;
    // 先释放已有操作，避免后续断言失败留下持有 read gate 的任务。
    gate.release_operation().await;
    present.await.unwrap().unwrap();
    shutdown.await.unwrap().unwrap();
    assert!(matches!(
        result.unwrap().unwrap(),
        Err(ReviewLifecycleError::ShuttingDown)
    ));
    assert_eq!(pause.events.lock().unwrap().len(), 1);
}
