#![cfg(feature = "dev-observability")]

use super::super::observability::AnalysisLifecycle;
use crate::babysit::BabysitError;
use std::fmt;
use std::sync::{Arc, Mutex};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;

/// 记录单个结构化 tracing event 的字段，供生命周期 seam 做行为断言。
#[derive(Clone, Debug, Default)]
struct CapturedEvent {
    fields: Vec<(String, String)>,
}

/// 将 tracing 字段转换为不依赖 formatter 输出的测试值。
#[derive(Default)]
struct FieldVisitor {
    fields: Vec<(String, String)>,
}

impl tracing::field::Visit for FieldVisitor {
    /// 兜底记录非字符串字段，测试只使用安全的结构化 primitive。
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.fields
            .push((field.name().to_owned(), format!("{value:?}")));
    }

    /// 保留字符串字段的原值，避免 debug 引号影响断言。
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields
            .push((field.name().to_owned(), value.to_owned()));
    }

    /// 保留无符号关联 id 与 repair attempt 的原值。
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields
            .push((field.name().to_owned(), value.to_string()));
    }
}

/// 只观察 event 字段，不读取 span 内部实现，模拟 OTLP log bridge 的输入 seam。
#[derive(Clone)]
struct CaptureLayer {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber,
{
    /// 收集生命周期 event 的安全字段；不把 subscriber 输出写回应用状态。
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        self.events
            .lock()
            .expect("capture layer lock should remain available")
            .push(CapturedEvent {
                fields: visitor.fields,
            });
    }
}

/// 在局部 subscriber 中执行生命周期动作并返回捕获的结构化 events。
fn capture(run: impl FnOnce()) -> Vec<CapturedEvent> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(CaptureLayer {
        events: events.clone(),
    });
    tracing::subscriber::with_default(subscriber, run);
    Arc::try_unwrap(events)
        .expect("capture layer should be the only owner")
        .into_inner()
        .expect("capture layer lock should remain available")
}

/// 读取捕获 event 的一个字段，避免测试绑定 subscriber 的内部数据结构。
fn field(event: &CapturedEvent, name: &str) -> Option<&str> {
    event
        .fields
        .iter()
        .find(|(field, _)| field == name)
        .map(|(_, value)| value.as_str())
}

#[test]
fn lifecycle_events_cover_cache_generation_repair_and_completion() {
    let events = capture(|| {
        let lifecycle = AnalysisLifecycle::new(42);
        lifecycle.bind("custom", Some("openai_chat_completions"));
        lifecycle.cache_hit();
        lifecycle.generation();
        lifecycle.repair(1);
        lifecycle.finish(&Ok(()));
    });

    assert_eq!(
        events
            .iter()
            .map(|event| field(event, "phase"))
            .collect::<Vec<_>>(),
        [
            Some("cache_hit"),
            Some("generation"),
            Some("repair"),
            Some("complete")
        ]
    );
    for event in &events {
        assert_eq!(field(event, "analysis_run_id"), Some("42"));
        assert_eq!(field(event, "provider"), Some("custom"));
        assert_eq!(field(event, "protocol"), Some("openai_chat_completions"));
    }
    assert_eq!(field(&events[2], "attempt"), Some("1"));
}

#[test]
fn lifecycle_failure_and_cancellation_are_redacted_and_classified() {
    let events = capture(|| {
        let failure = AnalysisLifecycle::new(7);
        failure.bind("custom", Some("anthropic_messages"));
        let error = BabysitError::Provider {
            detail:
                "https://secret.example/path?key=secret-key prompt source explanation tool-result"
                    .to_owned(),
        };
        failure.finish(&Err(error));

        let cancelled = AnalysisLifecycle::new(8);
        cancelled.bind("custom", Some("anthropic_messages"));
        cancelled.finish(&Err(BabysitError::Cancelled));
    });

    assert_eq!(field(&events[0], "phase"), Some("failed"));
    assert_eq!(field(&events[0], "error_category"), Some("provider"));
    assert_eq!(field(&events[0], "analysis_run_id"), Some("7"));
    assert_eq!(field(&events[1], "phase"), Some("cancelled"));
    assert_eq!(field(&events[1], "analysis_run_id"), Some("8"));
    assert!(events.iter().all(|event| {
        event.fields.iter().all(|(_, value)| {
            !value.contains("secret.example")
                && !value.contains("secret-key")
                && !value.contains("prompt")
                && !value.contains("source")
                && !value.contains("explanation")
                && !value.contains("tool-result")
        })
    }));
}
