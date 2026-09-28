use crate::language_support::{LanguageSupportPacks, PackCollection, PackId};

/// 查询已维护的本地快照，不检查远端更新或触发下载。
#[tauri::command]
pub(crate) fn language_support_snapshot(
    packs: tauri::State<'_, LanguageSupportPacks>,
) -> PackCollection {
    packs.snapshot()
}

/// 在异步运行时中登记重试，多个使用者共享后台任务。
#[tauri::command]
pub(crate) async fn language_support_retry(
    packs: tauri::State<'_, LanguageSupportPacks>,
    id: PackId,
) -> Result<PackCollection, String> {
    packs.retry(id)
}

/// 仅可删除已知且未被占用的包版本，调用方不能提交本地路径。
#[tauri::command]
pub(crate) async fn language_support_remove(
    packs: tauri::State<'_, LanguageSupportPacks>,
    id: PackId,
    version: String,
) -> Result<PackCollection, String> {
    let packs = packs.inner().clone();
    tokio::task::spawn_blocking(move || packs.remove(id, &version))
        .await
        .map_err(|error| error.to_string())?
}
