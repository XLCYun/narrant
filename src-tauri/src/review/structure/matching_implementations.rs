//! Rust 多文件实现组的连续身份、真实来源及聚合类型自身片段比较。

use std::collections::BTreeMap;

use super::super::extraction_model::ExtractedImplGroup;
use super::*;

/// 本地目标经声明映射取得身份，外部目标只生成缺少本地定义的节点。
fn bound_id(
    binding: &BindingResolution,
    side: &MatchedSide,
    external: &mut BTreeMap<StructureNodeId, StructureNode>,
) -> Option<StructureNodeId> {
    match binding {
        BindingResolution::Local(key) => side.declarations.get(key).cloned(),
        BindingResolution::External { package_name, name } => {
            let id = external_node_id(package_name, name);
            external.entry(id.clone()).or_insert_with(|| {
                StructureNode::External(StructureExternalNode {
                    id: id.clone(),
                    name: name.clone(),
                    package_name: package_name.clone(),
                    definition_unavailable:
                        StructureExternalDefinitionUnavailable::ExternalSourceNotIndexed,
                })
            });
            Some(id)
        }
        _ => None,
    }
}

/// impl 的稳定槽位基于真实文件和本地编号，成员正文不参与分组身份。
fn implementation_id(side: &str, key: &ImplementationKey) -> StructureGroupId {
    StructureGroupId(format!(
        "implementation:{side}:{}:{}:{}",
        key.0.len(),
        key.0,
        key.1
    ))
}

/// 定位已提取的实现片段，调用方只传入来自索引的真实 key。
fn implementation<'a>(index: &'a SideIndex<'_>, key: &ImplementationKey) -> &'a ExtractedImplGroup {
    &index
        .files
        .iter()
        .find(|file| file.path == key.0)
        .expect("实现文件必须存在")
        .impl_groups[key.1]
}

/// 分组候选的成员签名帮助区分同类型多个同头部 impl，成员正文不影响连续性。
fn implementation_members(index: &SideIndex<'_>, key: &ImplementationKey) -> Vec<(String, String)> {
    let file = index
        .files
        .iter()
        .find(|file| file.path == key.0)
        .expect("实现文件必须存在");
    let mut members: Vec<_> = file.impl_groups[key.1]
        .members
        .iter()
        .map(|member| {
            (
                file.declarations[*member].name.clone(),
                file.declarations[*member].signature_tokens.clone(),
            )
        })
        .collect();
    members.sort();
    members
}

