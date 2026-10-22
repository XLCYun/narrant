//! 独立 CodeGraph 语言分析器消费边界；图差异与工作区生命周期仍由 Narrant 管理。

mod graph;
mod process;
mod protocol;
mod selection;

pub(crate) use selection::{python_needs_tools, typescript_needs_tools};

use super::code_graph::{
    AnalysisError, AnalysisInput, CodeGraph, LanguageAnalyzer, RustDependencyScope,
};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub const ENGINE_VERSION: &str = "0.1.0";
pub const PROTOCOL: &str = "codegraph.analyzer/1";
pub const PACK_IDS: &[&str] = &[
    "rust",
    "typescript-javascript",
    "python",
    "java",
    "go",
    "csharp",
    "c-cpp",
];

/// 已知包 ID 的公开入口命名规则，安装校验与实际启动共用同一相对路径。
pub(crate) fn public_entry(id: &str, windows: bool) -> String {
    format!("bin/codegraph-{id}{}", if windows { ".exe" } else { "" })
}

/// 取消信号只表达意图；返回调用方前仍同步等待整棵进程树与管道线程。
pub type CancellationCheck = Arc<dyn Fn() -> bool + Send + Sync>;

/// 消费端固定的有界读取与绝对分析期限；终结后另收紧退出期限。
#[derive(Clone, Debug)]
pub struct AnalysisLimits {
    pub analysis_timeout: Duration,
    pub exit_timeout: Duration,
    pub max_result_bytes: usize,
    pub max_diagnostic_bytes: usize,
}

impl Default for AnalysisLimits {
    /// 与公共协议上限一致；取消立即交由进程树所有者回收。
    fn default() -> Self {
        Self {
            analysis_timeout: Duration::from_secs(30 * 60),
            exit_timeout: Duration::from_secs(5),
            max_result_bytes: 128 * 1024 * 1024,
            max_diagnostic_bytes: 256 * 1024,
        }
    }
}

/// 公开包描述符只有五项身份字段，不读取私有 helper/runtime/SDK 配置。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    schema_version: u32,
    pack_id: String,
    pack_version: String,
    engine_version: String,
    protocol: String,
}

/// 持有已验证包的独立分析器路径；应用另以 PackLease 保护整个调用期间的安装目录。
#[derive(Clone)]
pub struct CodeGraphAnalyzer {
    id: String,
    root: PathBuf,
    executable: PathBuf,
    scope: RustDependencyScope,
    limits: AnalysisLimits,
    cancelled: CancellationCheck,
    #[cfg(test)]
    test_prefix_args: Vec<std::ffi::OsString>,
    #[cfg(test)]
    test_environment: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
}

impl CodeGraphAnalyzer {
    /// 从调用方已经完成完整性校验的绝对包根绑定公共描述符及对应语言入口。
    pub fn from_pack(id: &str, root: &Path) -> Result<Self> {
        if !PACK_IDS.contains(&id) || !root.is_absolute() || !root.is_dir() {
            return Err(failure("包 ID 或绝对安装根无效"));
        }
        let root = root.canonicalize().map_err(failure)?;
        validate_descriptor(id, &root)?;
        let executable = inside(&root, &public_entry(id, cfg!(windows)))?;
        Ok(Self {
            id: id.into(),
            root,
            executable,
            scope: RustDependencyScope::RepositoryOnly,
            limits: AnalysisLimits::default(),
            cancelled: Arc::new(|| false),
            #[cfg(test)]
            test_prefix_args: Vec::new(),
            #[cfg(test)]
            test_environment: Default::default(),
        })
    }

    /// 固定本次分析的 Rust 依赖范围，其他语言同样使用公共选项封套。
    pub fn with_dependency_scope(mut self, scope: RustDependencyScope) -> Self {
        self.scope = scope;
        self
    }

    /// 绑定实际工作区 worker 的取消观察。
    pub fn with_cancellation(mut self, cancelled: CancellationCheck) -> Self {
        self.cancelled = cancelled;
        self
    }

    /// 测试与受控调用可收紧期限，但不能取消资源与回收边界。
    pub fn with_limits(mut self, limits: AnalysisLimits) -> Self {
        self.limits = limits;
        self
    }

    /// 仅供生命周期验收观察真实 CLI；产品路径始终直接启动包内固定入口。
    #[cfg(all(test, unix))]
    pub(crate) fn with_test_launcher(
        mut self,
        executable: PathBuf,
        args: Vec<std::ffi::OsString>,
        environment: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
    ) -> Self {
        self.executable = executable;
        self.test_prefix_args = args;
        self.test_environment = environment;
        self
    }

