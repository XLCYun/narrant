//! 在覆盖证据约束下保守配对两侧声明，并保留多来源和双侧层级。

use std::collections::{HashMap, HashSet};

use super::extraction_model::{ExtractedDeclaration, ExtractedFile};
use super::matching_model::*;
use super::model::*;
use super::repository_index::{BindingResolution, RepositoryIndex};

#[path = "matching_candidates.rs"]
mod candidates;
use candidates::CandidateEvidence;

#[path = "matching_implementations.rs"]
mod implementations;
use implementations::{apply_implementation_changes, materialize_groups};

/// 一个声明及其文件内身份；只在本次配对调用期间借用提取结果。
struct DeclarationRef<'a> {
    key: DeclarationKey,
    file: &'a ExtractedFile,
    declaration: &'a ExtractedDeclaration,
}

/// impl 的 owner/trait 已经过真实模块与导入索引解析。
struct ImplementationBinding {
    owner: BindingResolution,
    trait_target: Option<BindingResolution>,
}

/// 一侧配对所需的平坦索引；文件来源仍是 Snapshot 的真实文件对。
struct SideIndex<'a> {
    files: &'a [ExtractedFile],
    declarations: Vec<DeclarationRef<'a>>,
    by_key: HashMap<DeclarationKey, usize>,
    implementations: HashMap<ImplementationKey, ImplementationBinding>,
    source_ids: HashMap<String, StructureSourceId>,
}

impl<'a> SideIndex<'a> {
    /// 固定每侧排序与导入配置，不以全局 basename 兜底类型归属。
    fn new(
        files: &'a [ExtractedFile],
        sources: &[StructurePeekSource],
        side: StructureSide,
        config: &HashMap<String, String>,
    ) -> Self {
        let repository = RepositoryIndex::new(files, config);
        let mut declarations: Vec<_> = files
            .iter()
            .flat_map(|file| {
                file.declarations
                    .iter()
                    .enumerate()
                    .map(move |(index, declaration)| DeclarationRef {
                        key: (file.path.clone(), index),
                        file,
                        declaration,
                    })
            })
            .collect();
        declarations.sort_by(|left, right| left.key.cmp(&right.key));
        let by_key = declarations
            .iter()
            .enumerate()
            .map(|(index, declaration)| (declaration.key.clone(), index))
            .collect();
        let mut implementations = HashMap::new();
        for file in files {
            for (index, group) in file.impl_groups.iter().enumerate() {
                let owner = if group.reliable {
                    repository.resolve_type(&file.path, group.scope, &group.owner)
                } else {
                    BindingResolution::Unknown(StructureDiagnosticReason::UnreliableScope)
                };
                let trait_target = group
                    .trait_name
                    .as_ref()
                    .map(|name| repository.resolve_type(&file.path, group.scope, name));
                implementations.insert(
                    (file.path.clone(), index),
                    ImplementationBinding {
                        owner,
                        trait_target,
                    },
                );
            }
        }
        let source_ids = sources
            .iter()
            .filter_map(|source| {
                let part = if side == StructureSide::Before {
                    &source.before
                } else {
                    &source.after
                };
                match part {
                    StructureSourceSide::Present { path, .. } => {
                        Some((path.clone(), source.id.clone()))
                    }
                    StructureSourceSide::Absent => None,
                }
            })
            .collect();
        Self {
            files,
            declarations,
            by_key,
            implementations,
            source_ids,
        }
    }

    /// 原生归属优先于文件；impl 成员只能归到明确解析的 owner。
    fn parent(&self, declaration: &DeclarationRef<'_>) -> Parent {
        if let Some(group) = declaration.declaration.impl_group {
            return self
                .implementations
                .get(&(declaration.file.path.clone(), group))
                .map_or(Parent::Unknown, |binding| match &binding.owner {
                    BindingResolution::Local(key) => Parent::Declaration(key.clone()),
                    BindingResolution::External { package_name, name } => {
                        Parent::External(external_node_id(package_name, name))
                    }
                    _ => Parent::Unknown,
                });
        }
        if let Some(parent) = declaration.declaration.parent {
            return Parent::Declaration((declaration.file.path.clone(), parent));
        }
        self.source_ids
            .get(&declaration.file.path)
            .map_or(Parent::Unknown, |source| Parent::File(file_node_id(source)))
    }
}

