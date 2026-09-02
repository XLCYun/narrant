use super::BabysitProviderSetupFailure;
use crate::operation::OperationError;
use serde::Serialize;
use std::fmt;

/// Babysit 对外暴露的稳定失败类别。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BabysitError {
    UnsupportedFile {
        reason: String,
    },
    InvalidProviderOutput {
        detail: String,
    },
    RepairExhausted {
        detail: String,
    },
    /// 模型明确因固定输出预算结束；该失败不能进入格式修复流程。
    OutputLimit {
        limit: u64,
    },
    Cancelled,
    Provider {
        detail: String,
    },
    /// provider 失败附带经过白名单约束的诊断字段，不包含原始请求、响应或 endpoint。
    ProviderDiagnostic {
        detail: String,
        /// 诊断字段独立装箱，避免放大每一个 Babysit 失败返回值。
        diagnostic: Box<BabysitProviderDiagnostic>,
    },
    ProgressClosed,
    CacheUnavailable,
    /// 任意 Installed provider 共享的 setup failure；provider identity 不参与 enum 形状。
    ProviderSetup {
        provider_id: String,
        failure: BabysitProviderSetupFailure,
    },
    /// 保存的模型或思考强度不再被动态目录支持，需要修改设置。
    ModelPreferenceInvalid,
    /// 模型偏好已通过能力验证，但统一设置文件无法原子保存。
    SettingsPersistenceFailed,
}

/// 允许跨 Tauri IPC 与剪贴板传递的 Custom provider 脱敏诊断。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BabysitProviderDiagnostic {
    provider_id: String,
    protocol: String,
    phase: String,
    status: String,
    reason: String,
    error_category: String,
}

impl BabysitProviderDiagnostic {
    /// 从 Custom transport 的稳定枚举和值构造白名单诊断。
    pub(crate) fn custom_transport(
        protocol: impl Into<String>,
        phase: impl Into<String>,
        status: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            provider_id: "custom".to_owned(),
            protocol: protocol.into(),
            phase: phase.into(),
            status: status.into(),
            reason: reason.into(),
            error_category: "transport".to_owned(),
        }
    }
}

impl BabysitError {
    /// 以 provider identity 和通用分类构造稳定 setup failure。
    pub fn provider_setup(
        provider_id: impl Into<String>,
        failure: BabysitProviderSetupFailure,
    ) -> Self {
        Self::ProviderSetup {
            provider_id: provider_id.into(),
            failure,
        }
    }

    /// 返回 setup failure 关联的稳定 provider identity；普通分析错误没有 identity。
    pub fn provider_id(&self) -> Option<&str> {
        match self {
            Self::ProviderSetup { provider_id, .. } => Some(provider_id),
            _ => None,
        }
    }
}

/// Babysit command 对前端暴露的稳定错误 code；detail 只承载当前失败的可读诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BabysitErrorCode {
    UnsupportedFile,
    InvalidProviderOutput,
    RepairExhausted,
    OutputLimit,
    Cancelled,
    Provider,
    ProgressClosed,
    CacheUnavailable,
    InputUnavailable,
    InvalidRequest,
    OperationConflict,
    OperationUnavailable,
    ProviderNotFound,
    ProviderExecutableInvalid,
    ProviderSignedOut,
    ProviderIncompatible,
    ProviderInitializationFailed,
    ModelPreferenceInvalid,
    SettingsPersistenceFailed,
}

/// 前端可以直接执行的最小恢复动作；不携带可执行路径或进程原文。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BabysitRecoveryAction {
    LoginProvider,
    UpgradeProvider,
    OpenApplicationSettings,
    Retry,
}

/// Tauri command 失败时保持 code/detail 结构，避免把 BabysitError 压扁为字符串。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BabysitErrorPayload {
    pub code: BabysitErrorCode,
    pub detail: String,
    /// setup failure 所属的稳定 provider identity；普通错误不填写。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<BabysitRecoveryAction>,
    /// 仅包含白名单分类的 provider 诊断；不得放入 detail、URL 或 provider body。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<Box<BabysitProviderDiagnostic>>,
}

impl BabysitErrorPayload {
    /// 将 immutable Review 读取失败映射为不依赖 provider 的输入错误。
    pub fn input_unavailable(_detail: impl Into<String>) -> Self {
        Self {
            code: BabysitErrorCode::InputUnavailable,
            // 下层 Review/Git 诊断可能含绝对路径；IPC detail 只保留稳定错误类别。
            detail: "Review File Change 内容不可用。".to_owned(),
            provider_id: None,
            recovery: None,
            diagnostic: None,
        }
    }

    /// 将 cancellable operation registry 的错误映射到稳定 Babysit error code。
    pub fn from_operation(error: OperationError) -> Self {
        let code = match &error {
            OperationError::InvalidOperationId => BabysitErrorCode::InvalidRequest,
            OperationError::DuplicateOperation => BabysitErrorCode::OperationConflict,
            OperationError::Cancelled => BabysitErrorCode::Cancelled,
            OperationError::RegistryUnavailable => BabysitErrorCode::OperationUnavailable,
        };
        Self {
            code,
            detail: error.to_string(),
            provider_id: None,
            recovery: None,
            diagnostic: None,
        }
    }
}

