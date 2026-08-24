use crate::application_settings::BabysitProviderSettings;
use serde::Serialize;

/// Claude Code 在设置页公开的一项离线模型能力。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudeModelCapability {
    /// 设置与目录 revision 使用的稳定模型标识。
    pub id: String,
    /// 设置页展示名称；滚动 alias 的版本不会固化进名称。
    pub name: String,
    /// `None` 表示不传 `--effort`，其余值原样传给 Claude Code。
    pub supported_reasoning_efforts: Vec<Option<String>>,
}

/// 随 Narrant 发布且无需启动 Claude Code 的模型目录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClaudeModelCatalog {
    pub models: Vec<ClaudeModelCapability>,
}

/// 一次分析开始前冻结的 Claude Code 模型与 effort 偏好解析结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedClaudeModel {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
}

impl ClaudeModelCatalog {
    /// 构造官方滚动 alias 与 Custom 组成的离线目录，不访问账户、网络或 TUI。
    pub(crate) fn offline() -> Self {
        let all_efforts = || {
            [
                None,
                Some("low"),
                Some("medium"),
                Some("high"),
                Some("xhigh"),
                Some("max"),
            ]
            .into_iter()
            .map(|effort| effort.map(str::to_owned))
            .collect()
        };
        let model = |id: &str, name: &str, supported_reasoning_efforts| ClaudeModelCapability {
            id: id.to_owned(),
            name: name.to_owned(),
            supported_reasoning_efforts,
        };
        Self {
            models: vec![
                model("default", "Default", all_efforts()),
                model("best", "Best（使用最强的可用模型）", all_efforts()),
                model("fable", "Fable", all_efforts()),
                model("opus", "Opus", all_efforts()),
                model("sonnet", "Sonnet", all_efforts()),
                model("haiku", "Haiku", vec![None]),
                model("custom", "Custom", all_efforts()),
            ],
        }
    }

    /// 将 nullable 设置解析为 CLI 覆盖；已知 alias 拒绝不支持的 effort，自定义模型仅要求非空。
    pub(crate) fn resolve(
        &self,
        preference: &BabysitProviderSettings,
    ) -> Result<ResolvedClaudeModel, ()> {
        let model = preference.model.clone();
        if model
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(());
        }
        let reasoning_effort = preference.reasoning_effort.clone();
        if reasoning_effort
            .as_deref()
            .is_some_and(|effort| !matches!(effort, "low" | "medium" | "high" | "xhigh" | "max"))
        {
            return Err(());
        }

        if let Some(alias) = model.as_deref().filter(|value| *value != "custom") {
            if let Some(capability) = self.models.iter().find(|candidate| candidate.id == alias) {
                let supported = capability
                    .supported_reasoning_efforts
                    .iter()
                    .any(|candidate| candidate.as_deref() == reasoning_effort.as_deref());
                if !supported {
                    return Err(());
                }
            }
        }

        Ok(ResolvedClaudeModel {
            model: match model.as_deref() {
                Some("default") => None,
                _ => model,
            },
            reasoning_effort,
        })
    }
}
