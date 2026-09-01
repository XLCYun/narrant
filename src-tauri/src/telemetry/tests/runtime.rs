use super::*;

use std::sync::{Mutex, OnceLock};
#[cfg(feature = "dev-observability")]
use std::time::{Duration, Instant};

#[cfg(feature = "dev-observability")]
use std::path::Path;
#[cfg(feature = "dev-observability")]
use std::sync::Arc;

/// 进程环境是全局可变状态；所有 endpoint 覆盖测试共用锁并在结束时恢复原值。
static OTEL_ENDPOINT_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
const OTEL_ENDPOINT_ENV_NAME: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// 测试修改 endpoint 环境变量时自动恢复原值，即使断言 panic 也不污染其他测试。
struct EnvironmentValueRestore {
    name: &'static str,
    previous: Option<String>,
}

impl Drop for EnvironmentValueRestore {
    /// 恢复环境变量的进入前状态。
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

#[test]
fn endpoint_defaults_to_local_venator() {
    let endpoint = EndpointConfig::from_optional(None);

    assert_eq!(endpoint.base_url(), DEFAULT_OTLP_ENDPOINT);
}

#[test]
fn endpoint_override_trims_whitespace_and_trailing_slashes() {
    let endpoint = EndpointConfig::from_optional(Some("  http://fixture:4318///  "));

    assert_eq!(endpoint.base_url(), "http://fixture:4318");
    assert_eq!(endpoint.traces_url(), "http://fixture:4318/v1/traces");
    assert_eq!(endpoint.logs_url(), "http://fixture:4318/v1/logs");
}

#[test]
fn standard_endpoint_environment_override_is_serial_and_restored() {
    let _guard = OTEL_ENDPOINT_ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("OTLP endpoint environment lock");
    let restore = EnvironmentValueRestore {
        name: OTEL_ENDPOINT_ENV_NAME,
        previous: std::env::var(OTEL_ENDPOINT_ENV_NAME).ok(),
    };
    std::env::set_var(OTEL_ENDPOINT_ENV_NAME, "  http://env-fixture:4318///  ");

    let endpoint = EndpointConfig::from_environment();

    assert_eq!(endpoint.base_url(), "http://env-fixture:4318");
    assert_eq!(endpoint.traces_url(), "http://env-fixture:4318/v1/traces");
    assert_eq!(endpoint.logs_url(), "http://env-fixture:4318/v1/logs");
    drop(restore);
}

#[test]
fn empty_endpoint_uses_the_safe_default() {
    let endpoint = EndpointConfig::from_optional(Some("   "));

    assert_eq!(endpoint.base_url(), DEFAULT_OTLP_ENDPOINT);
}

#[test]
fn sensitive_event_field_names_are_rejected() {
    for name in [
        "prompt",
        "source",
        "explanation",
        "message",
        "error",
        "body",
        "tool_arguments",
        "tool_result",
        "gen_ai.tool.call.arguments",
        "gen_ai.tool.call.result",
        "gen_ai.input.messages",
        "gen_ai.output.messages",
        "gen_ai.system_instructions",
    ] {
        assert!(is_sensitive_field(name), "field should be rejected: {name}");
    }
}

#[test]
fn structural_telemetry_fields_remain_available() {
    for name in ["analysis_id", "phase", "provider", "status", "duration_ms"] {
        assert!(
            !is_sensitive_field(name),
            "field should remain available: {name}"
        );
    }
}

#[test]
fn both_signal_queues_are_explicitly_bounded() {
    assert!(TRACE_MAX_QUEUE_SIZE >= TRACE_MAX_EXPORT_BATCH_SIZE);
    assert!(LOG_MAX_QUEUE_SIZE >= LOG_MAX_EXPORT_BATCH_SIZE);
    assert!(TRACE_MAX_QUEUE_SIZE > 0);
    assert!(LOG_MAX_QUEUE_SIZE > 0);
}

#[cfg(feature = "dev-observability")]
#[test]
fn terminal_fallback_rejects_sensitive_warn_and_error_events() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing::{Event, Subscriber};
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