/// 未确认的归属不能与任意同名类型相等。
enum Parent {
    Declaration(DeclarationKey),
    File(StructureNodeId),
    External(StructureNodeId),
    Unknown,
}

/// 两个方向的唯一配对决定持久图身份，不让候选评分强选歧义目标。
#[derive(Default)]
struct Pairs {
    forward: HashMap<usize, usize>,
    backward: HashMap<usize, usize>,
}

impl Pairs {
    /// 仅接受尚未配对的双侧声明。
    fn insert(&mut self, before: usize, after: usize) {
        self.forward.insert(before, after);
        self.backward.insert(after, before);
    }
}

/// 调用方只启动一次比较；所有源路径来自真实 Snapshot 文件对。
pub(crate) fn compare_declarations(
    before: &[ExtractedFile],
    after: &[ExtractedFile],
    sources: &[StructurePeekSource],
    coverage: &StructureCoverage,
    before_config: &HashMap<String, String>,
    after_config: &HashMap<String, String>,
    should_continue: &dyn Fn() -> bool,
) -> MatchedStructure {
    let before = SideIndex::new(before, sources, StructureSide::Before, before_config);
    let after = SideIndex::new(after, sources, StructureSide::After, after_config);
    let mut pairs = Pairs::default();
    pair_unchanged_sources(&before, &after, sources, &mut pairs, should_continue);
    let mut cancelled = !should_continue();
    while !cancelled {
        let previous = pairs.forward.len();
        pair_named_declarations(&before, &after, &mut pairs, should_continue);
        cancelled = !should_continue();
        if cancelled {
            break;
        }
        pair_changed_locations(&before, &after, &mut pairs, should_continue);
        cancelled = !should_continue();
        if previous == pairs.forward.len() {
            break;
        }
    }
    let mut matched = MatchedStructure::default();
    matched.before.source_ids = before.source_ids.clone();
    matched.after.source_ids = after.source_ids.clone();
    for source in sources {
        matched.nodes.push(StructureNode::File(StructureFileNode {
            id: file_node_id(&source.id),
            source_id: source.id.clone(),
            contained_changes: empty_counts(),
        }));
    }
    for (index, declaration) in before.declarations.iter().enumerate() {
        let id = declaration_id("before", &declaration.key);
        matched
            .before
            .declarations
            .insert(declaration.key.clone(), id.clone());
        if let Some(after_index) = pairs.forward.get(&index) {
            matched
                .after
                .declarations
                .insert(after.declarations[*after_index].key.clone(), id);
        }
    }
    for declaration in &after.declarations {
        matched
            .after
            .declarations
            .entry(declaration.key.clone())
            .or_insert_with(|| declaration_id("after", &declaration.key));
    }
    cancelled = materialize_groups(
        &before,
        &after,
        &mut matched,
        coverage,
        cancelled,
        should_continue,
    );
    let before_complete = complete_side(&before, coverage, StructureSide::Before) && !cancelled;
    let after_complete = complete_side(&after, coverage, StructureSide::After) && !cancelled;
    for (index, declaration) in before.declarations.iter().enumerate() {
        let counterpart = pairs
            .forward
            .get(&index)
            .map(|index| &after.declarations[*index]);
        let node = declaration_node(
            Some(declaration),
            counterpart,
            &before,
            &after,
            &matched,
            before_complete,
            after_complete,
            cancelled,
        );
        matched.nodes.push(StructureNode::Declaration(node));
    }
    for (index, declaration) in after.declarations.iter().enumerate() {
        if !pairs.backward.contains_key(&index) {
            let node = declaration_node(
                None,
                Some(declaration),
                &before,
                &after,
                &matched,
                before_complete,
                after_complete,
                cancelled,
            );
            matched.nodes.push(StructureNode::Declaration(node));
        }
    }
    apply_implementation_changes(
        &before,
        &after,
        &mut matched,
        before_complete && after_complete,
    );
    update_contained_counts(&mut matched.nodes);
    if cancelled {
        matched.diagnostics.push(diagnostic(
            StructureDiagnosticReason::Cancelled,
            None,
            None,
            "结构配对已取消；未配对声明的另一侧保持未知。",
        ));
    }
    matched
}

