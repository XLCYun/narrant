use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::Notify;

#[tokio::test]
async fn cancellation_before_start_prevents_the_operation_body() {
    let operations = CancellableOperations::default();
    let id = OperationId::new("operation-before-start").unwrap();
    let body_ran = Arc::new(AtomicBool::new(false));

    operations.cancel(&id).unwrap();
    let body_ran_for_operation = body_ran.clone();
    let error = operations
        .run(id, move |_| async move {
            body_ran_for_operation.store(true, Ordering::Release);
        })
        .await
        .unwrap_err();

    assert_eq!(error, OperationError::Cancelled);
    assert!(!body_ran.load(Ordering::Acquire));
}

#[tokio::test]
async fn duplicate_running_operation_is_rejected_without_replacing_the_first() {
    let operations = CancellableOperations::default();
    let id = OperationId::new("operation-duplicate").unwrap();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let first_operations = operations.clone();
    let first_id = id.clone();
    let first_started = started.clone();
    let first_release = release.clone();
    let first = tokio::spawn(async move {
        first_operations
            .run(first_id, move |_| async move {
                first_started.notify_one();
                first_release.notified().await;
                7
            })
            .await
    });
    started.notified().await;

    let error = operations.run(id, |_| async { 9 }).await.unwrap_err();
    release.notify_one();

    assert_eq!(error, OperationError::DuplicateOperation);
    assert_eq!(first.await.unwrap().unwrap(), 7);
}

#[tokio::test]
async fn running_operation_observes_cancellation_and_is_reusable_after_exit() {
    let operations = CancellableOperations::default();
    let id = OperationId::new("operation-running").unwrap();
    let started = Arc::new(Notify::new());
    let running_operations = operations.clone();
    let running_id = id.clone();
    let running_started = started.clone();
    let running = tokio::spawn(async move {
        running_operations
            .run(running_id, move |context| async move {
                running_started.notify_one();
                context.cancellation().cancelled().await;
                "cancelled"
            })
            .await
    });
    started.notified().await;

    operations.cancel(&id).unwrap();
    assert_eq!(running.await.unwrap().unwrap(), "cancelled");
    assert_eq!(
        operations.run(id, |_| async { "reused" }).await.unwrap(),
        "reused"
    );
}

#[tokio::test]
async fn cancel_all_notifies_every_running_operation() {
    let operations = CancellableOperations::default();
    let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut workers = Vec::new();
    for index in 0..2 {
        let worker_operations = operations.clone();
        let worker_started = started.clone();
        workers.push(tokio::spawn(async move {
            worker_operations
                .run(
                    OperationId::new(format!("operation-{index}")).unwrap(),
                    move |context| async move {
                        worker_started.fetch_add(1, Ordering::AcqRel);
                        context.cancellation().cancelled().await;
                    },
                )
                .await
        }));
    }
    while started.load(Ordering::Acquire) != 2 {
        tokio::task::yield_now().await;
    }

    operations.cancel_all().unwrap();

    for worker in workers {
        worker.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn close_and_drain_closes_admission_and_ignores_early_cancellations() {
    let operations = CancellableOperations::default();
    let early_id = OperationId::new("early-cancellation").unwrap();
    operations.cancel(&early_id).unwrap();

    operations.close_and_drain().await.unwrap();

    let error = operations
        .run(OperationId::new("after-close").unwrap(), |_| async {})
        .await
        .unwrap_err();
    assert_eq!(error, OperationError::Cancelled);
    assert_eq!(operations.cancel(&early_id), Err(OperationError::Cancelled));
}

#[tokio::test]
async fn close_and_drain_waits_for_every_running_registration_to_release() {
    let operations = CancellableOperations::default();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let running_operations = operations.clone();
    let running_started = started.clone();
    let running_release = release.clone();
    let running = tokio::spawn(async move {
        running_operations
            .run(
                OperationId::new("drained-operation").unwrap(),
                move |context| async move {
                    running_started.notify_one();
                    context.cancellation().cancelled().await;
                    running_release.notified().await;
                },
            )
            .await
    });
    started.notified().await;

    let draining_operations = operations.clone();
    let mut draining = tokio::spawn(async move { draining_operations.close_and_drain().await });
    tokio::task::yield_now().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut draining)
            .await
            .is_err()
    );

    release.notify_one();
    running.await.unwrap().unwrap();
    draining.await.unwrap().unwrap();
}

#[tokio::test]
async fn async_cancellation_wakes_managed_timer_and_drain_closes_future_admission() {
    let operations = CancellableOperations::default();
    let running_operations = operations.clone();
    let started = Arc::new(Notify::new());
    let running_started = started.clone();
    let timer = tokio::spawn(async move {
        running_operations
            .run(
                OperationId::new("application-update:periodic:1").unwrap(),
                move |context| async move {
                    running_started.notify_one();
                    context.cancellation().cancelled().await;
                },
            )
            .await
    });
    started.notified().await;

    operations.close_and_drain().await.unwrap();

    timer.await.unwrap().unwrap();
    assert_eq!(
        operations
            .run(
                OperationId::new("application-update:check:2").unwrap(),
                |_| async {}
            )
            .await,
        Err(OperationError::Cancelled)
    );
}

#[test]
fn background_context_starts_uncancelled() {
    assert!(!OperationContext::background().cancellation().is_cancelled());
}

#[test]
fn derived_convergence_context_ignores_parent_cancellation() {
    let cancellation = CancellationToken::new();
    let operation = OperationContext::new(cancellation.clone());
    let convergence = operation.without_cancellation();

    cancellation.cancel();

    assert!(operation.cancellation().is_cancelled());
    assert!(!convergence.cancellation().is_cancelled());
}

/// 子操作直接接收两级父取消的唤醒；子取消不影响父级与兄弟，已取消父级的后续子操作也立即取消。
#[tokio::test]
async fn child_context_propagates_cancellation_downward_without_polling_timers() {
    for cancel_parent in [false, true] {
        let root = OperationContext::background();
        let child = root.child();
        let grandchild = child.child();
        let sibling = root.child();
        let wake = Arc::new(WakeCount::default());
        let waker = futures_util::task::waker(wake.clone());
        let mut context = std::task::Context::from_waker(&waker);
        let mut waiting = Box::pin(grandchild.cancellation().cancelled());
        assert!(waiting.as_mut().poll(&mut context).is_pending());
        if cancel_parent {
            root.cancellation().cancel();
        } else {
            child.cancellation().cancel();
        }
        assert!(
            wake.0.load(Ordering::Acquire) > 0,
            "父取消必须直接唤醒后代等待者"
        );
        assert!(waiting.as_mut().poll(&mut context).is_ready());
        assert!(child.cancellation().is_cancelled());
        assert_eq!(root.cancellation().is_cancelled(), cancel_parent);
        assert_eq!(sibling.cancellation().is_cancelled(), cancel_parent);
        assert!(child.child().cancellation().is_cancelled());
    }
}

/// 用唤醒计数验证异步通知，不通过真实时间等待掩盖轮询实现。
#[derive(Default)]
struct WakeCount(std::sync::atomic::AtomicUsize);
impl futures_util::task::ArcWake for WakeCount {
    /// 保存运行时会收到的唤醒，供公开信号行为断言。
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.0.fetch_add(1, Ordering::Release);
    }
}
