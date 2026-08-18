use super::*;

impl OAuthDeviceCodes {
    /// 为认证行为测试构建不含密钥的 Device Flow 展示数据。
    pub(crate) fn for_test() -> Self {
        Self {
            inner: None,
            device_code: SecretString::from("test-device-code"),
            user_code: "TEST-CODE".to_owned(),
            verification_uri: "https://github.com/login/device".to_owned(),
            expires_in: 900,
            interval: 1,
        }
    }
}

#[test]
fn maps_terminal_device_flow_errors_to_recovery_guidance() {
    for (code, expected) in [
        ("access_denied", "你已取消 GitHub 授权。"),
        ("expired_token", "设备码已过期，请重新开始授权。"),
        (
            "device_flow_disabled",
            "此 GitHub App 尚未启用 Device Flow。",
        ),
        ("malformed", "GitHub 授权失败，请重新尝试。"),
    ] {
        assert_eq!(map_device_flow_error(code), expected);
    }
}

#[tokio::test]
async fn constructs_the_octocrab_client_inside_a_tokio_runtime() {
    OctocrabOAuthClient::new("https://github.com")
        .await
        .expect("在 Tokio runtime 中构建 OAuth 客户端");
}