/// Git 确认未修改的完整文件拥有精确位置连续性，重复声明不必被迫退回增删。
fn pair_unchanged_sources(
    before: &SideIndex<'_>,
    after: &SideIndex<'_>,
    sources: &[StructurePeekSource],
    pairs: &mut Pairs,
    should_continue: &dyn Fn() -> bool,
) {
    let unchanged: HashSet<_> = sources
        .iter()
        .filter_map(|source| match &source.classification {
            StructureSourceClassification::Unchanged { path } => Some(path.as_str()),
            _ => None,
        })
        .collect();
    for (index, left) in before.declarations.iter().enumerate() {
        if !should_continue() {
            return;
        }
        if !unchanged.contains(left.file.path.as_str()) || !left.declaration.reliable {
            continue;
        }
        let Some(&target) = after.by_key.get(&left.key) else {
            continue;
        };
        let right = after.declarations[target].declaration;
        let left = left.declaration;
        if right.reliable
            && left.range == right.range
            && left.name == right.name
            && left.native_kind == right.native_kind
            && left.signature_tokens == right.signature_tokens
            && left.body_tokens == right.body_tokens
            && left.documentation == right.documentation
            && left.parent == right.parent
            && left.impl_group == right.impl_group
        {
            pairs.insert(index, target);
        }
    }
}

/// 同一原生类别、作用域与名称优先配对，重载必须靠唯一签名区分。
fn pair_named_declarations(
    before: &SideIndex<'_>,
    after: &SideIndex<'_>,
    pairs: &mut Pairs,
    should_continue: &dyn Fn() -> bool,
) {
    let mut buckets: HashMap<_, Vec<usize>> = HashMap::new();
    let mut overload_counts = HashMap::new();
    for item in &before.declarations {
        *overload_counts
            .entry((
                item.file.path.as_str(),
                item.declaration.name.as_str(),
                item.declaration.parent,
                item.declaration.impl_group,
            ))
            .or_insert(0usize) += 1;
    }
    for (index, item) in after
        .declarations
        .iter()
        .enumerate()
        .filter(|(index, item)| !pairs.backward.contains_key(index) && item.declaration.reliable)
    {
        buckets
            .entry((
                item.declaration.native_kind.language,
                item.declaration.native_kind.category,
                item.declaration.name.as_str(),
            ))
            .or_default()
            .push(index);
    }
    let mut candidates = Vec::new();
    for (index, item) in before.declarations.iter().enumerate() {
        if !should_continue() {
            return;
        }
        if pairs.forward.contains_key(&index) || !item.declaration.reliable {
            continue;
        }
        let key = (
            item.declaration.native_kind.language,
            item.declaration.native_kind.category,
            item.declaration.name.as_str(),
        );
        let Some(bucket) = buckets.get(&key) else {
            continue;
        };
        let mut targets: Vec<_> = bucket
            .iter()
            .copied()
            .filter(|target| same_context(before, after, item, &after.declarations[*target], pairs))
            .collect();
        let overloaded_before = overload_counts[&(
            item.file.path.as_str(),
            item.declaration.name.as_str(),
            item.declaration.parent,
            item.declaration.impl_group,
        )] > 1;
        if targets.len() > 1 || overloaded_before {
            targets.retain(|target| {
                after.declarations[*target].declaration.signature_tokens
                    == item.declaration.signature_tokens
            });
        }
        candidates.extend(targets.into_iter().map(|target| (index, target)));
    }
    accept_unique(candidates, pairs);
}

/// 跨位置配对同时要求签名及正文证据，所有合理候选都保留到唯一性检查。
fn pair_changed_locations(
    before: &SideIndex<'_>,
    after: &SideIndex<'_>,
    pairs: &mut Pairs,
    should_continue: &dyn Fn() -> bool,
) {
    let before_evidence: HashMap<_, _> = before
        .declarations
        .iter()
        .enumerate()
        .filter(|(index, item)| !pairs.forward.contains_key(index) && item.declaration.reliable)
        .map(|(index, item)| (index, CandidateEvidence::new(item.declaration)))
        .collect();
    let after_evidence: HashMap<_, _> = after
        .declarations
        .iter()
        .enumerate()
        .filter(|(index, item)| !pairs.backward.contains_key(index) && item.declaration.reliable)
        .map(|(index, item)| (index, CandidateEvidence::new(item.declaration)))
        .collect();
    let mut buckets: HashMap<_, Vec<usize>> = HashMap::new();
    for &index in after_evidence.keys() {
        let item = after.declarations[index].declaration;
        buckets
            .entry((
                item.native_kind.language,
                item.native_kind.category,
                item.signature_tokens.as_str(),
            ))
            .or_default()
            .push(index);
    }
    let mut candidates = Vec::new();
    for (&index, evidence) in &before_evidence {
        if !should_continue() {
            return;
        }
        let item = &before.declarations[index];
        let declaration = item.declaration;
        let Some(bucket) = buckets.get(&(
            declaration.native_kind.language,
            declaration.native_kind.category,
            declaration.signature_tokens.as_str(),
        )) else {
            continue;
        };
        let mut targets = Vec::new();
        for &target in bucket {
            if !should_continue() {
                return;
            }
            let other = &after.declarations[target];
            if !compatible_member_context(before, after, item, other, pairs) {
                continue;
            }
            let exact_body = declaration.body_tokens == other.declaration.body_tokens
                && evidence.is_substantive();
            if exact_body
                || evidence.resembles(&after_evidence[&target])
                || matching_children(item, other)
            {
                targets.push(target);
            }
        }
        candidates.extend(targets.into_iter().map(|target| (index, target)));
    }
    accept_unique(candidates, pairs);
}

