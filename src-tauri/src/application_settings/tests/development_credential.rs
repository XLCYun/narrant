use super::credential::{CustomCredentialAdapter, DevelopmentCustomCredentialAdapter};
use super::{
    ApplicationSettingsApplication, CustomProviderConnection, CustomProviderEffort,
    CustomProviderProtocol,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// 只记录读取次数的迁移来源，用于证明开发凭据文件会屏蔽钥匙串访问。
struct CountingCredentials {
    value: Option<String>,
    load_count: AtomicUsize,
}

impl CountingCredentials {
    /// 创建持有可选 fixture key 的计数 adapter。
    fn new(value: Option<&str>) -> Self {
        Self {
            value: value.map(str::to_owned),
            load_count: AtomicUsize::new(0),
        }
    }

    /// 返回迁移来源被读取的次数。
    fn load_count(&self) -> usize {
        self.load_count.load(Ordering::Relaxed)
    }
}

impl CustomCredentialAdapter for CountingCredentials {
    /// 记录一次读取并返回冻结的 fixture key。
    fn load(&self) -> Result<Option<String>, String> {
        self.load_count.fetch_add(1, Ordering::Relaxed);
        Ok(self.value.clone())
    }

    /// 迁移来源在这些测试中只读，写入即表示 seam 使用错误。
    fn save(&self, _value: &str) -> Result<(), String> {
        Err("fixture migration source is read-only".to_owned())
    }

    /// 迁移来源在这些测试中只读，删除即表示 seam 使用错误。
    fn delete(&self) -> Result<(), String> {
        Err("fixture migration source is read-only".to_owned())
    }
}

/// 构造开发凭据测试使用的完整 Custom 连接。
fn custom_connection(api_key: &str) -> CustomProviderConnection {
    CustomProviderConnection {
        protocol: CustomProviderProtocol::OpenaiChatCompletions,
        base_url: "https://example.test/v1".to_owned(),
        api_key: api_key.to_owned(),
        model: "fixture-model".to_owned(),
        effort: CustomProviderEffort::ProviderDefault,
    }
}

/// Debug 默认 adapter 把 key 写入独立本地文件，并保持正式 settings.json 无 secret。
#[test]
fn debug_custom_connection_round_trip_uses_local_credential_file() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let credential_path = directory.path().join("custom-provider-credential.dev.json");
    let application = ApplicationSettingsApplication::new(settings_path.clone());

    application
        .update_custom_connection(custom_connection("local-dev-key"))
        .unwrap();

    let reloaded = ApplicationSettingsApplication::new(settings_path)
        .load_custom_connection()
        .unwrap();
    assert_eq!(reloaded.api_key, "local-dev-key");
    assert!(credential_path.is_file());
    let settings_json = std::fs::read_to_string(directory.path().join("settings.json")).unwrap();
    assert!(!settings_json.contains("local-dev-key"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(credential_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

/// 已存在的 Debug 本地 key 优先于迁移来源，因而不会触发钥匙串读取。
#[test]
fn debug_local_credential_prevents_keychain_access() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let keychain = Arc::new(CountingCredentials::new(Some("keychain-key")));
    let adapter = DevelopmentCustomCredentialAdapter::new(&settings_path, keychain.clone(), true);
    adapter.save("local-key").unwrap();

    assert_eq!(adapter.load().unwrap().as_deref(), Some("local-key"));
    assert_eq!(keychain.load_count(), 0);
}

/// 显式迁移最多读取一次钥匙串，随后新 adapter 只读取已经冻结的本地文档。
#[test]
fn debug_keychain_import_is_persisted_for_future_loads() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let keychain = Arc::new(CountingCredentials::new(Some("migrated-key")));
    let importing = DevelopmentCustomCredentialAdapter::new(&settings_path, keychain.clone(), true);

    assert_eq!(importing.load().unwrap().as_deref(), Some("migrated-key"));
    assert_eq!(keychain.load_count(), 1);

    let future_keychain = Arc::new(CountingCredentials::new(Some("unexpected-key")));
    let reloaded =
        DevelopmentCustomCredentialAdapter::new(&settings_path, future_keychain.clone(), true);
    assert_eq!(reloaded.load().unwrap().as_deref(), Some("migrated-key"));
    assert_eq!(future_keychain.load_count(), 0);
}

/// Debug 删除写入空值哨兵，即使迁移开关仍存在也不会恢复旧钥匙串 key。
#[test]
fn debug_deleted_credential_does_not_reimport_keychain() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let keychain = Arc::new(CountingCredentials::new(Some("keychain-key")));
    let adapter = DevelopmentCustomCredentialAdapter::new(&settings_path, keychain.clone(), true);
    adapter.save("local-key").unwrap();
    adapter.delete().unwrap();

    assert_eq!(adapter.load().unwrap(), None);
    assert_eq!(keychain.load_count(), 0);
}
