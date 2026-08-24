//! Installed Claude Code 的无状态 Babysit provider。

use super::{
    parse_provider_response, validate_response, BabysitAgentProvider, BabysitError,
    BabysitFileExplanations, BabysitProviderRequest, CancellationSignal, FileSide,
    ValidationFailure, MAX_EXPLANATION_CHARS, MAX_REPAIR_ATTEMPTS,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

mod model_catalog;

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
#[derive(Clone, Copy)]
struct ClaudeRunState {
    deadline: Instant,
    repair_attempts: u8,
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
        command
            .args(&self.prefix_args)
            .arg("-p")
            .arg("--no-session-persistence")
            .arg("--output-format")
            .arg("json")
            .arg("--safe-mode")
            .arg("--permission-mode")
            .arg("dontAsk")
            .arg("--tools")
            .arg("Read,Glob,Grep")
            .arg("--disallowedTools")
            .arg("Bash,Write,Edit,Web,mcp__*")
            .arg("--model")
            .arg("sonnet")
            .arg("--effort")
            .arg("low")
            .arg("--json-schema")
            .arg(schema.to_string())
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
        let status = loop {
            tokio::select! {
                status = child.wait() => break status.map_err(|_| redacted_provider_error("wait_failed"))?,
                () = &mut deadline_sleep => {
                    terminate_process_tree(&mut child, process_id).await;
                    return Err(redacted_provider_error("timeout"));
                },
                () = wait_for_cancellation(cancellation) => {
                    terminate_process_tree(&mut child, process_id).await;
                    return Err(BabysitError::Cancelled);
                },
            }
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
    /// 返回稳定 Claude provider identity。
    fn provider_id(&self) -> &str {
        "claude-code"
    }

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
        self.runs
            .lock()
            .map_err(|_| redacted_provider_error("state_unavailable"))?
            .insert(
                key.clone(),
                ClaudeRunState {
                    deadline,
                    repair_attempts: 0,
                },
            );
        let prompt = generation_prompt(&request);
        let result = self
            .run_round(&request, prompt, deadline, cancellation)
            .await;
        if result
            .as_ref()
            .map_or(true, |output| validate_response(&request, output).is_ok())
        {
            self.remove_run(&key);
        }
        result
    }

    /// 用完整 request、previous output 与脱敏校验失败构造无状态 repair。
    async fn repair(
        &self,
        request: BabysitProviderRequest,
        previous: BabysitFileExplanations,
        failure: ValidationFailure,
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
            });
            state.repair_attempts = state.repair_attempts.saturating_add(1);
            *state
        };
        let prompt = repair_prompt(&request, &previous, &failure);
        let result = self
            .run_round(&request, prompt, state.deadline, cancellation)
            .await;
        if state.repair_attempts >= MAX_REPAIR_ATTEMPTS
            || result
                .as_ref()
                .map_or(true, |output| validate_response(&request, output).is_ok())
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

/// Claude JSON result envelope；未知字段由 serde 默认忽略。
#[derive(Deserialize)]
struct ClaudeResultEnvelope {
    #[serde(default)]
    structured_output: Option<Value>,
    #[serde(default)]
    is_error: Option<bool>,
    #[serde(default)]
    subtype: Option<String>,
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
            .map(|line| {
                (
                    line.clone(),
                    json!({"type":"string","minLength":1,"maxLength":MAX_EXPLANATION_CHARS}),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        json!({"type":"object","properties":properties,"required":lines,"additionalProperties":false})
    };
    json!({"type":"object","properties":{"before":side_schema(FileSide::Before),"after":side_schema(FileSide::After)},"required":["before","after"],"additionalProperties":false})
}

/// 构造包含完整源内容与目标 inventory 的单批生成请求。
fn generation_prompt(request: &BabysitProviderRequest) -> String {
    format!(
        "请只解释代码，不评价或建议。使用 locale {}。完整请求 JSON：{}",
        request.locale,
        serde_json::to_string(request).unwrap_or_default()
    )
}

/// 生成只在内存中使用的 request key，不进入日志或 IPC。
fn request_key(request: &BabysitProviderRequest) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    request.path.hash(&mut hasher);
    request.workspace_path.hash(&mut hasher);
    request.before.hash(&mut hasher);
    request.after.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// 构造自包含 repair；failure 文本只含 side/line/category，不含源代码或路径。
fn repair_prompt(
    request: &BabysitProviderRequest,
    previous: &BabysitFileExplanations,
    failure: &ValidationFailure,
) -> String {
    format!(
        "修复上一轮结构化输出。完整请求 JSON：{}\n上一轮输出 JSON：{}\n脱敏校验失败：{}",
        serde_json::to_string(request).unwrap_or_default(),
        serde_json::to_string(previous).unwrap_or_default(),
        failure
    )
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

/// 将进程失败压缩为不含路径、prompt、stdout/stderr 的稳定类别。
fn redacted_provider_error(category: &str) -> BabysitError {
    BabysitError::Provider {
        detail: format!("Claude Code 进程失败（{category}）"),
    }
}

/// 仅解析结构化 stdout 中的稳定错误类别，不回传原始内容。
fn classify_failure(stdout: &[u8]) -> BabysitError {
    serde_json::from_slice::<ClaudeResultEnvelope>(stdout)
        .ok()
        .filter(|value| value.is_error.unwrap_or(false))
        .map(|value| classify_result_error(value.subtype.as_deref()))
        .unwrap_or_else(|| redacted_provider_error("process_exit"))
}

/// 将 Claude result subtype 投影为现有稳定 setup/provider 类别。
fn classify_result_error(subtype: Option<&str>) -> BabysitError {
    match subtype {
        Some("authentication_failed" | "not_authenticated" | "signed_out") => {
            BabysitError::provider_setup(
                "claude-code",
                super::BabysitProviderSetupFailure::SignedOut,
            )
        }
        Some("incompatible" | "unsupported") => BabysitError::provider_setup(
            "claude-code",
            super::BabysitProviderSetupFailure::Incompatible,
        ),
        _ => redacted_provider_error("result_error"),
    }
}
