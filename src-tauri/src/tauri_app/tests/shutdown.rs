use super::*;
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
    let steps = RecordingStopSteps::successful();

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
    let steps = RecordingStopSteps::failing_babysit();

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
            .stop_once_with_steps(ApplicationStopIntent::OrdinaryExit, first_steps.as_ref())
            .await
    });
    steps.operations_started.notified().await;
    let second_shutdown = shutdown.clone();
    let second_steps = steps.clone();
    let second = tokio::spawn(async move {
        second_shutdown
            .stop_once_with_steps(ApplicationStopIntent::UpdateRestart, second_steps.as_ref())
            .await
    });

    steps.operations_release.notify_waiters();
    let first_outcome = first.await.unwrap();
    let second_outcome = second.await.unwrap();
    let repeated_outcome = shutdown
        .stop_once_with_steps(ApplicationStopIntent::UpdateRestart, steps.as_ref())
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
async fn timeout_still_allows_final_exit_but_never_installation() {
    let shutdown = ApplicationShutdown::with_deadline(Duration::from_millis(20));
    let steps = RecordingStopSteps::blocking();

    assert_eq!(
        shutdown.intercept_exit_request(),
        ExitRequestDecision::PreventAndStart
    );
    assert_eq!(
        shutdown.intercept_exit_request(),
        ExitRequestDecision::Prevent
    );
    let outcome = shutdown
        .stop_once_with_steps(ApplicationStopIntent::OrdinaryExit, &steps)
        .await;
    assert_eq!(outcome, ApplicationStopOutcome::TimedOut);
    assert!(!outcome.allows_update_installation());

    shutdown.mark_final_exit();
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
