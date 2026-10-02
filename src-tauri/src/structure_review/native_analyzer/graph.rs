//! 验证 coverage/origin 后把协议 DTO 转为唯一公共 CodeGraph。

use super::{
    failure,
    input::{self, CheckedInput},
    protocol::*,
    HelperIdentity, Result,
};
use crate::structure_review::code_graph::{
    CodeGraph, CodeNode, CodeNodeData, CodeReference, CodeRoot,
    ExternalSymbol as GraphExternalSymbol, LanguageId, NodeId, ReferenceTarget, SymbolScope,
};
use std::collections::{BTreeMap, BTreeSet};

/// 交叉验证全部来源，不允许缺失文件、排除文件或未批准平台支持成功图。
pub(super) fn build(
    completed: Completed,
    input: &CheckedInput,
    identity: &HelperIdentity,
) -> Result<CodeGraph> {
    let evidence = &completed.evidence;
    let configurations = input::unique(evidence.configurations.iter().map(|c| &c.id), "配置 ID")?;
    let platforms = input
        .platforms
        .iter()
        .map(|p| p.id.as_str())
        .collect::<BTreeSet<_>>();
    for configuration in &evidence.configurations {
        for path in input::unique(&configuration.source_paths, "配置来源")? {
            input::repository_file(&input.repository, path)?;
        }
        if !input::unique(&configuration.platform_ids, "配置平台")?.is_subset(&platforms) {
            return Err(failure("配置引用未批准的平台"));
        }
    }
    let mut files = BTreeMap::new();
    for file in &evidence.files {
        if !input.files.contains(&file.path)
            || files
                .insert(file.path.as_str(), &file.disposition)
                .is_some()
        {
            return Err(failure("文件覆盖出现未请求或重复路径"));
        }
        let ids = input::unique(&file.configuration_ids, "文件配置")?;
        if ids.is_empty() || !ids.is_subset(&configurations) {
            return Err(failure("文件覆盖缺少实际配置或引用未知配置"));
        }
        if input.include_tests && file.disposition == Disposition::ExcludedTest {
            return Err(failure("include_tests=true 时不能排除选中测试文件"));
        }
    }
    if files.len() != input.files.len() {
        return Err(failure("成功结果遗漏请求文件覆盖"));
    }
    let nodes = input::unique(completed.graph.nodes.iter().map(|node| &node.id), "节点 ID")?;
    let origins = input::unique(
        evidence.origins.iter().map(|origin| &origin.node_id),
        "节点来源",
    )?;
    if nodes != origins {
        return Err(failure("节点与来源记录不一一对应"));
    }
    for origin in &evidence.origins {
        let paths = input::unique(&origin.selected_paths, "节点来源路径")?;
        if paths.is_empty()
            || paths
                .iter()
                .any(|path| files.get(path) != Some(&&Disposition::Analyzed))
        {
            return Err(failure("节点来源必须属于已分析的选中文件"));
        }
    }
    let final_ids = nodes
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut converted = Vec::new();
    let mut referenced_external = BTreeSet::new();
    for node in completed.graph.nodes {
        if !identity.languages.contains(&node.language)
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
                return Err(failure("helper 未去重重复关系事实"));
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