    let _subscriber_guard = lock_test_subscriber();

    /// 只统计经过 fallback filter 的 event，不保留任何字段内容。
    struct CountEvents(Arc<AtomicUsize>);

    impl<S: Subscriber> Layer<S> for CountEvents {
        /// 记录通过隐私 allowlist 的 event 数量。
        fn on_event(&self, _event: &Event<'_>, _context: Context<'_, S>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    let accepted = Arc::new(AtomicUsize::new(0));
    let subscriber = tracing_subscriber::registry().with(
        CountEvents(accepted.clone())
            .with_filter(tracing_subscriber::filter::filter_fn(is_lifecycle_callsite)),
    );
    tracing::subscriber::with_default(subscriber, || {
        tracing::warn!(target: "rig", message = "SECRET_WARN", body = "SECRET_BODY");
        tracing::error!(target: "provider", error = "SECRET_ERROR", body = "SECRET_BODY");
        tracing::info!(
            target: LIFECYCLE_TARGET,
            analysis_run_id = 158_u64,
            provider = "custom",
            protocol = "openai_chat_completions",
            phase = "generation"
        );
    });

    assert_eq!(accepted.load(Ordering::Relaxed), 1);
}

#[cfg(feature = "dev-observability")]
/// 通过真实 Custom/Rig agent 验证 OTLP protobuf 中的 span、logs 和关联关系。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_analysis_exports_correlated_safe_otel_signals() {
    use httpmock::prelude::*;
    use opentelemetry_proto::tonic::collector::{
        logs::v1::ExportLogsServiceRequest, trace::v1::ExportTraceServiceRequest,
    };
    use prost::Message;

    let _environment_guard = OTEL_ENDPOINT_ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("OTLP endpoint environment lock");
    let _subscriber_guard = lock_test_subscriber();
    const SECRET_PROMPT: &str = "SECRET_PROMPT_158";
    const SECRET_SOURCE: &str = "SECRET_SOURCE_158";
    const SECRET_EXPLANATION: &str = "SECRET_EXPLANATION_158";
    const SECRET_TOOL_ARGS: &str = "SECRET_TOOL_ARGS_158";
    const SECRET_TOOL_RESULT: &str = "SECRET_TOOL_RESULT_158";

