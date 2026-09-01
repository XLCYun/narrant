mod adapter;
mod agent_run;
mod anthropic;
mod connection;
mod final_tool;
mod openai;
mod provider;
mod runtime;

#[cfg(test)]
pub(crate) use adapter::{CustomAdapterError, CustomProtocolAdapter};
#[cfg(test)]
pub(crate) use agent_run::{CustomRunRequest, CustomRunResult, MAX_MODEL_TURNS, MAX_OUTPUT_TOKENS};
#[cfg(test)]
pub(crate) use anthropic::{
    AnthropicAdapter, AnthropicAdapterConfig, AnthropicAdapterError, AnthropicAdapterRequest,
};
#[cfg(test)]
pub(crate) use connection::{CustomConnectionSnapshot, CustomEffort, CustomProtocol};
#[cfg(test)]
pub(crate) use final_tool::FINAL_OUTPUT_TOOL_NAME;
#[cfg(test)]
pub(crate) use openai::OpenAiChatCompletionsAdapter;
pub(crate) use provider::CustomCapabilityBundle;
#[cfg(test)]
pub(crate) use provider::CustomProvider;
#[cfg(test)]
use provider::PROVIDER_ID;
#[cfg(test)]
pub(crate) use runtime::CustomRuntime;
#[cfg(test)]
use runtime::MAX_REPAIR_ATTEMPTS;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
