//! 将单侧词法关系绑定为已匹配端点，并在覆盖证明允许时计算关系变化。

use std::collections::{BTreeMap, HashMap, HashSet};

use super::extraction_model::{ExtractedFile, ExtractedOccurrence};
use super::matching_model::{
    external_node_id, file_node_id, MatchedSide, MatchedStructure, ResolvedRelationships,
};
use super::model::*;
use super::repository_index::{BindingResolution, ModuleResolution, RepositoryIndex};

/// 稳定键不包含行号、源码路径或发生次数，因此纯行位移不会制造关系变化。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeKey {
    from: StructureNodeId,
    to: StructureNodeId,
    kind: StructureRelationshipKind,
    certainty: StructureCertainty,
}

/// 每侧保留关系证据及不能用于缺失证明的来源/种类。
#[derive(Default)]
struct SideRelationships {
    edges: BTreeMap<EdgeKey, Vec<StructureOccurrence>>,
    unresolved: Vec<StructureUnresolvedRelationship>,
    external: BTreeMap<StructureNodeId, StructureExternalNode>,
    coverage: Vec<StructureFileCoverage>,
    coverage_index: HashMap<String, usize>,
    origins: HashMap<StructureNodeId, Vec<String>>,
    incomplete: HashMap<(StructureNodeId, StructureRelationshipKind), Vec<StructureDiagnostic>>,
    unknown_origins: HashMap<StructureRelationshipKind, Vec<StructureDiagnostic>>,
    cancelled: bool,
}

/// 以两侧独立配置解析关系；取消后保留已观察结果，但不证明未处理关系缺失。
pub(crate) fn resolve_relationships(
    before: &[ExtractedFile],
    after: &[ExtractedFile],
    matched: &MatchedStructure,
    coverage: &StructureCoverage,
    before_config: &HashMap<String, String>,
    after_config: &HashMap<String, String>,
    should_continue: &dyn Fn() -> bool,
) -> ResolvedRelationships {
    let mut old = analyze_side(
        before,
        before_config,
        &matched.before,
        StructureSide::Before,
        coverage,
        should_continue,
    );
    let mut new = analyze_side(
        after,
        after_config,
        &matched.after,
        StructureSide::After,
        coverage,
        should_continue,
    );
    let mut keys: Vec<_> = old.edges.keys().chain(new.edges.keys()).cloned().collect();
    keys.sort();
    keys.dedup();
    let mut relationships = Vec::with_capacity(keys.len());
    for key in keys {
        if !should_continue() {
            old.cancelled = true;
            new.cancelled = true;
        }
        let before = edge_fact(
            &key,
            &old,
            &matched.before,
            &matched.nodes,
            StructureSide::Before,
            coverage,
        );
        let after = edge_fact(
            &key,
            &new,
            &matched.after,
            &matched.nodes,
            StructureSide::After,
            coverage,
        );
        let certainty = key.certainty;
        let change = if certainty == StructureCertainty::Candidate {
            StructureRelationshipChange::Indeterminate
        } else {
            match (&before, &after) {
                (StructureFact::Present { .. }, StructureFact::Present { .. }) => {
                    StructureRelationshipChange::Retained
                }
                (StructureFact::Absent, StructureFact::Present { .. }) => {
                    StructureRelationshipChange::Added
                }
                (StructureFact::Present { .. }, StructureFact::Absent) => {
                    StructureRelationshipChange::Deleted
                }
                _ => StructureRelationshipChange::Indeterminate,
            }
        };
        relationships.push(StructureRelationship {
            id: relation_id(&key),
            from_id: key.from,
            to_id: key.to,
            kind: key.kind,
            certainty,
            before,
            after,
            change,
        });
    }
    let known_external: HashSet<_> = matched
        .nodes
        .iter()
        .filter_map(|node| {
            if let StructureNode::External(node) = node {
                Some(node.id.clone())
            } else {
                None
            }
        })
        .collect();
    let external: BTreeMap<_, _> = old.external.into_iter().chain(new.external).collect();
    ResolvedRelationships {
        relationships,
        unresolved_relationships: old.unresolved.into_iter().chain(new.unresolved).collect(),
        external_nodes: external
            .into_values()
            .filter(|node| !known_external.contains(&node.id))
            .map(StructureNode::External)
            .collect(),
        file_coverage: old.coverage.into_iter().chain(new.coverage).collect(),
    }
}

