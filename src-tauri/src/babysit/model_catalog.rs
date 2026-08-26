use super::{claude, codex};
use serde::{Deserialize, Serialize};

/// 设置页与 provider adapters 共用的一项模型能力。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BabysitModelCapability {
    pub id: String,
    #[serde(skip_serializing)]
    pub model: String,
    pub name: String,
    pub is_default: bool,
    pub hidden: bool,
    pub supported_reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: String,
}

impl From<codex::CodexModelCapability> for BabysitModelCapability {
    /// 将 Codex 协议目录项逐字段转换到 provider-neutral 能力。
    fn from(value: codex::CodexModelCapability) -> Self {
        Self {
            id: value.id,
            model: value.model,
            name: value.name,
            is_default: value.is_default,
            hidden: value.hidden,
            supported_reasoning_efforts: value.supported_reasoning_efforts,
            default_reasoning_effort: value.default_reasoning_effort,
        }
    }
}
/// Provider-neutral 的模型解析结果；具体目录协议只在 adapter 内转换。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BabysitResolvedModel {
    pub(crate) model: String,
    pub(crate) reasoning_effort: String,
}

impl From<codex::ResolvedCodexModel> for BabysitResolvedModel {
    /// 将 Codex adapter 的内部解析结果投影到 provider-neutral 快照。
    fn from(value: codex::ResolvedCodexModel) -> Self {
        Self {
            model: value.model,
            reasoning_effort: value.reasoning_effort,
        }
    }
}

impl From<claude::ResolvedClaudeModel> for BabysitResolvedModel {
    /// 将 Claude nullable CLI 覆盖投影为空字符串哨兵，直到构造 provider request 时再还原。
    fn from(value: claude::ResolvedClaudeModel) -> Self {
        Self {
            model: value.model.unwrap_or_default(),
            reasoning_effort: value.reasoning_effort.unwrap_or_default(),
        }
    }
}

/// Provider-neutral 的模型目录快照；任意 adapter 均可直接构造。
#[derive(Clone)]
pub(crate) struct BabysitProviderModelCatalog {
    models: Vec<BabysitModelCapability>,
}

impl BabysitProviderModelCatalog {
    /// 返回设置页需要的模型能力列表。
    pub(super) fn models(&self) -> Vec<BabysitModelCapability> {
        self.models.clone()
    }

    /// 使用当前目录快照校验并解析一次模型偏好。
    pub(super) fn resolve(
        &self,
        preference: &crate::application_settings::BabysitModelPreference,
    ) -> Result<BabysitResolvedModel, ()> {
        let (model, requested_effort) = match preference {
            crate::application_settings::BabysitModelPreference::Automatic => {
                (self.models.iter().find(|model| model.is_default), None)
            }
            crate::application_settings::BabysitModelPreference::Explicit {
                model,
                reasoning_effort,
            } => (
                self.models.iter().find(|candidate| candidate.id == *model),
                reasoning_effort.as_deref(),
            ),
        };
        let model = model.ok_or(())?;
        let effort = requested_effort.unwrap_or(&model.default_reasoning_effort);
        if !model
            .supported_reasoning_efforts
            .iter()
            .any(|supported| supported == effort)
        {
            return Err(());
        }
        Ok(BabysitResolvedModel {
            model: model.model.clone(),
            reasoning_effort: effort.to_owned(),
        })
    }
}

impl From<codex::CodexModelCatalog> for BabysitProviderModelCatalog {
    /// 将 Codex 动态目录包装到 provider-neutral registry seam。
    fn from(value: codex::CodexModelCatalog) -> Self {
        Self {
            models: value.models.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<claude::ClaudeModelCatalog> for BabysitProviderModelCatalog {
    /// 将 Claude 离线 alias/custom 目录投影到设置页共用的 provider-neutral 能力。
    fn from(value: claude::ClaudeModelCatalog) -> Self {
        let models = value
            .models
            .into_iter()
            .map(|model| {
                let is_default = model.id == "default";
                BabysitModelCapability {
                    model: if is_default {
                        String::new()
                    } else {
                        model.id.clone()
                    },
                    id: model.id,
                    name: model.name,
                    is_default,
                    hidden: false,
                    supported_reasoning_efforts: model
                        .supported_reasoning_efforts
                        .iter()
                        .filter_map(Clone::clone)
                        .collect(),
                    default_reasoning_effort: String::new(),
                }
            })
            .collect();
        Self { models }
    }
}
/// 设置页一次模型编辑会话使用的不可持久化目录快照。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BabysitModelCatalog {
    /// 目录所属的稳定 provider ID，避免 revision 跨 provider 误用。
    pub provider: crate::application_settings::BabysitProvider,
    pub revision: String,
    pub models: Vec<BabysitModelCapability>,
}
