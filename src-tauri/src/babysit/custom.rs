use super::*;

// expand-first seam：#143 会把两个协议 adapter 接入共享 Custom runtime。
#[allow(dead_code)]
mod anthropic;

#[allow(unused_imports)]
pub(crate) use anthropic::{
    AnthropicAdapter, AnthropicAdapterConfig, AnthropicAdapterError, AnthropicAdapterRequest,
    AnthropicAdapterResponse,
};

/// Custom provider 在 registry 中使用的稳定 identity。
pub(crate) const PROVIDER_ID: &str = "custom";

/// 尚未接入 Rig transport 的 Custom provider；本切片只负责配置 readiness。
pub(crate) struct CustomProvider;

impl CustomProvider {
    /// 构造无网络状态的 Custom provider，不读取 endpoint 或 credential。
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait]
impl BabysitAgentProvider for CustomProvider {
    /// 配置变化不应复用先前的 Custom 运行结果。
    fn process_generation(&self) -> String {
        "custom-config-v1".to_owned()
    }

    /// 139B 不实现 Rig 请求；配置就绪后以稳定 setup failure 表示待后续切片接入。
    async fn generate(
        &self,
        _request: BabysitProviderRequest,
        _cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        Err(BabysitError::provider_setup(
            PROVIDER_ID,
            BabysitProviderSetupFailure::Incompatible,
        ))
    }

    /// 139B 不创建模型会话，因此也不执行 repair 请求。
    async fn repair(
        &self,
        _request: BabysitProviderRequest,
        _previous: BabysitFileExplanations,
        _failures: Vec<ValidationFailure>,
        _cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        Err(BabysitError::provider_setup(
            PROVIDER_ID,
            BabysitProviderSetupFailure::Incompatible,
        ))
    }
}

/// Custom capability bundle 只投影 settings readiness，不做 endpoint 探测。
pub(crate) struct CustomCapabilityBundle {
    provider: Arc<CustomProvider>,
}

impl CustomCapabilityBundle {
    /// 创建空网络、空 credential ownership 的 Custom capability。
    pub(crate) fn new() -> Self {
        Self {
            provider: Arc::new(CustomProvider::new()),
        }
    }
}

#[async_trait]
impl BabysitProviderCapabilities for CustomCapabilityBundle {
    /// 从同一 settings 快照投影配置状态，并冻结模型与 effort。
    async fn prepare_analysis(
        &self,
        _workspace_path: &Path,
        preference: Option<&crate::application_settings::BabysitProviderPreference<'_>>,
    ) -> Result<PreparedBabysitAnalysis, BabysitError> {
        let Some(crate::application_settings::BabysitProviderPreference::Custom(connection)) =
            preference
        else {
            return Err(BabysitError::provider_setup(
                PROVIDER_ID,
                BabysitProviderSetupFailure::NotFound,
            ));
        };
        if !connection.is_configured() {
            return Err(BabysitError::provider_setup(
                PROVIDER_ID,
                BabysitProviderSetupFailure::NotFound,
            ));
        }
        Ok(PreparedBabysitAnalysis {
            provider: self.provider.clone(),
            resolved: BabysitResolvedModel {
                model: connection.model.trim().to_owned(),
                reasoning_effort: connection.effort.as_str().to_owned(),
            },
            process_generation: self.provider.process_generation(),
            _lease: None,
        })
    }

    /// Custom 不依赖模型目录；设置页应使用完整连接编辑器。
    async fn model_catalog(&self) -> Result<BabysitProviderModelCatalog, BabysitError> {
        Err(BabysitError::Provider {
            detail: "Custom provider 不支持模型目录。".to_owned(),
        })
    }

    /// Custom 不接受 executable 路径，避免误把 Installed 设置写入 Custom。
    async fn validate_executable(&self, _path: &Path) -> Result<std::path::PathBuf, BabysitError> {
        Err(BabysitError::provider_setup(
            PROVIDER_ID,
            BabysitProviderSetupFailure::NotFound,
        ))
    }

    /// 当前 Custom capability 没有网络或子进程资源需要释放。
    async fn shutdown(&self) {}
}

impl crate::application_settings::CustomProviderEffort {
    /// 将设置中的 effort 投影为 provider-neutral cache identity 文本。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}
