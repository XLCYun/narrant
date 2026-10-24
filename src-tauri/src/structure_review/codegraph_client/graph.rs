//! 严格校验公共 CLI 返回图，再交给 Narrant 既有 CodeGraph 构造器。

use super::{failure, protocol::*, Result};
use crate::structure_review::code_graph::{
    CodeGraph, CodeNode, CodeNodeData, CodeReference, CodeRoot,
    ExternalSymbol as GraphExternalSymbol, LanguageId, NodeId, ReferenceTarget, SymbolScope,
};
use std::collections::{BTreeMap, BTreeSet};

/// 验证公共节点、关系、外部符号与包含树；来源证据由 CodeGraph 在输出前验证。
pub(super) fn build(completed: Completed, languages: &[&str]) -> Result<CodeGraph> {
    let mut nodes = BTreeSet::new();
    for node in &completed.graph.nodes {
        if !nodes.insert(node.id.as_str()) {
            return Err(failure("CodeGraph 节点 ID 重复"));
        }
    }
    let final_ids = nodes
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut converted = Vec::new();
    let mut referenced_external = BTreeSet::new();
    for node in completed.graph.nodes {
        if !languages.contains(&node.language.as_str())
            || node.native_kind.is_empty()
            || !node.id.starts_with(&format!("{}:", node.language))
            || node.id.len() <= node.language.len() + 1
        {
            return Err(failure("节点语言、身份命名空间或原生种类无效"));
        }
        if node.extensions.keys().any(|key| !namespaced(key)) {
            return Err(failure("语言扩展字段缺少明确命名空间"));
        }
        let mut references = BTreeSet::new();
        for reference in node.references {
            if !namespaced(&reference.semantic) {
                return Err(failure("关系语义缺少明确命名空间"));
            }
            let target = match reference.target {
                Target::Node { id } if !id.is_empty() => ReferenceTarget::Node(NodeId(id)),
                Target::External { symbol }
                    if symbol
                        .split_once(':')
                        .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty()) =>
                {
                    if final_ids.contains(&symbol) {
                        return Err(failure("已在最终图中的目标不能标为 External"));
                    }
                    referenced_external.insert(symbol.clone());
                    ReferenceTarget::External { symbol }
                }
                Target::Unresolved { name } if !name.is_empty() => {
                    ReferenceTarget::Unresolved { name }
                }
                _ => return Err(failure("关系目标为空或身份缺少语言命名空间")),
            };
            if !references.insert(CodeReference {
                semantic: reference.semantic,
                target,
            }) {
                return Err(failure("CodeGraph 未去重重复关系事实"));
            }
        }
        let data = CodeNodeData {
            id: NodeId(node.id),
            language: LanguageId(node.language),
            native_kind: node.native_kind,
            name: node.name.0,
            signature: node.signature.0,
            code: node.code.0,
            source: node.source.0,
            contains: node.contains.into_iter().map(NodeId).collect(),
            references: references.into_iter().collect(),
            extensions: node.extensions,
        };
        converted.push(match node.category {
            Category::Container => CodeNode::Container(data),
            Category::Type => CodeNode::Type(data),
            Category::Callable => CodeNode::Callable(data),
            Category::Value => CodeNode::Value(data),
            Category::ExecutionRegion => CodeNode::ExecutionRegion(data),
            Category::Extension => CodeNode::Extension(data),
        });
    }
    let mut external_symbols = BTreeMap::new();
    for symbol in completed.graph.external_symbols {
        if symbol.name.is_empty() || !referenced_external.contains(&symbol.symbol) {
            return Err(failure("图外符号描述为空或未被引用"));
        }
        let scope = match symbol.scope {
            ExternalScope::Repository => SymbolScope::Repository,
            ExternalScope::Dependency => SymbolScope::Dependency,
        };
        if external_symbols
            .insert(
                symbol.symbol,
                GraphExternalSymbol {
                    name: symbol.name,
                    scope,
                },
            )
            .is_some()
        {
            return Err(failure("图外符号描述重复"));
        }
    }
    if external_symbols.len() != referenced_external.len() {
        return Err(failure("图外符号缺少来源描述"));
    }
    CodeGraph::new(
        CodeRoot {
            contains: completed
                .graph
                .root_contains
                .into_iter()
                .map(NodeId)
                .collect(),
        },
        converted,
    )
    .map(|graph| graph.with_external_symbols(external_symbols))
    .map_err(failure)
}

/// 公共语义和扩展键都要求非空命名空间与非空名称。
fn namespaced(value: &str) -> bool {
    !value.contains('\0')
        && value
            .split_once('.')
            .is_some_and(|(namespace, name)| !namespace.is_empty() && !name.is_empty())
}
