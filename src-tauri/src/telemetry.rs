//! 开发可观测性 runtime。
//!
//! 生产构建不安装 subscriber 或 exporter；只有显式启用
//! `dev-observability` feature 时，应用启动才会把 tracing spans/events 接到
//! OpenTelemetry traces/logs。这个模块拥有 exporter 的生命周期，但不拥有业务
//! span；业务模块只需要使用 tracing 的公开宏即可。

use std::time::Duration;

/// Venator 的本地 OTLP HTTP endpoint；标准环境变量可以覆盖它。
pub(crate) const DEFAULT_OTLP_ENDPOINT: &str = "http://127.0.0.1:8362";
/// exporter 单次 HTTP 请求的上限，避免无响应 receiver 拖住 shutdown。
pub(crate) const OTLP_EXPORT_TIMEOUT: Duration = Duration::from_secs(1);
/// 应用退出时为 traces 与 logs 合计保留的 flush 上限。
pub(crate) const OTLP_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
/// traces 的有界批处理队列大小。
pub(crate) const TRACE_MAX_QUEUE_SIZE: usize = 256;
/// traces 单批最大条目数。
pub(crate) const TRACE_MAX_EXPORT_BATCH_SIZE: usize = 64;
/// logs 的有界批处理队列大小。
pub(crate) const LOG_MAX_QUEUE_SIZE: usize = 256;
/// logs 单批最大条目数。
pub(crate) const LOG_MAX_EXPORT_BATCH_SIZE: usize = 64;
const OTLP_ENDPOINT_ENV: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// 应用生命周期持有的 telemetry owner。
///
/// 即使未启用 feature，也保留相同的 `init`/`shutdown` seam，让 Tauri bootstrap
/// 不需要在业务代码中散落 cfg；默认实现是零成本 no-op。
pub(crate) struct Telemetry {
    #[cfg(feature = "dev-observability")]
    inner: Option<EnabledTelemetry>,
    #[cfg(feature = "dev-observability")]
    shutdown_started: std::sync::atomic::AtomicBool,
}

/// 初始化进程级 tracing subscriber；失败时仍保留终端 fmt，不让 dashboard 配置
/// 阻止 Narrant 启动。
pub(crate) fn init() -> Telemetry {
    #[cfg(feature = "dev-observability")]
    {
        let inner = EnabledTelemetry::install().ok();
        if inner.is_none() {
            install_terminal_subscriber();
        }
        return Telemetry {
            inner,
            shutdown_started: std::sync::atomic::AtomicBool::new(false),
        };
    }

    #[cfg(not(feature = "dev-observability"))]
    Telemetry {}
}

impl Telemetry {
    /// 在应用退出时尽力 flush 两类 exporter；dashboard 缺席或超时只丢弃遥测，
    /// 不改变业务退出结果。
    pub(crate) async fn shutdown(&self) {
        #[cfg(feature = "dev-observability")]
        {
            use std::sync::atomic::Ordering;

            if self
                .shutdown_started
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return;
            }

            let Some(inner) = &self.inner else {
                return;
            };

            // SDK 的 provider shutdown 是阻塞方法；分别放到 blocking pool 后再
            // 并行等待，保证 traces + logs 的总墙钟时间不超过统一上限。
            let tracer_provider = inner.tracer_provider.clone();
            let logger_provider = inner.logger_provider.clone();
            let trace_shutdown = tokio::task::spawn_blocking(move || {
                let _ = tracer_provider.shutdown_with_timeout(OTLP_SHUTDOWN_TIMEOUT);
            });
            let log_shutdown = tokio::task::spawn_blocking(move || {
                let _ = logger_provider.shutdown_with_timeout(OTLP_SHUTDOWN_TIMEOUT);
            });
            let _ = tokio::time::timeout(OTLP_SHUTDOWN_TIMEOUT, async {
                let _ = tokio::join!(trace_shutdown, log_shutdown);
            })
            .await;
        }
    }
}

