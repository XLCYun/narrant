use super::*;

#[cfg(feature = "dev-observability")]
use std::time::{Duration, Instant};

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
        "tool_arguments",
        "tool_result",
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
    })
}

#[cfg(feature = "dev-observability")]
/// 查找字节切片中的首个子序列位置。
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