/// 单侧循环只解释当前源码及配置，任何未知发生点都会记录诊断和局部证明缺口。
fn analyze_side(
    files: &[ExtractedFile],
    config: &HashMap<String, String>,
    matched: &MatchedSide,
    side: StructureSide,
    coverage: &StructureCoverage,
    should_continue: &dyn Fn() -> bool,
) -> SideRelationships {
    let active = should_continue();
    let index = RepositoryIndex::new(if active { files } else { &[] }, config);
    let mut result = SideRelationships {
        cancelled: !active,
        ..SideRelationships::default()
    };
    for (key, id) in &matched.declarations {
        result
            .origins
            .entry(id.clone())
            .or_default()
            .push(key.0.clone());
    }
    for (path, source) in &matched.source_ids {
        result
            .origins
            .entry(file_node_id(source))
            .or_default()
            .push(path.clone());
    }
    for (key, implementation) in &matched.implementations {
        if let Some(owner) = &implementation.owner_id {
            result
                .origins
                .entry(owner.clone())
                .or_default()
                .push(key.0.clone());
        }
    }
    for paths in result.origins.values_mut() {
        paths.sort();
        paths.dedup();
    }
    for file in coverage.files.iter().filter(|file| {
        file.side == side
            && file.path.ends_with(".rs")
            && file.extraction != StructureCoverageStatus::Complete
    }) {
        let diagnostic = diagnostic(
            side,
            &file.path,
            None,
            StructureDiagnosticReason::UnreliableScope,
        );
        for kind in [
            StructureRelationshipKind::Implements,
            StructureRelationshipKind::TypeReference,
        ] {
            result
                .unknown_origins
                .entry(kind)
                .or_default()
                .push(diagnostic.clone());
        }
    }
    for file in files {
        let mut file_coverage = coverage
            .files
            .iter()
            .find(|entry| entry.side == side && entry.path == file.path)
            .cloned()
            .unwrap_or_else(|| StructureFileCoverage {
                side,
                path: file.path.clone(),
                extraction: StructureCoverageStatus::Partial,
                relationships: StructureCoverageStatus::NotStarted,
                diagnostics: Vec::new(),
            });
        file_coverage.relationships =
            if file.complete && file_coverage.extraction == StructureCoverageStatus::Complete {
                StructureCoverageStatus::Complete
            } else {
                StructureCoverageStatus::Partial
            };
        let initial_unresolved = result.unresolved.len();
        if !should_continue() {
            result.cancelled = true;
        }
        if result.cancelled {
            file_coverage.relationships = StructureCoverageStatus::NotStarted;
            file_coverage.diagnostics.push(diagnostic(
                side,
                &file.path,
                None,
                StructureDiagnosticReason::Cancelled,
            ));
            result.coverage.push(file_coverage);
            continue;
        }
        // Rust trait impl 已由提取层提供精确 trait 发生点；这里绑定聚合 owner，不再用整块范围重复合成。
        for occurrence in &file.occurrences {
            if !should_continue() {
                result.cancelled = true;
                break;
            }
            // 显式导入由导入表处理；动态导入仍保留未知模块依赖发生点。
            if occurrence.kind == StructureRelationshipKind::Imports {
                if file
                    .imports
                    .iter()
                    .any(|import| import.range.bytes == occurrence.range.bytes)
                {
                    continue;
                }
                if let Some(source) = matched.source_ids.get(&file.path) {
                    let provenance = occurrence_value(
                        source,
                        side,
                        occurrence,
                        StructureEvidenceKind::ModulePath,
                    );
                    record_unresolved(
                        &mut result,
                        side,
                        file,
                        occurrence,
                        file_node_id(source),
                        provenance,
                        Vec::new(),
                        StructureDiagnosticReason::DynamicTarget,
                        should_continue,
                    );
                }
                continue;
            }
            let Some(from) = occurrence_source(file, occurrence, matched) else {
                continue;
            };
            if occurrence.impl_group.is_some_and(|group| {
                matched
                    .implementations
                    .get(&(file.path.clone(), group))
                    .and_then(|implementation| implementation.owner_id.as_ref())
                    .is_none()
            }) && (occurrence.kind == StructureRelationshipKind::Implements
                || occurrence.declaration.is_none())
            {
                let diagnostic = diagnostic(
                    side,
                    &file.path,
                    Some(occurrence.range.position.clone()),
                    StructureDiagnosticReason::UnreliableScope,
                );
                result
                    .unknown_origins
                    .entry(occurrence.kind)
                    .or_default()
                    .push(diagnostic);
                if let Some(source) = matched.source_ids.get(&file.path) {
                    let provenance = occurrence_value(
                        source,
                        side,
                        occurrence,
                        StructureEvidenceKind::ExplicitType,
                    );
                    record_unresolved(
                        &mut result,
                        side,
                        file,
                        occurrence,
                        from,
                        provenance,
                        Vec::new(),
                        StructureDiagnosticReason::UnreliableScope,
                        should_continue,
                    );
                }
                continue;
            }
            let resolved = if !occurrence.reliable || !scope_reliable(file, occurrence.scope) {
                BindingResolution::Unknown(StructureDiagnosticReason::UnreliableScope)
            } else {
                resolve_target(&index, file, occurrence)
            };
            record_resolution(
                &index,
                &mut result,
                matched,
                side,
                file,
                occurrence,
                from,
                resolved,
                should_continue,
            );
        }
        if !result.cancelled {
            for import in &file.imports {
                // 本文件内 export 别名并不依赖一个新模块。
                if import.module.is_empty() && !file.path.ends_with(".rs") {
                    continue;
                }
                if !should_continue() {
                    result.cancelled = true;
                    break;
                }
                let Some(source) = matched.source_ids.get(&file.path) else {
                    continue;
                };
                let from = file_node_id(source);
                let occurrence = ExtractedOccurrence {
                    kind: StructureRelationshipKind::Imports,
                    written_target: import.module.clone(),
                    declaration: None,
                    impl_group: None,
                    scope: import.scope,
                    range: import.range.clone(),
                    callback_range: None,
                    dynamic_this: false,
                    reliable: scope_reliable(file, import.scope),
                };
                let provenance =
                    occurrence_value(source, side, &occurrence, StructureEvidenceKind::ModulePath);
                match if occurrence.reliable {
                    index.resolve_import_module(&file.path, import)
                } else {
                    ModuleResolution::Unknown(StructureDiagnosticReason::UnreliableScope)
                } {
                    ModuleResolution::Files(paths) => {
                        let targets: Vec<_> = paths
                            .iter()
                            .filter_map(|path| matched.source_ids.get(path).map(file_node_id))
                            .collect();
                        if targets.len() == 1 && paths.len() == 1 {
                            insert_edge(
                                &mut result,
                                &from,
                                &targets[0],
                                occurrence.kind,
                                StructureCertainty::Resolved,
                                provenance,
                            );
                        } else {
                            record_unresolved(
                                &mut result,
                                side,
                                file,
                                &occurrence,
                                from,
                                provenance,
                                targets,
                                StructureDiagnosticReason::AmbiguousTarget,
                                should_continue,
                            );
                        }
                    }
                    ModuleResolution::External(package_name) => {
                        let target =
                            external_target(&mut result, package_name.clone(), package_name);
                        insert_edge(
                            &mut result,
                            &from,
                            &target,
                            occurrence.kind,
                            StructureCertainty::Resolved,
                            provenance,
                        );
                    }
                    ModuleResolution::Unknown(reason) => record_unresolved(
                        &mut result,
                        side,
                        file,
                        &occurrence,
                        from,
                        provenance,
                        Vec::new(),
                        reason,
                        should_continue,
                    ),
                }
            }
        }
        if result.cancelled {
            file_coverage.relationships = StructureCoverageStatus::Partial;
            file_coverage.diagnostics.push(diagnostic(
                side,
                &file.path,
                None,
                StructureDiagnosticReason::Cancelled,
            ));
        }
        if result.unresolved.len() > initial_unresolved {
            file_coverage.relationships = StructureCoverageStatus::Partial;
            for unresolved in &result.unresolved[initial_unresolved..] {
                for diagnostic in &unresolved.diagnostics {
                    if !file_coverage.diagnostics.contains(diagnostic) {
                        file_coverage.diagnostics.push(diagnostic.clone());
                    }
                }
            }
        }
        result.coverage.push(file_coverage);
    }
    for entry in coverage
        .files
        .iter()
        .filter(|entry| entry.side == side && !files.iter().any(|file| file.path == entry.path))
    {
        let mut entry = entry.clone();
        entry.relationships = StructureCoverageStatus::NotStarted;
        result.coverage.push(entry);
    }
    result.coverage_index = result
        .coverage
        .iter()
        .enumerate()
        .map(|(index, file)| (file.path.clone(), index))
        .collect();
    result
}