/// 先绑定两侧 owner/trait，再按其连续身份和唯一头部/成员证据匹配 impl 分组。
pub(super) fn materialize_groups(
    before: &SideIndex<'_>,
    after: &SideIndex<'_>,
    matched: &mut MatchedStructure,
    coverage: &StructureCoverage,
    mut cancelled: bool,
    should_continue: &dyn Fn() -> bool,
) -> bool {
    let mut external = BTreeMap::new();
    for (label, index, side) in [
        ("before", before, &mut matched.before),
        ("after", after, &mut matched.after),
    ] {
        for (key, binding) in &index.implementations {
            let owner_id = bound_id(&binding.owner, side, &mut external);
            let trait_target_id = binding
                .trait_target
                .as_ref()
                .and_then(|target| bound_id(target, side, &mut external));
            side.implementations.insert(
                key.clone(),
                MatchedImplementation {
                    group_id: implementation_id(label, key),
                    owner_id,
                    trait_target_id,
                },
            );
        }
    }
    matched.nodes.extend(external.into_values());
    let mut before_keys: Vec<_> = before.implementations.keys().cloned().collect();
    let mut after_keys: Vec<_> = after.implementations.keys().cloned().collect();
    before_keys.sort();
    after_keys.sort();
    let mut candidates = Vec::new();
    if !cancelled {
        for (left_index, left_key) in before_keys.iter().enumerate() {
            if !should_continue() {
                cancelled = true;
                break;
            }
            let left = &matched.before.implementations[left_key];
            if left.owner_id.is_none() || !implementation(before, left_key).reliable {
                continue;
            }
            let mut targets: Vec<_> = after_keys
                .iter()
                .enumerate()
                .filter(|(_, key)| {
                    let right = &matched.after.implementations[*key];
                    right.owner_id == left.owner_id
                        && right.trait_target_id == left.trait_target_id
                        && (left.trait_target_id.is_some()
                            || implementation(before, left_key).trait_name
                                == implementation(after, key).trait_name)
                        && implementation(after, key).reliable
                })
                .map(|(index, _)| index)
                .collect();
            if targets.len() > 1 {
                targets.retain(|index| {
                    implementation(before, left_key).header_tokens
                        == implementation(after, &after_keys[*index]).header_tokens
                        && implementation_members(before, left_key)
                            == implementation_members(after, &after_keys[*index])
                });
            }
            candidates.extend(targets.into_iter().map(|target| (left_index, target)));
        }
    }
    let mut pairs = Pairs::default();
    if cancelled {
        candidates.clear();
    }
    accept_unique(candidates, &mut pairs);
    for (left_index, right_index) in &pairs.forward {
        let id = matched.before.implementations[&before_keys[*left_index]]
            .group_id
            .clone();
        matched
            .after
            .implementations
            .get_mut(&after_keys[*right_index])
            .expect("分组已注册")
            .group_id = id;
    }
    let before_complete = complete_side(before, coverage, StructureSide::Before) && !cancelled;
    let after_complete = complete_side(after, coverage, StructureSide::After) && !cancelled;
    for (index, key) in before_keys.iter().enumerate() {
        let right = pairs.forward.get(&index).map(|index| &after_keys[*index]);
        matched
            .implementation_groups
            .push(StructureImplementationGroup {
                id: matched.before.implementations[key].group_id.clone(),
                before: implementation_part(before, &matched.before, key, StructureSide::Before),
                after: right
                    .map(|key| {
                        implementation_part(after, &matched.after, key, StructureSide::After)
                    })
                    .unwrap_or_else(|| {
                        absent_implementation(after_complete, StructureSide::After, key, cancelled)
                    }),
            });
    }
    for (index, key) in after_keys
        .iter()
        .enumerate()
        .filter(|(index, _)| !pairs.backward.contains_key(index))
    {
        let _ = index;
        matched
            .implementation_groups
            .push(StructureImplementationGroup {
                id: matched.after.implementations[key].group_id.clone(),
                before: absent_implementation(
                    before_complete,
                    StructureSide::Before,
                    key,
                    cancelled,
                ),
                after: implementation_part(after, &matched.after, key, StructureSide::After),
            });
    }
    cancelled
}

/// impl 默认位置为其自身真实片段，相关文档另保留准确范围。
fn implementation_part(
    index: &SideIndex<'_>,
    matched: &MatchedSide,
    key: &ImplementationKey,
    side: StructureSide,
) -> StructureFact<StructureImplementationSide> {
    let group = implementation(index, key);
    let metadata = &matched.implementations[key];
    let Some(owner_id) = metadata.owner_id.clone().filter(|_| group.reliable) else {
        return StructureFact::Unknown {
            diagnostics: vec![StructureDiagnostic {
                reason: StructureDiagnosticReason::UnreliableScope,
                message: format!("无法可靠确定 impl {} 的所属类型。", group.owner),
                side: Some(side),
                path: Some(key.0.clone()),
                range: Some(group.range.position.clone()),
                retryable: false,
            }],
        };
    };
    let source_id = &matched.source_ids[&key.0];
    let mut locations = vec![StructureLocation {
        source_id: source_id.clone(),
        side,
        range: group.range.position.clone(),
        role: StructureLocationRole::Implementation,
    }];
    if let Some(range) = &group.documentation_range {
        locations.push(StructureLocation {
            source_id: source_id.clone(),
            side,
            range: range.position.clone(),
            role: StructureLocationRole::Documentation,
        });
    }
    StructureFact::Present {
        value: StructureImplementationSide {
            owner_id,
            trait_name: group.trait_name.clone(),
            trait_target_id: metadata.trait_target_id.clone(),
            signature: group.signature.clone(),
            locations,
        },
    }
}

