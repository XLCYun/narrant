use base64::Engine;
use minisign_verify::PublicKey;
use reqwest::Url;

pub(crate) const PRODUCTION_UPDATE_ENDPOINT: &str =
    "https://updates.cyunlab.com/narrant/channels/stable/latest.json";
const RELEASE_DOWNLOAD_PREFIX: &str = "/narrant/releases/";
const RELEASE_MANIFEST_PATH: &str = "/narrant/channels/stable/latest.json";

/// 更新信任配置的稳定失败分类，不包含公钥内容或环境变量值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdaterConfigError {
    MissingEndpoint,
    UnexpectedEndpoint,
    InsecureEndpoint,
    MissingPublicKey,
    InvalidPublicKey,
}

/// 编译进应用且同时供 updater plugin 与自定义 adapter 使用的信任配置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrustedUpdaterConfig {
    endpoint: Url,
    public_key: String,
}

impl TrustedUpdaterConfig {
    /// 读取编译期发布输入；缺失或与固定 production endpoint 不一致时关闭更新能力。
    pub(crate) fn from_build() -> Result<Self, UpdaterConfigError> {
        Self::parse(
            option_env!("NARRANT_UPDATER_ENDPOINT"),
            option_env!("NARRANT_UPDATER_PUBLIC_KEY"),
        )
    }

    /// 解析显式构建输入，测试与 production 使用同一 fail-closed 验证。
    pub(crate) fn parse(
        endpoint: Option<&str>,
        public_key: Option<&str>,
    ) -> Result<Self, UpdaterConfigError> {
        let endpoint = endpoint
            .filter(|value| !value.trim().is_empty())
            .ok_or(UpdaterConfigError::MissingEndpoint)?;
        let parsed = Url::parse(endpoint).map_err(|_| UpdaterConfigError::UnexpectedEndpoint)?;
        if parsed.scheme() != "https" {
            return Err(UpdaterConfigError::InsecureEndpoint);
        }
        if parsed.as_str() != PRODUCTION_UPDATE_ENDPOINT {
            return Err(UpdaterConfigError::UnexpectedEndpoint);
        }
        let public_key = public_key
            .filter(|value| !value.trim().is_empty())
            .ok_or(UpdaterConfigError::MissingPublicKey)?;
        decode_public_key(public_key)?;
        Ok(Self {
            endpoint: parsed,
            public_key: public_key.to_owned(),
        })
    }

    /// 返回固定且已经验证为 HTTPS 的 manifest endpoint。
    pub(crate) fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// 返回只供 updater/verifier 构建器消费的 Tauri 外层 base64 公钥。
    pub(crate) fn public_key(&self) -> &str {
        &self.public_key
    }
}

/// 解码并验证 Tauri updater 外层 base64 包装的 minisign 公钥文本。
pub(super) fn decode_public_key(encoded: &str) -> Result<PublicKey, UpdaterConfigError> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or(UpdaterConfigError::InvalidPublicKey)?;
    PublicKey::decode(&decoded).map_err(|_| UpdaterConfigError::InvalidPublicKey)
}

/// 验证 manifest 选出的包仍属于 Narrant tag-specific HTTPS Release 路径。
pub(crate) fn trusted_package_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("updates.cyunlab.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path().starts_with(RELEASE_DOWNLOAD_PREFIX)
}

/// OSS 更新包不得跨域重定向。
pub(crate) fn trusted_redirect_url(url: &Url) -> bool {
    trusted_package_url(url)
}

/// 限制 manifest 请求只能落到 OSS 的固定 Stable 对象，且不接受重定向变体。
pub(crate) fn trusted_manifest_url(url: &Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return false;
    }
    if url.fragment().is_some() {
        return false;
    }
    url.host_str() == Some("updates.cyunlab.com")
        && url.path() == RELEASE_MANIFEST_PATH
        && url.query().is_none()
}