/// 同一目标只能有一个来源候选；不按行号或 hash 排序打破歧义。
fn accept_unique(candidates: Vec<(usize, usize)>, pairs: &mut Pairs) {
    let mut incoming = HashMap::new();
    let mut outgoing = HashMap::new();
    for (source, target) in &candidates {
        *incoming.entry(*target).or_insert(0usize) += 1;
        *outgoing.entry(*source).or_insert(0usize) += 1;
    }
    for (source, target) in candidates {
        if incoming[&target] == 1 && outgoing[&source] == 1 {
            pairs.insert(source, target);
        }
    }
}

/// 成员不能仅凭同名同正文跨不同类型或不同 trait 实现配对。
fn compatible_member_context(
    before: &SideIndex<'_>,
    after: &SideIndex<'_>,
    left: &DeclarationRef<'_>,
    right: &DeclarationRef<'_>,
    pairs: &Pairs,
) -> bool {
    if left.declaration.impl_group.is_some()
        || right.declaration.impl_group.is_some()
        || matches!(
            left.declaration.native_kind.category,
            StructureCategory::Method | StructureCategory::Field | StructureCategory::Member
        )
    {
        same_context(before, after, left, right, pairs)
    } else {
        true
    }
}

/// 层级相同还要求 impl 的 trait 目标连续，未知目标不建立连续性。
fn same_context(
    before: &SideIndex<'_>,
    after: &SideIndex<'_>,
    left: &DeclarationRef<'_>,
    right: &DeclarationRef<'_>,
    pairs: &Pairs,
) -> bool {
    let parent_matches = match (before.parent(left), after.parent(right)) {
        (Parent::File(left), Parent::File(right))
        | (Parent::External(left), Parent::External(right)) => left == right,
        (Parent::Declaration(left), Parent::Declaration(right)) => {
            local_targets_match(before, after, &left, &right, pairs)
        }
        _ => false,
    };
    if !parent_matches {
        return false;
    }
    match (left.declaration.impl_group, right.declaration.impl_group) {
        (None, None) => true,
        (Some(left_group), Some(right_group)) => {
            let left_binding = &before.implementations[&(left.file.path.clone(), left_group)];
            let right_binding = &after.implementations[&(right.file.path.clone(), right_group)];
            let same_trait = match (&left_binding.trait_target, &right_binding.trait_target) {
                (None, None) => true,
                (Some(left), Some(right)) => bindings_match(before, after, left, right, pairs),
                _ => false,
            };
            if !same_trait {
                return false;
            }
            let left_header = &left.file.impl_groups[left_group].header_tokens;
            let right_header = &right.file.impl_groups[right_group].header_tokens;
            if left_header == right_header {
                return true;
            }
            let context_count = |index: &SideIndex<'_>, target: &ImplementationBinding| {
                index
                    .implementations
                    .values()
                    .filter(|binding| {
                        binding.owner == target.owner && binding.trait_target == target.trait_target
                    })
                    .count()
            };
            // 单个 impl 的自身约束可编辑；多个特化 impl 则必须保留各自头部身份。
            context_count(before, left_binding) == 1 && context_count(after, right_binding) == 1
        }
        _ => false,
    }
}

