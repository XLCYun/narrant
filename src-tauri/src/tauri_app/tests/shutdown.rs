use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// 记录停止步骤并允许测试暂停 operation drain 的确定性替身。
#[derive(Default)]
struct RecordingStopSteps {
    calls: Mutex<Vec<&'static str>>,
    operations_started: Notify,
    operations_release: Notify,
    block_operations: bool,
    fail_babysit: bool,
}

/// 记录最终退出次数与共享停止结果的可注入 adapter。
struct RecordingFinalExit {
    shutdown: Arc<ApplicationShutdown>,
    exit_count: AtomicUsize,
    outcome: Mutex<Option<ApplicationStopOutcome>>,
}

impl RecordingFinalExit {
    /// 创建绑定被测退出协调器的记录 adapter。
    fn new(shutdown: Arc<ApplicationShutdown>) -> Self {
        Self {
            shutdown,
            exit_count: AtomicUsize::new(0),
            outcome: Mutex::new(None),
        }
    }

    /// 返回最终退出接收到的停止结果。
    fn outcome(&self) -> Option<ApplicationStopOutcome> {
        *self.outcome.lock().unwrap()
    }
}

impl FinalExit for RecordingFinalExit {
    /// 模拟生产 adapter 标记最终阶段并记录一次退出。
    fn exit(&self, outcome: ApplicationStopOutcome) {
        self.shutdown.mark_final_exit();
        *self.outcome.lock().unwrap() = Some(outcome);
        self.exit_count.fetch_add(1, Ordering::AcqRel);
    }
}

impl RecordingStopSteps {
    /// 创建不阻塞且全部成功的步骤替身。
    fn successful() -> Self {
        Self::default()
    }

    /// 创建会停在 cancel/drain 步骤的替身。
    fn blocking() -> Self {
        Self {
            block_operations: true,
            ..Self::default()
        }
    }

    /// 创建 Babysit 报错但允许后续步骤继续的替身。
    fn failing_babysit() -> Self {
        Self {
            fail_babysit: true,
            ..Self::default()
        }
    }

