use super::{StableRelease, VerifiedStagedPackage};
use async_trait::async_trait;
use std::fmt;
use std::time::Duration;

/// 为更新调度提供可替换的单调时钟边界。
pub(crate) trait Clock {
    /// 返回自任意固定起点以来经过的单调时间。
    fn now(&self) -> Duration;
}

/// 从 Rust 持有的 Stable endpoint 读取目标平台元数据的网络边界。
#[async_trait]
pub(crate) trait StableReleaseProvider: Send + Sync {
    /// 返回当前目标的可信候选；没有严格更新时返回空值。
    async fn check_stable(&self) -> Result<Option<StableRelease>, super::UpdateFailure>;
}

/// 区分正常退出安装与用户主动请求的更新重启。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallIntent {
    /// 应用正常退出后安装，不要求安装器重新启动应用。
    NormalExit,
    /// 用户主动选择立即重启并更新。
    UpdateRestart,
}

/// 描述平台安装器接管后的稳定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallOutcome {
    /// 当前进程已完成原路径替换，可以清理暂存。
    Installed,
    /// 外部安装器已接管，需由新版本进程确认后再清理暂存。
    HandedOff,
}

/// 平台安装器边界的脱敏错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstallError {
    message: String,
}

impl InstallError {
    /// 创建不包含路径、签名或凭据的安装错误。
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// 返回可安全投影到诊断的错误文本。
    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for InstallError {
    /// 输出平台安装器提供的脱敏诊断。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// 在共享应用停止成功后消费已复验包的平台安装边界。
pub(crate) trait UpdateInstaller: Send + Sync {
    /// 在 staging 独占 lease 内消费包；HandedOff 仅能在外部进程已安全 open/launch 后返回。
    fn install(
        &self,
        package: &VerifiedStagedPackage,
        intent: InstallIntent,
    ) -> Result<InstallOutcome, InstallError>;
}
