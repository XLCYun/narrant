//! 直接读取 Review 拥有的 Git workspace；临时目录仅保存分析工具和控制消息。
mod mapping;
use super::{failure, runtime, tools::TypeScriptToolchain, Result};
use crate::lsp::CancellationCheck;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::TempDir;

/// 一个明确的真实项目；配置项目的注册别名由会话产生，不作为查询名。
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectRegistration {
    pub name: PathBuf,
    pub roots: Vec<PathBuf>,
    pub options: Value,
}

/// 按文件或配置限定的有界分析说明，不冒充成功的完整语义。
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalysisNote {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub project: Option<PathBuf>,
    pub file: PathBuf,
    pub message: String,
}

/// 源码快照保留 Git workspace 的真实路径，不建立第二份源码目录。
#[derive(Clone, Debug)]
pub(crate) struct SourceSnapshot {
    pub path: PathBuf,
    pub source: String,
}

/// 选中源码对应的全部配置候选，供后续语义层分别查询并比较证据。
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileProjects {
    pub file: PathBuf,
    pub projects: Vec<PathBuf>,
}

/// 助手的严格输出结构；未知字段和缺失字段均视为协议失败。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedOutput {
    config_files: Vec<PathBuf>,
    projects: Vec<ProjectRegistration>,
    associations: Vec<FileProjects>,
    sources: Vec<PathBuf>,
    metadata: Vec<PathBuf>,
    notes: Vec<AnalysisNote>,
}

/// 同一 Git workspace 的分析上下文；助手文件的回收晚于全部 LSP 子进程。
#[derive(Debug)]
pub(crate) struct PreparedProject {
    _directory: TempDir,
    pub workspace_root: PathBuf,
    pub support_root: PathBuf,
    pub toolchain: TypeScriptToolchain,
    pub config_files: Vec<PathBuf>,
    pub registrations: Vec<ProjectRegistration>,
    pub files: Vec<FileProjects>,
    pub allowed_roots: Vec<PathBuf>,
    pub notes: Vec<AnalysisNote>,
    pub sources: BTreeMap<PathBuf, SourceSnapshot>,
    pub metadata: BTreeSet<PathBuf>,
}
impl PreparedProject {
    /// 非空选集才验证工具并准备工作区读取上下文；空结果无需启动服务。
    pub fn prepare(
        root: &Path,
        selected: &[PathBuf],
        tools: &TypeScriptToolchain,
        timeout: Duration,
        cancelled: &CancellationCheck,
    ) -> Result<Option<Self>> {
        if selected.is_empty() {
            return Ok(None);
        }
        if !root.is_absolute() {
            return Err(failure("TypeScript 仓库路径必须为绝对路径"));
        }
        let workspace_root = root.canonicalize().map_err(failure)?;
        let selected = selected
            .iter()
            .map(|file| {
                mapping::lexical_under(root, file)?;
                let normalized = workspace_root.join(file.strip_prefix(root).map_err(failure)?);
                mapping::checked_under(&workspace_root, &normalized)?;
                // 同一个 Git 文件使用同一规范路径，不再维护原始路径与副本的映射。
                normalized.canonicalize().map_err(failure)
            })
            .collect::<Result<Vec<_>>>()?;
        let toolchain = tools.validate(timeout, cancelled)?;
        let directory = tempfile::tempdir().map_err(failure)?;
        let work = directory.path().canonicalize().map_err(failure)?;
        let server_root = work.join("server");
        std::fs::create_dir_all(server_root.join("lib")).map_err(failure)?;
        // 这里只写 Narrant 自身的启动器和版本元数据，源码与声明均保持原位。
        std::fs::write(
            server_root.join("package.json"),
            r#"{"name":"narrant-typescript-host","version":"6.0.3"}"#,
        )
        .map_err(failure)?;
        std::fs::write(server_root.join("lib/tsserver.js"), "import('../../server.mjs').catch(error => { console.error(error); process.exit(1); });").map_err(failure)?;
        for (name, source) in runtime::HELPERS {
            std::fs::write(work.join(name), source).map_err(failure)?;
        }
        let output = work.join("output.json");
        let input = work.join("input.json");
        std::fs::write(&input, serde_json::to_vec(&json!({"root": workspace_root, "runtime": toolchain.runtime_root, "selected": selected, "output": output, "policy_error": work.join("access-error.txt")})).map_err(failure)?).map_err(failure)?;
        let mut command = toolchain.command();
        command
            .arg(work.join("prepare.mjs"))
            .arg(input)
            .current_dir(&work);
        runtime::run(command, timeout, cancelled)?;
        let result: PreparedOutput =
            serde_json::from_str(&std::fs::read_to_string(output).map_err(failure)?)
                .map_err(|error| failure(format!("TypeScript 项目助手响应无效：{error}")))?;
        let allowed_roots = vec![
            workspace_root.clone(),
            toolchain.runtime_root.join("typescript/lib"),
            toolchain.runtime_root.join("@types/node"),
            toolchain.runtime_root.join("undici-types"),
        ];
        let mut prepared = Self {
            _directory: directory,
            workspace_root,
            support_root: work,
            toolchain,
            config_files: result.config_files,
            registrations: result.projects,
            files: result.associations,
            allowed_roots,
            notes: result.notes,
            sources: BTreeMap::new(),
            metadata: BTreeSet::new(),
        };
        for path in result.sources {
            mapping::checked_under(&prepared.workspace_root, &path)?;
            let source = std::fs::read_to_string(&path).map_err(failure)?;
            if prepared
                .sources
                .insert(path.clone(), SourceSnapshot { path, source })
                .is_some()
            {
                return Err(failure("重复的 workspace 源码路径"));
            }
        }
        for path in result.metadata {
            mapping::checked_under(&prepared.workspace_root, &path)?;
            if !prepared.metadata.insert(path) {
                return Err(failure("重复的 workspace 元数据路径"));
            }
        }
        prepared.check_access()?;
        prepared.validate_output(&selected)?;
        Ok(Some(prepared))
    }

