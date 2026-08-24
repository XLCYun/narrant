use super::*;
use crate::application_settings::{ApplicationSettingsApplication, BabysitProvider};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// 记录完整 capability seam 调用的确定性 bundle。
struct RecordingBundle {
    identity: &'static str,
    provider: Arc<FakeBabysitProvider>,
    prepare_calls: AtomicUsize,
    shutdown_calls: AtomicUsize,
    observed_models: Mutex<Vec<Option<String>>>,
}

impl RecordingBundle {
    /// 构造尚未发生任何 provider I/O 的 bundle。
    fn new(identity: &'static str) -> Self {
        Self {
            identity,
            provider: Arc::new(FakeBabysitProvider::default()),
            prepare_calls: AtomicUsize::new(0),
            shutdown_calls: AtomicUsize::new(0),
            observed_models: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl BabysitProviderCapabilities for RecordingBundle {
    /// 返回 fixture 的稳定 provider identity。
    fn provider_id(&self) -> &str {
        self.identity
    }
    /// 记录准备次数及传入的设置快照，并返回确定性分析快照。
    async fn prepare_analysis(
        &self,
        _workspace_path: &Path,
        preference: Option<&crate::application_settings::BabysitProviderSettings>,
    ) -> Result<PreparedBabysitAnalysis, BabysitError> {
        self.prepare_calls.fetch_add(1, Ordering::AcqRel);
        self.observed_models
            .lock()
            .unwrap()
            .push(preference.and_then(|settings| settings.model.clone()));
        Ok(PreparedBabysitAnalysis {
            provider: self.provider.clone(),
            resolved: codex::ResolvedCodexModel {
                model: "model".to_owned(),
                reasoning_effort: "high".to_owned(),
            }
            .into(),
            process_generation: "generation".to_owned(),
            _lease: None,
        })
    }
    /// 返回空目录；本 fixture 的目录内容不参与路由断言。
    async fn model_catalog(&self) -> Result<BabysitProviderModelCatalog, BabysitError> {
        Ok(codex::CodexModelCatalog { models: Vec::new() }.into())
    }
    /// 原样返回 fixture 路径，避免触碰真实文件系统。
    async fn validate_executable(&self, path: &Path) -> Result<std::path::PathBuf, BabysitError> {
        Ok(path.to_path_buf())
    }
    /// 接受 fixture executable；选择行为不参与本组断言。
    async fn select_executable(&self, _path: &Path) -> Result<(), BabysitError> {
        Ok(())
    }
    /// 记录 shutdown 次数，验证 registry 会遍历全部 bundle。
    async fn shutdown(&self) {
        self.shutdown_calls.fetch_add(1, Ordering::AcqRel);
    }
}

/// Claude capability bundle 通过 provider-neutral seam 暴露离线目录，且不会为读取目录启动 CLI。
#[tokio::test]
async fn claude_bundle_exposes_its_offline_catalog_without_runtime_io() {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(ClaudeCodeProvider::new("unused-claude"));
    let bundle = ClaudeCapabilityBundle {
        provider: provider.clone(),
        runtime: Arc::new(super::super::claude::runtime::ClaudeRuntime::new(
            provider,
            directory.path().join("settings.json"),
        )),
    };

    let models = bundle.model_catalog().await.unwrap().models();
    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["default", "best", "fable", "opus", "sonnet", "haiku", "custom"]
    );
    assert!(models
        .iter()
        .find(|model| model.id == "haiku")
        .unwrap()
        .supported_reasoning_efforts
        .is_empty());
}

#[test]
fn claude_bundle_resolves_nullable_and_explicit_settings_without_runtime_io() {
    assert_eq!(
        ClaudeCapabilityBundle::resolve_preference(
            &crate::application_settings::BabysitProviderSettings::default()
        )
        .unwrap(),
        BabysitResolvedModel {
            model: String::new(),
            reasoning_effort: String::new(),
        }
    );
    assert_eq!(
        ClaudeCapabilityBundle::resolve_preference(
            &crate::application_settings::BabysitProviderSettings {
                executable: None,
                model: Some("sonnet".to_owned()),
                reasoning_effort: Some("max".to_owned()),
            }
        )
        .unwrap(),
        BabysitResolvedModel {
            model: "sonnet".to_owned(),
            reasoning_effort: "max".to_owned(),
        }
    );
}

/// 通过公开应用 seam 验证每次分析都按设备设置选择 provider。
#[tokio::test]
async fn new_analysis_routes_to_the_selected_provider_without_touching_the_other() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let settings = ApplicationSettingsApplication::new(settings_path.clone());
    settings
        .update_provider(BabysitProvider::ClaudeCode)
        .unwrap();
    let codex = Arc::new(FakeBabysitProvider::default());
    let claude = Arc::new(FakeBabysitProvider::default());
    let application = BabysitApplication::with_provider_registry_at(
        settings_path,
        [
            (BabysitProvider::Codex, codex.clone()),
            (BabysitProvider::ClaudeCode, claude.clone()),
        ],
    );

    application
        .explain_file(
            request("review", "snapshot", "src/lib.rs"),
            text_change("src/lib.rs", Some("fn before() {}"), Some("fn after() {}")),
            &OperationContext::background().cancellation().clone(),
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(codex.generate_calls(), 0);
    assert_eq!(claude.generate_calls(), 1);
}

/// 只有选中的 bundle 才会在分析开始时执行 readiness/prepare。
#[tokio::test]
async fn readiness_is_lazy_and_limited_to_the_selected_bundle() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let settings = ApplicationSettingsApplication::new(settings_path.clone());
    settings
        .update_provider_settings(
            BabysitProvider::ClaudeCode,
            crate::application_settings::BabysitProviderSettings {
                executable: None,
                model: Some("sonnet".to_owned()),
                reasoning_effort: Some("high".to_owned()),
            },
        )
        .unwrap();
    settings
        .update_provider(BabysitProvider::ClaudeCode)
        .unwrap();
    let codex = Arc::new(RecordingBundle::new("codex"));
    let claude = Arc::new(RecordingBundle::new("claude-code"));
    let application = BabysitApplication::with_capability_registry_at(
        settings_path,
        [
            (
                BabysitProvider::Codex,
                codex.clone() as Arc<dyn BabysitProviderCapabilities>,
            ),
            (
                BabysitProvider::ClaudeCode,
                claude.clone() as Arc<dyn BabysitProviderCapabilities>,
            ),
        ],
    );

    assert_eq!(codex.prepare_calls.load(Ordering::Acquire), 0);
    assert_eq!(claude.prepare_calls.load(Ordering::Acquire), 0);
    application
        .explain_file(
            request("r", "s", "src/lib.rs"),
            text_change("src/lib.rs", None, Some("fn main() {}")),
            &OperationContext::background().cancellation().clone(),
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(codex.prepare_calls.load(Ordering::Acquire), 0);
    assert_eq!(claude.prepare_calls.load(Ordering::Acquire), 1);
    assert_eq!(
        claude.observed_models.lock().unwrap().as_slice(),
        &[Some("sonnet".to_owned())]
    );
}

/// 设置选择了未注册 provider 时返回稳定缺失错误，且不会准备其他 bundle。
#[tokio::test]
async fn missing_selected_registration_fails_without_touching_registered_bundles() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    ApplicationSettingsApplication::new(settings_path.clone())
        .update_provider(BabysitProvider::ClaudeCode)
        .unwrap();
    let codex = Arc::new(RecordingBundle::new("codex"));
    let application = BabysitApplication::with_capability_registry_at(
        settings_path,
        [(
            BabysitProvider::Codex,
            codex.clone() as Arc<dyn BabysitProviderCapabilities>,
        )],
    );

    let error = application
        .explain_file(
            request("r", "s", "src/lib.rs"),
            text_change("src/lib.rs", None, Some("fn main() {}")),
            &OperationContext::background().cancellation().clone(),
            |_| Ok(()),
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        BabysitError::ProviderSetup { provider_id, failure: BabysitProviderSetupFailure::NotFound }
            if provider_id == "claude-code"
    ));
    assert_eq!(codex.prepare_calls.load(Ordering::Acquire), 0);
}

