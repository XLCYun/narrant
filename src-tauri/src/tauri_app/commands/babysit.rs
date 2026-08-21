use crate::babysit::{
    BabysitApplication, BabysitFileExplanations, BabysitFileRequest, BabysitProgress,
    ReviewIdentity,
};
use crate::operation;
use crate::review;
use tauri::ipc::Channel;

use super::review::run_cancellable_command;

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
) -> Result<BabysitFileExplanations, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        let content = lifecycle
            .read_file_change(&operation, &review_id, &snapshot_id, &path)
            .await
            .map_err(|error| error.to_string())?;
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
            .map_err(|error| error.to_string())
    })
    .await
}
