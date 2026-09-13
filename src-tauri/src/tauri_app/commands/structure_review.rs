use crate::{
    operation::{CancellableOperations, OperationId},
    review::ReviewLifecycle,
    structure_review::{
        code_graph::AnalysisOptions, StructureComparison, StructureReview, StructureReviewError,
    },
};

/// 前端提交固定 Snapshot 和分析范围；省略测试选项时默认排除明确的测试代码。
#[tauri::command]
pub(crate) async fn analyze_structure_review(
    lifecycle: tauri::State<'_, ReviewLifecycle>,
    structure: tauri::State<'_, StructureReview>,
    operations: tauri::State<'_, CancellableOperations>,
    operation_id: String,
    review_id: String,
    snapshot_id: String,
    include_tests: Option<bool>,
) -> Result<StructureComparison, StructureReviewError> {
    let operation = OperationId::new(operation_id).map_err(|error| StructureReviewError {
        code: "operation-failed",
        detail: error.to_string(),
    })?;
    operations
        .run(operation, |operation| async move {
            structure
                .analyze(
                    &lifecycle,
                    &operation,
                    &review_id,
                    &snapshot_id,
                    AnalysisOptions {
                        include_tests: include_tests.unwrap_or_default(),
                    },
                )
                .await
        })
        .await
        .map_err(|error| StructureReviewError {
            code: "cancelled",
            detail: error.to_string(),
        })?
}
