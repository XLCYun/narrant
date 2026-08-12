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
                while !context.cancellation().is_cancelled() {
                    tokio::task::yield_now().await;
                }
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
