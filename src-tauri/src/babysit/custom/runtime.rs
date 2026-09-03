use super::adapter::{CustomAdapterError, CustomProtocolAdapter};
use super::agent_run::{CustomRunRequest, CustomRunResult};
use super::final_tool::FINAL_OUTPUT_TOOL_NAME;
use crate::babysit::{
    BabysitError, BabysitFileExplanations, BabysitProviderRequest, LineTarget, ValidationFailure,
};
use crate::operation::CancellationSignal;
use rig::completion::Message;
use rig::message::ToolCall;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Custom 运行允许的最大修复轮数；共享 runtime 负责具体 repair 编排。
pub(super) const MAX_REPAIR_ATTEMPTS: u8 = 3;

/// 一次 Custom 分析的单内存会话；generate 与 repair 共用 Rig transcript。
struct CustomSession {
    adapter: Arc<dyn CustomProtocolAdapter>,
    history: Vec<Message>,
    repair_attempts: u8,
}

/// Custom provider 的共享 runtime，统一管理跨请求 transcript、repair 和取消边界。
pub(crate) struct CustomRuntime {
    sessions: Mutex<HashMap<u64, CustomSession>>,
}

impl CustomRuntime {
    /// 创建没有外部线程或持久化状态的进程内 runtime。
    pub(crate) fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// 创建一次完整分析会话，并把单次 Rig agent loop 交给 adapter 执行。
    pub(crate) async fn generate(
        &self,
        adapter: Arc<dyn CustomProtocolAdapter>,
        request: BabysitProviderRequest,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        ensure_not_cancelled(cancellation)?;
        let run_id = request.analysis_run_id;
        self.sessions
            .lock()
            .map_err(|_| runtime_unavailable())?
            .insert(
                run_id,
                CustomSession {
                    adapter,
                    history: Vec::new(),
                    repair_attempts: 0,
                },
            );

        let result = self
            .execute_agent_run(run_id, request, generation_prompt, cancellation)
            .await;
        if result.is_err() {
            self.remove_session(run_id);
        }
        result
    }

    /// 在同一会话历史后追加 repair prompt，最多允许三次修复。
    pub(crate) async fn repair(
        &self,
        request: BabysitProviderRequest,
        _previous: BabysitFileExplanations,
        failures: Vec<ValidationFailure>,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        ensure_not_cancelled(cancellation)?;
        let run_id = request.analysis_run_id;
        {
            let mut sessions = self.sessions.lock().map_err(|_| runtime_unavailable())?;
            let session = sessions.get_mut(&run_id).ok_or_else(runtime_unavailable)?;
            if session.repair_attempts >= MAX_REPAIR_ATTEMPTS {
                return Err(BabysitError::RepairExhausted {
                    detail: "Custom repair 已达到上限。".to_owned(),
                });
            }
            session.repair_attempts += 1;
        }
        let result = self
            .execute_agent_run(
                run_id,
                request,
                |request| repair_prompt(request, &failures),
                cancellation,
            )
            .await;
        if result.is_err() {
            self.remove_session(run_id);
        }
        result
    }

    /// 丢弃取消、失败或已完成分析的内存会话。
    pub(crate) fn abandon(&self, run_id: u64) {
        self.remove_session(run_id);
    }

    /// 应用退出时清空所有仍在内存中的 Custom 会话。
    pub(crate) fn shutdown(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.clear();
        }
    }

    /// 执行一次 generate/repair 的 Rig agent loop，并严格在边界检查取消状态。
    async fn execute_agent_run<F>(
        &self,
        run_id: u64,
        request: BabysitProviderRequest,
        prompt: F,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError>
    where
        F: Fn(&BabysitProviderRequest) -> String,
    {
        ensure_not_cancelled(cancellation)?;
        let (adapter, history) = self.session_context(run_id)?;
        let prompt = Message::user(prompt(&request));
        let run = CustomRunRequest::from_prompt(
            prompt.clone(),
            history.clone(),
            request.repository_exploration,
        );
        let response = adapter
            .run_agent(run, cancellation)
            .await
            .map_err(adapter_error)?;
        // Rig future 即使刚刚完成，也不能在取消之后接受 final 或继续写入 transcript。
        ensure_not_cancelled(cancellation)?;
        self.record_response(run_id, &history, &prompt, &response)?;

        if let Some(final_call) = response
            .tool_calls()
            .into_iter()
            .find(|call| call.function.name == FINAL_OUTPUT_TOOL_NAME)
        {
            tracing::debug!(
                target: "narrant::babysit::custom",
                analysis_run_id = run_id,
                tool = FINAL_OUTPUT_TOOL_NAME,
                phase = "called",
            );
            ensure_not_cancelled(cancellation)?;
            return Ok(parse_final_result(final_call));
        }
        tracing::debug!(
            target: "narrant::babysit::custom",
            analysis_run_id = run_id,
            tool = FINAL_OUTPUT_TOOL_NAME,
            phase = "not_called",
        );
        // 让上层 validation/repair merging 决定如何处理未完成的输出。
        Ok(BabysitFileExplanations::default())
    }

    /// 读取当前会话的 adapter 和不可变消息历史，避免跨 await 持有锁。
    fn session_context(
        &self,
        run_id: u64,
    ) -> Result<(Arc<dyn CustomProtocolAdapter>, Vec<Message>), BabysitError> {
        let sessions = self.sessions.lock().map_err(|_| runtime_unavailable())?;
        let session = sessions.get(&run_id).ok_or_else(runtime_unavailable)?;
        Ok((session.adapter.clone(), session.history.clone()))
    }

    /// 将 adapter 返回的 Rig transcript 合并进跨 generation/repair 会话。
    fn record_response(
        &self,
        run_id: u64,
        previous_history: &[Message],
        prompt: &Message,
        response: &CustomRunResult,
    ) -> Result<(), BabysitError> {
        let mut sessions = self.sessions.lock().map_err(|_| runtime_unavailable())?;
        let session = sessions.get_mut(&run_id).ok_or_else(runtime_unavailable)?;
        if let Some(transcript) = response
            .transcript
            .as_deref()
            .filter(|messages| !messages.is_empty())
        {
            session.history = merge_transcript(previous_history, transcript);
        } else {
            // 没有 extended transcript 的测试/adapter 仍需保留本轮 prompt 与 assistant。
            session.history = previous_history.to_vec();
            session.history.push(prompt.clone());
            session.history.push(Message::Assistant {
                id: response.identity.message_id.clone(),
                content: response.choice.clone(),
            });
        }
        Ok(())
    }

    /// 删除指定会话；取消路径不允许它继续影响后续请求。
    fn remove_session(&self, run_id: u64) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(&run_id);
        }
    }
}

