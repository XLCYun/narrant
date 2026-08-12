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
