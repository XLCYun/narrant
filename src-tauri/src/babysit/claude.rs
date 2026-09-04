//! Installed Claude Code 的无状态 Babysit provider。

use super::{
    parse_provider_response, validate_response, BabysitAgentProvider, BabysitError,
    BabysitFileExplanations, BabysitProviderRequest, CancellationSignal, FileSide,
    ValidationFailure, MAX_REPAIR_ATTEMPTS,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

mod discovery;
mod model_catalog;
mod process;
mod protocol;
pub(crate) mod runtime;

use process::configure_round_command;
use protocol::{
    classify_failure, classify_result_error, redacted_provider_error, ClaudeResultEnvelope,
};

pub(crate) use model_catalog::{ClaudeModelCatalog, ResolvedClaudeModel};

#[cfg(test)]
#[path = "tests/claude/model_catalog.rs"]
mod model_catalog_tests;

/// Claude Code generation 与 repair 共用的总运行上限；上层仍拥有完整分析截止时间。
const CLAUDE_ROUND_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// 每轮都启动 fresh、non-persistent CLI 的 installed Claude Code adapter。
pub struct ClaudeCodeProvider {
    program: Mutex<PathBuf>,
    prefix_args: Vec<OsString>,
    generation: String,
    timeout: Duration,
    runs: Mutex<std::collections::HashMap<String, ClaudeRunState>>,
}

/// 一次 File Change 分析跨 fresh process 保留的最小内存状态。
#[derive(Clone)]
struct ClaudeRunState {
    deadline: Instant,
    repair_attempts: u8,
    resolved_model: ResolvedClaudeModel,
}

impl ClaudeCodeProvider {
    /// 使用已发现的 Claude Code executable 构造生产 provider。
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: Mutex::new(program.into()),
            prefix_args: Vec::new(),
            generation: "claude-installed-1".to_owned(),
            timeout: CLAUDE_ROUND_TIMEOUT,
            runs: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// 注入确定性 CLI fixture，默认 CI 不需要 Claude 安装、登录或付费请求。
    #[cfg(test)]
    pub(super) fn for_test(
        fixture: PathBuf,
        scenario: &str,
        events: PathBuf,
        timeout: Duration,
    ) -> Self {
        let program = if cfg!(windows) {
            PathBuf::from(r"C:\Program Files\Git\bin\sh.exe")
        } else {
            PathBuf::from("sh")
        };
        Self {
            program: Mutex::new(program),
            prefix_args: vec![
                fixture.into_os_string(),
                scenario.into(),
                events.into_os_string(),
            ],
            generation: format!("claude-fixture-{scenario}"),
            timeout,
            runs: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// 执行一轮独立 Claude 进程，并只返回结构化输出。
    async fn run_round(
        &self,
        request: &BabysitProviderRequest,
        prompt: String,
        resolved_model: &ResolvedClaudeModel,
        deadline: Instant,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        if cancellation.is_cancelled() {
            return Err(BabysitError::Cancelled);
        }
        let schema = output_schema(request);
        let program = self
            .program
            .lock()
            .map_err(|_| redacted_provider_error("state_unavailable"))?
            .clone();
        let mut command = Command::new(program);
        configure_round_command(
            &mut command,
            &self.prefix_args,
            resolved_model,
            &schema,
            request.repository_exploration,
        );
        command
            .current_dir(&request.workspace_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::child_process::configure_tokio_command(&mut command);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .map_err(|_| redacted_provider_error("spawn_failed"))?;
        let process_id = child.id();
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| redacted_provider_error("stdin_unavailable"))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| redacted_provider_error("stdout_unavailable"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| redacted_provider_error("stderr_unavailable"))?;
        let writer = tokio::spawn(async move { stdin.write_all(prompt.as_bytes()).await });
        let stdout_reader = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).await.map(|_| bytes)
        });
        let stderr_reader = tokio::spawn(async move {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).await.map(|_| bytes)
        });
        let deadline_sleep = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline));
        tokio::pin!(deadline_sleep);
        let status = tokio::select! {
            status = child.wait() => status.map_err(|_| redacted_provider_error("wait_failed"))?,
            () = &mut deadline_sleep => {
                terminate_process_tree(&mut child, process_id).await;
                return Err(redacted_provider_error("timeout"));
            },
            () = wait_for_cancellation(cancellation) => {
                terminate_process_tree(&mut child, process_id).await;
                return Err(BabysitError::Cancelled);
            },
        };
        let _ = writer.await;
        let stdout = stdout_reader
            .await
            .ok()
            .and_then(Result::ok)
            .ok_or_else(|| redacted_provider_error("stdout_read_failed"))?;
        let _ = stderr_reader.await;
        if !status.success() {
            return Err(classify_failure(&stdout));
        }
        let envelope: ClaudeResultEnvelope =
            serde_json::from_slice(&stdout).map_err(|_| redacted_provider_error("invalid_json"))?;
        if envelope.is_error.unwrap_or(false) {
            return Err(classify_result_error(envelope.subtype.as_deref()));
        }
        let structured_output = envelope
            .structured_output
            .ok_or_else(|| redacted_provider_error("missing_structured_output"))?;
        parse_provider_response(&structured_output.to_string())
    }
}