/// this/self 只沿真实声明或 impl 归属解析，不使用全局同名方法搜索。
fn resolve_target(
    index: &RepositoryIndex<'_>,
    file: &ExtractedFile,
    occurrence: &ExtractedOccurrence,
) -> BindingResolution {
    let written = occurrence.written_target.trim();
    let mut static_this = None;
    if written == "Self" {
        if let Some(group) = occurrence
            .impl_group
            .and_then(|group| file.impl_groups.get(group))
        {
            return index.resolve_type(&file.path, group.scope, &group.owner);
        }
    }
    if let Some(member) = written
        .strip_prefix("this.")
        .or_else(|| written.strip_prefix("self."))
        .or_else(|| written.strip_prefix("Self::"))
    {
        if written.starts_with("this.") && occurrence.dynamic_this {
            return BindingResolution::Unknown(StructureDiagnosticReason::DynamicTarget);
        }
        let owner = if let Some(group) = occurrence
            .impl_group
            .and_then(|index| file.impl_groups.get(index))
        {
            index.resolve_type(&file.path, group.scope, &group.owner)
        } else {
            let mut current = occurrence.declaration;
            let mut found = None;
            while let Some(declaration) = current {
                let item = &file.declarations[declaration];
                if written.starts_with("this.")
                    && static_this.is_none()
                    && matches!(
                        item.native_kind.category,
                        StructureCategory::Method | StructureCategory::Field
                    )
                {
                    static_this = Some(super::repository_index::is_static_signature(
                        &item.signature,
                    ));
                }
                if written.starts_with("this.")
                    && item.native_kind.name == StructureNativeName::Function
                {
                    return BindingResolution::Unknown(StructureDiagnosticReason::DynamicTarget);
                }
                if matches!(
                    item.native_kind.category,
                    StructureCategory::Class
                        | StructureCategory::Struct
                        | StructureCategory::Trait
                        | StructureCategory::Interface
                ) {
                    found = Some((file.path.clone(), declaration));
                    break;
                }
                current = item.parent;
            }
            found.map_or(
                BindingResolution::Unknown(StructureDiagnosticReason::DynamicTarget),
                BindingResolution::Local,
            )
        };
        return if written.starts_with("this.") {
            index.resolve_receiver_member(owner, member, static_this.unwrap_or(false))
        } else {
            index.resolve_member(owner, member)
        };
    }
    index.resolve_occurrence(
        &file.path,
        occurrence.scope,
        written,
        occurrence.range.bytes.start,
        occurrence.kind,
    )
}