    let model_server = MockServer::start_async().await;
    let model_mock = model_server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "id": "telemetry-fixture-response",
                    "object": "chat.completion",
                    "created": 1,
                    "model": "fixture-model",
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "tool_calls": [{
                                "id": SECRET_TOOL_RESULT,
                                "type": "function",
                                "function": {
                                    "name": "final_result",
                                    "arguments": serde_json::json!({
                                        "before": {"1": SECRET_EXPLANATION},
                                        "after": {"1": SECRET_EXPLANATION}
                                    }).to_string()
                                }
                            }]
                        },
                        "finish_reason": "tool_calls"
                    }],
                    "usage": {"prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7}
                }));
        })
        .await;

    let workspace = tempfile::tempdir().expect("create telemetry workspace");
    let settings = tempfile::tempdir().expect("create telemetry settings");
    let application = custom_application(
        settings.path().join("settings.json"),
        format!("{}/v1", model_server.url("")),
    );
    let (request, content) = telemetry_fixture_input(workspace.path(), SECRET_SOURCE);

    let fixture = OtlpFixture::start(2);
    let endpoint_restore = EnvironmentValueRestore {
        name: OTEL_ENDPOINT_ENV_NAME,
        previous: std::env::var(OTEL_ENDPOINT_ENV_NAME).ok(),
    };
    std::env::set_var(OTEL_ENDPOINT_ENV_NAME, fixture.endpoint());
    let endpoint = EndpointConfig::from_environment();
    let telemetry = EnabledTelemetry::build(&endpoint).expect("build OTLP integration telemetry");
    let subscriber = telemetry.subscriber();
    let cancellation = crate::operation::OperationContext::background()
        .cancellation()
        .clone();
    let result = tracing::subscriber::with_default(subscriber, || {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                let result = application
                    .explain_file(request, content, &cancellation, |_| Ok(()))
                    .await;
                // 在同一局部 subscriber 活跃时发出代表性不安全 provider event；
                // 即使所有 span 都保留，这些 callsite 也必须被拒绝。
                tracing::debug!(target: "rig", message = SECRET_PROMPT, error = SECRET_TOOL_RESULT);
                tracing::debug!(target: "provider", body = SECRET_SOURCE, tool_args = SECRET_TOOL_ARGS);
                result
            })
        })
    });
    let result = result.expect("real Rig analysis should succeed");
    assert_eq!(result.before.get("1"), Some(&SECRET_EXPLANATION.to_owned()));
    assert_eq!(result.after.get("1"), Some(&SECRET_EXPLANATION.to_owned()));
    assert_eq!(
        model_mock.hits_async().await,
        1,
        "analysis should use one Rig turn"
    );

    telemetry.shutdown_with_timeout(OTLP_SHUTDOWN_TIMEOUT);
    let requests = fixture.finish();
    assert_eq!(
        requests.len(),
        2,
        "traces and logs should both reach local OTLP fixture: {:?}",
        requests
            .iter()
            .map(|request| &request.path)
            .collect::<Vec<_>>()
    );

    let trace_request = requests
        .iter()
        .find(|request| request.path == "/v1/traces")
        .expect("trace OTLP request");
    let log_request = requests
        .iter()
        .find(|request| request.path == "/v1/logs")
        .expect("log OTLP request");
    let traces = ExportTraceServiceRequest::decode(trace_request.body.as_slice())
        .expect("decode OTLP traces protobuf");
    let logs = ExportLogsServiceRequest::decode(log_request.body.as_slice())
        .expect("decode OTLP logs protobuf");

    let spans = traces
        .resource_spans
        .iter()
        .flat_map(|resource| resource.scope_spans.iter())
        .flat_map(|scope| scope.spans.iter())
        .collect::<Vec<_>>();
    let lifecycle_span = spans
        .iter()
        .find(|span| span.name == "babysit.analysis")
        .expect("Narrant lifecycle span");
    let rig_span = spans
        .iter()
        .find(|span| {
            span.parent_span_id == lifecycle_span.span_id
                && span.attributes.iter().any(|attribute| {
                    attribute.key == "gen_ai.operation.name"
                        || attribute.key == "gen_ai.provider.name"
                        || attribute.key == "gen_ai.request.model"
                })
        })
        .expect("Rig GenAI span");
    assert!(!lifecycle_span.trace_id.is_empty());
    assert_eq!(rig_span.trace_id, lifecycle_span.trace_id);
    assert_eq!(rig_span.parent_span_id, lifecycle_span.span_id);
    assert!(rig_span
        .attributes
        .iter()
        .any(|attribute| attribute.key == "gen_ai.operation.name"));

    let logs = logs
        .resource_logs
        .iter()
        .flat_map(|resource| resource.scope_logs.iter())
        .flat_map(|scope| scope.log_records.iter())
        .collect::<Vec<_>>();
    assert!(
        !logs.is_empty(),
        "Narrant lifecycle logs should be exported"
    );
    assert!(logs.iter().all(|log| {
        log.body.is_none()
            && !log.event_name.is_empty()
            && log.attributes.iter().all(|attribute| {
                matches!(
                    attribute.key.as_str(),
                    "analysis_run_id"
                        | "provider"
                        | "protocol"
                        | "phase"
                        | "attempt"
                        | "error_category"
                )
            })
    }));
    assert!(logs
        .iter()
        .all(|log| log.trace_id == lifecycle_span.trace_id));

    let decoded = format!("{traces:?}{logs:?}");
    for secret in [
        SECRET_PROMPT,
        SECRET_SOURCE,
        SECRET_EXPLANATION,
        SECRET_TOOL_ARGS,
        SECRET_TOOL_RESULT,
    ] {
        assert!(!decoded.contains(secret), "telemetry leaked {secret}");
    }
    drop(endpoint_restore);
}