    /// 读取接口发现越界源码时保留明确原因，不能因服务返回空目标而掩盖失败。
    pub fn check_access(&self) -> Result<()> {
        match std::fs::read_to_string(self.support_root.join("access-error.txt")) {
            Ok(message) => Err(failure(message)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(failure(error)),
        }
    }

    /// 固定服务器加载器位于工具目录，实际源码根始终是 workspace_root。
    pub fn server_entry(&self) -> PathBuf {
        self.support_root.join("server/lib/tsserver.js")
    }

    /// 校验助手给出的每个项目名、注册根与选中关联，不信任宽松 JSON 输出。
    fn validate_output(&self, selected: &[PathBuf]) -> Result<()> {
        for file in &self.config_files {
            mapping::checked_under(&self.workspace_root, file)?;
        }
        let mut names = std::collections::BTreeSet::new();
        for project in &self.registrations {
            mapping::lexical_under(&self.workspace_root, &project.name)?;
            if !names.insert(&project.name)
                || project.roots.is_empty()
                || !project.options.is_object()
            {
                return Err(failure("无效或重复的项目注册"));
            }
            for root in &project.roots {
                mapping::checked_under(&self.workspace_root, root)?;
            }
        }
        if self.files.len() != selected.len() {
            return Err(failure("助手未返回所有选中源码的关联"));
        }
        for (file, original) in self.files.iter().zip(selected) {
            if &file.file != original
                || !self.sources.contains_key(&file.file)
                || file.projects.is_empty()
                || file.projects.iter().any(|name| !names.contains(name))
            {
                return Err(failure("选中源码的项目关联无效"));
            }
        }
        if self.notes.len() > 200 {
            return Err(failure("助手分析说明超出上限"));
        }
        for note in &self.notes {
            mapping::lexical_under(&self.workspace_root, &note.file)?;
            if note.message.chars().count() > 4096 {
                return Err(failure("助手分析说明过长"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/project.rs"]
mod tests;