/// 归属可靠性沿完整 scope 链检查；局部 token 可读不足以证明执行上下文。
fn scope_reliable(file: &ExtractedFile, scope: usize) -> bool {
    let mut current = Some(scope);
    let mut seen = HashSet::new();
    while let Some(scope) = current {
        if !seen.insert(scope) {
            return false;
        }
        let Some(context) = file.scopes.get(scope) else {
            return false;
        };
        if !context.reliable {
            return false;
        }
        current = context.parent;
    }
    true
}

/// 匿名回调继续使用词法容器端点，执行来源由独立元数据明确区分。
fn occurrence_source(
    file: &ExtractedFile,
    occurrence: &ExtractedOccurrence,
    matched: &MatchedSide,
) -> Option<StructureNodeId> {
    if occurrence.kind == StructureRelationshipKind::Implements {
        if let Some(owner) = occurrence
            .impl_group
            .and_then(|index| matched.implementations.get(&(file.path.clone(), index)))
            .and_then(|implementation| implementation.owner_id.clone())
        {
            return Some(owner);
        }
    }
    occurrence
        .declaration
        .and_then(|index| {
            matched
                .declarations
                .get(&(file.path.clone(), index))
                .cloned()
        })
        .or_else(|| {
            occurrence
                .impl_group
                .and_then(|index| matched.implementations.get(&(file.path.clone(), index)))
                .and_then(|implementation| implementation.owner_id.clone())
        })
        .or_else(|| matched.source_ids.get(&file.path).map(file_node_id))
}

