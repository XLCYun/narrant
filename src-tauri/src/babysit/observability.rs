use super::{BabysitError, BabysitProviderSetupFailure};
use std::sync::Mutex;
use std::time::Instant;

/// Narrant 自己产生的 Babysit 生命周期日志 target。
const LOG_TARGET: &str = "narrant::babysit";
/// 尚未完成 provider 解析时使用的安全上下文占位值。
const UNKNOWN_CONTEXT: &str = "unknown";

/// 一次 Babysit 分析生命周期的结构化日志句柄。
pub(super) struct AnalysisLifecycle {
    analysis_run_id: u64,
    started_at: Instant,
    span: tracing::Span,
    context: Mutex<LifecycleContext>,
}

/// lifecycle event 共用的 provider/protocol 安全上下文。
struct LifecycleContext {
    provider: &'static str,
    protocol: &'static str,
}

impl AnalysisLifecycle {
    /// 创建只携带进程内 run id 的分析 span，不读取请求正文或 credential。
    pub(super) fn new(analysis_run_id: u64) -> Self {
        let span = tracing::info_span!(
            target: LOG_TARGET,
            "babysit.analysis",
            analysis_run_id,
            provider = UNKNOWN_CONTEXT,
            protocol = UNKNOWN_CONTEXT,
        );
        Self {
            analysis_run_id,
            started_at: Instant::now(),
            span,
            context: Mutex::new(LifecycleContext {
                provider: UNKNOWN_CONTEXT,
                protocol: UNKNOWN_CONTEXT,
            }),
        }
    }

    /// 绑定稳定 provider identity 与协议名；两者都不包含 key 或模型正文。
    pub(super) fn bind(&self, provider: &'static str, protocol: Option<&'static str>) {
        let protocol = protocol.unwrap_or(UNKNOWN_CONTEXT);
        if let Ok(mut context) = self.context.lock() {
            context.provider = provider;
            context.protocol = protocol;
        }
        self.span.record("provider", provider);
        self.span.record("protocol", protocol);
    }

    /// 记录命中进程内完整结果缓存的阶段。
    pub(super) fn cache_hit(&self) {
        self.event("cache_hit", None, None);
    }

    /// 记录一次完整 generation 开始。
    pub(super) fn generation(&self) {
        self.event("generation", None, None);
    }

    /// 记录一次 validation repair 开始。
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

    /// 将分析 future 放入 lifecycle span，使 Rig completion spans 继承关联上下文。
    pub(super) async fn in_scope<F, T>(&self, future: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        use tracing::Instrument;
        future.instrument(self.span.clone()).await
    }

    /// 输出一条不包含业务正文的结构化生命周期事件。
    fn event(
        &self,
        phase: &'static str,
        attempt: Option<u8>,
        error_category: Option<&'static str>,
    ) {
        let (provider, protocol) = self
            .context
            .lock()
            .map(|context| (context.provider, context.protocol))
            .unwrap_or((UNKNOWN_CONTEXT, UNKNOWN_CONTEXT));
        tracing::info!(
            target: LOG_TARGET,
            parent: &self.span,
            analysis_run_id = self.analysis_run_id,
            provider,
            protocol,
            phase,
            attempt = attempt.unwrap_or(0),
            error_category = error_category.unwrap_or("none"),
            elapsed_ms = self.started_at.elapsed().as_millis() as u64,
        );
    }
}

/// 将 BabysitError 映射为不包含 detail 的稳定分类。
fn error_category(error: &BabysitError) -> &'static str {
    match error {
        BabysitError::UnsupportedFile { .. } => "unsupported_file",
        BabysitError::InvalidProviderOutput { .. } => "invalid_provider_output",
        BabysitError::RepairExhausted { .. } => "repair_exhausted",
        BabysitError::OutputLimit { .. } => "output_limit",
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