/// 分组缺侧与声明一样依赖覆盖，不因实现尚未解析就推断删除。
fn absent_implementation(
    complete: bool,
    side: StructureSide,
    key: &ImplementationKey,
    cancelled: bool,
) -> StructureFact<StructureImplementationSide> {
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
                message: format!("另一侧覆盖不足，无法确认 {} 中实现组的对应来源。", key.0),
                side: Some(side),
                path: None,
                range: None,
                retryable: cancelled,
            }],
        }
    }
}

/// 聚合类型只比较自己的 impl 头部和文档，多来源成员正文完全排除。
pub(super) fn apply_implementation_changes(
    before: &SideIndex<'_>,
    after: &SideIndex<'_>,
    matched: &mut MatchedStructure,
    complete: bool,
) {
    let fragments = |index: &SideIndex<'_>, side: &MatchedSide| {
        let mut result: HashMap<StructureNodeId, (Vec<String>, Vec<String>)> = HashMap::new();
        for (key, metadata) in &side.implementations {
            let group = implementation(index, key);
            if let Some(owner) = metadata.owner_id.as_ref().filter(|_| group.reliable) {
                let entry = result.entry(owner.clone()).or_default();
                entry.0.push(group.header_tokens.clone());
                if !group.documentation.is_empty() {
                    entry.1.push(group.documentation.clone());
                }
            }
        }
        for entry in result.values_mut() {
            entry.0.sort();
            entry.1.sort();
        }
        result
    };
    let before_fragments = fragments(before, &matched.before);
    let after_fragments = fragments(after, &matched.after);
    let mut confirmed_partial: HashMap<StructureNodeId, (bool, bool)> = HashMap::new();
    if !complete {
        let old_groups: HashMap<_, _> = matched
            .before
            .implementations
            .iter()
            .map(|(key, metadata)| (&metadata.group_id, (key, metadata)))
            .collect();
        for (key, metadata) in &matched.after.implementations {
            let Some((old_key, old_metadata)) = old_groups.get(&metadata.group_id) else {
                continue;
            };
            if metadata.owner_id != old_metadata.owner_id {
                continue;
            }
            let Some(owner) = &metadata.owner_id else {
                continue;
            };
            let left = implementation(before, old_key);
            let right = implementation(after, key);
            if !left.reliable || !right.reliable {
                continue;
            }
            let changes = confirmed_partial.entry(owner.clone()).or_default();
            changes.0 |= left.header_tokens != right.header_tokens;
            changes.1 |= left.documentation != right.documentation;
        }
    }
    for node in &mut matched.nodes {
        let StructureNode::Declaration(node) = node else {
            continue;
        };
        if !matches!(node.before, StructureFact::Present { .. })
            || !matches!(node.after, StructureFact::Present { .. })
        {
            continue;
        }
        let empty = (Vec::new(), Vec::new());
        let left = before_fragments.get(&node.id).unwrap_or(&empty);
        let right = after_fragments.get(&node.id).unwrap_or(&empty);
        let differences = if complete {
            (left.0 != right.0, left.1 != right.1)
        } else {
            confirmed_partial.get(&node.id).copied().unwrap_or_default()
        };
        if !complete
            && (before_fragments.contains_key(&node.id) || after_fragments.contains_key(&node.id))
        {
            node.comparison = StructureComparison::Incomplete;
        }
        if differences.0 && !node.changes.contains(&StructureChangeLabel::Modified) {
            node.changes.push(StructureChangeLabel::Modified);
        }
        if differences.1 && !node.changes.contains(&StructureChangeLabel::Documentation) {
            node.changes.push(StructureChangeLabel::Documentation);
        }
    }
}