#[async_trait]
impl BabysitAgentProvider for ClaudeCodeProvider {
    /// 返回 executable/probe generation，供缓存隔离使用。
    fn process_generation(&self) -> String {
        self.generation.clone()
    }

    /// 用一轮 fresh CLI process 生成完整 File Change 解释。
    async fn generate(
        &self,
        request: BabysitProviderRequest,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        let key = request_key(&request);
        let deadline = Instant::now() + self.timeout;
        let resolved_model = resolved_model_from_request(&request);
        self.runs
            .lock()
            .map_err(|_| redacted_provider_error("state_unavailable"))?
            .insert(
                key.clone(),
                ClaudeRunState {
                    deadline,
                    repair_attempts: 0,
                    resolved_model: resolved_model.clone(),
                },
            );
        let mut structural_retries = 0;
        let result = loop {
            let result = self
                .run_round(
                    &request,
                    generation_prompt(&request),
                    &resolved_model,
                    deadline,
                    cancellation,
                )
                .await;
            match result {
                Err(error)
                    if is_structured_output_error(&error)
                        && structural_retries < MAX_REPAIR_ATTEMPTS =>
                {
                    structural_retries += 1;
                }
                result => break result,
            }
        };
        if let Ok(mut runs) = self.runs.lock() {
            if let Some(state) = runs.get_mut(&key) {
                state.repair_attempts = structural_retries;
            }
        }
        if result
            .as_ref()
            .map_or(true, |output| validate_response(&request, output).is_ok())
        {
            self.remove_run(&key);
        }
        result
    }

    /// 用完整编号源码与问题行 inventory 构造无状态局部 repair。
    async fn repair(
        &self,
        request: BabysitProviderRequest,
        _previous: BabysitFileExplanations,
        failures: Vec<ValidationFailure>,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        let key = request_key(&request);
        let state = {
            let mut runs = self
                .runs
                .lock()
                .map_err(|_| redacted_provider_error("state_unavailable"))?;
            let state = runs.entry(key.clone()).or_insert(ClaudeRunState {
                deadline: Instant::now() + self.timeout,
                repair_attempts: 0,
                resolved_model: resolved_model_from_request(&request),
            });
            if state.repair_attempts >= MAX_REPAIR_ATTEMPTS {
                return Err(BabysitError::RepairExhausted {
                    detail: "Claude Code 已用尽本次分析的结构与局部修复预算。".to_owned(),
                });
            }
            state.repair_attempts = state.repair_attempts.saturating_add(1);
            state.clone()
        };
        let repair_request = request.repair_request(&failures);
        let prompt = repair_prompt(&repair_request, &failures);
        let result = self
            .run_round(
                &repair_request,
                prompt,
                &state.resolved_model,
                state.deadline,
                cancellation,
            )
            .await;
        if state.repair_attempts >= MAX_REPAIR_ATTEMPTS
            || result.as_ref().map_or(true, |output| {
                validate_response(&repair_request, output).is_ok()
            })
        {
            self.remove_run(&key);
        }
        result
    }

    /// 提前退出 repair loop 时只清除内存中的 deadline/attempt 状态。
    async fn abandon(&self, request: &BabysitProviderRequest) {
        self.remove_run(&request_key(request));
    }
}

/// 只有 provider 成功退出但结果无法形成契约 JSON 时才允许完整重试。
fn is_structured_output_error(error: &BabysitError) -> bool {
    match error {
        BabysitError::InvalidProviderOutput { .. } => true,
        BabysitError::Provider { detail } => {
            detail.contains("invalid_json") || detail.contains("missing_structured_output")
        }
        _ => false,
    }
}

impl ClaudeCodeProvider {
    /// 在 runtime 完成 discovery 与能力探针后切换到已验证的 executable。
    pub(crate) fn set_program(&self, program: PathBuf) -> Result<(), BabysitError> {
        *self
            .program
            .lock()
            .map_err(|_| redacted_provider_error("state_unavailable"))? = program;
        Ok(())
    }