/// 绑定结果转换为稳定图身份；提取声明没有匹配身份时仍保留未知发生点。
fn record_resolution(
    index: &RepositoryIndex<'_>,
    result: &mut SideRelationships,
    matched: &MatchedSide,
    side: StructureSide,
    file: &ExtractedFile,
    occurrence: &ExtractedOccurrence,
    from: StructureNodeId,
    resolved: BindingResolution,
    should_continue: &dyn Fn() -> bool,
) {
    let Some(source) = matched.source_ids.get(&file.path) else {
        return;
    };
    let evidence = if occurrence.kind == StructureRelationshipKind::TypeReference
        || occurrence.kind == StructureRelationshipKind::Implements
        || occurrence.kind == StructureRelationshipKind::Extends
    {
        StructureEvidenceKind::ExplicitType
    } else {
        index.evidence_kind(
            &file.path,
            occurrence.scope,
            &occurrence.written_target,
            occurrence.range.bytes.start,
        )
    };
    let provenance = occurrence_value(source, side, occurrence, evidence);
    match resolved {
        BindingResolution::Local(key) => {
            if let Some(target) = matched.declarations.get(&key) {
                insert_edge(
                    result,
                    &from,
                    target,
                    occurrence.kind,
                    StructureCertainty::Resolved,
                    provenance,
                );
            } else {
                record_unresolved(
                    result,
                    side,
                    file,
                    occurrence,
                    from,
                    provenance,
                    Vec::new(),
                    StructureDiagnosticReason::UnreliableScope,
                    should_continue,
                );
            }
        }
        BindingResolution::External { package_name, name } => {
            let target = external_target(result, package_name, name);
            insert_edge(
                result,
                &from,
                &target,
                occurrence.kind,
                StructureCertainty::Resolved,
                provenance,
            );
        }
        BindingResolution::Candidates(keys) => {
            let targets = keys
                .iter()
                .filter_map(|key| matched.declarations.get(key).cloned())
                .collect();
            record_unresolved(
                result,
                side,
                file,
                occurrence,
                from,
                provenance,
                targets,
                StructureDiagnosticReason::AmbiguousTarget,
                should_continue,
            );
        }
        BindingResolution::Unknown(reason) => record_unresolved(
            result,
            side,
            file,
            occurrence,
            from,
            provenance,
            Vec::new(),
            reason,
            should_continue,
        ),
    }
}

/// 外部身份和 matcher 共用同一编码，定义不可用原因明确且不伪造源码位置。
fn external_target(
    result: &mut SideRelationships,
    package_name: String,
    name: String,
) -> StructureNodeId {
    let id = external_node_id(&package_name, &name);
    result
        .external
        .entry(id.clone())
        .or_insert_with(|| StructureExternalNode {
            id: id.clone(),
            name,
            package_name,
            definition_unavailable:
                StructureExternalDefinitionUnavailable::ExternalSourceNotIndexed,
        });
    id
}

/// 发生点按真实位置去重，多个调用保留各自原始位置。
fn insert_edge(
    result: &mut SideRelationships,
    from: &StructureNodeId,
    to: &StructureNodeId,
    kind: StructureRelationshipKind,
    certainty: StructureCertainty,
    occurrence: StructureOccurrence,
) {
    let key = EdgeKey {
        from: from.clone(),
        to: to.clone(),
        kind,
        certainty,
    };
    let occurrences = result.edges.entry(key).or_default();
    if !occurrences.contains(&occurrence) {
        occurrences.push(occurrence);
    }
}