/// 生成阶段的 provider-neutral 指令，source 只进入发送给模型的 prompt。
fn generation_prompt(request: &BabysitProviderRequest) -> String {
    format!(
        "Review this complete file change and explain every requested non-empty line.\n\
Before ({path}):\n{before}\n\
After ({path}):\n{after}\n\
Requested targets:\n{targets}\n\
Write every line explanation in the language specified by locale {locale}; use Simplified Chinese for zh-CN. Keep every explanation to one or two short sentences and at most 240 Unicode characters.\n\
{exploration_guidance}\n\
When finished, call final_result with both before and after maps.",
        path = request.path,
        before = crate::babysit::numbered_content(&request.before),
        after = crate::babysit::numbered_content(&request.after),
        targets = format_targets(&request.targets),
        locale = request.locale,
        exploration_guidance = repository_exploration_guidance(request.repository_exploration),
    )
}

/// 修复阶段只请求上层 validation 指出的目标，保持同一模型会话上下文。
fn repair_prompt(request: &BabysitProviderRequest, failures: &[ValidationFailure]) -> String {
    format!(
        "Repair the previous Line Explanation result for this file change.\n\
File ({path}) before:\n{before}\n\
File ({path}) after:\n{after}\n\
Only these validation failures require new explanations:\n{failures}\n\
Write every repaired line explanation in the language specified by locale {locale}; use Simplified Chinese for zh-CN. Keep every repaired explanation preferably within 240 Unicode characters.\n\
{exploration_guidance}\n\
Return the complete result by calling final_result; preserve valid prior explanations.",
        path = request.path,
        before = crate::babysit::numbered_content(&request.before),
        after = crate::babysit::numbered_content(&request.after),
        locale = request.locale,
        failures = failures
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        exploration_guidance = repository_exploration_guidance(request.repository_exploration),
    )
}

/// 按冻结偏好切换 Custom prompt 的仓库探索约束。
fn repository_exploration_guidance(enabled: bool) -> &'static str {
    if enabled {
        "The complete before/after content is already provided and is normally sufficient. Do not explore the repository by default. Use read, glob, or grep only when one specific unresolved symbol or behavior prevents an accurate line explanation. Query the smallest relevant scope, and do not repeat or broaden a query after the required information is available."
    } else {
        "Use only the supplied before/after content. Do not call tools or explore the repository."
    }
}

/// 格式化目标 inventory，不把 source 再复制进 repair 诊断。
fn format_targets(targets: &[LineTarget]) -> String {
    targets
        .iter()
        .map(|target| format!("{:?} line {}", target.side, target.line_number))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 严格读取 synthetic final tool；坏 JSON 交给上层 validation 触发 repair。
fn parse_final_result(call: &ToolCall) -> BabysitFileExplanations {
    serde_json::from_value(call.function.arguments.clone()).unwrap_or_default()
}

/// 适配器取消错误保持公共语义；transport 失败额外保留白名单诊断字段。
fn adapter_error(error: CustomAdapterError) -> BabysitError {
    match error {
        CustomAdapterError::Cancelled => BabysitError::Cancelled,
        CustomAdapterError::OutputLimit { limit } => BabysitError::OutputLimit { limit },
        CustomAdapterError::Transport(failure) => BabysitError::ProviderDiagnostic {
            detail: CustomAdapterError::Transport(failure.clone()).to_string(),
            diagnostic: Box::new(crate::babysit::BabysitProviderDiagnostic::custom_transport(
                failure.protocol().as_str(),
                failure.phase().as_str(),
                failure.status().as_str(),
                failure.reason().as_str(),
            )),
        },
        other => BabysitError::Provider {
            detail: other.to_string(),
        },
    }
}

/// runtime mutex 损坏时保持 provider-independent failure，不泄漏内部状态。
fn runtime_unavailable() -> BabysitError {
    BabysitError::Provider {
        detail: "Custom runtime 不可用。".to_owned(),
    }
}

/// 把 adapter 返回的本轮 transcript 与既有会话无重复地拼接。
fn merge_transcript(previous: &[Message], transcript: &[Message]) -> Vec<Message> {
    if transcript.starts_with(previous) {
        return transcript.to_vec();
    }
    let overlap = (1..=previous.len().min(transcript.len()))
        .rev()
        .find(|length| previous[previous.len() - length..] == transcript[..*length])
        .unwrap_or(0);
    previous
        .iter()
        .cloned()
        .chain(transcript.iter().skip(overlap).cloned())
        .collect()
}

/// 每个模型调用、工具前后和 final 接受前共享的取消 guard。
fn ensure_not_cancelled(cancellation: &CancellationSignal) -> Result<(), BabysitError> {
    cancellation
        .is_cancelled()
        .then_some(BabysitError::Cancelled)
        .map_or(Ok(()), Err)
}
