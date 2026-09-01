use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

/// 启动一次或多次真实环回 HTTP 响应，并返回首请求 endpoint。
fn serve(responses: Vec<(Duration, String)>) -> Url {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
    let address = listener.local_addr().expect("fixture address");
    thread::spawn(move || {
        for (delay, response) in responses {
            let (mut stream, _) = listener.accept().expect("accept fixture");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            thread::sleep(delay);
            stream
                .write_all(response.as_bytes())
                .expect("write fixture");
        }
    });
    Url::parse(&format!("http://{address}/latest.json")).expect("fixture url")
}

/// 构造符合 Tauri static stable manifest wire shape 的响应体。
fn manifest(target: &str) -> String {
    serde_json::json!({
        "version": "2.0.0",
        "notes": "safe",
        "platforms": {
            target: {
                "url": "https://github.com/cyunlab/narrant/releases/download/v2.0.0/Narrant.exe",
                "signature": "trusted-signature"
            }
        }
    })
    .to_string()
}

/// 构造带 Content-Length 的完整 HTTP response。
fn response(status: &str, body: &str, headers: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn stream_hard_cap_rejects_chunked_body_before_json_parsing() {
    let chunk = "x".repeat(MAX_MANIFEST_BYTES + 1);
    let raw = format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
        chunk.len(), chunk
    );
    let endpoint = serve(vec![(Duration::ZERO, raw)]);
    let adapter = BoundedManifestAdapter::for_test(
        endpoint,
        "darwin-aarch64",
        InstallKind::MacosAppArchive,
        "1.0.0",
        Duration::from_secs(1),
    )
    .expect("adapter");
    let error = adapter.check().await.expect_err("oversize must fail");
    assert_eq!(error.failure().category(), UpdateFailureCategory::Manifest);
}

#[tokio::test]
async fn status_timeout_malformed_and_missing_target_are_classified_exactly() {
    for (status, expected) in [
        ("404 Not Found", UpdateFailureCategory::Manifest),
        ("408 Request Timeout", UpdateFailureCategory::RequestTimeout),
        ("429 Too Many Requests", UpdateFailureCategory::RateLimited),
        ("503 Unavailable", UpdateFailureCategory::Server),
    ] {
        let endpoint = serve(vec![(Duration::ZERO, response(status, "", ""))]);
        let adapter = BoundedManifestAdapter::for_test(
            endpoint,
            "darwin-aarch64",
            InstallKind::MacosAppArchive,
            "1.0.0",
            Duration::from_secs(1),
        )
        .expect("adapter");
        let error = adapter.check().await.expect_err("status must fail");
        assert_eq!(error.failure().category(), expected);
        assert!(error.status().is_some());
    }

    for body in [
        "not-json".to_owned(),
        serde_json::json!({"version": "not-semver", "url": "https://github.com/cyunlab/narrant/releases/download/v2/Narrant.exe", "signature": "sig"}).to_string(),
        serde_json::json!({"version": "2.0.0", "url": "https://github.com/cyunlab/narrant/releases/download/v2/Narrant.exe", "signature": ""}).to_string(),
        manifest("windows-x86_64"),
    ] {
        let endpoint = serve(vec![(Duration::ZERO, response("200 OK", &body, ""))]);
        let adapter = BoundedManifestAdapter::for_test(
            endpoint,
            "darwin-aarch64",
            InstallKind::MacosAppArchive,
            "1.0.0",
            Duration::from_secs(1),
        )
        .expect("adapter");
        assert_eq!(
            adapter
                .check()
                .await
                .expect_err("contract must fail")
                .failure()
                .category(),
            UpdateFailureCategory::Manifest
        );
    }

    let endpoint = serve(vec![(
        Duration::from_millis(100),
        response("200 OK", &manifest("darwin-aarch64"), ""),
    )]);
    let adapter = BoundedManifestAdapter::for_test(
        endpoint,
        "darwin-aarch64",
        InstallKind::MacosAppArchive,
        "1.0.0",
        Duration::from_millis(10),
    )
    .expect("adapter");
    assert_eq!(
        adapter
            .check()
            .await
            .expect_err("timeout")
            .failure()
            .category(),
        UpdateFailureCategory::RequestTimeout
    );
}

#[tokio::test]
async fn redirect_policy_allows_same_origin_and_rejects_another_host() {
    let endpoint = serve(Vec::new());
    let port = endpoint.port().expect("fixture port");
    drop(endpoint);
    let allowed = serve(vec![
        (
            Duration::ZERO,
            response("302 Found", "", "Location: /asset.json\r\n"),
        ),
        (
            Duration::ZERO,
            response("200 OK", &manifest("darwin-aarch64"), ""),
        ),
    ]);
    let adapter = BoundedManifestAdapter::for_test(
        allowed,
        "darwin-aarch64",
        InstallKind::MacosAppArchive,
        "1.0.0",
        Duration::from_secs(1),
    )
    .expect("adapter");
    assert!(adapter.check().await.expect("allowed redirect").is_some());

    let denied = serve(vec![(
        Duration::ZERO,
        response(
            "302 Found",
            "",
            &format!("Location: http://localhost:{port}/asset.json\r\n"),
        ),
    )]);
    let adapter = BoundedManifestAdapter::for_test(
        denied,
        "darwin-aarch64",
        InstallKind::MacosAppArchive,
        "1.0.0",
        Duration::from_secs(1),
    )
    .expect("adapter");
    assert_eq!(
        adapter
            .check()
            .await
            .expect_err("host redirect denied")
            .failure()
            .category(),
        UpdateFailureCategory::Manifest
    );
}