/// 候选也进入可切换的边集合，同时作为未解析发生点使相关缺失证明失效。
fn record_unresolved(
    result: &mut SideRelationships,
    side: StructureSide,
    file: &ExtractedFile,
    occurrence: &ExtractedOccurrence,
    from: StructureNodeId,
    provenance: StructureOccurrence,
    mut targets: Vec<StructureNodeId>,
    reason: StructureDiagnosticReason,
    should_continue: &dyn Fn() -> bool,
) {
    targets.sort();
    targets.dedup();
    let diagnostic = diagnostic(
        side,
        &file.path,
        Some(occurrence.range.position.clone()),
        reason,
    );
    result
        .incomplete
        .entry((from.clone(), occurrence.kind))
        .or_default()
        .push(diagnostic.clone());
    for target in &targets {
        if !should_continue() {
            result.cancelled = true;
            break;
        }
        insert_edge(
            result,
            &from,
            target,
            occurrence.kind,
            StructureCertainty::Candidate,
            provenance.clone(),
        );
    }
    result.unresolved.push(StructureUnresolvedRelationship {
        side,
        from_id: from,
        kind: occurrence.kind,
        written_target: occurrence.written_target.clone(),
        occurrence: provenance,
        candidate_target_ids: targets,
        diagnostics: vec![diagnostic],
    });
}

/// 源码定位始终指向实际文件，callback range 不改变容器身份。
fn occurrence_value(
    source: &StructureSourceId,
    side: StructureSide,
    occurrence: &ExtractedOccurrence,
    evidence: StructureEvidenceKind,
) -> StructureOccurrence {
    StructureOccurrence {
        location: StructureLocation {
            source_id: source.clone(),
            side,
            range: occurrence.range.position.clone(),
            role: StructureLocationRole::Occurrence,
        },
        execution: occurrence.callback_range.as_ref().map_or_else(
            || {
                if occurrence.declaration.is_some() || occurrence.impl_group.is_some() {
                    StructureExecution::DeclarationBody
                } else {
                    StructureExecution::File
                }
            },
            |callback| StructureExecution::AnonymousCallback {
                callback_range: callback.position.clone(),
            },
        ),
        evidence: vec![StructureEvidence {
            kind: evidence,
            detail: occurrence.written_target.clone(),
        }],
    }
}

/// 缺侧关系只在来源完整且该来源同类关系没有未知目标时才能证明不存在。
fn edge_fact(
    key: &EdgeKey,
    side: &SideRelationships,
    _matched: &MatchedSide,
    nodes: &[StructureNode],
    which: StructureSide,
    coverage: &StructureCoverage,
) -> StructureFact<Vec<StructureOccurrence>> {
    if let Some(value) = side.edges.get(key) {
        return StructureFact::Present {
            value: value.clone(),
        };
    }
    if side.cancelled {
        return unknown_fact(which, StructureDiagnosticReason::Cancelled);
    }
    if let Some(diagnostics) = side.incomplete.get(&(key.from.clone(), key.kind)) {
        return StructureFact::Unknown {
            diagnostics: diagnostics.clone(),
        };
    }
    if let Some(diagnostics) = side.unknown_origins.get(&key.kind) {
        return StructureFact::Unknown {
            diagnostics: diagnostics.clone(),
        };
    }
    if key.certainty != StructureCertainty::Resolved {
        return unknown_fact(which, StructureDiagnosticReason::AmbiguousTarget);
    }
    let discovery = if which == StructureSide::Before {
        coverage.discovery.before
    } else {
        coverage.discovery.after
    };
    if discovery != StructureCoverageStatus::Complete {
        return unknown_fact(which, StructureDiagnosticReason::UnreliableScope);
    }
    if let Some(paths) = side.origins.get(&key.from) {
        for path in paths {
            let Some(file) = side
                .coverage_index
                .get(path)
                .and_then(|index| side.coverage.get(*index))
            else {
                return unknown_fact(which, StructureDiagnosticReason::UnreliableScope);
            };
            // 文件级语法缺口可能藏住任何关系；已知别处动态调用不污染当前来源。
            if file.extraction != StructureCoverageStatus::Complete
                || file.relationships == StructureCoverageStatus::NotStarted
            {
                return StructureFact::Unknown {
                    diagnostics: if file.diagnostics.is_empty() {
                        vec![diagnostic(
                            which,
                            &path,
                            None,
                            StructureDiagnosticReason::UnreliableScope,
                        )]
                    } else {
                        file.diagnostics.clone()
                    },
                };
            }
        }
        return StructureFact::Absent;
    }
    // 已由 matcher 证明声明缺侧时，可证明该声明不可能发出关系。
    if nodes.iter().any(|node| match node {
        StructureNode::Declaration(node) if node.id == key.from => matches!(
            if which == StructureSide::Before {
                &node.before
            } else {
                &node.after
            },
            StructureFact::Absent
        ),
        _ => false,
    }) {
        return StructureFact::Absent;
    }
    if nodes
        .iter()
        .any(|node| matches!(node, StructureNode::File(node) if node.id == key.from))
    {
        return StructureFact::Absent;
    }
    unknown_fact(which, StructureDiagnosticReason::UnreliableScope)
}

