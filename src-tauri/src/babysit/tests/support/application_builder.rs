use super::*;

/// 测试 provider 额外提供可控的模型解析，不扩张生产 provider interface。
#[async_trait]
pub(crate) trait TestBabysitAgentProvider: BabysitAgentProvider {
    /// 返回本轮测试分析冻结的 provider-neutral 模型配置。
    async fn resolve_test_model(&self) -> Result<BabysitResolvedModel, BabysitError>;
}

/// 测试侧把 deterministic provider 适配到真实 capability registry seam。
struct TestCapabilityBundle {
    identity: String,
    provider: Arc<dyn TestBabysitAgentProvider>,
    runtime: Option<Arc<dyn InstalledProviderRuntime>>,
}

#[async_trait]
impl BabysitProviderCapabilities for TestCapabilityBundle {
    /// 准备 runtime 并冻结测试指定的模型结果。
    async fn prepare_analysis(
        &self,
        workspace_path: &Path,
        _preference: Option<&crate::application_settings::BabysitProviderSettings>,
    ) -> Result<PreparedBabysitAnalysis, BabysitError> {
        if let Some(runtime) = &self.runtime {
            runtime.ensure_ready(workspace_path).await?;
        }
        Ok(PreparedBabysitAnalysis {
            provider: self.provider.clone(),
            resolved: self.provider.resolve_test_model().await?,
            process_generation: "test-process-v1".to_owned(),
            _lease: None,
        })
    }

    /// 通用测试 adapter 不模拟模型目录。
    async fn model_catalog(&self) -> Result<BabysitProviderModelCatalog, BabysitError> {
        Err(BabysitError::Provider {
            detail: "测试 provider 不支持模型目录。".to_owned(),
        })
    }

    /// 通用测试 adapter 不模拟 executable 校验。
    async fn validate_executable(&self, _path: &Path) -> Result<std::path::PathBuf, BabysitError> {
        Err(BabysitError::provider_setup(
            &self.identity,
            BabysitProviderSetupFailure::ExecutableInvalid,
        ))
    }

    /// 优先释放 runtime；无 runtime 时直接释放 deterministic provider。
    async fn shutdown(&self) {
        if let Some(runtime) = &self.runtime {
            runtime.shutdown().await;
        } else {
            self.provider.shutdown().await;
        }
    }
}

/// 测试侧统一通过 capability registry seam 构造宝宝模式应用。
pub(crate) struct TestBabysitApplication;

impl TestBabysitApplication {
    /// 将单个 deterministic provider 注册为默认 Codex adapter。
    pub(crate) fn with_provider<P>(provider: Arc<P>) -> BabysitApplication
    where
        P: TestBabysitAgentProvider + 'static,
    {
        Self::with_optional_runtime(provider, None)
    }

    /// 将 provider 与 installed runtime 组合成默认 Codex capability bundle。
    pub(crate) fn with_provider_runtime<P>(
        provider: Arc<P>,
        runtime: Arc<dyn InstalledProviderRuntime>,
    ) -> BabysitApplication
    where
        P: TestBabysitAgentProvider + 'static,
    {
        Self::with_optional_runtime(provider, Some(runtime))
    }

    /// 使用持久化设置选择多个 provider adapter。
    pub(crate) fn with_providers_at<const N: usize, P>(
        settings_path: impl Into<std::path::PathBuf>,
        providers: [(crate::application_settings::BabysitProvider, Arc<P>); N],
    ) -> BabysitApplication
    where
        P: TestBabysitAgentProvider + 'static,
    {
        Self::with_provider_objects_at(
            settings_path,
            providers.map(|(identity, provider)| {
                (identity, provider as Arc<dyn TestBabysitAgentProvider>)
            }),
        )
    }

    /// 使用持久化设置注入可包含不同 concrete 类型的测试 providers。
    pub(crate) fn with_provider_objects_at<const N: usize>(
        settings_path: impl Into<std::path::PathBuf>,
        providers: [(
            crate::application_settings::BabysitProvider,
            Arc<dyn TestBabysitAgentProvider>,
        ); N],
    ) -> BabysitApplication {
        let bundles = providers.map(|(identity, provider)| {
            (
                identity,
                Arc::new(TestCapabilityBundle {
                    identity: identity.identity().to_owned(),
                    provider,
                    runtime: None,
                }) as Arc<dyn BabysitProviderCapabilities>,
            )
        });
        Self::with_capabilities_at(settings_path, bundles)
    }

    /// 使用持久化设置注入完整 capability bundles。
    pub(crate) fn with_capabilities_at<const N: usize>(
        settings_path: impl Into<std::path::PathBuf>,
        bundles: [(
            crate::application_settings::BabysitProvider,
            Arc<dyn BabysitProviderCapabilities>,
        ); N],
    ) -> BabysitApplication {
        BabysitApplication::from_registry(
            bundles.into_iter().collect(),
            Some(
                crate::application_settings::ApplicationSettingsApplication::new(
                    settings_path.into(),
                ),
            ),
        )
    }

    /// 构造不读取持久化设置的单 provider registry，默认选择 Codex identity。
    fn with_optional_runtime<P>(
        provider: Arc<P>,
        runtime: Option<Arc<dyn InstalledProviderRuntime>>,
    ) -> BabysitApplication
    where
        P: TestBabysitAgentProvider + 'static,
    {
        let provider = provider as Arc<dyn TestBabysitAgentProvider>;
        let bundle = Arc::new(TestCapabilityBundle {
            identity: crate::application_settings::BabysitProvider::Codex
                .identity()
                .to_owned(),
            provider,
            runtime,
        }) as Arc<dyn BabysitProviderCapabilities>;
        BabysitApplication::from_registry(
            [(crate::application_settings::BabysitProvider::Codex, bundle)]
                .into_iter()
                .collect(),
            None,
        )
    }
}
