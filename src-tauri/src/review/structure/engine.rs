//! 一次请求拥有固定版本的发现、提取、配对和关系分析，调用方只接收进度与结果。

use std::collections::{HashMap, HashSet};

use crate::core_git::{CoreGitFailureKind, GitCommandError};
use crate::operation::OperationContext;
use crate::review::content::{FileContent, SnapshotSide, SnapshotSourceError, SnapshotSourceTree};
use crate::review::SnapshotContentReader;

use super::extraction_model::ExtractedFile;
use super::matching_model::file_node_id;
use super::sources::SourceCatalog;
use super::*;

/// 一侧只保留提取后的证据；完整 AST 在处理下一文件前释放。
#[derive(Default)]
struct ExtractedSide {
    files: Vec<ExtractedFile>,
    configuration: HashMap<String, String>,
}

/// 分析固定 Snapshot；普通取消返回可阅读的部分结果，失效 Review 不再发布任何结果。
pub(crate) fn analyze_structure(
    reader: &SnapshotContentReader,
    operation: &OperationContext,
    review_id: &str,
    snapshot_id: &str,
    is_current: impl Fn() -> bool,
    mut emit: impl FnMut(StructureEvent),
) -> Result<StructureResult, GitCommandError> {
    if !is_current() {
        return Err(GitCommandError::cancelled());
    }
    let mut coverage = StructureCoverage {
        discovery: StructureDiscoveryCoverage {
            before: StructureCoverageStatus::NotStarted,
            after: StructureCoverageStatus::NotStarted,
        },
        files: Vec::new(),
        diagnostics: Vec::new(),
    };
    let before_tree = discover(
        reader,
        operation,
        StructureSide::Before,
        &is_current,
        &mut emit,
        &mut coverage,
    );
    let after_tree = discover(
        reader,
        operation,
        StructureSide::After,
        &is_current,
        &mut emit,
        &mut coverage,
    );
    let catalog = SourceCatalog::build(reader, before_tree.as_ref(), after_tree.as_ref());
    let before = extract_side(
        before_tree.as_ref(),
        StructureSide::Before,
        operation,
        &is_current,
        &mut emit,
        &mut coverage,
    );
    let after = extract_side(
        after_tree.as_ref(),
        StructureSide::After,
        operation,
        &is_current,
        &mut emit,
        &mut coverage,
    );
    record_unsupported_changes(&catalog.sources, &mut coverage);
    if !is_current() {
        return Err(GitCommandError::cancelled());
    }
    let should_continue = || is_current() && !operation.cancellation().is_cancelled();
    emit_phase(&mut emit, StructurePhase::Matching, None, 0, None);
    let mut matched = compare_declarations(
        &before.files,
        &after.files,
        &catalog.sources,
        &coverage,
        &before.configuration,
        &after.configuration,
        &should_continue,
    );
    if !is_current() {
        return Err(GitCommandError::cancelled());
    }
    emit_phase(&mut emit, StructurePhase::Relationships, None, 0, None);
    let relations = resolve_relationships(
        &before.files,
        &after.files,
        &matched,
        &coverage,
        &before.configuration,
        &after.configuration,
        &should_continue,
    );
    coverage.diagnostics.append(&mut matched.diagnostics);
    for updated in relations.file_coverage {
        if let Some(existing) = coverage
            .files
            .iter_mut()
            .find(|file| file.side == updated.side && file.path == updated.path)
        {
            *existing = updated;
        } else {
            coverage.files.push(updated);
        }
    }
    matched.nodes.extend(relations.external_nodes);
    let status = if operation.cancellation().is_cancelled() {
        StructureAnalysisStatus::Cancelled
    } else if before_tree.is_none() && after_tree.is_none() {
        StructureAnalysisStatus::Failed
    } else {
        StructureAnalysisStatus::Completed
    };
    let mut result = StructureResult {
        review_id: review_id.to_owned(),
        snapshot_id: snapshot_id.to_owned(),
        status,
        nodes: matched.nodes,
        implementation_groups: matched.implementation_groups,
        relationships: relations.relationships,
        unresolved_relationships: relations.unresolved_relationships,
        sources: catalog.sources,
        coverage,
        summary: StructureSummary {
            changed_declarations: 0,
            relationship_changes: 0,
            incomplete_declarations: 0,
            unresolved_occurrences: 0,
            complete: false,
        },
        initial_focus: StructureInitialFocus {
            node_ids: Vec::new(),
            relationship_ids: Vec::new(),
        },
    };
    summarize(&mut result);
    if !is_current() {
        return Err(GitCommandError::cancelled());
    }
    // 首次发布配对后的完整检查点，避免中途重配导致已展示的身份改变。
    emit(StructureEvent::Checkpoint {
        result: result.clone(),
    });
    Ok(result)
}

