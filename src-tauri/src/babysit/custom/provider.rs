use super::adapter::{CustomAdapterError, CustomProtocolAdapter};
use super::anthropic::AnthropicMessagesAdapter;
use super::connection::{CustomConnectionSnapshot, CustomProtocol};
use super::openai::OpenAiChatCompletionsAdapter;
use super::runtime::CustomRuntime;
use crate::babysit::{
    BabysitAgentProvider, BabysitError, BabysitFileExplanations, BabysitProviderCapabilities,
    BabysitProviderModelCatalog, BabysitProviderRequest, BabysitProviderSetupFailure,
    BabysitResolvedModel, PreparedBabysitAnalysis, ValidationFailure,
};
use crate::operation::CancellationSignal;
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

/// Custom provider 在 registry 中使用的稳定 identity。
pub(super) const PROVIDER_ID: &str = "custom";

/// 已冻结连接的 Custom provider；请求编排委托给共享 runtime。
pub(crate) struct CustomProvider {
    runtime: Arc<CustomRuntime>,
    adapter: Arc<dyn CustomProtocolAdapter>,
    process_generation: String,
}

impl CustomProvider {
    /// 将单一冻结 adapter 接入共享 runtime，不在构造阶段发出网络请求。
    pub(super) fn new(
        runtime: Arc<CustomRuntime>,
        adapter: Arc<dyn CustomProtocolAdapter>,
        process_generation: String,
    ) -> Self {
        Self {
            runtime,
            adapter,
            process_generation,
        }
    }
}

#[async_trait]
impl BabysitAgentProvider for CustomProvider {
    /// 冻结连接的非 secret identity 参与缓存隔离。
    fn process_generation(&self) -> String {
        self.process_generation.clone()
    }
    /// 通过共享 runtime 生成完整 before/after 结果。
    async fn generate(
        &self,
        request: BabysitProviderRequest,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        self.runtime
            .generate(self.adapter.clone(), request, cancellation)
            .await
    }
    /// 在同一内存会话中执行 validation 指向的 repair。
    async fn repair(
        &self,
        request: BabysitProviderRequest,
        previous: BabysitFileExplanations,
        failures: Vec<ValidationFailure>,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        self.runtime
            .repair(request, previous, failures, cancellation)
            .await
    }
    /// 取消、失败或完成后丢弃该分析在 runtime 中的单内存会话。
    async fn abandon(&self, request: &BabysitProviderRequest) {
        self.runtime.abandon(request.analysis_run_id);
    }
    /// 应用退出时清空 runtime 中尚未结束的 Custom 会话。
    async fn shutdown(&self) {
        self.runtime.shutdown();
    }
}

/// Custom capability bundle 负责读取完整连接并为每次分析冻结 adapter。
pub(crate) struct CustomCapabilityBundle {
    settings: crate::application_settings::ApplicationSettingsApplication,
    runtime: Arc<CustomRuntime>,
    #[cfg(test)]
    tool_boundary_cancellation: Option<crate::operation::TestCancellation>,
}

impl CustomCapabilityBundle {
    /// 创建不发起网络请求的 Custom capability，并共享进程内 runtime。
    pub(crate) fn new(
        settings: crate::application_settings::ApplicationSettingsApplication,
    ) -> Self {
        Self {
            settings,
            runtime: Arc::new(CustomRuntime::new()),
            #[cfg(test)]
            tool_boundary_cancellation: None,
        }
    }
    /// 测试专用：在真实 repository tool seam 注入一次共享取消信号。
    #[cfg(test)]
    pub(crate) fn with_tool_boundary_cancellation(
        settings: crate::application_settings::ApplicationSettingsApplication,
        cancellation: crate::operation::TestCancellation,
    ) -> Self {
        Self {
            settings,
            runtime: Arc::new(CustomRuntime::new()),
            tool_boundary_cancellation: Some(cancellation),
        }
    }
}

#[async_trait]
impl BabysitProviderCapabilities for CustomCapabilityBundle {
    /// 从同一 settings 快照投影配置状态，并冻结模型与 effort。
    async fn prepare_analysis(
        &self,
        workspace_path: &Path,
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
        let (full_connection, credential_generation) = self
            .settings
            .load_custom_connection_with_generation()
            .map_err(|_| BabysitError::Provider {
                detail: "Custom provider 连接不可用。".to_owned(),
            })?;
        let snapshot = CustomConnectionSnapshot::from_connection(
            full_connection,
            credential_generation.to_string(),
        )
        .map_err(|error| match error {
            CustomAdapterError::UnsupportedProtocol(_) => {
                BabysitError::provider_setup(PROVIDER_ID, BabysitProviderSetupFailure::Incompatible)
            }
            other => BabysitError::Provider {
                detail: other.to_string(),
            },
        })?;
        let process_generation = snapshot.cache_identity();
        let adapter: Arc<dyn CustomProtocolAdapter> = match snapshot.protocol() {
            CustomProtocol::OpenaiChatCompletions => {
                let adapter = OpenAiChatCompletionsAdapter::new(snapshot.clone(), workspace_path)
                    .map_err(|error| BabysitError::Provider {
                    detail: error.to_string(),
                })?;
                #[cfg(test)]
                let adapter = match self.tool_boundary_cancellation.clone() {
                    Some(cancellation) => adapter.with_tool_boundary_cancellation(cancellation),
                    None => adapter,
                };
                Arc::new(adapter)
            }
            CustomProtocol::AnthropicMessages => {
                let adapter = AnthropicMessagesAdapter::new(snapshot.clone(), workspace_path)
                    .map_err(|error| BabysitError::Provider {
                        detail: error.to_string(),
                    })?;
                #[cfg(test)]
                let adapter = match self.tool_boundary_cancellation.clone() {
                    Some(cancellation) => adapter.with_tool_boundary_cancellation(cancellation),
                    None => adapter,
                };
                Arc::new(adapter)
            }
        };
        let provider = Arc::new(CustomProvider::new(
            self.runtime.clone(),
            adapter,
            process_generation,
        ));
        Ok(PreparedBabysitAnalysis {
            provider,
            resolved: BabysitResolvedModel {
                model: snapshot.model().to_owned(),
                reasoning_effort: snapshot.effort().as_str().to_owned(),
            },
            process_generation: snapshot.cache_identity(),
            protocol: Some(snapshot.protocol().as_str()),
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
    /// 清空共享 runtime，不持有网络线程或子进程。
    async fn shutdown(&self) {
        self.runtime.shutdown();
    }
}
