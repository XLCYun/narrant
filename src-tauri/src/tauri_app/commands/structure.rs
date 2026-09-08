use super::review::run_cancellable_command;
use crate::operation;
use crate::review::structure::{
    StructureEvent, StructurePeekContent, StructureResult, StructureSide, StructureSourceRequest,
};
use crate::review::ReviewLifecycle;

/// 对当前打开的固定 Snapshot 启动可取消结构分析，通过同一请求通道按序报告进度。
#[tauri::command]
pub(crate) async fn analyze_review_structure(
    lifecycle: tauri::State<'_, ReviewLifecycle>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
    snapshot_id: String,
    progress: tauri::ipc::Channel<StructureEvent>,
) -> Result<StructureResult, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .analyze_structure(&operation, &review_id, &snapshot_id, move |event| {
                let _ = progress.send(event);
            })
            .await
            .map_err(|error| error.to_string())
    })
    .await
}

/// 从固定 Snapshot 的真实路径读取完整文件对，独立于结构分析及普通 File Changes 选择。
#[tauri::command]
pub(crate) async fn read_review_structure_source(
    lifecycle: tauri::State<'_, ReviewLifecycle>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
    snapshot_id: String,
    side: StructureSide,
    path: String,
) -> Result<StructurePeekContent, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .read_structure_source(
                &operation,
                &review_id,
                &snapshot_id,
                StructureSourceRequest { side, path },
            )
            .await
            .map_err(|error| error.to_string())
    })
    .await
}

#[cfg(test)]
#[path = "tests/structure.rs"]
mod tests;