/// 只有成功的完整 tree 枚举才能证明一侧文件缺失。
fn discover(
    reader: &SnapshotContentReader,
    operation: &OperationContext,
    side: StructureSide,
    is_current: &impl Fn() -> bool,
    emit: &mut impl FnMut(StructureEvent),
    coverage: &mut StructureCoverage,
) -> Option<SnapshotSourceTree> {
    if !is_current() {
        return None;
    }
    emit_phase(emit, StructurePhase::Discovery, Some(side), 0, None);
    let result = reader.source_tree(
        operation,
        match side {
            StructureSide::Before => SnapshotSide::Before,
            StructureSide::After => SnapshotSide::After,
        },
    );
    let status = match side {
        StructureSide::Before => &mut coverage.discovery.before,
        StructureSide::After => &mut coverage.discovery.after,
    };
    match result {
        Ok(tree) => {
            *status = StructureCoverageStatus::Complete;
            Some(tree)
        }
        Err(error) => {
            *status = if error.is_cancelled() {
                StructureCoverageStatus::NotStarted
            } else {
                StructureCoverageStatus::Partial
            };
            coverage
                .diagnostics
                .push(git_diagnostic(&error, side, None));
            None
        }
    }
}

/// 批量读取原文后逐文件提取；未处理项保留覆盖缺口，单文件失败不丢弃同侧其他文件。
fn extract_side(
    tree: Option<&SnapshotSourceTree>,
    side: StructureSide,
    operation: &OperationContext,
    is_current: &impl Fn() -> bool,
    emit: &mut impl FnMut(StructureEvent),
    coverage: &mut StructureCoverage,
) -> ExtractedSide {
    let mut extracted = ExtractedSide::default();
    let Some(tree) = tree else {
        return extracted;
    };
    let paths: Vec<_> = tree
        .entries()
        .iter()
        .filter(|entry| {
            !dependency_path(&entry.path)
                && (source_language(&entry.path).is_some() || configuration_path(&entry.path))
        })
        .map(|entry| entry.path.as_str())
        .collect();
    let total = paths
        .iter()
        .filter(|path| source_language(path).is_some())
        .count() as u32;
    let start = coverage.files.len();
    for path in paths.iter().filter(|path| source_language(path).is_some()) {
        coverage.files.push(StructureFileCoverage {
            side,
            path: (*path).to_owned(),
            extraction: StructureCoverageStatus::NotStarted,
            relationships: StructureCoverageStatus::NotStarted,
            diagnostics: Vec::new(),
        });
    }
    if !is_current() {
        return extracted;
    }
    emit_phase(emit, StructurePhase::Extraction, Some(side), 0, Some(total));
    let reads = match tree.read_batch(operation, &paths) {
        Ok(reads) => reads,
        Err(error) => {
            for file in &mut coverage.files[start..] {
                file.diagnostics
                    .push(git_diagnostic(&error, side, Some(&file.path)));
            }
            return extracted;
        }
    };
    let mut processed = 0;
    for read in reads {
        if !is_current() || operation.cancellation().is_cancelled() {
            break;
        }
        let language = source_language(&read.path);
        if configuration_path(&read.path) {
            if let Ok(Some(FileContent::Text { ref text, .. })) = read.content {
                extracted
                    .configuration
                    .insert(read.path.clone(), text.clone());
            } else {
                coverage.diagnostics.push(StructureDiagnostic {
                    reason: StructureDiagnosticReason::ConfigurationUnavailable,
                    message: "固定 Snapshot 中的项目配置不可读取，相关导入绑定可能不完整。".into(),
                    side: Some(side),
                    path: Some(read.path.clone()),
                    range: None,
                    retryable: true,
                });
            }
        }
        let Some(language) = language else {
            continue;
        };
        let file = coverage.files[start..]
            .iter_mut()
            .find(|file| file.path == read.path)
            .expect("supported path has a coverage entry");
        match read.content {
            Ok(Some(FileContent::Text { text, .. })) => match parse_source(language, &text) {
                Ok(parsed) => {
                    let mut evidence = extract_source(&read.path, &parsed);
                    for diagnostic in &mut evidence.diagnostics {
                        diagnostic.side = Some(side);
                        diagnostic.path = Some(read.path.clone());
                    }
                    file.extraction = if evidence.complete {
                        StructureCoverageStatus::Complete
                    } else {
                        StructureCoverageStatus::Partial
                    };
                    file.diagnostics = evidence.diagnostics.clone();
                    extracted.files.push(evidence);
                }
                Err(error) => {
                    file.extraction = StructureCoverageStatus::Partial;
                    file.diagnostics.push(StructureDiagnostic {
                        reason: StructureDiagnosticReason::ParseError,
                        message: error.to_string(),
                        side: Some(side),
                        path: Some(read.path),
                        range: None,
                        retryable: true,
                    });
                }
            },
            Ok(Some(_)) => {
                file.extraction = StructureCoverageStatus::Partial;
                file.diagnostics.push(StructureDiagnostic {
                    reason: StructureDiagnosticReason::UnsupportedFileKind,
                    message: "此来源是二进制、符号链接或子模块，不能作为普通源码解析。".into(),
                    side: Some(side),
                    path: Some(read.path),
                    range: None,
                    retryable: false,
                });
            }
            Ok(None) => {
                file.extraction = StructureCoverageStatus::Partial;
                file.diagnostics.push(StructureDiagnostic {
                    reason: StructureDiagnosticReason::SourceUnreadable,
                    message: "已枚举的固定来源无法读取，不能据此推断声明不存在。".into(),
                    side: Some(side),
                    path: Some(read.path),
                    range: None,
                    retryable: true,
                });
            }
            Err(error) => {
                file.extraction = StructureCoverageStatus::Partial;
                file.diagnostics
                    .push(source_diagnostic(&error, side, &read.path));
            }
        }
        processed += 1;
        if is_current() {
            emit_phase(
                emit,
                StructurePhase::Extraction,
                Some(side),
                processed,
                Some(total),
            );
        }
    }
    for file in &mut coverage.files[start..] {
        if file.extraction == StructureCoverageStatus::NotStarted && file.diagnostics.is_empty() {
            file.diagnostics.push(StructureDiagnostic {
                reason: StructureDiagnosticReason::Cancelled,
                message: "分析在处理此文件前已取消。".into(),
                side: Some(side),
                path: Some(file.path.clone()),
                range: None,
                retryable: true,
            });
        }
    }
    extracted
}

