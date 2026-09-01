mod adapter;
mod anthropic;
mod connection;
mod final_tool;
mod openai;
mod provider;
mod round;
mod runtime;

#[cfg(test)]
pub(crate) use adapter::{CustomAdapterError, CustomProtocolAdapter};
#[cfg(test)]
pub(crate) use anthropic::{
    AnthropicAdapter, AnthropicAdapterConfig, AnthropicAdapterError, AnthropicAdapterRequest,
};
#[cfg(test)]
pub(crate) use connection::{CustomConnectionSnapshot, CustomEffort, CustomProtocol};
#[cfg(test)]
pub(crate) use final_tool::{line_explanation_schema, FINAL_OUTPUT_TOOL_NAME};
#[cfg(test)]
pub(crate) use openai::OpenAiChatCompletionsAdapter;
pub(crate) use provider::CustomCapabilityBundle;
#[cfg(test)]
pub(crate) use provider::CustomProvider;
#[cfg(test)]
use provider::PROVIDER_ID;
#[cfg(test)]
pub(crate) use round::{CustomRoundRequest, CustomRoundResult, MAX_MODEL_TURNS, MAX_OUTPUT_TOKENS};
#[cfg(test)]
pub(crate) use runtime::CustomRuntime;
#[cfg(test)]
use runtime::MAX_REPAIR_ATTEMPTS;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
