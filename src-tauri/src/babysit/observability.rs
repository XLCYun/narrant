use super::{BabysitError, BabysitProviderSetupFailure};

#[cfg(feature = "dev-observability")]
use std::sync::Mutex;

/// Narrant 自己产生的 Babysit lifecycle telemetry target；模型与工具由 Rig 单独负责。
#[cfg(feature = "dev-observability")]
use crate::telemetry::LIFECYCLE_TARGET as TELEMETRY_TARGET;

/// 尚未完成 provider 解析时使用的安全上下文占位值。
#[cfg(feature = "dev-observability")]
const UNKNOWN_CONTEXT: &str = "unknown";

/// 一次 Babysit 分析生命周期的 feature-gated 观测句柄。
///
/// 默认构建保留同一调用 seam，但内部不创建 tracing span，也不引入 exporter 行为；
/// development-observability 构建才把安全字段写入 tracing subscriber。
pub(super) struct AnalysisLifecycle {
    #[cfg(feature = "dev-observability")]
    analysis_run_id: u64,
    #[cfg(feature = "dev-observability")]
    span: tracing::Span,
    #[cfg(feature = "dev-observability")]
    context: Mutex<LifecycleContext>,
}

#[cfg(feature = "dev-observability")]
/// lifecycle event 共用的 provider/protocol 安全上下文。
struct LifecycleContext {
    provider: &'static str,
    protocol: &'static str,
}

impl AnalysisLifecycle {
    /// 创建一个只携带进程内 run id 的分析 span，不读取请求内容或连接 secret。
    pub(super) fn new(analysis_run_id: u64) -> Self {
        #[cfg(feature = "dev-observability")]
        {
            let span = tracing::info_span!(
                target: TELEMETRY_TARGET,
                "babysit.analysis",
                analysis_run_id,
                provider = UNKNOWN_CONTEXT,
                protocol = UNKNOWN_CONTEXT,
                phase = "started",
            );
            return Self {
                analysis_run_id,
                span,
                context: Mutex::new(LifecycleContext {
                    provider: UNKNOWN_CONTEXT,
                    protocol: UNKNOWN_CONTEXT,
                }),
            };
        }

        #[cfg(not(feature = "dev-observability"))]
        {
            let _ = analysis_run_id;
            Self {}
        }
    }

    /// 绑定稳定 provider identity 与协议名；两者都不包含 URL、key 或模型内容。
    pub(super) fn bind(&self, provider: &'static str, protocol: Option<&'static str>) {
        #[cfg(feature = "dev-observability")]
        {
            let protocol = protocol.unwrap_or(UNKNOWN_CONTEXT);
            if let Ok(mut context) = self.context.lock() {
                context.provider = provider;
                context.protocol = protocol;
            }
            self.span.record("provider", provider);
            self.span.record("protocol", protocol);
        }

        #[cfg(not(feature = "dev-observability"))]
        {
            let _ = (provider, protocol);
        }
    }

    /// 记录命中进程内完整结果缓存的阶段，并继续由 finish 记录 complete。
    pub(super) fn cache_hit(&self) {
        self.event("cache_hit", None, None);
    }

    /// 记录一次完整 generation 开始。
    pub(super) fn generation(&self) {
        self.event("generation", None, None);
    }

    /// 记录一次 validation repair 开始；attempt 只允许小整数。
    pub(super) fn repair(&self, attempt: u8) {
        self.event("repair", Some(attempt), None);
    }

    /// 根据应用结果记录 complete、cancelled 或稳定分类的 failed 阶段。
    pub(super) fn finish<T>(&self, result: &Result<T, BabysitError>) {
        match result {
            Ok(_) => self.event("complete", None, None),
            Err(BabysitError::Cancelled) => self.event("cancelled", None, None),
            Err(error) => self.event("failed", None, Some(error_category(error))),
        }
    }

    /// 将整个异步分析 future 放入 lifecycle span，使 Rig GenAI spans 继承同一关联上下文。
    pub(super) async fn in_scope<F, T>(&self, future: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        #[cfg(feature = "dev-observability")]
        {
            use tracing::Instrument;
            future.instrument(self.span.clone()).await
        }

        #[cfg(not(feature = "dev-observability"))]
        {
            future.await
        }
    }

    /// 将单个安全 lifecycle event 交给 tracing subscriber；默认构建为 no-op。
    fn event(
        &self,
        phase: &'static str,
        attempt: Option<u8>,
        error_category: Option<&'static str>,
    ) {
        #[cfg(feature = "dev-observability")]
        {
            let (provider, protocol) = self
                .context
                .lock()
                .map(|context| (context.provider, context.protocol))
                .unwrap_or((UNKNOWN_CONTEXT, UNKNOWN_CONTEXT));
            self.span.record("phase", phase);

            // tracing 宏的四个静态字段形状必须分别保留，避免把不存在的
            // `attempt`/`error_category` 序列化成空值；事件本身不附带 message，
            // 由 telemetry 层的 target+字段 allowlist 再做一次边界校验。
            match (attempt, error_category) {
                (Some(attempt), Some(error_category)) => tracing::info!(
                    target: TELEMETRY_TARGET,
                    parent: &self.span,
                    analysis_run_id = self.analysis_run_id,
                    provider,
                    protocol,
                    phase,
                    attempt,
                    error_category
                ),
                (Some(attempt), None) => tracing::info!(
                    target: TELEMETRY_TARGET,
                    parent: &self.span,
                    analysis_run_id = self.analysis_run_id,
                    provider,
                    protocol,
                    phase,
                    attempt
                ),
                (None, Some(error_category)) => tracing::info!(
                    target: TELEMETRY_TARGET,
                    parent: &self.span,
                    analysis_run_id = self.analysis_run_id,
                    provider,
                    protocol,
                    phase,
                    error_category
                ),
                (None, None) => tracing::info!(
                    target: TELEMETRY_TARGET,
                    parent: &self.span,
                    analysis_run_id = self.analysis_run_id,
                    provider,
                    protocol,
                    phase
                ),
            }
        }

        #[cfg(not(feature = "dev-observability"))]
        {
            let _ = (phase, attempt, error_category);
        }
    }
}

/// 将 BabysitError 映射为不包含 detail 的稳定分类，绝不把原始正文送入 telemetry。
fn error_category(error: &BabysitError) -> &'static str {
    match error {
        BabysitError::UnsupportedFile { .. } => "unsupported_file",
        BabysitError::InvalidProviderOutput { .. } => "invalid_provider_output",
        BabysitError::RepairExhausted { .. } => "repair_exhausted",
        BabysitError::Cancelled => "cancelled",
        BabysitError::Provider { .. } => "provider",
        BabysitError::ProviderDiagnostic { .. } => "provider_diagnostic",
        BabysitError::ProgressClosed => "progress_closed",
        BabysitError::CacheUnavailable => "cache_unavailable",
        BabysitError::ProviderSetup { failure, .. } => match failure {
            BabysitProviderSetupFailure::NotFound => "provider_not_found",
            BabysitProviderSetupFailure::ExecutableInvalid => "provider_executable_invalid",
            BabysitProviderSetupFailure::SignedOut => "provider_signed_out",
            BabysitProviderSetupFailure::Incompatible => "provider_incompatible",
            BabysitProviderSetupFailure::InitializationFailed => "provider_initialization_failed",
        },
        BabysitError::ModelPreferenceInvalid => "model_preference_invalid",
        BabysitError::SettingsPersistenceFailed => "settings_persistence_failed",
    }
}