#[cfg(feature = "dev-observability")]
/// receiver 不存在时，真实 Rig analysis 仍返回同一业务结果且不抛遥测错误。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_analysis_result_is_unchanged_without_otlp_receiver() {
    use httpmock::prelude::*;

    let _subscriber_guard = lock_test_subscriber();
    let model_server = MockServer::start_async().await;
    let model_mock = model_server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "id": "absent-receiver-response",
                    "object": "chat.completion",
                    "created": 1,
                    "model": "fixture-model",
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "tool_calls": [{
                                "id": "absent-final",
                                "type": "function",
                                "function": {
                                    "name": "final_result",
                                    "arguments": "{\"before\":{\"1\":\"stable explanation\"},\"after\":{\"1\":\"stable explanation\"}}"
                                }
                            }]
                        },
                        "finish_reason": "tool_calls"
                    }],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                }));
        })
        .await;
    let workspace = tempfile::tempdir().expect("create absent receiver workspace");
    let settings = tempfile::tempdir().expect("create absent receiver settings");
    let application = custom_application(
        settings.path().join("settings.json"),
        format!("{}/v1", model_server.url("")),
    );
    let (request, content) = telemetry_fixture_input(workspace.path(), "source without receiver");
    let baseline_cancellation = crate::operation::OperationContext::background()
        .cancellation()
        .clone();
    let baseline = application
        .explain_file(
            request.clone(),
            content.clone(),
            &baseline_cancellation,
            |_| Ok(()),
        )
        .await
        .expect("baseline Rig analysis should succeed");

    let endpoint = EndpointConfig::from_optional(Some("http://127.0.0.1:1"));
    let telemetry = EnabledTelemetry::build(&endpoint).expect("build absent receiver telemetry");
    let subscriber = telemetry.subscriber();
    let telemetry_cancellation = crate::operation::OperationContext::background()
        .cancellation()
        .clone();
    let result = tracing::subscriber::with_default(subscriber, || {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                let mut request = request;
                request.generation_policy = crate::babysit::BabysitGenerationPolicy::Regenerate;
                application
                    .explain_file(request, content, &telemetry_cancellation, |_| Ok(()))
                    .await
            })
        })
    });
    let result = result.expect("absent receiver must not change analysis result");
    telemetry.shutdown_with_timeout(Duration::ZERO);

    assert_eq!(result, baseline);
    assert_eq!(
        model_mock.hits_async().await,
        2,
        "both runs should reach Rig model"
    );
}

#[cfg(feature = "dev-observability")]
/// 注入设置和 HTTP fixture，确保测试走生产 Custom capability 与真实 Rig agent。
fn custom_application(
    settings_path: impl Into<std::path::PathBuf>,
    base_url: String,
) -> Arc<crate::babysit::BabysitApplication> {
    use crate::application_settings::{
        ApplicationSettingsApplication, BabysitProvider, CustomCredentialAdapter,
        CustomProviderConnection, CustomProviderEffort, CustomProviderProtocol,
    };

    /// 集成测试使用的空凭据存储。
    struct Credentials;

    impl CustomCredentialAdapter for Credentials {
        /// fixture 不保存 API key，避免测试触发平台凭据存储。
        fn load(&self) -> Result<Option<String>, String> {
            Ok(None)
        }

        /// fixture 忽略保存操作。
        fn save(&self, _value: &str) -> Result<(), String> {
            Ok(())
        }

        /// fixture 忽略删除操作。
        fn delete(&self) -> Result<(), String> {
            Ok(())
        }
    }

    let settings_path = settings_path.into();
    let settings = ApplicationSettingsApplication::with_credential_adapter(
        settings_path.clone(),
        Arc::new(Credentials),
    );
    settings
        .update_custom_connection(CustomProviderConnection {
            protocol: CustomProviderProtocol::OpenaiChatCompletions,
            base_url,
            api_key: String::new(),
            model: "fixture-model".to_owned(),
            effort: CustomProviderEffort::High,
        })
        .expect("persist custom telemetry connection");
    settings
        .update_provider(BabysitProvider::Custom)
        .expect("select custom telemetry provider");
    Arc::new(
        crate::babysit::BabysitApplication::with_installed_provider_registry(
            settings_path,
            settings,
        ),
    )
}

