use crate::application_update::{
    FilePreferenceStore, PreferenceCommitPoint, PreferenceError, PreferenceFaultInjector,
    PreferenceSaveOutcome, PreferenceStore,
};
use std::fs;

/// 在一个精确提交点返回稳定存储错误的测试故障器。
struct FailAt(PreferenceCommitPoint);

impl PreferenceFaultInjector for FailAt {
    /// 只拒绝测试指定的提交点。
    fn checkpoint(&self, point: PreferenceCommitPoint) -> Result<(), PreferenceError> {
        if point == self.0 {
            Err(PreferenceError::Storage)
        } else {
            Ok(())
        }
    }
}

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
    assert_eq!(
        store.save_automatic_download(false),
        PreferenceSaveOutcome::Committed {
            applied_value: false,
        }
    );
    assert_eq!(
        FilePreferenceStore::new(&path)
            .load_automatic_download()
            .unwrap(),
        Some(false)
    );
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("\"automaticDownload\": false"));
    assert_eq!(
        store.save_automatic_download(true),
        PreferenceSaveOutcome::Committed {
            applied_value: true,
        }
    );
    assert_eq!(
        FilePreferenceStore::new(&path)
            .load_automatic_download()
            .unwrap(),
        Some(true)
    );
    assert!(fs::read_to_string(&path)
        .unwrap()
        .contains("\"automaticDownload\": true"));
}

/// rename 前失败明确拒绝提交，并保留正式文件的旧 bytes 与下次加载值。
#[test]
fn pre_replace_failure_is_rejected_without_changing_formal_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("update/preferences.json");
    let mut initial = FilePreferenceStore::new(&path);
    assert!(matches!(
        initial.save_automatic_download(true),
        PreferenceSaveOutcome::Committed { .. }
    ));
    let old_bytes = fs::read(&path).unwrap();
    let mut store =
        FilePreferenceStore::with_faults(&path, FailAt(PreferenceCommitPoint::BeforeReplace));

    assert_eq!(
        store.save_automatic_download(false),
        PreferenceSaveOutcome::Rejected {
            error: PreferenceError::Storage,
        }
    );
    assert_eq!(fs::read(&path).unwrap(), old_bytes);
    assert_eq!(
        FilePreferenceStore::new(&path)
            .load_automatic_download()
            .unwrap(),
        Some(true)
    );
}

/// rename 后目录屏障失败明确报告已应用值，正式 bytes 与下次加载均为新值。
#[test]
fn post_replace_sync_failure_reports_durability_unknown_with_applied_value() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("update/preferences.json");
    let mut initial = FilePreferenceStore::new(&path);
    assert!(matches!(
        initial.save_automatic_download(true),
        PreferenceSaveOutcome::Committed { .. }
    ));
    let old_bytes = fs::read(&path).unwrap();
    let mut store = FilePreferenceStore::with_faults(
        &path,
        FailAt(PreferenceCommitPoint::AfterReplaceBeforeDirectorySync),
    );

    assert_eq!(
        store.save_automatic_download(false),
        PreferenceSaveOutcome::DurabilityUnknown {
            applied_value: false,
            error: PreferenceError::Storage,
        }
    );
    let new_bytes = fs::read(&path).unwrap();
    assert_ne!(new_bytes, old_bytes);
    assert!(String::from_utf8(new_bytes)
        .unwrap()
        .contains("\"automaticDownload\": false"));
    assert_eq!(
        FilePreferenceStore::new(&path)
            .load_automatic_download()
            .unwrap(),
        Some(false)
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