/// 将仓库索引的本地类型映射到已确认声明配对。
fn local_targets_match(
    before: &SideIndex<'_>,
    after: &SideIndex<'_>,
    left: &DeclarationKey,
    right: &DeclarationKey,
    pairs: &Pairs,
) -> bool {
    before
        .by_key
        .get(left)
        .and_then(|index| pairs.forward.get(index))
        .is_some_and(|index| after.declarations[*index].key == *right)
}

/// 外部限定名称可直接比较，本地类型必须经过配对，歧义一律不相等。
fn bindings_match(
    before: &SideIndex<'_>,
    after: &SideIndex<'_>,
    left: &BindingResolution,
    right: &BindingResolution,
    pairs: &Pairs,
) -> bool {
    match (left, right) {
        (BindingResolution::Local(left), BindingResolution::Local(right)) => {
            local_targets_match(before, after, left, right, pairs)
        }
        (
            BindingResolution::External {
                package_name: lp,
                name: ln,
            },
            BindingResolution::External {
                package_name: rp,
                name: rn,
            },
        ) => lp == rp && ln == rn,
        _ => false,
    }
}

/// 类型空壳不能靠大括号配对；保留名称加成员证据，或多个独特成员共同支持改名。
fn matching_children(left: &DeclarationRef<'_>, right: &DeclarationRef<'_>) -> bool {
    let left_name_equal = left.declaration.name == right.declaration.name;
    let anchors = |item: &DeclarationRef<'_>| -> HashSet<_> {
        item.file
            .declarations
            .iter()
            .filter(|child| child.parent == Some(item.key.1))
            .map(|child| (child.name.clone(), child.signature_tokens.clone()))
            .collect()
    };
    let left = anchors(left);
    let right = anchors(right);
    let required = if left_name_equal { 1 } else { 2 };
    left.intersection(&right).count() >= required && left == right
}

/// 声明身份使用唯一文件内槽位，名称和正文变化不产生重复主节点。
fn declaration_id(side: &str, key: &DeclarationKey) -> StructureNodeId {
    StructureNodeId(format!(
        "declaration:{side}:{}:{}:{}",
        key.0.len(),
        key.0,
        key.1
    ))
}

/// 全仓库发现及该侧提取完整才允许无可靠配对时回退增删。
fn complete_side(index: &SideIndex<'_>, coverage: &StructureCoverage, side: StructureSide) -> bool {
    let discovery = if side == StructureSide::Before {
        coverage.discovery.before
    } else {
        coverage.discovery.after
    };
    discovery == StructureCoverageStatus::Complete
        && index.files.iter().all(|file| {
            file.complete
                && file
                    .declarations
                    .iter()
                    .all(|declaration| declaration.reliable)
        })
        && coverage
            .files
            .iter()
            .filter(|file| file.side == side && supported_source_path(&file.path))
            .all(|file| file.extraction == StructureCoverageStatus::Complete)
}

/// 非源码文档不属于可配对声明域；支持后缀的不可读文件仍会阻断缺失证明。
fn supported_source_path(path: &str) -> bool {
    super::parser_model::SourceLanguage::from_path(path).is_some()
}

/// 创建可直接显示的诊断，不向 UI 暴露覆盖证明引用。
fn diagnostic(
    reason: StructureDiagnosticReason,
    side: Option<StructureSide>,
    item: Option<&DeclarationRef<'_>>,
    message: &str,
) -> StructureDiagnostic {
    StructureDiagnostic {
        reason,
        message: message.to_owned(),
        side,
        path: item.map(|item| item.file.path.clone()),
        range: item.map(|item| item.declaration.range.position.clone()),
        retryable: matches!(
            reason,
            StructureDiagnosticReason::Cancelled
                | StructureDiagnosticReason::ResourceLimit
                | StructureDiagnosticReason::GitObjectUnavailable
        ),
    }
}

/// 初始化尚未汇总的父级变化数量。
fn empty_counts() -> StructureContainedChanges {
    StructureContainedChanges {
        before: 0,
        after: 0,
        union: 0,
    }
}

/// 仅在词法归属和 impl trait 都可信时允许确认另一侧缺失。
fn trustworthy(item: &DeclarationRef<'_>, index: &SideIndex<'_>) -> bool {
    if !item.declaration.reliable || matches!(index.parent(item), Parent::Unknown) {
        return false;
    }
    item.declaration.impl_group.is_none_or(|group| {
        index
            .implementations
            .get(&(item.file.path.clone(), group))
            .is_some_and(|binding| {
                binding.trait_target.as_ref().is_none_or(|target| {
                    matches!(
                        target,
                        BindingResolution::Local(_) | BindingResolution::External { .. }
                    )
                })
            })
    })
}