#[cfg(feature = "dev-observability")]
/// 构造含有敏感源码的完整 File Change；敏感值只应抵达模型 fixture。
fn telemetry_fixture_input(
    workspace: &Path,
    source: &str,
) -> (
    crate::babysit::BabysitFileRequest,
    crate::review::FileChangeContent,
) {
    use crate::babysit::{BabysitFileRequest, BabysitGenerationPolicy, ReviewIdentity};
    use crate::review::{FileChangeContent, FileChangeStatus, FileContent};

    let path = "fixture.rs".to_owned();
    (
        BabysitFileRequest {
            identity: ReviewIdentity {
                review_id: "telemetry-review".to_owned(),
                snapshot_id: "telemetry-snapshot".to_owned(),
            },
            path: path.clone(),
            workspace_path: workspace.to_string_lossy().into_owned(),
            detected_language: Some("rust".to_owned()),
            generation_policy: BabysitGenerationPolicy::Regenerate,
        },
        FileChangeContent {
            path,
            old_path: None,
            status: FileChangeStatus::Modified,
            before: Some(FileContent::Text {
                text: source.to_owned(),
                size: source.len(),
            }),
            after: Some(FileContent::Text {
                text: source.to_owned(),
                size: source.len(),
            }),
        },
    )
}

#[cfg(feature = "dev-observability")]
#[test]
fn otlp_http_fixture_receives_traces_and_logs_as_protobuf() {
    use opentelemetry::logs::{LogRecord, Logger, LoggerProvider};
    use opentelemetry::trace::{Span, Tracer, TracerProvider};

    let fixture = OtlpFixture::start(2);
    let endpoint = EndpointConfig::from_optional(Some(fixture.endpoint()));
    let telemetry = EnabledTelemetry::build(&endpoint).expect("build fixture telemetry");

    let tracer = telemetry.tracer_provider.tracer("fixture");
    let mut span = tracer.start("fixture.span");
    span.end();

    let logger = telemetry.logger_provider.logger("fixture");
    let mut record = logger.create_log_record();
    record.set_event_name("fixture.log");
    logger.emit(record);

    telemetry.shutdown_with_timeout(OTLP_SHUTDOWN_TIMEOUT);
    let requests = fixture.finish();

    assert_eq!(requests.len(), 2, "both signals should reach the fixture");
    for request in &requests {
        assert_eq!(request.method, "POST");
        assert_eq!(
            request.content_type.as_deref(),
            Some("application/x-protobuf")
        );
        assert!(request.body_len > 0, "protobuf payload must not be empty");
    }
    let paths = requests
        .iter()
        .map(|request| request.path.as_str())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"/v1/traces"));
    assert!(paths.contains(&"/v1/logs"));
}