    /// 返回已经执行过的稳定步骤顺序。
    fn recorded_calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ApplicationStopSteps for RecordingStopSteps {
    /// 记录 operation drain，并在需要时等待测试放行。
    async fn close_operations(&self) -> Result<(), String> {
        self.calls.lock().unwrap().push("operations");
        self.operations_started.notify_one();
        if self.block_operations {
            self.operations_release.notified().await;
        }
        Ok(())
    }

    /// 记录 Babysit shutdown 并按 fixture 返回结果。
    async fn shutdown_babysit(&self) -> Result<(), String> {
        self.calls.lock().unwrap().push("babysit");
        if self.fail_babysit {
            Err("babysit failed".to_owned())
        } else {
            Ok(())
        }
    }

    /// 记录 Review shutdown。
    async fn shutdown_review(&self) -> Result<(), String> {
        self.calls.lock().unwrap().push("review");
        Ok(())
    }

    /// 记录 telemetry shutdown。
    async fn shutdown_telemetry(&self) -> Result<(), String> {
        self.calls.lock().unwrap().push("telemetry");
        Ok(())
    }
}

#[tokio::test]
async fn successful_stop_runs_steps_in_contract_order_and_is_installable() {
    let shutdown = ApplicationShutdown::with_deadline(Duration::from_secs(1));
    let steps = Arc::new(RecordingStopSteps::successful());

    let outcome = shutdown
        .stop_once_with_steps(ApplicationStopIntent::UpdateRestart, &steps)
        .await;

    assert_eq!(outcome, ApplicationStopOutcome::Completed);
    assert!(outcome.allows_update_installation());
    assert_eq!(
        steps.recorded_calls(),
        vec!["operations", "babysit", "review", "telemetry"]
    );
}

#[tokio::test]
async fn failed_step_keeps_later_cleanup_but_disallows_installation() {
    let shutdown = ApplicationShutdown::with_deadline(Duration::from_secs(1));
    let steps = Arc::new(RecordingStopSteps::failing_babysit());

    let outcome = shutdown
        .stop_once_with_steps(ApplicationStopIntent::OrdinaryExit, &steps)
        .await;

    assert_eq!(outcome, ApplicationStopOutcome::Failed);
    assert!(!outcome.allows_update_installation());
    assert_eq!(
        steps.recorded_calls(),
        vec!["operations", "babysit", "review", "telemetry"]
    );
}

#[tokio::test]
async fn concurrent_and_repeated_callers_share_one_outcome() {
    let shutdown = Arc::new(ApplicationShutdown::with_deadline(Duration::from_secs(1)));
    let steps = Arc::new(RecordingStopSteps::blocking());
    let first_shutdown = shutdown.clone();
    let first_steps = steps.clone();
    let first = tokio::spawn(async move {
        first_shutdown
            .stop_once_with_steps(ApplicationStopIntent::OrdinaryExit, &first_steps)
            .await
    });
    steps.operations_started.notified().await;
    let second_shutdown = shutdown.clone();
    let second_steps = steps.clone();
    let second = tokio::spawn(async move {
        second_shutdown
            .stop_once_with_steps(ApplicationStopIntent::UpdateRestart, &second_steps)
            .await
    });

    steps.operations_release.notify_waiters();
    let first_outcome = first.await.unwrap();
    let second_outcome = second.await.unwrap();
    let repeated_outcome = shutdown
        .stop_once_with_steps(ApplicationStopIntent::UpdateRestart, &steps)
        .await;

    assert_eq!(first_outcome, ApplicationStopOutcome::Completed);
    assert_eq!(second_outcome, first_outcome);
    assert_eq!(repeated_outcome, first_outcome);
    assert_eq!(
        steps.recorded_calls(),
        vec!["operations", "babysit", "review", "telemetry"]
    );
}

#[tokio::test]
async fn aborting_initiator_does_not_restart_cleanup_or_its_deadline() {
    let shutdown = Arc::new(ApplicationShutdown::with_deadline(Duration::from_millis(
        20,
    )));
    let steps = Arc::new(RecordingStopSteps::blocking());
    let first_shutdown = shutdown.clone();
    let first_steps = steps.clone();
    let first = tokio::spawn(async move {
        first_shutdown
            .stop_once_with_steps(ApplicationStopIntent::OrdinaryExit, &first_steps)
            .await
    });
    steps.operations_started.notified().await;

    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    tokio::time::sleep(Duration::from_millis(30)).await;
    let outcome = tokio::time::timeout(
        Duration::from_millis(10),
        shutdown.stop_once_with_steps(ApplicationStopIntent::UpdateRestart, &steps),
    )
    .await
    .expect("原始停止任务应已按最初 deadline 发布结果");

    assert_eq!(outcome, ApplicationStopOutcome::TimedOut);
    assert_eq!(steps.recorded_calls(), vec!["operations"]);
}

#[tokio::test]
async fn timeout_runs_the_final_exit_seam_but_never_becomes_installable() {
    let shutdown = Arc::new(ApplicationShutdown::with_deadline(Duration::from_millis(
        20,
    )));
    let steps = Arc::new(RecordingStopSteps::blocking());
    let final_exit = Arc::new(RecordingFinalExit::new(shutdown.clone()));

    assert_eq!(
        shutdown.intercept_exit_request(),
        ExitRequestDecision::PreventAndStart
    );
    assert_eq!(
        shutdown.intercept_exit_request(),
        ExitRequestDecision::Prevent
    );
    let stop_shutdown = shutdown.clone();
    let stop_steps = steps.clone();
    let task = spawn_final_exit_with(
        async move {
            stop_shutdown
                .stop_once_with_steps(ApplicationStopIntent::OrdinaryExit, &stop_steps)
                .await
        },
        final_exit.clone(),
    );
    task.await.unwrap();

    let outcome = final_exit.outcome().unwrap();
    assert_eq!(outcome, ApplicationStopOutcome::TimedOut);
    assert!(!outcome.allows_update_installation());
    assert_eq!(final_exit.exit_count.load(Ordering::Acquire), 1);
    assert_eq!(
        shutdown.intercept_exit_request(),
        ExitRequestDecision::AllowFinalExit
    );
}

#[test]
fn repeated_exit_events_only_start_one_cleanup_task() {
    let shutdown = ApplicationShutdown::default();

    assert_eq!(
        shutdown.intercept_exit_request(),
        ExitRequestDecision::PreventAndStart
    );
    assert_eq!(
        shutdown.intercept_exit_request(),
        ExitRequestDecision::Prevent
    );
    assert_eq!(
        shutdown.intercept_exit_request(),
        ExitRequestDecision::Prevent
    );
}

#[test]
fn production_stop_boundary_uses_one_fifteen_second_deadline() {
    let shutdown = ApplicationShutdown::default();

    assert_eq!(shutdown.deadline, Duration::from_secs(15));
}

#[test]
fn completed_cleanup_runs_install_after_stop_in_order() {
    let events = Arc::new(Mutex::new(vec!["cleanup"]));
    let install_events = events.clone();

    let result = install_after_successful_stop(ApplicationStopOutcome::Completed, move || {
        install_events.lock().unwrap().push("install");
        "installed"
    });

    assert_eq!(result, Some("installed"));
    assert_eq!(*events.lock().unwrap(), vec!["cleanup", "install"]);
}

#[test]
fn timeout_and_failed_cleanup_skip_install_but_allow_existing_exit_path() {
    for outcome in [
        ApplicationStopOutcome::TimedOut,
        ApplicationStopOutcome::Failed,
    ] {
        let installs = AtomicUsize::new(0);
        let result = install_after_successful_stop(outcome, || {
            installs.fetch_add(1, Ordering::AcqRel);
        });
        assert!(result.is_none());
        assert_eq!(installs.load(Ordering::Acquire), 0);
        assert!(!outcome.allows_update_installation());
    }
}
