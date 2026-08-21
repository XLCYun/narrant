use crate::babysit::{
    BabysitApplication, BabysitErrorPayload, BabysitFileExplanations, BabysitFileRequest,
    BabysitProgress, ReviewIdentity,
};
use crate::operation::{self, OperationContext, OperationId};
use crate::review;
use std::future::Future;
use tauri::ipc::Channel;

/// 运行 Babysit 专属 cancellable command，并保留结构化错误 payload。
async fn run_babysit_command<T, F, Fut>(
    operations: &operation::CancellableOperations,
    operation_id: String,
    run: F,
) -> Result<T, BabysitErrorPayload>
where
    F: FnOnce(OperationContext) -> Fut,
    Fut: Future<Output = Result<T, BabysitErrorPayload>>,
{
    let id = OperationId::new(operation_id).map_err(BabysitErrorPayload::from_operation)?;
    operations
        .run(id, run)
        .await
        .map_err(BabysitErrorPayload::from_operation)?
}

/// 按 immutable Review identity 读取完整 File Change，并运行一次 Babysit provider batch。
#[tauri::command]
pub(crate) async fn explain_review_file_change(
    lifecycle: tauri::State<'_, review::ReviewLifecycle>,
    babysit: tauri::State<'_, BabysitApplication>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
    snapshot_id: String,
    path: String,
    locale: String,
    progress: Channel<BabysitProgress>,
) -> Result<BabysitFileExplanations, BabysitErrorPayload> {
    run_babysit_command(&operations, operation_id, |operation| async move {
        let content = lifecycle
            .read_file_change(&operation, &review_id, &snapshot_id, &path)
            .await
            .map_err(|error| BabysitErrorPayload::input_unavailable(error.to_string()))?;
        babysit
            .explain_file(
                BabysitFileRequest {
                    identity: ReviewIdentity {
                        review_id,
                        snapshot_id,
                    },
                    path,
                    locale,
                },
                content,
                operation.cancellation(),
                |event| {
                    progress
                        .send(event)
                        .map_err(|_| crate::babysit::BabysitError::ProgressClosed)
                },
            )
            .await
            .map_err(BabysitErrorPayload::from)
    })
    .await
}
