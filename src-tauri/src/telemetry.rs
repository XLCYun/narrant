//! 本机开发日志初始化。
//!
//! Debug 构建把 `tracing` spans/events 直接打印到终端；Release 构建不安装
//! subscriber。该模块不持有 exporter，也不写入文件或网络。

/// 未提供或无法解析 `RUST_LOG` 时使用的安全默认过滤器。
#[cfg(any(debug_assertions, test))]
pub(crate) const DEFAULT_LOG_FILTER: &str = "narrant=info,rig=info";

/// 应用生命周期持有的日志 owner。
///
/// 当前终端 subscriber 不需要 flush，但保留统一 shutdown seam，避免 Tauri
/// bootstrap 感知构建模式。
pub(crate) struct Telemetry;

/// 初始化 Debug 进程级终端 subscriber；Release 路径是 no-op。
pub(crate) fn init() -> Telemetry {
    #[cfg(debug_assertions)]
    install_debug_subscriber();
    Telemetry
}

#[cfg(debug_assertions)]
/// 安装支持标准 `RUST_LOG` 的紧凑单行 formatter。
fn install_debug_subscriber() {
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::{Layer, SubscriberExt};
    use tracing_subscriber::util::SubscriberInitExt;

    let (filter, invalid_filter) = filter_from_environment();
    let fmt_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_span_events(FmtSpan::CLOSE)
        // Rig completion spans are safe after content telemetry is disabled. Arbitrary
        // dependency events may carry provider bodies, so only Narrant events pass.
        .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
            !metadata.is_event() || metadata.target().starts_with("narrant")
        }))
        .with_filter(filter);
    let subscriber = tracing_subscriber::registry().with(fmt_layer);
    if subscriber.try_init().is_ok() && invalid_filter {
        tracing::warn!(
            target: "narrant::telemetry",
            "RUST_LOG 无法解析，已使用默认日志过滤器"
        );
    }
}

#[cfg(debug_assertions)]
/// 解析环境过滤器；错误值不回显，避免意外把环境内容写入日志。
fn filter_from_environment() -> (tracing_subscriber::EnvFilter, bool) {
    match tracing_subscriber::EnvFilter::try_from_default_env() {
        Ok(filter) => (filter, false),
        Err(_) => (
            tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER),
            std::env::var_os("RUST_LOG").is_some(),
        ),
    }
}

impl Telemetry {
    /// 终端输出无需异步 flush；保留异步签名供应用统一关闭。
    pub(crate) async fn shutdown(&self) {}
}

#[cfg(test)]
#[path = "telemetry/tests/runtime.rs"]
mod tests;
