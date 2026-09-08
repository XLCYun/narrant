use crate::operation::CancellationSignal;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const QUERY_TIMEOUT: Duration = Duration::from_secs(60);
const WORKSPACE_INSPECT_TIMEOUT: Duration = Duration::from_secs(60);
const WORKSPACE_MUTATION_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const DEFAULT_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

/// 产生超限输出的 Core Git 管道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitOutputStream {
    Stdout,
    Stderr,
}

/// Core Git 失败的稳定类别；诊断文本不参与业务分支。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreGitFailureKind {
    Cancelled,
    TimedOut,
    OutputLimitExceeded {
        stream: GitOutputStream,
        limit: usize,
    },
    SpawnFailed,
    Exited {
        code: Option<i32>,
    },
    OutputReadFailed,
    InputWriteFailed,
    WaitFailed,
    TerminationFailed,
    InvalidData,
}

/// Core Git 命令失败，消息只用于诊断，不参与业务分支判断。
#[derive(Clone, Debug)]
pub struct CoreGitFailure {
    kind: CoreGitFailureKind,
    message: String,
}

impl CoreGitFailure {
    /// 构造调用方解析或校验失败；该兼容入口不代表进程执行失败。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            kind: CoreGitFailureKind::InvalidData,
            message: message.into(),
        }
    }

    /// 构造由调用方取消产生的稳定错误类别。
    pub(crate) fn cancelled() -> Self {
        Self {
            kind: CoreGitFailureKind::Cancelled,
            message: "Core Git 操作已取消".to_owned(),
        }
    }

    /// 构造指定稳定类别和诊断文本的 Core Git 失败。
    fn from_kind(kind: CoreGitFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// 返回供业务分支判断的稳定失败类别。
    #[allow(dead_code, reason = "保留稳定失败分类 interface，当前生产分支无需读取")]
    pub fn kind(&self) -> CoreGitFailureKind {
        self.kind
    }

    /// 添加领域上下文，同时保留原始稳定失败类别。
    pub fn with_context(mut self, context: impl AsRef<str>) -> Self {
        self.message = format!("{}：{}", context.as_ref(), self.message);
        self
    }

    /// 返回错误是否由合作式取消产生。
    pub fn is_cancelled(&self) -> bool {
        self.kind == CoreGitFailureKind::Cancelled
    }

    /// 返回可用于测试和界面诊断的错误消息。
    #[allow(dead_code, reason = "保留稳定诊断 interface，当前生产分支使用 Display")]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for CoreGitFailure {
    /// 将稳定的诊断消息写入错误 formatter。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CoreGitFailure {}

/// 兼容上层领域模块的既有名称；稳定失败模型由 `CoreGitFailure` 提供。
pub type GitCommandError = CoreGitFailure;

/// 一次成功的 Core Git 命令输出。
#[derive(Debug)]
pub struct GitOutput {
    pub stdout: Vec<u8>,
}

/// Core Git 调用的策略类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitOperationClass {
    Query,
    Network,
    WorkspaceInspect,
    WorkspaceMutation,
}

/// 一次 Core Git 调用的仓库、参数数组与策略类别。
pub struct GitInvocation {
    repository_path: PathBuf,
    arguments: Vec<OsString>,
    class: GitOperationClass,
    input: Option<Vec<u8>>,
}

impl GitInvocation {
    /// 从直接参数数组创建调用；参数不会经过 shell。
    pub fn new<I, S>(repository_path: &Path, arguments: I, class: GitOperationClass) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Self {
            repository_path: repository_path.to_owned(),
            arguments: arguments
                .into_iter()
                .map(|argument| argument.as_ref().to_owned())
                .collect(),
            class,
            input: None,
        }
    }

    /// 为批处理命令提供完整标准输入，由执行器并发写入并关闭管道。
    pub fn with_input(mut self, input: Vec<u8>) -> Self {
        self.input = Some(input);
        self
    }
}

/// 统一执行、限制、终止并回收 Core Git 子进程。
#[derive(Clone, Default)]
pub struct CoreGitProcess {
    limits: Option<(Duration, usize)>,
}

/// 直接执行 Core Git 参数数组，并统一施加超时、取消和输出上限。
#[derive(Clone)]
pub struct GitProcessAdapter {
    repository_path: PathBuf,
}

impl GitProcessAdapter {
    /// 为指定仓库创建使用默认安全限制的进程 adapter。
    pub fn new(repository_path: &Path) -> Self {
        Self {
            repository_path: repository_path.to_owned(),
        }
    }
}

mod process;
mod workspace;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
