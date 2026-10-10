//! 从同源原生 AST/导航迁移完整 Rust 对象规则，只输出统一 DTO，Host 负责最终构图。

mod model;
mod references;
mod symbols;
mod syntax;
mod test_code;

use crate::{DependencyScope, RustProject, protocol::Request};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

/// 图投影只保存精确的物理文件与字节位置，不保留 LSP 传输位置。
#[derive(Clone)]
struct Navigation {
    key: String,
    offset: ::syntax::TextSize,
}

/// 为迁入的投影规则统一构造带上下文的错误。
fn failure(message: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(message.to_string())
}

/// 每个请求独立加载并输出全图，任何查询错误立即丢弃项目和全部候选数据。
pub fn analyze(request: &Request) -> Result<Value> {
    request.validate()?;
    let repository = request.input.repository_root.canonicalize()?;
    let mut selected: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    for relative in &request.input.files {
        let path = repository
            .join(relative)
            .canonicalize()
            .with_context(|| format!("无法读取 {relative}"))?;
        ensure!(
            path.starts_with(&repository)
                && path.is_file()
                && path.extension().is_some_and(|extension| extension == "rs"),
            "选集必须为仓库内 Rust 源码"
        );
        selected.entry(path).or_default().push(relative.clone());
    }
    if selected.is_empty() {
        return Ok(
            json!({"graph":{"root_contains":[],"nodes":[],"external_symbols":[]},"evidence":{"configurations":[],"files":[],"origins":[]}}),
        );
    }
    let manifests = crate::discovery::manifests(&repository)?;
    ensure!(
        !manifests.is_empty(),
        "未发现 Rust workspace，无法完整加载 standalone 源码"
    );
    let scope = match request.input.profile.as_str() {
        "rust-repository-v1" => DependencyScope::RepositoryOnly,
        "rust-standard-library-v1" => DependencyScope::StandardLibrary,
        "rust-all-dependencies-v1" => DependencyScope::AllDependencies,
        _ => unreachable!("请求已验证"),
    };
    let policies =
        crate::discovery::policies(&manifests, scope, request.environment.platforms.first())?;
    let project = RustProject::load_projects(
        &manifests,
        &policies,
        &selected.keys().cloned().collect::<Vec<_>>(),
    )?;
    let options = model::AnalysisOptions {
        include_tests: request.input.include_tests,
    };
    let documents = selected
        .keys()
        .map(|path| syntax::Document::parse(&repository, path.clone(), &project, options))
        .collect::<Result<Vec<_>>>()?;
    let (graph, test_files) = references::analyze(&project, &repository, &documents, options)?;
    let mut origins = Vec::new();
    for node in &graph.nodes {
        let id = &node.data().id;
        let document = documents
            .iter()
            .find(|document| {
                document
                    .declarations
                    .iter()
                    .any(|declaration| &declaration.node.data().id == id)
                    || syntax::file_scope(&repository, &document.path)
                        .is_some_and(|scope| id.0 == format!("{scope}::crate"))
            })
            .context("输出节点缺少选集出处")?;
        origins.push(json!({"node_id":id.0,"selected_paths":selected[&document.path]}));
    }
    let files: Vec<_> = documents.iter().flat_map(|document| {
        let analyzed = !test_files.get(&document.key).copied().unwrap_or(false);
        selected[&document.path].iter().map(move |path| json!({"path":path,"disposition":if analyzed {"analyzed"} else {"excluded_test"},"configuration_ids":["rust-projects"]}))
    }).collect();
    // selected 源码固定到一次请求结束；变化不能被误报成同一侧成功结果。
    for document in &documents {
        ensure!(
            std::fs::read_to_string(&document.path)? == document.source,
            "分析期间选中源码已改变：{}",
            document.path.display()
        );
    }
    let source_paths: Vec<_> = manifests
        .iter()
        .filter_map(|path| path.strip_prefix(&repository).ok())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    let platform_ids: Vec<_> = request
        .environment
        .platforms
        .iter()
        .map(|platform| &platform.id)
        .collect();
    Ok(
        json!({"graph":graph,"evidence":{"configurations":[{"id":"rust-projects","source_paths":source_paths,"platform_ids":platform_ids}],"files":files,"origins":origins}}),
    )
}
