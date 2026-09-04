use super::*;
use crate::application_settings::{
    ApplicationLanguage, ApplicationSettingsApplication, BabysitProvider,
};
use crate::operation::OperationContext;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// 构造不携带调用方语言的请求，验证语言只能来自 Application Settings。
fn request() -> BabysitFileRequest {
    BabysitFileRequest {
        identity: ReviewIdentity {
            review_id: "review-language".to_owned(),
            snapshot_id: "snapshot-language".to_owned(),
        },
        path: "src/example.ts".to_owned(),
        workspace_path: "/tmp/narrant-test-workspace".to_owned(),
        detected_language: None,
        generation_policy: BabysitGenerationPolicy::ReuseCached,
        changed_ranges: vec![
            crate::babysit::ChangedLineRange {
                side: crate::babysit::FileSide::Before,
                start_line_number: 1,
                end_line_number: 1,
            },
            crate::babysit::ChangedLineRange {
                side: crate::babysit::FileSide::After,
                start_line_number: 1,
                end_line_number: 1,
            },
        ],
    }
}

/// 让 provider 在生成中停住，观察设置切换是否影响已经冻结的语言。
struct InFlightLanguageProvider {
    observed_locales: Mutex<Vec<String>>,
    started: Notify,
    release: Notify,
}

#[async_trait]
impl BabysitAgentProvider for InFlightLanguageProvider {
    /// 返回固定 generation，使测试只观察 language identity。
    fn process_generation(&self) -> String {
        "language-process-v1".to_owned()
    }

    /// 记录启动时 locale，并等待测试明确释放当前运行。
    async fn generate(
        &self,
        request: BabysitProviderRequest,
        _cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        let first_run = {
            let mut locales = self.observed_locales.lock().unwrap();
            locales.push(request.locale.clone());
            locales.len() == 1
        };
        if first_run {
            self.started.notify_one();
            self.release.notified().await;
        }
        Ok(FakeBabysitProvider::complete_output(&request))
    }

    /// 完整 fake 输出不会进入 repair。
    async fn repair(
        &self,
        _request: BabysitProviderRequest,
        _previous: BabysitFileExplanations,
        _failures: Vec<ValidationFailure>,
        _cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        unreachable!("完整输出不会进入 repair")
    }
}

#[async_trait]
impl TestBabysitAgentProvider for InFlightLanguageProvider {
    /// 提供稳定模型组合，避免测试依赖外部 provider 目录。
    async fn resolve_test_model(&self) -> Result<BabysitResolvedModel, BabysitError> {
        Ok(BabysitResolvedModel {
            model: "language-test-model".to_owned(),
            reasoning_effort: "low".to_owned(),
        })
    }
}

/// Application Settings 只在测试中落盘语言，其他字段保持默认快照。
fn settings_at(
    path: &std::path::Path,
    language: ApplicationLanguage,
) -> ApplicationSettingsApplication {
    let settings = ApplicationSettingsApplication::new(path.to_owned());
    settings.update_language(language).unwrap();
    settings.update_provider(BabysitProvider::Codex).unwrap();
    settings
}

#[tokio::test]
async fn application_language_is_frozen_for_in_flight_run_and_regenerate_reads_latest_value() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let settings = settings_at(&settings_path, ApplicationLanguage::SimplifiedChinese);
    let provider = Arc::new(InFlightLanguageProvider {
        observed_locales: Mutex::new(Vec::new()),
        started: Notify::new(),
        release: Notify::new(),
    });
    let application = Arc::new(TestBabysitApplication::with_providers_at(
        settings_path.clone(),
        [(BabysitProvider::Codex, provider.clone())],
    ));
    let running = {
        let application = application.clone();
        tokio::spawn(async move {
            application
                .explain_file(
                    request(),
                    text_change("src/example.ts", Some("old();"), Some("next();")),
                    &OperationContext::background().cancellation().clone(),
                    |_| Ok(()),
                )
                .await
        })
    };

    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        provider.started.notified(),
    )
    .await
    .expect("provider should receive the in-flight request");
    settings
        .update_language(ApplicationLanguage::English)
        .unwrap();
    provider.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(2), running)
        .await
        .expect("in-flight request should finish after release")
        .unwrap()
        .unwrap();

    let mut regeneration = request();
    regeneration.generation_policy = BabysitGenerationPolicy::Regenerate;
    application
        .explain_file(
            regeneration,
            text_change("src/example.ts", Some("old();"), Some("next();")),
            &OperationContext::background().cancellation().clone(),
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(
        provider.observed_locales.lock().unwrap().as_slice(),
        &["zh-CN".to_owned(), "en-US".to_owned()]
    );
}

#[tokio::test]
async fn application_language_is_part_of_completed_cache_identity() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let settings = settings_at(&settings_path, ApplicationLanguage::SimplifiedChinese);
    let provider = Arc::new(FakeBabysitProvider::default());
    let application = TestBabysitApplication::with_providers_at(
        settings_path,
        [(BabysitProvider::Codex, provider.clone())],
    );
    let content = text_change(
        "src/example.ts",
        Some("const value = 1;"),
        Some("const value = 2;"),
    );
    let cancellation = OperationContext::background().cancellation().clone();

    let chinese = application
        .explain_file(request(), content.clone(), &cancellation, |_| Ok(()))
        .await
        .unwrap();
    settings
        .update_language(ApplicationLanguage::English)
        .unwrap();
    let english = application
        .explain_file(request(), content, &cancellation, |_| Ok(()))
        .await
        .unwrap();

    assert_eq!(provider.generate_calls(), 2);
    assert_ne!(chinese.after.get("1"), english.after.get("1"));
    assert!(english.after["1"].starts_with("Explains"));
}

#[tokio::test]
async fn unreadable_application_settings_fail_before_provider_generation() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    std::fs::create_dir(&settings_path).unwrap();
    let provider = Arc::new(FakeBabysitProvider::default());
    let application = TestBabysitApplication::with_providers_at(
        settings_path,
        [(BabysitProvider::Codex, provider.clone())],
    );

    let error = application
        .explain_file(
            request(),
            text_change("src/example.ts", Some("old();"), Some("next();")),
            &OperationContext::background().cancellation().clone(),
            |_| Ok(()),
        )
        .await
        .unwrap_err();

    assert_eq!(error, BabysitError::SettingsPersistenceFailed);
    assert_eq!(provider.generate_calls(), 0);
}