/// grammar 选择留在分析边界内，支持对应语言族的常见模块后缀。
fn source_language(path: &str) -> Option<SourceLanguage> {
    SourceLanguage::from_path(path)
}

/// 配置包括可被 extends 引用的任意 JSON 文件，解析规则由仓库索引统一处理。
fn configuration_path(path: &str) -> bool {
    path.ends_with(".json")
        || path.ends_with(".jsonc")
        || path.rsplit('/').next() == Some("Cargo.toml")
}

/// 变更列表中不支持的文件仍有来源入口，并明确说明未进行声明分析。
fn record_unsupported_changes(sources: &[StructurePeekSource], coverage: &mut StructureCoverage) {
    for source in sources {
        if !matches!(
            source.classification,
            StructureSourceClassification::FileChange { .. }
        ) {
            continue;
        }
        for (side, source_side) in [
            (StructureSide::Before, &source.before),
            (StructureSide::After, &source.after),
        ] {
            let StructureSourceSide::Present { path, .. } = source_side else {
                continue;
            };
            if source_language(path).is_some() || configuration_path(path) {
                continue;
            }
            coverage.files.push(StructureFileCoverage {
                side,
                path: path.clone(),
                extraction: StructureCoverageStatus::Partial,
                relationships: StructureCoverageStatus::NotStarted,
                diagnostics: vec![StructureDiagnostic {
                    reason: StructureDiagnosticReason::UnsupportedLanguage,
                    message: "此文件不属于当前支持的 TypeScript、JavaScript 或 Rust 源码，可在 Peek 中阅读原文件。".into(),
                    side: Some(side), path: Some(path.clone()), range: None, retryable: false,
                }],
            });
        }
    }
}

/// 固定 tree 内的依赖和构建产物不作为仓库自有源码展开；已跟踪生成源码仍会分析。
fn dependency_path(path: &str) -> bool {
    path.split('/')
        .any(|part| part == "node_modules" || part == ".git")
        || path.starts_with("target/")
        || path.starts_with("src-tauri/target/")
}

/// 进度使用同一请求内有序事件，无需公开额外分析运行身份。
fn emit_phase(
    emit: &mut impl FnMut(StructureEvent),
    phase: StructurePhase,
    side: Option<StructureSide>,
    processed_files: u32,
    total_files: Option<u32>,
) {
    emit(StructureEvent::Progress {
        progress: StructureProgress {
            phase,
            side,
            processed_files,
            total_files,
        },
    });
}

/// 保留 Git 的取消与输出资源限制，不将失败归为空目录。
fn git_diagnostic(
    error: &GitCommandError,
    side: StructureSide,
    path: Option<&str>,
) -> StructureDiagnostic {
    let reason = match error.kind() {
        CoreGitFailureKind::Cancelled => StructureDiagnosticReason::Cancelled,
        CoreGitFailureKind::OutputLimitExceeded { .. } => StructureDiagnosticReason::ResourceLimit,
        CoreGitFailureKind::Exited { .. } => StructureDiagnosticReason::GitObjectUnavailable,
        _ => StructureDiagnosticReason::SourceUnreadable,
    };
    StructureDiagnostic {
        reason,
        message: error.to_string(),
        side: Some(side),
        path: path.map(str::to_owned),
        range: None,
        retryable: true,
    }
}

