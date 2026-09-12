use crate::{
    operation::{CancellableOperations, OperationId},
    review::ReviewLifecycle,
    structure_review::{StructureComparison, StructureReview, StructureReviewError},
};

/// 前端仅提交固定 Review/Snapshot 身份，目录和语言服务器由应用 Module 编排。
#[tauri::command]
pub(crate) async fn analyze_structure_review(
    lifecycle: tauri::State<'_, ReviewLifecycle>,
    structure: tauri::State<'_, StructureReview>,
    operations: tauri::State<'_, CancellableOperations>,
    operation_id: String,
    review_id: String,
    snapshot_id: String,
) -> Result<StructureComparison, StructureReviewError> {
    let operation = OperationId::new(operation_id).map_err(|error| StructureReviewError {
        code: "operation-failed",
        detail: error.to_string(),
    })?;
    operations
        .run(operation, |operation| async move {
            structure
                .analyze(&lifecycle, &operation, &review_id, &snapshot_id)
                .await
        })
        .await
        .map_err(|error| StructureReviewError {
            code: "cancelled",
            detail: error.to_string(),
        })?
}
