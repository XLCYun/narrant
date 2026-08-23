use crate::babysit::{
    BabysitApplication, BabysitEligibility, BabysitErrorPayload, BabysitFileExplanations,
    BabysitFileRequest, BabysitGenerationPolicy, BabysitProgress, ReviewIdentity,
};
use crate::operation::{self, OperationContext, OperationId};
use crate::review;
use std::future::Future;
use std::path::PathBuf;
use tauri::ipc::Channel;

/// Explain command 的后端 eligibility gate；通过前不得触碰 runtime/discovery。
fn validate_explain_input(
    babysit: &BabysitApplication,
    path: &str,
    content: &review::FileChangeContent,
    detected_language: Option<&str>,
) -> Result<(), BabysitErrorPayload> {
    babysit
        .validate_file_eligibility(path, content, detected_language)
        .map_err(BabysitErrorPayload::from)
}

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

/// 读取 immutable File Change，并由 Babysit 深模块返回唯一 eligibility 结论。
#[tauri::command]
pub(crate) async fn check_babysit_file_eligibility(
    lifecycle: tauri::State<'_, review::ReviewLifecycle>,
    babysit: tauri::State<'_, BabysitApplication>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
    snapshot_id: String,
    path: String,
    detected_language: Option<String>,
) -> Result<BabysitEligibility, BabysitErrorPayload> {
    run_babysit_command(&operations, operation_id, |operation| async move {
        let content = lifecycle
            .read_file_change(&operation, &review_id, &snapshot_id, &path)
            .await
            .map_err(|error| BabysitErrorPayload::input_unavailable(error.to_string()))?;
        Ok(babysit.eligibility(&path, &content, detected_language.as_deref()))
    })
    .await
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
    detected_language: Option<String>,
    generation_policy: BabysitGenerationPolicy,
    progress: Channel<BabysitProgress>,
) -> Result<BabysitFileExplanations, BabysitErrorPayload> {
    run_babysit_command(&operations, operation_id, |operation| async move {
        let content = lifecycle
            .read_file_change(&operation, &review_id, &snapshot_id, &path)
            .await
            .map_err(|error| BabysitErrorPayload::input_unavailable(error.to_string()))?;
        // 先通过后端权威 eligibility gate；不支持的文件不能触发 provider runtime I/O。
        validate_explain_input(&babysit, &path, &content, detected_language.as_deref())?;
        let workspace_path = lifecycle
            .workspace_path(&review_id, &snapshot_id)
            .map_err(|error| BabysitErrorPayload::input_unavailable(error.to_string()))?;
        // 只有用户已经点击宝宝模式并真正请求解释时，才进行 provider discovery/initialize。
        babysit
            .prepare_runtime(&workspace_path)
            .await
            .map_err(BabysitErrorPayload::from)?;
        babysit
            .explain_file(
                BabysitFileRequest {
                    identity: ReviewIdentity {
                        review_id,
                        snapshot_id,
                    },
                    path,
                    locale,
                    workspace_path: workspace_path.to_string_lossy().into_owned(),
                    detected_language,
                    generation_policy,
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

/// 接收 provider-aware native executable picker 的结果，先验证再持久化。
#[tauri::command]
pub(crate) async fn select_babysit_provider_executable(
    babysit: tauri::State<'_, BabysitApplication>,
    provider_id: String,
    executable: String,
) -> Result<(), BabysitErrorPayload> {
    babysit
        .select_provider_executable(&provider_id, &PathBuf::from(executable))
        .await
        .map_err(BabysitErrorPayload::from)
}

/// 兼容现有 Codex picker command；新 provider 使用通用 command。
#[tauri::command]
pub(crate) async fn select_babysit_codex_executable(
    babysit: tauri::State<'_, BabysitApplication>,
    executable: String,
) -> Result<(), BabysitErrorPayload> {
    babysit
        .select_codex_executable(&PathBuf::from(executable))
        .await
        .map_err(BabysitErrorPayload::from)
}

/// 刷新 Snapshot 后删除旧 identity 的所有进程内解释缓存。
#[tauri::command]
pub(crate) fn clear_babysit_snapshot_cache(
    babysit: tauri::State<'_, BabysitApplication>,
    review_id: String,
    snapshot_id: String,
) -> Result<(), BabysitErrorPayload> {
    babysit
        .clear_snapshot(&review_id, &snapshot_id)
        .map_err(BabysitErrorPayload::from)
}

/// 关闭或切换 Review 后删除该 Review 的所有进程内解释缓存。
#[tauri::command]
pub(crate) fn clear_babysit_review_cache(
    babysit: tauri::State<'_, BabysitApplication>,
    review_id: String,
) -> Result<(), BabysitErrorPayload> {
    babysit
        .clear_review(&review_id)
        .map_err(BabysitErrorPayload::from)
}

#[cfg(test)]
#[path = "tests/babysit.rs"]
mod tests;