#[cfg(feature = "dev-observability")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_is_bounded_when_receiver_is_absent() {
    use opentelemetry::logs::{Logger, LoggerProvider};
    use opentelemetry::trace::{Span, Tracer, TracerProvider};

    let endpoint = EndpointConfig::from_optional(Some("http://127.0.0.1:1"));
    let enabled = EnabledTelemetry::build(&endpoint).expect("build absent receiver telemetry");
    let tracer = enabled.tracer_provider.tracer("absent-fixture");
    let mut span = tracer.start("absent-fixture.span");
    span.end();
    let logger = enabled.logger_provider.logger("absent-fixture");
    let record = logger.create_log_record();
    logger.emit(record);
    let telemetry = Telemetry {
        inner: Some(enabled),
        shutdown_started: std::sync::atomic::AtomicBool::new(false),
    };
    let started = Instant::now();

    telemetry.shutdown().await;

    assert!(
        started.elapsed() <= OTLP_SHUTDOWN_TIMEOUT + Duration::from_millis(500),
        "shutdown should respect the two-second bound"
    );
}

#[cfg(feature = "dev-observability")]
#[derive(Debug)]
/// fixture 解析出的最小 HTTP 请求信息。
struct OtlpRequest {
    method: String,
    path: String,
    content_type: Option<String>,
    body_len: usize,
    body: Vec<u8>,
}

#[cfg(feature = "dev-observability")]
/// 使用标准库实现的本地 OTLP HTTP 接收端，避免测试依赖外部服务。
struct OtlpFixture {
    endpoint: String,
    join_handle: Option<std::thread::JoinHandle<Vec<OtlpRequest>>>,
}

#[cfg(feature = "dev-observability")]
impl OtlpFixture {
    /// 启动一个只接受预期请求数的本地 HTTP fixture，避免访问外部服务。
    fn start(expected_requests: usize) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind OTLP fixture");
        let address = listener.local_addr().expect("read fixture address");
        let endpoint = format!("http://{address}");
        let join_handle = std::thread::spawn(move || collect_requests(listener, expected_requests));
        Self {
            endpoint,
            join_handle: Some(join_handle),
        }
    }

    /// 返回 exporter 使用的 endpoint 基址。
    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// 等待 fixture 收齐请求并返回解析结果。
    fn finish(mut self) -> Vec<OtlpRequest> {
        self.join_handle
            .take()
            .expect("fixture join handle")
            .join()
            .expect("fixture thread")
    }
}

#[cfg(feature = "dev-observability")]
/// 接收并应答 fixture 请求，直到收齐预期 signal 数量或超时。
fn collect_requests(listener: std::net::TcpListener, expected_requests: usize) -> Vec<OtlpRequest> {
    use std::io::Write;

    listener
        .set_nonblocking(true)
        .expect("configure fixture listener");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut requests = Vec::with_capacity(expected_requests);
    while requests.len() < expected_requests && Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Some(request) = read_request(&mut stream) {
                    requests.push(request);
                }
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("accept OTLP fixture request: {error}"),
        }
    }
    requests
}

#[cfg(feature = "dev-observability")]
/// 读取一个带 Content-Length 的 HTTP 请求并提取 OTLP signal path。
fn read_request(stream: &mut std::net::TcpStream) -> Option<OtlpRequest> {
    use std::io::Read;

    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut bytes = Vec::new();
    let headers_end = loop {
        let mut chunk = [0_u8; 1024];
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_bytes(&bytes, b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..headers_end]);
    let mut request_line = headers.lines().next()?.split_whitespace();
    let method = request_line.next()?.to_owned();
    let path = request_line.next()?.to_owned();
    let content_type = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-type")
            .then(|| value.trim().to_owned())
    });
    let content_length = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    })?;
    while bytes.len() < headers_end + content_length {
        let mut chunk = [0_u8; 1024];
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    Some(OtlpRequest {
        method,
        path,
        content_type,
        body_len: content_length,
        body: bytes[headers_end..headers_end + content_length].to_vec(),
    })
}

#[cfg(feature = "dev-observability")]
/// 查找字节切片中的首个子序列位置。
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