/// 从一个 endpoint 基址解析 traces/logs 共用的 exporter 配置。
///
/// 这是有意保持纯函数的 seam：测试可传入环境变量值而无需并发修改进程环境，
/// 运行时只在启动时读取一次 `OTEL_EXPORTER_OTLP_ENDPOINT`。OTLP exporter 自己
/// 为不同 signal 追加 `/v1/traces` 与 `/v1/logs`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EndpointConfig {
    base_url: String,
}

impl EndpointConfig {
    /// 使用标准环境变量，空值按未配置处理。
    pub(crate) fn from_environment() -> Self {
        Self::from_optional(std::env::var(OTLP_ENDPOINT_ENV).ok().as_deref())
    }

    /// 从可注入的环境变量值创建配置。
    pub(crate) fn from_optional(value: Option<&str>) -> Self {
        let base_url = value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_OTLP_ENDPOINT)
            .trim_end_matches('/')
            .to_owned();
        Self { base_url }
    }

    /// 返回供 OTLP signal builder 使用的基址；builder 会追加 signal path。
    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }
}

/// 判断一个 tracing 字段名是否可能承载分析内容。
///
/// logs bridge 默认会把 event 字段转成 LogRecord attributes。仅靠调用方自律
/// 不足以形成安全边界，因此 logs layer 会拒绝声明这些字段的 event；Rig 的
/// GenAI spans 仍完整保留（它们只声明空的 content fields，且默认不写入值）。
pub(crate) fn is_sensitive_field(name: &str) -> bool {
    matches!(
        name,
        "prompt"
            | "source"
            | "explanation"
            | "line_explanation"
            | "tool_args"
            | "tool_arguments"
            | "tool_result"
            | "tool_results"
            | "tool_output"
            | "tool_outputs"
            | "content"
            | "input"
            | "output"
            | "gen_ai.input.messages"
            | "gen_ai.output.messages"
            | "gen_ai.system_instructions"
            | "gen_ai.completion"
    )
}

#[cfg(feature = "dev-observability")]
/// 判断一个调用点是否可以进入 traces、logs 与终端 fmt layer。
fn is_public_callsite(metadata: &tracing::Metadata<'_>) -> bool {
    !metadata.is_event()
        || !metadata
            .fields()
            .iter()
            .any(|field| is_sensitive_field(field.name()))
}

#[cfg(feature = "dev-observability")]
/// feature 开启后持有 traces/logs provider，确保退出时可显式 flush。
struct EnabledTelemetry {
    tracer_provider: opentelemetry_sdk::trace::SdkTracerProvider,
    logger_provider: opentelemetry_sdk::logs::SdkLoggerProvider,
}

#[cfg(feature = "dev-observability")]
impl EnabledTelemetry {
    /// 构造两个有界 batch provider，并把 traces/logs/fmt layer 一次安装到全局
    /// subscriber。构造失败只表示 exporter 不可用，不向业务返回错误。
    fn install() -> Result<Self, ()> {
        let endpoint = EndpointConfig::from_environment();
        let telemetry = Self::build(&endpoint)?;
        if telemetry.install_layers().is_err() {
            // 全局 subscriber 已被其他库占用时，立即停止新建的 provider，避免
            // provider 的 Drop 路径使用 SDK 默认的较长等待时间。
            telemetry.shutdown_with_timeout(Duration::ZERO);
            return Err(());
        }
        Ok(telemetry)
    }

