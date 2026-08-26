use super::{
    custom_tool_definitions, BabysitError, BabysitFileExplanations, BabysitProviderRequest,
    CancellationSignal, CustomAdapterError, CustomProtocolAdapter, CustomRoundRequest,
    CustomRoundResult, LineTarget, RepositoryToolError, RepositoryToolRequest, RepositoryTools,
    ValidationFailure, FINAL_OUTPUT_TOOL_NAME, MAX_MODEL_TURNS, MAX_REPAIR_ATTEMPTS,
};
use rig::completion::Message;
use rig::message::ToolCall;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 一次 Custom 分析的单内存会话；generate 与 repair 共用这份 history。
struct CustomSession {
    adapter: Arc<dyn CustomProtocolAdapter>,
    tools: RepositoryTools,
    history: Vec<Message>,
    repair_attempts: u8,
}

/// Custom provider 的共享 runtime，统一管理 turn、repair、工具和取消边界。
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

    /// 创建一次完整分析会话，并在同一会话内执行最多 16 次模型调用。
    pub(crate) async fn generate(
        &self,
        adapter: Arc<dyn CustomProtocolAdapter>,
        request: BabysitProviderRequest,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        ensure_not_cancelled(cancellation)?;
        let tools =
            RepositoryTools::new(&request.workspace_path).map_err(repository_setup_error)?;
        let run_id = request.analysis_run_id;
        self.sessions
            .lock()
            .map_err(|_| runtime_unavailable())?
            .insert(
                run_id,
                CustomSession {
                    adapter,
                    tools,
                    history: Vec::new(),
                    repair_attempts: 0,
                },
            );

        let result = self
            .run_round(run_id, request, generation_prompt, cancellation)
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
            .run_round(
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

    /// 执行一个 generate/repair round，并严格在每个边界检查取消状态。
    async fn run_round<F>(
        &self,
        run_id: u64,
        request: BabysitProviderRequest,
        prompt: F,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError>
    where
        F: Fn(&BabysitProviderRequest) -> String,
    {
        for turn in 1..=MAX_MODEL_TURNS {
            ensure_not_cancelled(cancellation)?;
            let (adapter, tools, history) = self.session_context(run_id)?;
            let round = CustomRoundRequest::new(
                Message::user(if turn == 1 {
                    prompt(&request)
                } else {
                    continuation_prompt()
                }),
                history,
                turn,
                custom_tool_definitions(&tools),
            )
            .map_err(adapter_error)?;
            let response = adapter
                .complete(round, cancellation)
                .await
                .map_err(adapter_error)?;
            // transport future 即使刚刚完成，也不能在取消之后接受 final 或 dispatch。
            ensure_not_cancelled(cancellation)?;
            self.append_assistant(run_id, &response)?;

            if let Some(final_call) = response
                .tool_calls()
                .into_iter()
                .find(|call| call.function.name == FINAL_OUTPUT_TOOL_NAME)
            {
                ensure_not_cancelled(cancellation)?;
                return Ok(parse_final_result(final_call));
            }

            let calls = response
                .tool_calls()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            for call in calls {
                ensure_not_cancelled(cancellation)?;
                let output = dispatch_repository_tool(&tools, &call, cancellation)?;
                ensure_not_cancelled(cancellation)?;
                self.append_tool_result(run_id, &call, output)?;
            }
        }
        // 让上层 validation/repair merging 决定如何处理未完成的输出。
        Ok(BabysitFileExplanations::default())
    }

    /// 读取当前会话的 adapter、工具和不可变消息历史，避免跨 await 持有锁。
    fn session_context(
        &self,
        run_id: u64,
    ) -> Result<
        (
            Arc<dyn CustomProtocolAdapter>,
            RepositoryTools,
            Vec<Message>,
        ),
        BabysitError,
    > {
        let sessions = self.sessions.lock().map_err(|_| runtime_unavailable())?;
        let session = sessions.get(&run_id).ok_or_else(runtime_unavailable)?;
        Ok((
            session.adapter.clone(),
            session.tools.clone(),
            session.history.clone(),
        ))
    }

    /// 将 provider assistant response 固定写入单内存会话历史。
    fn append_assistant(
        &self,
        run_id: u64,
        response: &CustomRoundResult,
    ) -> Result<(), BabysitError> {
        let mut sessions = self.sessions.lock().map_err(|_| runtime_unavailable())?;
        let session = sessions.get_mut(&run_id).ok_or_else(runtime_unavailable)?;
        session.history.push(Message::Assistant {
            id: response.identity.message_id.clone(),
            content: response.choice.clone(),
        });
        Ok(())
    }

    /// 将每一项 repository tool 的结果按 call id 回放给下一模型轮。
    fn append_tool_result(
        &self,
        run_id: u64,
        call: &ToolCall,
        output: String,
    ) -> Result<(), BabysitError> {
        let mut sessions = self.sessions.lock().map_err(|_| runtime_unavailable())?;
        let session = sessions.get_mut(&run_id).ok_or_else(runtime_unavailable)?;
        session.history.push(Message::tool_result(
            call.id.as_str(),
            call.function.name.clone(),
            output,
        ));
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
Use read, glob, and grep only inside the Review Workspace when more context is needed.\n\
When finished, call final_result with both before and after maps.",
        path = request.path,
        before = crate::babysit::numbered_content(&request.before),
        after = crate::babysit::numbered_content(&request.after),
        targets = format_targets(&request.targets),
    )
}

/// 修复阶段只请求上层 validation 指出的目标，保持同一模型会话上下文。
fn repair_prompt(request: &BabysitProviderRequest, failures: &[ValidationFailure]) -> String {
    format!(
        "Repair the previous Line Explanation result for this file change.\n\
File ({path}) before:\n{before}\n\
File ({path}) after:\n{after}\n\
Only these validation failures require new explanations:\n{failures}\n\
Return the complete result by calling final_result; preserve valid prior explanations.",
        path = request.path,
        before = crate::babysit::numbered_content(&request.before),
        after = crate::babysit::numbered_content(&request.after),
        failures = failures
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// 非首轮模型调用使用短 continuation，历史中已保留完整原始 prompt。
fn continuation_prompt() -> String {
    "Continue using the available repository tools, then call final_result when ready.".to_owned()
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

/// 只调度 Narrant-owned repository tools，未知工具不会触碰 Workspace。
fn dispatch_repository_tool(
    tools: &RepositoryTools,
    call: &ToolCall,
    cancellation: &CancellationSignal,
) -> Result<String, BabysitError> {
    let result = match call.function.name.as_str() {
        "read" => parse_args::<crate::babysit::ReadInput>(&call.function.arguments)
            .map(RepositoryToolRequest::Read),
        "glob" => parse_args::<crate::babysit::GlobRequest>(&call.function.arguments)
            .map(RepositoryToolRequest::Glob),
        "grep" => parse_args::<crate::babysit::GrepRequest>(&call.function.arguments)
            .map(RepositoryToolRequest::Grep),
        _ => Err(RepositoryToolError::InvalidInput),
    };
    let response = match result.and_then(|request| tools.execute(request, cancellation)) {
        Ok(response) => {
            serde_json::to_string(&response).unwrap_or_else(|_| RepositoryToolError::Io.to_string())
        }
        Err(RepositoryToolError::Cancelled) => return Err(BabysitError::Cancelled),
        Err(error) => error.to_string(),
    };
    Ok(response)
}

/// 将 provider tool arguments 限制在 typed repository 输入结构内。
fn parse_args<T: DeserializeOwned>(arguments: &Value) -> Result<T, RepositoryToolError> {
    serde_json::from_value(arguments.clone()).map_err(|_| RepositoryToolError::InvalidInput)
}

/// 适配器取消错误保持公共 Babysit cancelled 语义，其余错误只携带脱敏文本。
fn adapter_error(error: CustomAdapterError) -> BabysitError {
    match error {
        CustomAdapterError::Cancelled => BabysitError::Cancelled,
        other => BabysitError::Provider {
            detail: other.to_string(),
        },
    }
}

/// Workspace 初始化失败只通过稳定 repository 错误类别返回。
fn repository_setup_error(error: RepositoryToolError) -> BabysitError {
    if error == RepositoryToolError::Cancelled {
        BabysitError::Cancelled
    } else {
        BabysitError::Provider {
            detail: error.to_string(),
        }
    }
}

/// runtime mutex 损坏时保持 provider-independent failure，不泄漏内部状态。
fn runtime_unavailable() -> BabysitError {
    BabysitError::Provider {
        detail: "Custom runtime 不可用。".to_owned(),
    }
}

/// 每个模型调用、工具前后和 final 接受前共享的取消 guard。
fn ensure_not_cancelled(cancellation: &CancellationSignal) -> Result<(), BabysitError> {
    cancellation
        .is_cancelled()
        .then_some(BabysitError::Cancelled)
        .map_or(Ok(()), Err)
}
