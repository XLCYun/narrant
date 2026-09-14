//! 受控项目视图拥有源码快照与路径映射，生命周期由语义会话持有。
mod mapping;
use super::{failure, runtime, tools::TypeScriptToolchain, Result};
use crate::lsp::CancellationCheck;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
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
    pub file: PathBuf,
    pub message: String,
}

/// 源码始终保留原始侧身份；视图路径只供服务查询。
#[derive(Clone, Debug)]
pub(crate) struct SourceSnapshot {
    pub original: PathBuf,
    pub view: PathBuf,
    pub source: String,
}

/// 选中源码对应的全部配置候选，供后续语义层分别查询并比较证据。
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileProjects {
    pub original: PathBuf,
    pub view: PathBuf,
    pub projects: Vec<PathBuf>,
}

/// 助手的严格输出结构；未知字段和缺失字段均视为协议失败。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedOutput {
    config_files: Vec<PathBuf>,
    projects: Vec<ProjectRegistration>,
    associations: Vec<FileProjects>,
    sources: Vec<SourcePaths>,
    metadata: Vec<SourcePaths>,
    notes: Vec<AnalysisNote>,
}

/// 助手只返回路径，Rust 自己从准备后的不可变视图读取源码快照。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourcePaths {
    original: PathBuf,
    view: PathBuf,
}

/// 完整受控输入；析构临时目录必须晚于 LSP 全部进程回收。
#[derive(Debug)]
pub(crate) struct PreparedProject {
    _directory: TempDir,
    pub original_root: PathBuf,
    pub view_root: PathBuf,
    pub toolchain: TypeScriptToolchain,
    pub config_files: Vec<PathBuf>,
    pub registrations: Vec<ProjectRegistration>,
    pub files: Vec<FileProjects>,
    pub allowed_roots: Vec<PathBuf>,
    pub notes: Vec<AnalysisNote>,
    pub sources: BTreeMap<PathBuf, SourceSnapshot>,
    pub metadata: BTreeMap<PathBuf, PathBuf>,
}
impl PreparedProject {
    /// 非空选集才验证工具并准备视图；调用方对空结果无需启动服务。
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
        let original_root = root.canonicalize().map_err(failure)?;
        let selected = selected
            .iter()
            .map(|file| {
                mapping::lexical_under(root, file)?;
                let normalized = original_root.join(file.strip_prefix(root).map_err(failure)?);
                mapping::checked_under(&original_root, &normalized)?;
                // 将入口统一为根目录的规范路径，同时保留根内文件别名。
                Ok(original_root.join(file.strip_prefix(root).map_err(failure)?))
            })
            .collect::<Result<Vec<_>>>()?;
        let toolchain = tools.validate(timeout, cancelled)?;
        let directory = tempfile::tempdir().map_err(failure)?;
        let work = directory.path().canonicalize().map_err(failure)?;
        let view_root = work.join("view");
        std::fs::create_dir(&view_root).map_err(failure)?;
        for (name, source) in runtime::HELPERS {
            std::fs::write(work.join(name), source).map_err(failure)?;
        }
        let output = work.join("output.json");
        let input = work.join("input.json");
        std::fs::write(&input, serde_json::to_vec(&json!({"root": original_root, "view": view_root, "runtime": toolchain.runtime_root, "selected": selected, "output": output})).map_err(failure)?).map_err(failure)?;
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
            view_root.clone(),
            toolchain.runtime_root.join("typescript/lib"),
            toolchain.runtime_root.join("@types/node"),
            toolchain.runtime_root.join("undici-types"),
        ];
        let mut prepared = Self {
            _directory: directory,
            original_root,
            view_root,
            toolchain,
            config_files: result.config_files,
            registrations: result.projects,
            files: result.associations,
            allowed_roots,
            notes: result.notes,
            sources: BTreeMap::new(),
            metadata: BTreeMap::new(),
        };
        for paths in result.sources {
            mapping::checked_under(&prepared.original_root, &paths.original)?;
            mapping::checked_under(&prepared.view_root, &paths.view)?;
            if prepared.original_root.join(
                paths
                    .view
                    .strip_prefix(&prepared.view_root)
                    .map_err(failure)?,
            ) != paths.original
            {
                return Err(failure("助手源码映射不一致"));
            }
            let source = std::fs::read_to_string(&paths.view).map_err(failure)?;
            if prepared
                .sources
                .insert(
                    paths.view.clone(),
                    SourceSnapshot {
                        original: paths.original,
                        view: paths.view,
                        source,
                    },
                )
                .is_some()
            {
                return Err(failure("重复的源码视图路径"));
            }
        }
        for paths in result.metadata {
            mapping::checked_under(&prepared.original_root, &paths.original)?;
            mapping::checked_under(&prepared.view_root, &paths.view)?;
            if prepared.original_root.join(
                paths
                    .view
                    .strip_prefix(&prepared.view_root)
                    .map_err(failure)?,
            ) != paths.original
            {
                return Err(failure("助手元数据映射不一致"));
            }
            if prepared
                .metadata
                .insert(paths.view, paths.original)
                .is_some()
            {
                return Err(failure("重复的元数据视图路径"));
            }
        }
        prepared.validate_output(&selected)?;
        Ok(Some(prepared))
    }

    /// 校验助手给出的每个项目名、注册根与选中关联，不信任宽松 JSON 输出。
    fn validate_output(&self, selected: &[PathBuf]) -> Result<()> {
        for file in &self.config_files {
            mapping::checked_under(&self.view_root, file)?;
        }
        let mut names = std::collections::BTreeSet::new();
        for project in &self.registrations {
            mapping::lexical_under(&self.view_root, &project.name)?;
            if !names.insert(&project.name)
                || project.roots.is_empty()
                || !project.options.is_object()
            {
                return Err(failure("无效或重复的项目注册"));
            }
            for root in &project.roots {
                mapping::checked_under(&self.view_root, root)?;
            }
        }
        if self.files.len() != selected.len() {
            return Err(failure("助手未返回所有选中源码的关联"));
        }
        for (file, original) in self.files.iter().zip(selected) {
            if &file.original != original
                || self
                    .sources
                    .get(&file.view)
                    .is_none_or(|source| source.original != *original)
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
            mapping::lexical_under(&self.original_root, &note.file)?;
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
