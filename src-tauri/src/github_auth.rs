use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CREDENTIAL_SERVICE: &str = "com.narrant.github";
const CREDENTIAL_ACCOUNT: &str = "current-user";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct StoredCredentials {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    refresh_token_expires_at: u64,
}

impl StoredCredentials {
    fn from_token(token: TokenResponse, now: u64) -> Self {
        Self {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: now + token.expires_in,
            refresh_token_expires_at: now + token.refresh_token_expires_in,
        }
    }
}

pub trait CredentialStore: Send + Sync {
    fn load(&self) -> Result<Option<String>, String>;
    fn save(&self, value: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

pub trait GitHubRecentEntries: Send + Sync {
    fn clear_github_entries(&self) -> Result<(), String>;
}

pub struct NoGitHubRecentEntries;
impl GitHubRecentEntries for NoGitHubRecentEntries {
    fn clear_github_entries(&self) -> Result<(), String> {
        Ok(())
    }
}

pub struct SystemCredentialStore;
impl CredentialStore for SystemCredentialStore {
    fn load(&self) -> Result<Option<String>, String> {
        match keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
            .map_err(|_| credential_error())?
            .get_password()
        {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(credential_error()),
        }
    }

    fn save(&self, value: &str) -> Result<(), String> {
        keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
            .and_then(|entry| entry.set_password(value))
            .map_err(|_| credential_error())
    }

    fn delete(&self) -> Result<(), String> {
        match keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
            .map_err(|_| credential_error())?
            .delete_credential()
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(credential_error()),
        }
    }
}

fn credential_error() -> String {
    "无法访问当前系统用户的安全凭据存储。".to_owned()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationStatus {
    pub configured: bool,
    pub signed_in: bool,
    pub app_install_url: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthorization {
    pub authorization_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    refresh_token_expires_in: u64,
}

enum DevicePollAction {
    Pending,
    SlowDown,
    Fail(String),
}

fn device_poll_action(error: Option<&str>) -> DevicePollAction {
    match error {
        Some("authorization_pending") => DevicePollAction::Pending,
        Some("slow_down") => DevicePollAction::SlowDown,
        Some("access_denied") => DevicePollAction::Fail("你已取消 GitHub 授权。".to_owned()),
        Some("expired_token") => {
            DevicePollAction::Fail("设备码已过期，请重新开始授权。".to_owned())
        }
        Some("device_flow_disabled") => {
            DevicePollAction::Fail("此 GitHub App 尚未启用 Device Flow。".to_owned())
        }
        _ => DevicePollAction::Fail("GitHub 授权失败，请重新尝试。".to_owned()),
    }
}

pub struct GitHubAuth {
    client_id: String,
    app_slug: String,
    oauth_base_url: String,
    http: reqwest::blocking::Client,
    store: Arc<dyn CredentialStore>,
    recent_entries: Arc<dyn GitHubRecentEntries>,
    credentials: Mutex<Option<StoredCredentials>>,
    next_authorization_id: AtomicU64,
    pending_authorizations: Mutex<HashMap<String, PendingDeviceAuthorization>>,
}

#[derive(Clone)]
struct PendingDeviceAuthorization {
    device_code: String,
    expires_in: u64,
    interval: u64,
}

impl GitHubAuth {
    pub fn new(
        client_id: impl Into<String>,
        app_slug: impl Into<String>,
        oauth_base_url: impl Into<String>,
        store: Arc<dyn CredentialStore>,
        recent_entries: Arc<dyn GitHubRecentEntries>,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            app_slug: app_slug.into(),
            oauth_base_url: oauth_base_url.into().trim_end_matches('/').to_owned(),
            http: reqwest::blocking::Client::builder()
                .user_agent("Narrant/0.1")
                .build()
                .expect("build GitHub authorization client"),
            store,
            recent_entries,
            credentials: Mutex::new(None),
            next_authorization_id: AtomicU64::new(0),
            pending_authorizations: Mutex::new(HashMap::new()),
        }
    }

    pub fn status(&self) -> Result<AuthorizationStatus, String> {
        let signed_in = self.load_credentials()?.is_some();
        let configured = !self.client_id.is_empty() && !self.app_slug.is_empty();
        Ok(AuthorizationStatus {
            configured,
            signed_in,
            app_install_url: configured.then(|| {
                format!(
                    "https://github.com/apps/{}/installations/new",
                    self.app_slug
                )
            }),
        })
    }

    pub fn start_device_authorization(&self) -> Result<DeviceAuthorization, String> {
        self.require_configuration()?;
        let response = self
            .http
            .post(format!("{}/login/device/code", self.oauth_base_url))
            .header("Accept", "application/json")
            .form(&[("client_id", self.client_id.as_str())])
            .send()
            .map_err(|_| network_error())?;
        if !response.status().is_success() {
            return Err("GitHub 暂时无法开始设备授权，请稍后重试。".to_owned());
        }
        let response: DeviceCodeResponse = response
            .json()
            .map_err(|_| "GitHub 返回了无法识别的设备授权数据。".to_owned())?;
        let authorization_id = format!(
            "authorization-{}",
            self.next_authorization_id.fetch_add(1, Ordering::Relaxed)
        );
        self.pending_authorizations
            .lock()
            .map_err(|_| auth_state_error())?
            .insert(
                authorization_id.clone(),
                PendingDeviceAuthorization {
                    device_code: response.device_code.clone(),
                    expires_in: response.expires_in,
                    interval: response.interval,
                },
            );
        Ok(DeviceAuthorization {
            authorization_id,
            user_code: response.user_code,
            verification_uri: response.verification_uri,
            expires_in: response.expires_in,
            interval: response.interval,
        })
    }

    pub fn complete_device_authorization(&self, authorization_id: &str) -> Result<(), String> {
        let authorization = self
            .pending_authorizations
            .lock()
            .map_err(|_| auth_state_error())?
            .remove(authorization_id)
            .ok_or_else(|| "设备授权已失效，请重新开始授权。".to_owned())?;
        let deadline = SystemTime::now() + Duration::from_secs(authorization.expires_in);
        let mut interval = authorization.interval.max(1);
        loop {
            let response = self
                .http
                .post(format!("{}/login/oauth/access_token", self.oauth_base_url))
                .header("Accept", "application/json")
                .form(&[
                    ("client_id", self.client_id.as_str()),
                    ("device_code", authorization.device_code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .send()
                .map_err(|_| network_error())?;
            let value: serde_json::Value = response
                .json()
                .map_err(|_| "GitHub 返回了无法识别的授权结果。".to_owned())?;
            if let Ok(token) = serde_json::from_value::<TokenResponse>(value.clone()) {
                return self.persist_token(token);
            }
            match device_poll_action(value.get("error").and_then(|value| value.as_str())) {
                DevicePollAction::Pending => {}
                DevicePollAction::SlowDown => interval += 5,
                DevicePollAction::Fail(message) => return Err(message),
            }
            if SystemTime::now() >= deadline {
                return Err("设备码已过期，请重新开始授权。".to_owned());
            }
            std::thread::sleep(Duration::from_secs(interval));
        }
    }

    pub fn access_token(&self) -> Result<Option<String>, String> {
        let mut guard = self.credentials.lock().map_err(|_| auth_state_error())?;
        if guard.is_none() {
            *guard = self.read_store()?;
        }
        let Some(credentials) = guard.as_ref() else {
            return Ok(None);
        };
        if credentials.expires_at > now_seconds() + 60 {
            return Ok(Some(credentials.access_token.clone()));
        }
        if credentials.refresh_token_expires_at <= now_seconds() {
            self.store.delete()?;
            *guard = None;
            return Err(reauthorize_error());
        }
        let refresh_token = credentials.refresh_token.clone();
        match self.refresh(&refresh_token) {
            Ok(next) => {
                self.save_credentials(&next)?;
                let token = next.access_token.clone();
                *guard = Some(next);
                Ok(Some(token))
            }
            Err(_) => {
                self.store.delete()?;
                *guard = None;
                Err(reauthorize_error())
            }
        }
    }

    pub fn sign_out(&self) -> Result<(), String> {
        self.store.delete()?;
        *self.credentials.lock().map_err(|_| auth_state_error())? = None;
        self.recent_entries.clear_github_entries()
    }

    fn require_configuration(&self) -> Result<(), String> {
        if self.client_id.is_empty() || self.app_slug.is_empty() {
            Err("此构建未配置 GitHub App，仍可匿名打开公开 Pull Request。".to_owned())
        } else {
            Ok(())
        }
    }

    fn load_credentials(&self) -> Result<Option<StoredCredentials>, String> {
        let mut guard = self.credentials.lock().map_err(|_| auth_state_error())?;
        if guard.is_none() {
            *guard = self.read_store()?;
        }
        Ok(guard.clone())
    }

    fn read_store(&self) -> Result<Option<StoredCredentials>, String> {
        let Some(value) = self.store.load()? else {
            return Ok(None);
        };
        match serde_json::from_str(&value) {
            Ok(credentials) => Ok(Some(credentials)),
            Err(_) => {
                self.store.delete()?;
                Err(reauthorize_error())
            }
        }
    }

    fn save_credentials(&self, credentials: &StoredCredentials) -> Result<(), String> {
        let value = serde_json::to_string(credentials).map_err(|_| auth_state_error())?;
        self.store.save(&value)
    }

    fn persist_token(&self, token: TokenResponse) -> Result<(), String> {
        let credentials = StoredCredentials::from_token(token, now_seconds());
        self.save_credentials(&credentials)?;
        *self.credentials.lock().map_err(|_| auth_state_error())? = Some(credentials);
        Ok(())
    }

    fn refresh(&self, refresh_token: &str) -> Result<StoredCredentials, String> {
        let response = self
            .http
            .post(format!("{}/login/oauth/access_token", self.oauth_base_url))
            .header("Accept", "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .map_err(|_| network_error())?;
        if !response.status().is_success() {
            return Err(reauthorize_error());
        }
        let token: TokenResponse = response.json().map_err(|_| reauthorize_error())?;
        Ok(StoredCredentials::from_token(token, now_seconds()))
    }
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn auth_state_error() -> String {
    "GitHub 登录状态不可用。".to_owned()
}
fn network_error() -> String {
    "当前无法连接 GitHub，请检查网络后重试。".to_owned()
}
fn reauthorize_error() -> String {
    "GitHub 登录已失效，请重新授权。".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct MemoryStore(Mutex<Option<String>>);
    impl CredentialStore for MemoryStore {
        fn load(&self) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn save(&self, value: &str) -> Result<(), String> {
            *self.0.lock().unwrap() = Some(value.to_owned());
            Ok(())
        }
        fn delete(&self) -> Result<(), String> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }
    #[derive(Default)]
    struct RecentSpy(AtomicUsize);
    impl GitHubRecentEntries for RecentSpy {
        fn clear_github_entries(&self) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn device_flow_stores_tokens_only_in_the_credential_store() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/login/device/code")
                .body_contains("client_id=client-1");
            then.status(200).json_body_obj(&serde_json::json!({
                "device_code":"device-secret", "user_code":"ABCD-EFGH",
                "verification_uri":"https://github.com/login/device", "expires_in":900, "interval":1
            }));
        });
        server.mock(|when, then| {
            when.method(POST)
                .path("/login/oauth/access_token")
                .body_contains("device_code=device-secret");
            then.status(200).json_body_obj(&serde_json::json!({
                "access_token":"access-secret", "refresh_token":"refresh-secret",
                "expires_in":28800, "refresh_token_expires_in":15897600
            }));
        });
        let store = Arc::new(MemoryStore::default());
        let auth = GitHubAuth::new(
            "client-1",
            "narrant-test",
            server.base_url(),
            store.clone(),
            Arc::new(RecentSpy::default()),
        );

        let authorization = auth.start_device_authorization().unwrap();
        assert_eq!(authorization.user_code, "ABCD-EFGH");
        assert_eq!(
            serde_json::to_value(&authorization)
                .unwrap()
                .get("deviceCode"),
            None
        );
        auth.complete_device_authorization(&authorization.authorization_id)
            .unwrap();
        assert_eq!(
            auth.access_token().unwrap(),
            Some("access-secret".to_owned())
        );
        let stored = store.load().unwrap().unwrap();
        assert!(stored.contains("refresh-secret"));
    }

    #[test]
    fn concurrent_expired_token_reads_share_one_refresh_and_rotate_credentials() {
        let server = MockServer::start();
        let refresh = server.mock(|when, then| {
            when.method(POST)
                .path("/login/oauth/access_token")
                .body_contains("grant_type=refresh_token");
            then.status(200)
                .delay(Duration::from_millis(50))
                .json_body_obj(&serde_json::json!({
                    "access_token":"new-access", "refresh_token":"new-refresh",
                    "expires_in":28800, "refresh_token_expires_in":15897600
                }));
        });
        let store = Arc::new(MemoryStore::default());
        store
            .save(
                &serde_json::to_string(&StoredCredentials {
                    access_token: "old".into(),
                    refresh_token: "old-refresh".into(),
                    expires_at: 0,
                    refresh_token_expires_at: now_seconds() + 1000,
                })
                .unwrap(),
            )
            .unwrap();
        let auth = Arc::new(GitHubAuth::new(
            "client-1",
            "narrant-test",
            server.base_url(),
            store.clone(),
            Arc::new(RecentSpy::default()),
        ));
        let handles = (0..4)
            .map(|_| {
                let auth = auth.clone();
                std::thread::spawn(move || auth.access_token().unwrap())
            })
            .collect::<Vec<_>>();
        for handle in handles {
            assert_eq!(handle.join().unwrap(), Some("new-access".into()));
        }
        assert_eq!(refresh.hits(), 1);
        assert!(store.load().unwrap().unwrap().contains("new-refresh"));
    }

    #[test]
    fn failed_refresh_deletes_credentials_and_requires_authorization() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/login/oauth/access_token");
            then.status(400)
                .json_body_obj(&serde_json::json!({"error":"bad_refresh_token"}));
        });
        let store = Arc::new(MemoryStore::default());
        store
            .save(
                &serde_json::to_string(&StoredCredentials {
                    access_token: "old".into(),
                    refresh_token: "invalid".into(),
                    expires_at: 0,
                    refresh_token_expires_at: now_seconds() + 1000,
                })
                .unwrap(),
            )
            .unwrap();
        let auth = GitHubAuth::new(
            "client-1",
            "narrant-test",
            server.base_url(),
            store.clone(),
            Arc::new(RecentSpy::default()),
        );
        assert_eq!(
            auth.access_token().unwrap_err(),
            "GitHub 登录已失效，请重新授权。"
        );
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn malformed_credentials_are_deleted_and_never_exposed_as_signed_in() {
        let store = Arc::new(MemoryStore::default());
        store.save("not-json").unwrap();
        let auth = GitHubAuth::new(
            "client-1",
            "narrant-test",
            "https://github.com",
            store.clone(),
            Arc::new(RecentSpy::default()),
        );

        assert_eq!(
            auth.status().unwrap_err(),
            "GitHub 登录已失效，请重新授权。"
        );
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn sign_out_deletes_credentials_and_clears_only_through_recent_entries_contract() {
        let store = Arc::new(MemoryStore::default());
        store.save("credentials").unwrap();
        let recent = Arc::new(RecentSpy::default());
        let auth = GitHubAuth::new(
            "client",
            "narrant-test",
            "https://github.com",
            store.clone(),
            recent.clone(),
        );
        auth.sign_out().unwrap();
        assert_eq!(store.load().unwrap(), None);
        assert_eq!(recent.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn device_flow_maps_pending_slow_down_denial_expiry_and_disabled_states() {
        assert!(matches!(
            device_poll_action(Some("authorization_pending")),
            DevicePollAction::Pending
        ));
        assert!(matches!(
            device_poll_action(Some("slow_down")),
            DevicePollAction::SlowDown
        ));
        for (error, expected) in [
            ("access_denied", "你已取消 GitHub 授权。"),
            ("expired_token", "设备码已过期，请重新开始授权。"),
            (
                "device_flow_disabled",
                "此 GitHub App 尚未启用 Device Flow。",
            ),
            ("unknown", "GitHub 授权失败，请重新尝试。"),
        ] {
            let DevicePollAction::Fail(message) = device_poll_action(Some(error)) else {
                panic!("expected failure")
            };
            assert_eq!(message, expected);
        }
    }
}