/// 将本侧的真实来源和已配对 parent/group 写入 owned 声明。
fn declaration_part(
    item: &DeclarationRef<'_>,
    index: &SideIndex<'_>,
    matched: &MatchedSide,
    side: StructureSide,
) -> StructureDeclarationSide {
    let source_id = &matched.source_ids[&item.file.path];
    let parent_id = match index.parent(item) {
        Parent::Declaration(key) => matched
            .declarations
            .get(&key)
            .cloned()
            .unwrap_or_else(|| file_node_id(source_id)),
        Parent::External(id) | Parent::File(id) => id,
        Parent::Unknown => file_node_id(source_id),
    };
    let implementation_group_id = item
        .declaration
        .impl_group
        .and_then(|group| {
            matched
                .implementations
                .get(&(item.file.path.clone(), group))
        })
        .map(|group| group.group_id.clone());
    let mut locations = vec![StructureLocation {
        source_id: source_id.clone(),
        side,
        range: item.declaration.range.position.clone(),
        role: StructureLocationRole::Declaration,
    }];
    if let Some(range) = &item.declaration.documentation_range {
        locations.push(StructureLocation {
            source_id: source_id.clone(),
            side,
            range: range.position.clone(),
            role: StructureLocationRole::Documentation,
        });
    }
    StructureDeclarationSide {
        native_kind: item.declaration.native_kind.clone(),
        name: item.declaration.name.clone(),
        qualified_name: item.declaration.qualified_name.clone(),
        signature: item.declaration.signature.clone(),
        parent_id,
        implementation_group_id,
        locations,
    }
}

/// 缺侧未知携带直接原因，解析不完整和取消都不能转为增删。
fn missing_declaration_side<T>(
    complete: bool,
    side: StructureSide,
    observed: &DeclarationRef<'_>,
    cancelled: bool,
) -> StructureFact<T> {
    if complete {
        StructureFact::Absent
    } else {
        StructureFact::Unknown {
            diagnostics: vec![StructureDiagnostic {
                reason: if cancelled {
                    StructureDiagnosticReason::Cancelled
                } else {
                    StructureDiagnosticReason::UnreliableScope
                },
                message: format!(
                    "另一侧的提取或归属覆盖不足，无法确认 {} 中的 {} 是否不存在。",
                    observed.file.path, observed.declaration.name
                ),
                side: Some(side),
                path: None,
                range: None,
                retryable: cancelled,
            }],
        }
    }
}

/// 多标签比较只使用声明拥有的签名、正文和文档，绝不比较整段父级源码。
fn declaration_node(
    left: Option<&DeclarationRef<'_>>,
    right: Option<&DeclarationRef<'_>>,
    before: &SideIndex<'_>,
    after: &SideIndex<'_>,
    matched: &MatchedStructure,
    before_complete: bool,
    after_complete: bool,
    cancelled: bool,
) -> StructureDeclaration {
    let observed = right.or(left).expect("至少观察到一侧声明");
    let before_complete = before_complete
        && right.is_none_or(|item| counterpart_source_analyzed(item, after, before));
    let after_complete =
        after_complete && left.is_none_or(|item| counterpart_source_analyzed(item, before, after));
    let trusted = left.is_none_or(|item| trustworthy(item, before))
        && right.is_none_or(|item| trustworthy(item, after));
    let before_part =
        left.map(|item| declaration_part(item, before, &matched.before, StructureSide::Before));
    let after_part =
        right.map(|item| declaration_part(item, after, &matched.after, StructureSide::After));
    let mut changes = Vec::new();
    if let (Some(left), Some(right), Some(left_part), Some(right_part)) =
        (left, right, &before_part, &after_part)
    {
        if left.declaration.name != right.declaration.name {
            changes.push(StructureChangeLabel::Renamed);
        }
        if left.file.path != right.file.path
            || (trusted && left_part.parent_id != right_part.parent_id)
        {
            changes.push(StructureChangeLabel::Moved);
        }
        if left.declaration.signature_tokens != right.declaration.signature_tokens
            || left.declaration.body_tokens != right.declaration.body_tokens
            || left.declaration.native_kind != right.declaration.native_kind
        {
            changes.push(StructureChangeLabel::Modified);
        }
        if left.declaration.documentation != right.declaration.documentation {
            changes.push(StructureChangeLabel::Documentation);
        }
    } else if left.is_none() && before_complete && trusted {
        changes.push(StructureChangeLabel::Added);
    } else if right.is_none() && after_complete && trusted {
        changes.push(StructureChangeLabel::Deleted);
    }
    let complete =
        trusted && (left.is_some() || before_complete) && (right.is_some() || after_complete);
    let display_parent_id = after_part
        .as_ref()
        .or(before_part.as_ref())
        .expect("声明来源应存在")
        .parent_id
        .clone();
    let id = left
        .map(|item| matched.before.declarations[&item.key].clone())
        .unwrap_or_else(|| matched.after.declarations[&observed.key].clone());
    let diagnostics = if trusted {
        Vec::new()
    } else {
        vec![diagnostic(
            StructureDiagnosticReason::UnreliableScope,
            Some(if right.is_some() {
                StructureSide::After
            } else {
                StructureSide::Before
            }),
            Some(observed),
            "声明的作用域或 impl 归属未能可靠确认。",
        )]
    };
    StructureDeclaration {
        id,
        before: before_part
            .map(|value| StructureFact::Present { value })
            .unwrap_or_else(|| {
                missing_declaration_side(
                    before_complete && trusted,
                    StructureSide::Before,
                    observed,
                    cancelled,
                )
            }),
        after: after_part
            .map(|value| StructureFact::Present { value })
            .unwrap_or_else(|| {
                missing_declaration_side(
                    after_complete && trusted,
                    StructureSide::After,
                    observed,
                    cancelled,
                )
            }),
        changes,
        comparison: if complete {
            StructureComparison::Complete
        } else {
            StructureComparison::Incomplete
        },
        diagnostics,
        display_parent_id,
        contained_changes: empty_counts(),
    }
}

