use crate::application_update::{FilePreferenceStore, PreferenceError, PreferenceStore};
use std::fs;

/// 缺少偏好文件时保留状态机的默认开启策略且不产生文件。
#[test]
fn missing_preference_uses_default_without_writing() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("update/preferences.json");
    let store = FilePreferenceStore::new(&path);
    assert_eq!(store.load_automatic_download().unwrap(), None);
    assert!(!path.exists());
}

/// 自动下载偏好经真实文件系统原子保存并可跨仓储实例恢复。
#[test]
fn preference_survives_repository_restart() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("update/preferences.json");
    let mut store = FilePreferenceStore::new(&path);
    store.save_automatic_download(false).unwrap();
    assert_eq!(
        FilePreferenceStore::new(&path)
            .load_automatic_download()
            .unwrap(),
        Some(false)
    );
    store.save_automatic_download(true).unwrap();
    assert_eq!(
        FilePreferenceStore::new(&path)
            .load_automatic_download()
            .unwrap(),
        Some(true)
    );
}

/// 损坏和未知版本偏好不会被误当成默认值。
#[test]
fn malformed_or_unsupported_preference_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("preferences.json");
    fs::write(&path, b"not-json").unwrap();
    assert_eq!(
        FilePreferenceStore::new(&path).load_automatic_download(),
        Err(PreferenceError::InvalidDocument)
    );
    fs::write(&path, br#"{"version":2,"automaticDownload":true}"#).unwrap();
    assert_eq!(
        FilePreferenceStore::new(&path).load_automatic_download(),
        Err(PreferenceError::InvalidDocument)
    );
}
