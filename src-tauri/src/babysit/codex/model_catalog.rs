use crate::application_settings::BabysitModelPreference;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

/// Codex App Server 当前公开的一项模型能力。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexModelCapability {
    #[serde(alias = "model")]
    pub id: String,
    #[serde(alias = "displayName")]
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    #[serde(alias = "isHidden")]
    pub hidden: bool,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_reasoning_efforts")]
    pub supported_reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: String,
}

/// App Server 不同兼容版本可能把 effort 表示为字符串或带说明的对象。
#[derive(Deserialize)]
#[serde(untagged)]
enum ReasoningEffortValue {
    Name(String),
    Detail {
        #[serde(alias = "reasoningEffort")]
        effort: String,
    },
}

/// 将兼容的 effort 表示统一投影为稳定字符串标识。
fn deserialize_reasoning_efforts<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<ReasoningEffortValue>::deserialize(deserializer).map(|values| {
        values
            .into_iter()
            .map(|value| match value {
                ReasoningEffortValue::Name(name) => name,
                ReasoningEffortValue::Detail { effort } => effort,
            })
            .collect()
    })
}

/// UI 与分析前验证共用的、已过滤隐藏项的动态模型目录。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexModelCatalog {
    pub models: Vec<CodexModelCapability>,
}

/// 一次分析解析后固定使用的模型与思考强度。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedCodexModel {
    pub model: String,
    pub reasoning_effort: String,
}

/// model/list 单页的最小协议表示。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ModelListPage {
    data: Vec<CodexModelCapability>,
    next_cursor: Option<String>,
}

impl ModelListPage {
    /// 严格解码一页目录；缺失稳定字段时拒绝静默猜测。
    pub(super) fn decode(value: Value) -> Result<Self, ()> {
        serde_json::from_value(value).map_err(|_| ())
    }

    /// 将本页追加到完整目录，并返回下一页游标。
    pub(super) fn append_to(self, models: &mut Vec<CodexModelCapability>) -> Option<String> {
        models.extend(self.data);
        self.next_cursor.filter(|cursor| !cursor.trim().is_empty())
    }
}

impl CodexModelCatalog {
    /// 从完整分页结果建立可展示目录，并验证唯一且可见的默认模型。
    pub(super) fn from_pages(models: Vec<CodexModelCapability>) -> Result<Self, ()> {
        let visible = models
            .into_iter()
            .filter(|model| !model.hidden)
            .collect::<Vec<_>>();
        let unique_ids = visible
            .iter()
            .map(|model| model.id.as_str())
            .collect::<HashSet<_>>();
        if unique_ids.len() != visible.len()
            || visible.iter().filter(|model| model.is_default).count() != 1
            || visible.iter().any(|model| {
                model.id.trim().is_empty()
                    || model.name.trim().is_empty()
                    || model.default_reasoning_effort.trim().is_empty()
                    || !model
                        .supported_reasoning_efforts
                        .contains(&model.default_reasoning_effort)
            })
        {
            return Err(());
        }
        Ok(Self { models: visible })
    }

    /// 按当前偏好解析一次不可变分析身份；显式失效绝不回退。
    pub(crate) fn resolve(
        &self,
        preference: &BabysitModelPreference,
    ) -> Result<ResolvedCodexModel, ()> {
        let (model, requested_effort) = match preference {
            BabysitModelPreference::Automatic => (
                self.models.iter().find(|model| model.is_default),
                None,
            ),
            BabysitModelPreference::Explicit {
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
        Ok(ResolvedCodexModel {
            model: model.id.clone(),
            reasoning_effort: effort.to_owned(),
        })
    }
}

#[cfg(test)]
#[path = "tests/model_catalog.rs"]
mod tests;