    /// 按给定 endpoint 构造 provider；不安装全局 subscriber，供测试 fixture
    /// 直接驱动 traces/logs exporter。
    fn build(endpoint: &EndpointConfig) -> Result<Self, ()> {
        use opentelemetry_otlp::{Protocol, WithExportConfig};
        use opentelemetry_sdk::logs::{
            BatchConfigBuilder as LogBatchConfigBuilder, BatchLogProcessor, SdkLoggerProvider,
        };
        use opentelemetry_sdk::trace::{
            BatchConfigBuilder as TraceBatchConfigBuilder, BatchSpanProcessor, SdkTracerProvider,
        };
        use opentelemetry_sdk::Resource;
        let span_exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .with_endpoint(endpoint.base_url())
            .with_timeout(OTLP_EXPORT_TIMEOUT)
            .build()
            .map_err(|_| ())?;
        let log_exporter = opentelemetry_otlp::LogExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .with_endpoint(endpoint.base_url())
            .with_timeout(OTLP_EXPORT_TIMEOUT)
            .build()
            .map_err(|_| ())?;

        let trace_batch = BatchSpanProcessor::builder(span_exporter)
            .with_batch_config(
                TraceBatchConfigBuilder::default()
                    .with_max_queue_size(TRACE_MAX_QUEUE_SIZE)
                    .with_max_export_batch_size(TRACE_MAX_EXPORT_BATCH_SIZE)
                    .with_scheduled_delay(Duration::from_secs(1))
                    .build(),
            )
            .build();
        let log_batch = BatchLogProcessor::builder(log_exporter)
            .with_batch_config(
                LogBatchConfigBuilder::default()
                    .with_max_queue_size(LOG_MAX_QUEUE_SIZE)
                    .with_max_export_batch_size(LOG_MAX_EXPORT_BATCH_SIZE)
                    .with_scheduled_delay(Duration::from_secs(1))
                    .build(),
            )
            .build();

        let resource = Resource::builder().with_service_name("narrant").build();
        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_span_processor(trace_batch)
            .build();
        let logger_provider = SdkLoggerProvider::builder()
            .with_resource(resource)
            .with_log_processor(log_batch)
            .build();

        Ok(Self {
            tracer_provider,
            logger_provider,
        })
    }

    /// 将 provider 绑定到 tracing subscriber；只在进程首次初始化时调用。
    fn install_layers(&self) -> Result<(), ()> {
        use opentelemetry::global;
        use opentelemetry::trace::TracerProvider;
        use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
        use tracing_subscriber::layer::{Layer, SubscriberExt};
        use tracing_subscriber::util::SubscriberInitExt;

        let tracer = self.tracer_provider.tracer("narrant");
        let trace_layer = tracing_opentelemetry::layer()
            .with_tracer(tracer)
            .with_filter(tracing_subscriber::filter::filter_fn(is_public_callsite));
        let log_layer = OpenTelemetryTracingBridge::new(&self.logger_provider)
            .with_filter(tracing_subscriber::filter::filter_fn(is_public_callsite));
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_filter(tracing_subscriber::filter::filter_fn(is_public_callsite));
        let subscriber = tracing_subscriber::registry()
            .with(trace_layer)
            .with(log_layer)
            .with(fmt_layer);
        subscriber.try_init().map_err(|_| ())?;

        // Rig 的 tracing span 与其他 instrumentation 共用这个 global provider。
        global::set_tracer_provider(self.tracer_provider.clone());
        Ok(())
    }

    /// 在 subscriber 安装失败等异常路径中，以指定上限停止两个 processor。
    fn shutdown_with_timeout(&self, timeout: Duration) {
        let _ = self.tracer_provider.shutdown_with_timeout(timeout);
        let _ = self.logger_provider.shutdown_with_timeout(timeout);
    }
}

#[cfg(feature = "dev-observability")]
fn install_terminal_subscriber() {
    use tracing_subscriber::util::SubscriberInitExt;

    // endpoint 格式错误或其他库已占用全局 subscriber 时，仍保留终端 fmt。
    // 不打印 endpoint 细节或 exporter 错误。
    let _ = tracing_subscriber::fmt().try_init();
}

#[cfg(test)]
#[path = "telemetry/tests/runtime.rs"]
mod tests;