/// 缺失证明失败必须携带直接诊断。
fn unknown_fact(
    side: StructureSide,
    reason: StructureDiagnosticReason,
) -> StructureFact<Vec<StructureOccurrence>> {
    StructureFact::Unknown {
        diagnostics: vec![StructureDiagnostic {
            reason,
            message: diagnostic_message(reason).to_owned(),
            side: Some(side),
            path: None,
            range: None,
            retryable: reason == StructureDiagnosticReason::Cancelled,
        }],
    }
}

/// 诊断描述的是证据限制，不声称未知调用没有发生。
fn diagnostic(
    side: StructureSide,
    path: &str,
    range: Option<StructureRange>,
    reason: StructureDiagnosticReason,
) -> StructureDiagnostic {
    StructureDiagnostic {
        reason,
        message: diagnostic_message(reason).to_owned(),
        side: Some(side),
        path: Some(path.to_owned()),
        range,
        retryable: reason == StructureDiagnosticReason::Cancelled,
    }
}

/// 内置原因使用可直接显示的简短描述。
fn diagnostic_message(reason: StructureDiagnosticReason) -> &'static str {
    match reason {
        StructureDiagnosticReason::Cancelled => {
            "关系分析已取消，尚未处理的位置不能用于确认关系缺失。"
        }
        StructureDiagnosticReason::AmbiguousTarget => "存在多个可能目标，不能确认唯一调用或引用。",
        StructureDiagnosticReason::ConfigurationUnavailable => "当前侧模块或配置不足以绑定此路径。",
        StructureDiagnosticReason::UnreliableScope => {
            "声明或词法作用域未完整覆盖，关系目标尚不能确认。"
        }
        _ => "当前静态证据不足以确定调用或引用的目标。",
    }
}

/// 固定关系种类编号仅用于保持既有 ID 编码，所有分支必须穷尽枚举。
fn kind_id_tag(kind: StructureRelationshipKind) -> u8 {
    match kind {
        StructureRelationshipKind::Calls => 0,
        StructureRelationshipKind::Extends => 1,
        StructureRelationshipKind::Implements => 2,
        StructureRelationshipKind::TypeReference => 3,
        StructureRelationshipKind::Imports => 4,
    }
}

/// 确定性编号仅用于保持既有 ID 编码，内部逻辑直接使用枚举。
fn certainty_id_tag(certainty: StructureCertainty) -> u8 {
    match certainty {
        StructureCertainty::Resolved => 0,
        StructureCertainty::Candidate => 1,
    }
}

/// 长度前缀防止端点名称中的标点碰撞，不依赖随机散列或行号。
fn relation_id(key: &EdgeKey) -> StructureRelationId {
    StructureRelationId(format!(
        "relation:{}:{}:{}:{}:{}:{}",
        key.from.0.len(),
        key.from.0,
        key.to.0.len(),
        key.to.0,
        kind_id_tag(key.kind),
        certainty_id_tag(key.certainty)
    ))
}

#[cfg(test)]
#[path = "tests/relationships.rs"]
mod tests;