/// 单对象和文件种类失败分别保留；源码不可读不构成缺失证明。
fn source_diagnostic(
    error: &SnapshotSourceError,
    side: StructureSide,
    path: &str,
) -> StructureDiagnostic {
    if let SnapshotSourceError::Git(error) = error {
        return git_diagnostic(error, side, Some(path));
    }
    StructureDiagnostic {
        reason: match error {
            SnapshotSourceError::ObjectUnavailable { .. } => {
                StructureDiagnosticReason::GitObjectUnavailable
            }
            SnapshotSourceError::UnsupportedMode { .. } => {
                StructureDiagnosticReason::UnsupportedFileKind
            }
            SnapshotSourceError::Git(_) => unreachable!(),
        },
        message: error.to_string(),
        side: Some(side),
        path: Some(path.to_owned()),
        range: None,
        retryable: matches!(error, SnapshotSourceError::ObjectUnavailable { .. }),
    }
}

/// 变化计数与初始焦点使用双侧关系并集，关闭某种连线也不会失去变化入口。
fn summarize(result: &mut StructureResult) {
    let mut changed = HashSet::new();
    let mut initial = HashSet::new();
    let mut incomplete = 0;
    for node in &result.nodes {
        if let StructureNode::Declaration(declaration) = node {
            if !declaration.changes.is_empty() {
                changed.insert(declaration.id.clone());
                initial.insert(declaration.id.clone());
            }
            if declaration.comparison == StructureComparison::Incomplete {
                incomplete += 1;
                initial.insert(declaration.id.clone());
            }
        }
    }
    let mut relationship_changes = 0;
    for relation in &result.relationships {
        if matches!(
            relation.change,
            StructureRelationshipChange::Added | StructureRelationshipChange::Deleted
        ) {
            relationship_changes += 1;
        }
        if relation.change != StructureRelationshipChange::Retained {
            initial.insert(relation.from_id.clone());
            initial.insert(relation.to_id.clone());
        }
    }
    for file in &result.coverage.files {
        if file.extraction != StructureCoverageStatus::Complete {
            for source in &result.sources {
                let source_side = match file.side {
                    StructureSide::Before => &source.before,
                    StructureSide::After => &source.after,
                };
                if matches!(source_side, StructureSourceSide::Present { path, .. } if path == &file.path)
                {
                    initial.insert(file_node_id(&source.id));
                }
            }
        }
    }
    let seeds = initial.clone();
    let mut relationship_ids = Vec::new();
    for relation in &result.relationships {
        if seeds.contains(&relation.from_id) || seeds.contains(&relation.to_id) {
            initial.insert(relation.from_id.clone());
            initial.insert(relation.to_id.clone());
            relationship_ids.push(relation.id.clone());
        }
    }
    // 两侧 parent 都纳入容器闭包，因此移动结构仍能找到旧归属。
    loop {
        let previous_len = initial.len();
        for node in &result.nodes {
            if let StructureNode::Declaration(declaration) = node {
                if initial.contains(&declaration.id) {
                    for fact in [&declaration.before, &declaration.after] {
                        if let StructureFact::Present { value } = fact {
                            initial.insert(value.parent_id.clone());
                        }
                    }
                }
            }
        }
        if previous_len == initial.len() {
            break;
        }
    }
    let known: HashSet<_> = result
        .nodes
        .iter()
        .map(|node| match node {
            StructureNode::Declaration(value) => value.id.clone(),
            StructureNode::File(value) => value.id.clone(),
            StructureNode::External(value) => value.id.clone(),
        })
        .collect();
    let mut node_ids: Vec<_> = initial
        .into_iter()
        .filter(|id| known.contains(id))
        .collect();
    node_ids.sort();
    relationship_ids.sort();
    result.initial_focus = StructureInitialFocus {
        node_ids,
        relationship_ids,
    };
    result.summary = StructureSummary {
        changed_declarations: changed.len() as u32,
        relationship_changes,
        incomplete_declarations: incomplete,
        unresolved_occurrences: result.unresolved_relationships.len() as u32,
        complete: result.status == StructureAnalysisStatus::Completed
            && result.coverage.discovery.before == StructureCoverageStatus::Complete
            && result.coverage.discovery.after == StructureCoverageStatus::Complete
            && result.coverage.diagnostics.is_empty()
            && result.coverage.files.iter().all(|file| {
                file.extraction == StructureCoverageStatus::Complete
                    && file.relationships == StructureCoverageStatus::Complete
            })
            && incomplete == 0
            && result.unresolved_relationships.is_empty(),
    };
}

#[cfg(test)]
#[path = "tests/engine.rs"]
mod tests;
