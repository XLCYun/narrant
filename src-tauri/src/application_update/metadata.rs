use super::config::{trusted_manifest_url, trusted_package_url, TrustedUpdaterConfig};
use super::{InstallKind, ReleasePackage, StableRelease, UpdateFailure, UpdateFailureCategory};
use futures_util::StreamExt;
use reqwest::{StatusCode, Url};
use semver::Version;
use std::time::Duration;
use tauri_plugin_updater::RemoteRelease;

const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(30);

/// 保留稳定失败分类与 HTTP status、但不携带响应体或 URL 的元数据错误。
#[derive(Debug)]
pub(crate) struct ManifestAdapterError {
    failure: UpdateFailure,
    status: Option<u16>,
}

impl ManifestAdapterError {
    /// 返回供领域状态机消费的稳定失败分类。
    pub(crate) fn failure(&self) -> &UpdateFailure {
        &self.failure
    }

    /// 返回服务端明确给出的 HTTP status，网络与合约错误没有 status。
    pub(crate) fn status(&self) -> Option<u16> {
        self.status
    }

    /// 创建不包含外部文本的稳定错误。
    fn new(category: UpdateFailureCategory, diagnostic: &'static str) -> Self {
        Self {
            failure: UpdateFailure::new(category, diagnostic),
            status: None,
        }
    }

    /// 从 HTTP status 创建精确的 retryability 分类。
    fn from_status(status: StatusCode) -> Self {
        Self {
            failure: UpdateFailure::from_http_status(status.as_u16(), "manifest-status"),
            status: Some(status.as_u16()),
        }
    }
}

/// 使用固定 endpoint 与 Tauri stable manifest wire semantics 的有界元数据适配器。
pub(crate) struct BoundedManifestAdapter {
    client: reqwest::Client,
    endpoint: Url,
    target: String,
    install_kind: InstallKind,
    current_version: Version,
}

impl BoundedManifestAdapter {
    /// 从 production trust root 创建 30 秒、64 KiB 硬上限的 rustls 客户端。
    pub(crate) fn new(
        config: &TrustedUpdaterConfig,
        target: impl Into<String>,
        install_kind: InstallKind,
        current_version: &str,
    ) -> Result<Self, ManifestAdapterError> {
        Self::build(
            config.endpoint().clone(),
            target,
            install_kind,
            current_version,
            MANIFEST_TIMEOUT,
            false,
        )
    }

    /// 创建适配器，并在测试中允许环回 HTTP fixture。
    fn build(
        endpoint: Url,
        target: impl Into<String>,
        install_kind: InstallKind,
        current_version: &str,
        timeout: Duration,
        allow_loopback_http: bool,
    ) -> Result<Self, ManifestAdapterError> {
        let initial = endpoint.clone();
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                let candidate = attempt.url();
                let allowed = if allow_loopback_http {
                    candidate.scheme() == initial.scheme()
                        && candidate.host_str() == initial.host_str()
                        && candidate.port_or_known_default() == initial.port_or_known_default()
                } else {
                    trusted_manifest_url(candidate)
                };
                if attempt.previous().len() >= 5 {
                    attempt.stop()
                } else if allowed {
                    attempt.follow()
                } else {
                    attempt.error("untrusted manifest redirect")
                }
            }))
            .build()
            .map_err(|_| ManifestAdapterError::new(UpdateFailureCategory::Network, "client"))?;
        Ok(Self {
            client,
            endpoint,
            target: target.into(),
            install_kind,
            current_version: Version::parse(current_version).map_err(|_| {
                ManifestAdapterError::new(UpdateFailureCategory::Manifest, "current-version")
            })?,
        })
    }

    /// 获取并在 JSON DOM 创建前逐块施加 64 KiB hard cap。
    pub(crate) async fn check(&self) -> Result<Option<StableRelease>, ManifestAdapterError> {
        let response = self
            .client
            .get(self.endpoint.clone())
            .send()
            .await
            .map_err(classify_request_error)?;
        let status = response.status();
        if status == StatusCode::NO_CONTENT {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(ManifestAdapterError::from_status(status));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_MANIFEST_BYTES as u64)
        {
            return Err(ManifestAdapterError::new(
                UpdateFailureCategory::Manifest,
                "manifest-too-large",
            ));
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(classify_request_error)?;
            if body.len().saturating_add(chunk.len()) > MAX_MANIFEST_BYTES {
                return Err(ManifestAdapterError::new(
                    UpdateFailureCategory::Manifest,
                    "manifest-too-large",
                ));
            }
            body.extend_from_slice(&chunk);
        }
        let remote: RemoteRelease = serde_json::from_slice(&body).map_err(|_| {
            ManifestAdapterError::new(UpdateFailureCategory::Manifest, "manifest-json")
        })?;
        if !remote.version.pre.is_empty() {
            return Err(ManifestAdapterError::new(
                UpdateFailureCategory::Manifest,
                "manifest-prerelease",
            ));
        }
        if remote.version <= self.current_version {
            return Ok(None);
        }
        let url = remote.download_url(&self.target).map_err(|_| {
            ManifestAdapterError::new(UpdateFailureCategory::Manifest, "manifest-target")
        })?;
        let signature = remote.signature(&self.target).map_err(|_| {
            ManifestAdapterError::new(UpdateFailureCategory::Manifest, "manifest-signature")
        })?;
        if signature.trim().is_empty() || !trusted_package_url(url) {
            return Err(ManifestAdapterError::new(
                UpdateFailureCategory::Manifest,
                "manifest-package",
            ));
        }
        let package = ReleasePackage::new(
            url.to_string(),
            signature.clone(),
            self.target.clone(),
            self.install_kind,
        )
        .map_err(|_| {
            ManifestAdapterError::new(UpdateFailureCategory::Manifest, "manifest-contract")
        })?;
        Ok(Some(StableRelease::new(
            remote.version.to_string(),
            remote.notes.unwrap_or_default(),
            package,
        )))
    }

    /// 创建仅供真实环回 HTTP fixture 使用的测试适配器。
    #[cfg(test)]
    pub(crate) fn for_test(
        endpoint: Url,
        target: &str,
        install_kind: InstallKind,
        current_version: &str,
        timeout: Duration,
    ) -> Result<Self, ManifestAdapterError> {
        Self::build(
            endpoint,
            target,
            install_kind,
            current_version,
            timeout,
            true,
        )
    }
}

/// 将 reqwest timeout/transport 与 redirect contract 分到稳定错误类别。
fn classify_request_error(error: reqwest::Error) -> ManifestAdapterError {
    if error.is_timeout() {
        ManifestAdapterError::new(UpdateFailureCategory::RequestTimeout, "manifest-timeout")
    } else if error.is_redirect() {
        ManifestAdapterError::new(UpdateFailureCategory::Manifest, "manifest-redirect")
    } else {
        ManifestAdapterError::new(UpdateFailureCategory::Network, "manifest-transport")
    }
}

#[cfg(test)]
#[path = "tests/metadata.rs"]
mod tests;