/// 真实 rename 的另一侧若仍有文件但语言/内容不可分析，不能把它当成声明缺失。
fn counterpart_source_analyzed(
    item: &DeclarationRef<'_>,
    observed: &SideIndex<'_>,
    opposite: &SideIndex<'_>,
) -> bool {
    let Some(source_id) = observed.source_ids.get(&item.file.path) else {
        return false;
    };
    opposite
        .source_ids
        .iter()
        .filter(|(_, id)| *id == source_id)
        .all(|(path, _)| {
            opposite
                .files
                .iter()
                .any(|file| file.path == *path && file.complete)
        })
}

/// 沿两侧各自层级汇总后代集合，移动入口和多标签均不重复计算声明。
fn update_contained_counts(nodes: &mut [StructureNode]) {
    let mut before_parents = HashMap::new();
    let mut after_parents = HashMap::new();
    let mut changed = Vec::new();
    for node in nodes.iter() {
        let StructureNode::Declaration(node) = node else {
            continue;
        };
        if let StructureFact::Present { value } = &node.before {
            before_parents.insert(node.id.clone(), value.parent_id.clone());
        }
        if let StructureFact::Present { value } = &node.after {
            after_parents.insert(node.id.clone(), value.parent_id.clone());
        }
        if !node.changes.is_empty() {
            changed.push(node.id.clone());
        }
    }
    let collect = |parents: &HashMap<StructureNodeId, StructureNodeId>| {
        let mut counts: HashMap<StructureNodeId, HashSet<StructureNodeId>> = HashMap::new();
        for id in &changed {
            let mut visited = HashSet::from([id.clone()]);
            let mut parent = parents.get(id);
            while let Some(current) = parent {
                if !visited.insert(current.clone()) {
                    break;
                }
                counts
                    .entry(current.clone())
                    .or_default()
                    .insert(id.clone());
                parent = parents.get(current);
            }
        }
        counts
    };
    let before = collect(&before_parents);
    let after = collect(&after_parents);
    for node in nodes {
        let (id, counts) = match node {
            StructureNode::Declaration(node) => (&node.id, &mut node.contained_changes),
            StructureNode::File(node) => (&node.id, &mut node.contained_changes),
            StructureNode::External(_) => continue,
        };
        let empty = HashSet::new();
        let left = before.get(id).unwrap_or(&empty);
        let right = after.get(id).unwrap_or(&empty);
        *counts = StructureContainedChanges {
            before: left.len() as u32,
            after: right.len() as u32,
            union: left.union(right).count() as u32,
        };
    }
}

#[cfg(test)]
#[path = "tests/matching.rs"]
mod tests;
