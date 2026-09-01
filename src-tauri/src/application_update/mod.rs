mod config;
mod diagnostics;
mod metadata;
mod platform;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "Task 3 接线前仅生产构建未消费端口；测试构建保持 lint 开启"
    )
)]
mod ports;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "Task 3 接线前仅生产构建未创建偏好仓储；测试构建保持 lint 开启"
    )
)]
mod preference;
mod runtime;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "Task 3 接线前仅生产构建未创建 staging；测试构建保持 lint 开启"
    )
)]
mod staging;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "Task 3 接线前仅生产构建未创建状态机；测试构建保持 lint 开启"
    )
)]
mod state;
mod verifier;

pub(crate) use config::TrustedUpdaterConfig;
#[cfg_attr(
    not(test),
    allow(unused_imports, reason = "Task 3 将消费 provider 与 installer 端口")
)]
pub(crate) use ports::{
    Clock, InstallError, InstallIntent, InstallOutcome, StableReleaseProvider, UpdateInstaller,
};
#[cfg_attr(not(test), allow(unused_imports, reason = "Task 3 将创建文件偏好仓储"))]
pub(crate) use preference::{FilePreferenceStore, PreferenceError, PreferenceStore};
pub(crate) use runtime::ApplicationUpdateRuntime;
#[allow(
    unused_imports,
    reason = "Task 3 的 adapter 将消费暂存结果与稳定错误分类；接线后删除"
)]
pub(crate) use staging::StagingError;
#[cfg_attr(
    not(test),
    allow(unused_imports, reason = "Task 3 将消费 staging 事务入口")
)]
pub(crate) use staging::{
    DurableStagedUpdate, InstallAttemptError, InstallCleanupDisposition, InstallExecutionOutcome,
    InstallKind, MinisignVerifier, PendingStagedUpdate, StageCandidate, StageCommitOutcome,
    StagingContext, StagingRepository, VerifiedStagedPackage,
};
#[cfg(test)]
pub(crate) use staging::{StagingCommitPoint, StagingFaultInjector};
#[cfg_attr(not(test), allow(unused_imports, reason = "Task 3 将消费状态机入口"))]
pub(crate) use state::{
    DownloadProgress, ReleasePackage, StableRelease, UpdateController, UpdateEffect, UpdateFailure,
    UpdateFailureCategory, UpdateInput,
};
#[allow(
    unused_imports,
    reason = "Task 3 的 runtime 与 typed command 将消费这些纯数据契约；接线后删除"
)]
pub(crate) use state::{
    UpdateCandidate, UpdateDomainError, UpdateFailureSnapshot, UpdateOperation, UpdateOperationId,
    UpdateOutput, UpdateSnapshot, UpdateState,
};

#[cfg_attr(
    not(test),
    allow(dead_code, reason = "Task 3 接线后由 Stable notes 路径直接消费")
)]
const MAX_RELEASE_NOTES_BYTES: usize = 32 * 1024;

/// 将 Release Notes 收敛到可持久化和跨边界投影的 UTF-8 字节上限。
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "Task 3 接线后由 Stable notes 路径直接消费")
)]
fn bounded_release_notes(notes: &str) -> String {
    if notes.len() <= MAX_RELEASE_NOTES_BYTES {
        return notes.to_owned();
    }
    let mut boundary = MAX_RELEASE_NOTES_BYTES;
    while !notes.is_char_boundary(boundary) {
        boundary -= 1;
    }
    notes[..boundary].to_owned()
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