    /// 包 ID 对应的公共语言命名空间，用于检查返回图边界。
    fn languages(&self) -> &'static [&'static str] {
        match self.id.as_str() {
            "typescript-javascript" => &["typescript", "javascript"],
            "c-cpp" => &["c", "cpp"],
            "rust" => &["rust"],
            "python" => &["python"],
            "java" => &["java"],
            "go" => &["go"],
            "csharp" => &["csharp"],
            _ => unreachable!(),
        }
    }
}

impl LanguageAnalyzer for CodeGraphAnalyzer {
    /// 发送源码选集与分析选项；成功图必须同时满足严格图校验、退出码与后代退出屏障。
    fn analyze(&self, input: &AnalysisInput) -> Result<CodeGraph> {
        if (self.cancelled)() {
            return Err(cancelled());
        }
        if !input.repository_root.is_absolute() || !input.repository_root.is_dir() {
            return Err(failure("源码根必须是存在的绝对目录"));
        }
        let repository = input.repository_root.canonicalize().map_err(failure)?;
        if repository.starts_with(&self.root) || self.root.starts_with(&repository) {
            return Err(failure("源码仓库与支持包目录不能重叠"));
        }
        if input.files.len() > 100_000 {
            return Err(failure("源码选集超过文件数量上限"));
        }
        let mut files = BTreeSet::new();
        for path in &input.files {
            let parts = path
                .components()
                .map(|part| match part {
                    Component::Normal(value) => value
                        .to_str()
                        .filter(|s| !s.contains(['\\', ':', '\0']))
                        .ok_or_else(|| failure("源码路径不是有效 UTF-8 相对路径")),
                    _ => Err(failure("源码路径必须是仓库内相对路径")),
                })
                .collect::<Result<Vec<_>>>()?;
            let relative = parts.join("/");
            if relative.is_empty() || !files.insert(relative) {
                return Err(failure("源码选集含空路径或重复路径"));
            }
        }
        let request = serde_json::json!({"protocol": PROTOCOL, "type": "analyze", "id": protocol::REQUEST_ID,
            "repository_root": repository, "files": files,
            "options": {"include_tests": input.options.include_tests, "rust_dependency_scope": self.scope}});
        if serde_json::to_vec(&request).map_err(failure)?.len() > 128 * 1024 * 1024 {
            return Err(failure("分析请求超过字节上限"));
        }
        let temporary_root = std::env::temp_dir().canonicalize().map_err(failure)?;
        if temporary_root.starts_with(&repository) {
            return Err(failure("CLI 临时目录不能位于源码仓库内"));
        }
        let scratch = tempfile::Builder::new()
            .prefix("narrant-codegraph-")
            .tempdir_in(temporary_root)
            .map_err(failure)?;
        let graph = process::run(self, &request, scratch.path())?;
        if (self.cancelled)() {
            return Err(cancelled());
        }
        Ok(graph)
    }
}

/// 安装校验读取公开身份，跨平台归档验证不依赖当前宿主的可执行文件后缀。
pub(crate) fn validate_descriptor(id: &str, root: &Path) -> Result<()> {
    let descriptor = inside(root, "control/codegraph.json")?;
    let mut bytes = Vec::new();
    std::fs::File::open(descriptor)
        .map_err(failure)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(failure)?;
    if bytes.len() > 1024 * 1024 {
        return Err(failure("包描述符超过字节上限"));
    }
    let descriptor: Descriptor =
        serde_json::from_value(protocol::unique_json(&bytes)?).map_err(failure)?;
    if descriptor.schema_version != 1
        || descriptor.pack_id != id
        || descriptor.pack_version != ENGINE_VERSION
        || descriptor.engine_version != ENGINE_VERSION
        || descriptor.protocol != PROTOCOL
    {
        return Err(failure("包描述符与固定引擎、版本或协议不匹配"));
    }
    Ok(())
}

/// 描述符与执行入口均必须是包内普通文件，链接不得把边界延伸到其他目录。
fn inside(root: &Path, name: &str) -> Result<PathBuf> {
    let path = root.join(name).canonicalize().map_err(failure)?;
    if !path.starts_with(root) || !path.is_file() {
        return Err(failure("公共包入口不是安装根内的文件"));
    }
    Ok(path)
}

/// 消费层沿用现有分析错误，不能以空图替代协议或进程失败。
type Result<T> = std::result::Result<T, AnalysisError>;

/// 保留原始原因并标记出错边界。
fn failure(message: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::new(format!("CodeGraph：{message}"))
}

/// 取消保留独立类别，清理失败仍会升级为分析失败。
fn cancelled() -> AnalysisError {
    AnalysisError::cancelled("CodeGraph 分析已取消")
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