impl From<BabysitError> for BabysitErrorPayload {
    /// 将领域错误投影为 provider-independent 的 Tauri payload。
    fn from(error: BabysitError) -> Self {
        let code = match &error {
            BabysitError::UnsupportedFile { .. } => BabysitErrorCode::UnsupportedFile,
            BabysitError::InvalidProviderOutput { .. } => BabysitErrorCode::InvalidProviderOutput,
            BabysitError::RepairExhausted { .. } => BabysitErrorCode::RepairExhausted,
            BabysitError::OutputLimit { .. } => BabysitErrorCode::OutputLimit,
            BabysitError::Cancelled => BabysitErrorCode::Cancelled,
            BabysitError::Provider { .. } | BabysitError::ProviderDiagnostic { .. } => {
                BabysitErrorCode::Provider
            }
            BabysitError::ProgressClosed => BabysitErrorCode::ProgressClosed,
            BabysitError::CacheUnavailable => BabysitErrorCode::CacheUnavailable,
            BabysitError::ProviderSetup { failure, .. } => setup_error_code(*failure),
            BabysitError::ModelPreferenceInvalid => BabysitErrorCode::ModelPreferenceInvalid,
            BabysitError::SettingsPersistenceFailed => BabysitErrorCode::SettingsPersistenceFailed,
        };
        let recovery = match &error {
            BabysitError::ProviderSetup { failure, .. } => setup_recovery_action(*failure),
            BabysitError::ModelPreferenceInvalid => None,
            BabysitError::SettingsPersistenceFailed => None,
            BabysitError::RepairExhausted { .. }
            | BabysitError::OutputLimit { .. }
            | BabysitError::Provider { .. }
            | BabysitError::ProviderDiagnostic { .. }
            | BabysitError::InvalidProviderOutput { .. }
            | BabysitError::Cancelled
            | BabysitError::ProgressClosed
            | BabysitError::CacheUnavailable
            | BabysitError::UnsupportedFile { .. } => None,
        };
        let diagnostic = match &error {
            BabysitError::ProviderDiagnostic { diagnostic, .. } => Some(diagnostic.clone()),
            _ => None,
        };
        Self {
            code,
            detail: error.to_string(),
            provider_id: error.provider_id().map(str::to_owned),
            recovery,
            diagnostic,
        }
    }
}

impl fmt::Display for BabysitError {
    /// 将领域错误转换为可读诊断；稳定分支由 BabysitErrorCode 承担。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedFile { reason } => write!(formatter, "此文件不支持宝宝模式：{reason}"),
            Self::InvalidProviderOutput { detail } => {
                write!(formatter, "宝宝模式返回格式无效：{detail}")
            }
            Self::RepairExhausted { detail } => write!(formatter, "宝宝模式连续修复失败：{detail}"),
            Self::OutputLimit { limit } => {
                write!(formatter, "宝宝模式模型输出已达到固定上限 {limit}")
            }
            Self::Cancelled => formatter.write_str("宝宝模式分析已取消"),
            Self::Provider { detail } => write!(formatter, "宝宝模式 provider 失败：{detail}"),
            Self::ProviderDiagnostic { detail, .. } => {
                write!(formatter, "宝宝模式 provider 失败：{detail}")
            }
            Self::ProgressClosed => formatter.write_str("宝宝模式进度通道已关闭"),
            Self::CacheUnavailable => formatter.write_str("宝宝模式缓存不可用"),
            Self::ProviderSetup {
                provider_id,
                failure,
            } if provider_id == "custom" && *failure == BabysitProviderSetupFailure::NotFound => {
                formatter
                    .write_str("Custom provider 尚未配置；请在 Application Settings 中完成设置")
            }
            Self::ProviderSetup {
                provider_id,
                failure,
            } => match failure {
                BabysitProviderSetupFailure::NotFound => {
                    write!(formatter, "未找到 {provider_id} provider 可执行文件")
                }
                BabysitProviderSetupFailure::ExecutableInvalid => {
                    write!(formatter, "{provider_id} provider 可执行文件不可用")
                }
                BabysitProviderSetupFailure::SignedOut => {
                    write!(formatter, "{provider_id} provider 尚未登录")
                }
                BabysitProviderSetupFailure::Incompatible => {
                    write!(formatter, "{provider_id} provider 缺少宝宝模式所需能力")
                }
                BabysitProviderSetupFailure::InitializationFailed => {
                    write!(formatter, "{provider_id} provider 初始化失败")
                }
            },
            Self::ModelPreferenceInvalid => formatter
                .write_str("宝宝模式模型或思考强度已不可用；请在 Application Settings 中修改设置"),
            Self::SettingsPersistenceFailed => {
                formatter.write_str("宝宝模式模型设置保存失败；原设置保持不变")
            }
        }
    }
}

/// 将通用 setup failure 映射成前端稳定错误 code。
fn setup_error_code(failure: BabysitProviderSetupFailure) -> BabysitErrorCode {
    match failure {
        BabysitProviderSetupFailure::NotFound => BabysitErrorCode::ProviderNotFound,
        BabysitProviderSetupFailure::ExecutableInvalid => {
            BabysitErrorCode::ProviderExecutableInvalid
        }
        BabysitProviderSetupFailure::SignedOut => BabysitErrorCode::ProviderSignedOut,
        BabysitProviderSetupFailure::Incompatible => BabysitErrorCode::ProviderIncompatible,
        BabysitProviderSetupFailure::InitializationFailed => {
            BabysitErrorCode::ProviderInitializationFailed
        }
    }
}

/// 将通用 setup failure 映射成不依赖具体 provider 的恢复动作。
fn setup_recovery_action(failure: BabysitProviderSetupFailure) -> Option<BabysitRecoveryAction> {
    Some(match failure {
        BabysitProviderSetupFailure::NotFound | BabysitProviderSetupFailure::ExecutableInvalid => {
            BabysitRecoveryAction::OpenApplicationSettings
        }
        BabysitProviderSetupFailure::SignedOut => BabysitRecoveryAction::LoginProvider,
        BabysitProviderSetupFailure::Incompatible => BabysitRecoveryAction::UpgradeProvider,
        BabysitProviderSetupFailure::InitializationFailed => BabysitRecoveryAction::Retry,
    })
}

impl std::error::Error for BabysitError {}
