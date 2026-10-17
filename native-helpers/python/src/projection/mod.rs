//! Python 对象和运行时关系政策迁自既有实现，原文/AST/导航全部来自固定原生数据库。
mod bindings;
mod fields;
mod known;
mod project;
mod references;
mod syntax;

use crate::{ProjectInput, PythonProject, protocol::Request};
use anyhow::{Context, Result, ensure};
use narrant_native_common::model::{CodeGraph, NodeId, ReferenceTarget};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// 保留可读语言上下文，提取失败不变成空图。
fn failure(message: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("Python 结构分析：{message}")
}

/// 文件级测试约定只观察本仓库相对路径，实际路径也须核验。
fn is_test(relative: &Path) -> bool {
    relative
        .components()
        .any(|part| part.as_os_str() == "tests")
        || relative
            .file_stem()
            .and_then(|value| value.to_str())
            .is_some_and(|name| {
                name.starts_with("test_") || name.ends_with("_test") || name == "conftest"
            })
}

/// 每侧按原有配置/PEP723/版本组合加载独立数据库；上下文不自动变成图成员。
pub fn analyze(request: &Request) -> Result<Value> {
    request.validate()?;
    let repository = request.input.repository_root.canonicalize()?;
    let mut groups = BTreeMap::new();
    let mut files = Vec::new();
    let mut configurations = Vec::new();
    let mut snapshots = BTreeMap::new();
    for relative in &request.input.files {
        let path = repository.join(relative);
        let actual = project::checked_path(&repository, &path)?;
        ensure!(
            path.is_file()
                && !project::environment_path(&repository, &path)
                && !project::environment_path(&repository, &actual),
            "选中的 Python 路径不是仓库源码文件"
        );
        if !request.input.include_tests
            && (is_test(Path::new(relative)) || is_test(actual.strip_prefix(&repository)?))
        {
            files.push(json!({"path":relative,"disposition":"excluded_test","configuration_ids":["python-excluded"]}));
            continue;
        }
        let source = project::read_source(&path)?;
        let policy = project::Project::for_file(&repository, &path, &source)?;
        snapshots.insert(path.clone(), source.clone());
        groups
            .entry(policy)
            .or_insert_with(Vec::new)
            .push((path, source));
    }
    if files
        .iter()
        .any(|file| file["disposition"] == "excluded_test")
    {
        configurations.push(json!({"id":"python-excluded","source_paths":[],"platform_ids":[]}));
    }
    let mut root_contains = Vec::new();
    let mut nodes = Vec::new();
    let mut externals = BTreeMap::new();
    let mut origins = Vec::new();
    for (index, (policy, selected)) in groups.into_iter().enumerate() {
        let configuration_id = format!("python-project-{index}");
        let mut source_paths = Vec::new();
        for name in ["pyproject.toml", "ty.toml"] {
            let path = policy.root.join(name);
            if path.exists() {
                project::checked_path(&repository, &path)?;
                snapshots.insert(path.clone(), std::fs::read_to_string(&path)?);
                source_paths.push(
                    path.strip_prefix(&repository)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
        configurations.push(json!({"id":configuration_id,"source_paths":source_paths,"platform_ids":[request.environment.platforms[0].id]}));
        let native = PythonProject::load(ProjectInput {
            repository: repository.clone(),
            project_root: policy.root.strip_prefix(&repository)?.into(),
            source_roots: policy
                .source_roots
                .iter()
                .map(|path| path.strip_prefix(&repository).map(PathBuf::from))
                .collect::<std::result::Result<Vec<_>, _>>()?,
            python_minor: policy.minor,
        })?;
        let mut parsed = BTreeMap::new();
        for (path, source) in selected {
            let relative = path.strip_prefix(&repository)?;
            ensure!(
                native.source(relative)? == source,
                "分析期间选中源码已改变：{}",
                relative.display()
            );
            let ast = native
                .parse(relative)
                .with_context(|| format!("{}：Python 语法提取失败", relative.display()))?;
            let document =
                syntax::Document::from_ast(relative, &source, ast.syntax().body.clone())?;
            files.push(json!({"path":relative.to_string_lossy().replace('\\', "/"),"disposition":"analyzed","configuration_ids":[configuration_id]}));
            parsed.insert(path, document);
        }
        let selected_roots: BTreeMap<_, _> = parsed
            .iter()
            .filter_map(|(path, document)| {
                document.root.clone().map(|root| {
                    (
                        root,
                        path.strip_prefix(&repository)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    )
                })
            })
            .collect();
        let (documents, symbols) = references::analyze(
            &native,
            &repository,
            &policy,
            parsed,
            request.input.include_tests,
        )?;
        for document in documents {
            if let Some(root) = &document.root {
                let path = &selected_roots[root];
                origins.extend(
                    document
                        .nodes
                        .iter()
                        .map(|node| json!({"node_id":node.data().id.0,"selected_paths":[path]})),
                );
            }
            root_contains.extend(document.root);
            nodes.extend(document.nodes);
        }
        externals.extend(symbols);
    }
    let ids: BTreeSet<_> = nodes.iter().map(|node| node.data().id.clone()).collect();
    for node in &mut nodes {
        for reference in &mut syntax::node_data_mut(node).references {
            if let ReferenceTarget::External { symbol } = &reference.target {
                let id = NodeId(symbol.clone());
                if ids.contains(&id) {
                    reference.target = ReferenceTarget::Node { id };
                }
            }
        }
    }
    externals.retain(|symbol, _| !ids.contains(&NodeId(symbol.clone())));
    for (path, source) in snapshots {
        ensure!(
            std::fs::read_to_string(&path)? == source,
            "分析期间源码或配置已改变：{}",
            path.display()
        );
    }
    Ok(
        json!({"graph":CodeGraph {root_contains, nodes, external_symbols:externals.into_values().collect()},"evidence":{"configurations":configurations,"files":files,"origins":origins}}),
    )
}

#[cfg(test)]
#[path = "tests/project.rs"]
mod tests;