    /// 尽力清除一项 provider 私有运行状态；mutex poison 不得扩大为含敏感信息的诊断。
    fn remove_run(&self, key: &str) {
        if let Ok(mut runs) = self.runs.lock() {
            runs.remove(key);
        }
    }
}

/// 生成只允许 before/after 及精确目标行的 JSON Schema。
fn output_schema(request: &BabysitProviderRequest) -> Value {
    let side_schema = |side| {
        let lines = request
            .targets
            .iter()
            .filter(|target| target.side == side)
            .map(|target| target.line_number.to_string())
            .collect::<Vec<_>>();
        let properties = lines
            .iter()
            .map(|line| (line.clone(), json!({"type":"string","minLength":1})))
            .collect::<serde_json::Map<_, _>>();
        json!({"type":"object","properties":properties,"required":lines,"additionalProperties":false})
    };
    json!({"type":"object","properties":{"before":side_schema(FileSide::Before),"after":side_schema(FileSide::After)},"required":["before","after"],"additionalProperties":false})
}

/// 构造包含完整源内容与目标 inventory 的单批生成请求。
fn generation_prompt(request: &BabysitProviderRequest) -> String {
    let prompt_request = numbered_prompt_request(request);
    format!(
        "请只解释代码，不评价或建议。每条解释使用一到两个短句，并且最多 240 个 Unicode 字符。{}使用 locale {}。完整请求 JSON：{}",
        repository_exploration_guidance(request.repository_exploration), request.locale, prompt_request
    )
}

/// 返回 Narrant 为本次分析分配的进程内身份，不进入日志、prompt 或 IPC。
fn request_key(request: &BabysitProviderRequest) -> String {
    request.analysis_run_id.to_string()
}

/// 将 provider-independent 请求中的空字符串还原为 nullable Claude CLI 覆盖。
fn resolved_model_from_request(request: &BabysitProviderRequest) -> ResolvedClaudeModel {
    let present = |value: &str| (!value.is_empty()).then(|| value.to_owned());
    ResolvedClaudeModel {
        model: present(&request.model),
        reasoning_effort: present(&request.reasoning_effort),
    }
}

/// 构造自包含 repair；附完整编号源码，但只请求缺失或空白的目标行。
fn repair_prompt(request: &BabysitProviderRequest, failures: &[ValidationFailure]) -> String {
    let prompt_request = numbered_prompt_request(request);
    let requested = failures
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("；");
    format!(
        "补充上一轮缺失或空白的逐行解释。{}完整编号源码请求 JSON：{prompt_request}\n只返回这些目标行：{requested}。不要重复已经有效的行；每条解释建议不超过 240 个 Unicode 字符。",
        repository_exploration_guidance(request.repository_exploration)
    )
}

/// 按冻结偏好切换 Claude prompt 的仓库探索约束。
fn repository_exploration_guidance(enabled: bool) -> &'static str {
    if enabled {
        "当前请求已经提供完整的 before/after 内容，通常足以完成解释；默认不要探索仓库。只有某个具体且尚未解析的符号或行为会阻碍准确解释时，才在当前 Review Workspace 内查询最小必要范围；获得所需信息后，不要重复查询或扩大搜索范围。"
    } else {
        "只能根据当前请求提供的 before/after 内容生成解释；不要调用工具或探索仓库。"
    }
}

/// 把 provider 请求序列化为 prompt 专用视图，只替换 before/after 的展示格式。
fn numbered_prompt_request(request: &BabysitProviderRequest) -> Value {
    let mut prompt_request = serde_json::to_value(request).unwrap_or_default();
    prompt_request["before"] = Value::String(super::numbered_content(&request.before));
    prompt_request["after"] = Value::String(super::numbered_content(&request.after));
    prompt_request
}

/// 等待轮询式 cancellation，不向 provider 暴露 operation identity。
async fn wait_for_cancellation(cancellation: &CancellationSignal) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// 终止当前 CLI 及其后代；Windows 使用 taskkill 的 `/T` 覆盖整个进程树。
async fn terminate_process_tree(child: &mut tokio::process::Child, process_id: Option<u32>) {
    #[cfg(windows)]
    if let Some(process_id) = process_id {
        let mut command = Command::new("taskkill");
        command.args(["/PID", &process_id.to_string(), "/T", "/F"]);
        crate::child_process::configure_tokio_command(&mut command);
        let _ = command.status().await;
    }
    #[cfg(not(windows))]
    if let Some(process_id) = process_id {
        // SAFETY: child 在启动前已成为新进程组 leader；负 PID 只指向该 Claude 进程组。
        unsafe {
            libc::kill(-(process_id as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}
