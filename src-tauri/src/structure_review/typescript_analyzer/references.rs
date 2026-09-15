//! 选集只决定图成员；每个有效项目独立求证，未知与冲突证据不提升为确定关系。
#[path = "reference/targets.rs"]
mod targets;
use super::{
    failure,
    lsp::TypeScriptLsp,
    syntax::{self, Document, OccurrenceKind},
    Result,
};
use crate::{
    lsp::CancellationCheck,
    structure_review::{
        analysis::{AnalysisNote, AnalysisReport},
        code_graph::{AnalysisError, CodeGraph, CodeReference, CodeRoot, NodeId, ReferenceTarget},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

/// 各选中文件共享一个索引；上下文解析缓存不扩大输出范围。
pub(super) fn analyze(
    session: &mut TypeScriptLsp,
    documents: BTreeMap<PathBuf, Document>,
    cancelled: &CancellationCheck,
) -> Result<AnalysisReport> {
    let mut nodes = BTreeMap::new();
    let mut root = CodeRoot::default();
    let mut queries = Vec::new();
    for (path, document) in &documents {
        if document.excluded {
            continue;
        }
        for declaration in &document.declarations {
            if declaration.parent.is_none() {
                root.contains.push(declaration.node.data().id.clone());
            }
            nodes.insert(declaration.node.data().id.clone(), declaration.node.clone());
        }
        let projects = session
            .project()
            .files
            .iter()
            .find(|f| f.file == *path)
            .ok_or_else(|| failure("选中文档缺少真实项目关联"))?
            .projects
            .clone();
        for occurrence in &document.occurrences {
            queries.push((path.clone(), projects.clone(), occurrence.clone()));
        }
    }
    let membership: BTreeSet<NodeId> = nodes.keys().cloned().collect();
    let mut resolver = targets::Resolver::new(documents, membership);
    let mut notes = Vec::new();
    for (path, projects, occurrence) in queries {
        if cancelled() {
            return Err(AnalysisError::cancelled("TypeScript 结构分析已取消"));
        }
        if occurrence.kind == OccurrenceKind::Reference && occurrence.type_only {
            continue;
        }
        let mut alternatives = Vec::new();
        for project in &projects {
            alternatives.push(resolver.resolve(session, &path, &occurrence, project)?);
        }
        let conflict = alternatives.windows(2).any(|pair| pair[0] != pair[1]);
        if conflict {
            let file = session.project().sources.get(&path).map(|s| s.path.clone());
            push_note(
                &mut notes,
                AnalysisNote {
                    code: "typescript.conflicting_projects".into(),
                    message: format!(
                        "多个有效项目对 {} 的目标解析不一致，保留为未解析引用",
                        occurrence.text
                    ),
                    file: file.and_then(|f| {
                        f.strip_prefix(&session.project().workspace_root)
                            .ok()
                            .map(PathBuf::from)
                    }),
                    project: None,
                },
            );
        }
        // 普通读取只有确认为函数值时才保留；不扩大到任意数据读取或一般类型使用。
        if occurrence.kind == OccurrenceKind::Reference && alternatives.iter().all(|a| a.is_none())
        {
            continue;
        }
        let targets = if conflict {
            BTreeSet::from([ReferenceTarget::Unresolved {
                name: occurrence.semantic_text.clone(),
            }])
        } else {
            alternatives
                .into_iter()
                .next()
                .flatten()
                .unwrap_or_else(|| {
                    BTreeSet::from([ReferenceTarget::Unresolved {
                        name: occurrence.semantic_text.clone(),
                    }])
                })
        };
        let node = nodes
            .get_mut(&occurrence.owner)
            .ok_or_else(|| failure("引用所有者不在选集图内"))?;
        syntax::data_mut(node)
            .references
            .extend(targets.into_iter().map(|target| CodeReference {
                semantic: occurrence.kind.semantic().into(),
                target,
            }));
    }
    for note in &session.project().notes {
        let relative = note
            .file
            .strip_prefix(&session.project().workspace_root)
            .ok()
            .map(PathBuf::from);
        push_note(
            &mut notes,
            AnalysisNote {
                code: note
                    .code
                    .clone()
                    .unwrap_or_else(|| "typescript.context".into()),
                message: note.message.replace(
                    &session
                        .project()
                        .workspace_root
                        .to_string_lossy()
                        .to_string(),
                    "[仓库]",
                ),
                file: relative,
                project: note.project.as_ref().and_then(|p| {
                    p.strip_prefix(&session.project().workspace_root)
                        .ok()
                        .map(PathBuf::from)
                }),
            },
        );
    }
    for node in nodes.values_mut() {
        let refs = &mut syntax::data_mut(node).references;
        refs.sort();
        refs.dedup();
    }
    let used_external = nodes
        .values()
        .flat_map(|node| node.data().references.iter())
        .filter_map(|reference| {
            if let ReferenceTarget::External { symbol } = &reference.target {
                Some(symbol.clone())
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    resolver
        .external_symbols
        .retain(|symbol, _| used_external.contains(symbol));
    let graph = CodeGraph::new(root, nodes.into_values().collect())
        .map_err(failure)?
        .with_external_symbols(resolver.external_symbols);
    Ok(AnalysisReport { graph, notes })
}

/// 说明去重且有界；缺失上下文不能随引用数量无限放大。
fn push_note(notes: &mut Vec<AnalysisNote>, mut note: AnalysisNote) {
    note.message = note.message.chars().take(4096).collect();
    if notes.len() < 200 && !notes.contains(&note) {
        notes.push(note);
    }
}