/// provider 切换后的同一 File Change 不得命中另一 provider 的完成缓存。
#[tokio::test]
async fn provider_switch_keeps_completed_cache_identities_isolated() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let settings = ApplicationSettingsApplication::new(settings_path.clone());
    let codex = Arc::new(FakeBabysitProvider::default());
    let claude = Arc::new(FakeBabysitProvider::default());
    let application = BabysitApplication::with_provider_registry_at(
        settings_path,
        [
            (BabysitProvider::Codex, codex.clone()),
            (BabysitProvider::ClaudeCode, claude.clone()),
        ],
    );
    let content = text_change("src/lib.rs", None, Some("fn main() {}"));

    application
        .explain_file(
            request("r", "s", "src/lib.rs"),
            content.clone(),
            &OperationContext::background().cancellation().clone(),
            |_| Ok(()),
        )
        .await
        .unwrap();
    settings
        .update_provider(BabysitProvider::ClaudeCode)
        .unwrap();
    application
        .explain_file(
            request("r", "s", "src/lib.rs"),
            content,
            &OperationContext::background().cancellation().clone(),
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(codex.generate_calls(), 1);
    assert_eq!(claude.generate_calls(), 1);
}

/// 设置切换只影响后续分析，运行中的 provider/model 快照保持不变。
#[tokio::test]
async fn in_flight_analysis_keeps_the_provider_snapshot_selected_at_start() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let settings = ApplicationSettingsApplication::new(settings_path.clone());
    let codex_provider = Arc::new(InFlightModelProvider {
        resolved: Mutex::new(codex::ResolvedCodexModel {
            model: "codex-model".to_owned(),
            reasoning_effort: "high".to_owned(),
        }),
        observed: Mutex::new(Vec::new()),
        started: Notify::new(),
        release: Notify::new(),
    });
    let claude_provider = Arc::new(FakeBabysitProvider::default());
    let bundle = |identity: &str, provider: Arc<dyn BabysitAgentProvider>| {
        Arc::new(ProviderCapabilityBundle {
            identity: identity.to_owned(),
            provider,
            runtime: None,
        }) as Arc<dyn BabysitProviderCapabilities>
    };
    let application = Arc::new(BabysitApplication::with_capability_registry_at(
        settings_path,
        [
            (
                BabysitProvider::Codex,
                bundle("codex", codex_provider.clone()),
            ),
            (
                BabysitProvider::ClaudeCode,
                bundle("claude-code", claude_provider.clone()),
            ),
        ],
    ));
    let worker = {
        let application = application.clone();
        tokio::spawn(async move {
            application
                .explain_file(
                    request("r", "s", "src/lib.rs"),
                    text_change("src/lib.rs", None, Some("fn main() {}")),
                    &OperationContext::background().cancellation().clone(),
                    |_| Ok(()),
                )
                .await
        })
    };
    codex_provider.started.notified().await;
    settings
        .update_provider(BabysitProvider::ClaudeCode)
        .unwrap();
    codex_provider.release.notify_one();
    worker.await.unwrap().unwrap();

    assert_eq!(claude_provider.generate_calls(), 0);
    assert_eq!(
        codex_provider.observed.lock().unwrap().as_slice(),
        &[("codex-model".to_owned(), "high".to_owned())]
    );
}

/// 应用退出必须恰好关闭 registry 中的每个 capability bundle。
#[tokio::test]
async fn shutdown_visits_every_registered_provider_once() {
    let directory = tempfile::tempdir().unwrap();
    let codex = Arc::new(FakeBabysitProvider::default());
    let claude = Arc::new(FakeBabysitProvider::default());
    let application = BabysitApplication::with_provider_registry_at(
        directory.path().join("settings.json"),
        [
            (BabysitProvider::Codex, codex.clone()),
            (BabysitProvider::ClaudeCode, claude.clone()),
        ],
    );

    application.shutdown().await.unwrap();

    assert_eq!(codex.shutdown_calls(), 1);
    assert_eq!(claude.shutdown_calls(), 1);
}
