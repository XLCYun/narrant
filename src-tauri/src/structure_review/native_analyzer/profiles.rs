//! 迁移语言的固定政策只通过 profile 表达，不扩展协议消息字段。

/// 新 helper 的语言政策；Rust 三档与既有用户选项一一对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeProfile {
    RustRepository,
    RustStandardLibrary,
    RustAllDependencies,
    TypeScriptJavascript,
    Python,
}

impl NativeProfile {
    /// 这些稳定身份由 ready 宣告，并由 analyze 原样回传。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RustRepository => "rust-repository-v1",
            Self::RustStandardLibrary => "rust-standard-library-v1",
            Self::RustAllDependencies => "rust-all-dependencies-v1",
            Self::TypeScriptJavascript => "typescript-static-v1",
            Self::Python => "python-static-v1",
        }
    }

    /// 只识别迁移政策；已有四套 profile 的校验行为继续保持。
    pub(super) fn parse(value: &str) -> Option<Self> {
        [
            Self::RustRepository,
            Self::RustStandardLibrary,
            Self::RustAllDependencies,
            Self::TypeScriptJavascript,
            Self::Python,
        ]
        .into_iter()
        .find(|p| p.as_str() == value)
    }
}

/// 工具链与网络变量按固定白名单快照；禁止继承编译器包装器、构建参数或进程注入。
pub(crate) fn rust_environment(
    profile: NativeProfile,
    lookup: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> {
    let mut environment = std::collections::BTreeMap::new();
    for key in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "CARGO_NET_OFFLINE",
        "CARGO_NET_RETRY",
        "CARGO_NET_GIT_FETCH_WITH_CLI",
        "CARGO_HTTP_PROXY",
        "CARGO_HTTP_TIMEOUT",
        "CARGO_HTTP_CAINFO",
        "CARGO_HTTP_SSL_VERIFY",
        "CARGO_HTTP_CHECK_REVOKE",
        "CARGO_HTTP_MULTIPLEXING",
        "CARGO_HTTP_LOW_SPEED_LIMIT",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ] {
        if let Some(value) = lookup(key) {
            environment.insert(key.into(), value);
        }
    }
    environment.insert("RUSTUP_AUTO_INSTALL".into(), "0".into());
    environment.insert("CARGO_CACHE_RUSTC_INFO".into(), "0".into());
    if profile != NativeProfile::RustAllDependencies {
        environment.insert("CARGO_NET_OFFLINE".into(), "true".into());
    }
    environment
}
