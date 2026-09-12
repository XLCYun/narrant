use super::super::code_graph::{self, CodeCategory, CodeGraph, CodeGraphDiff, ReferenceTarget};
use serde::Serialize;

/// WebView 使用的结构比较结果；不携带 checkout 路径或布局数据。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StructureComparison {
    pub review_id: String,
    pub snapshot_id: String,
    pub comparison_base: String,
    pub head_revision: String,
    pub before: Graph,
    pub after: Graph,
    pub diff: GraphDifference,
    pub coverage: Vec<FileCoverage>,
}

/// 分侧说明文件是否进入分析，避免把未支持范围当作无变化。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileCoverage {
    pub path: String,
    pub old_path: Option<String>,
    pub before: &'static str,
    pub after: &'static str,
}

/// 已验证代码图的传输投影，父子和引用仍按不透明身份关联。
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Graph {
    pub root: Vec<String>,
    pub nodes: Vec<Node>,
}

/// 单个高层对象的代码语义，不将显示位置写回通用代码图。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Node {
    pub id: String,
    pub category: &'static str,
    pub language: String,
    pub native_kind: String,
    pub name: Option<String>,
    pub signature: Option<String>,
    pub code: Option<String>,
    pub contains: Vec<String>,
    pub references: Vec<Reference>,
    pub extensions: code_graph::ExtensionFields,
}

/// 关联的来源由所属节点确定。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Reference {
    pub semantic: String,
    pub target: Target,
}

/// 图内、图外和未解析目标保持通用 Interface 中的区别。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum Target {
    Node { id: String },
    External { symbol: String },
    Unresolved { name: String },
}

/// 保留节点自身变化、包含变化和引用变化三个独立维度。
#[derive(Clone, Debug, Serialize)]
pub(crate) struct GraphDifference {
    pub nodes: Vec<NodeDifference>,
}

/// 父节点只有关系变化时，content 仍为 null。
#[derive(Clone, Debug, Serialize)]
pub(crate) struct NodeDifference {
    pub id: String,
    pub content: Option<&'static str>,
    pub contains: Vec<SetChange<String>>,
    pub references: Vec<SetChange<Reference>>,
}

/// 集合成员的具体增删，保留可直接绘制的差异内容。
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub(crate) enum SetChange<T> {
    Added(T),
    Removed(T),
}

impl From<&CodeGraph> for Graph {
    /// 从根遍历完整节点表；只接受已经通过约束验证的图。
    fn from(graph: &CodeGraph) -> Self {
        let mut pending = graph.root().contains.clone();
        let mut nodes = Vec::new();
        while let Some(id) = pending.pop() {
            let node = graph.node(&id).expect("已验证图的 contains 目标必须存在");
            let data = node.data();
            pending.extend(data.contains.iter().cloned());
            nodes.push(Node {
                id: data.id.0.clone(),
                language: data.language.0.clone(),
                category: match node.category() {
                    CodeCategory::Container => "container",
                    CodeCategory::Type => "type",
                    CodeCategory::Callable => "callable",
                    CodeCategory::Value => "value",
                    CodeCategory::ExecutionRegion => "executionRegion",
                    CodeCategory::Extension => "extension",
                },
                native_kind: data.native_kind.clone(),
                name: data.name.clone(),
                signature: data.signature.clone(),
                code: data.code.clone(),
                contains: data.contains.iter().map(|id| id.0.clone()).collect(),
                references: data.references.iter().map(Reference::from).collect(),
                extensions: data.extensions.clone(),
            });
        }
        nodes.sort_by(|left, right| left.id.cmp(&right.id));
        Self {
            root: graph
                .root()
                .contains
                .iter()
                .map(|id| id.0.clone())
                .collect(),
            nodes,
        }
    }
}

impl From<&code_graph::CodeReference> for Reference {
    /// 原样映射语义与解析状态。
    fn from(reference: &code_graph::CodeReference) -> Self {
        Self {
            semantic: reference.semantic.clone(),
            target: match &reference.target {
                ReferenceTarget::Node(id) => Target::Node { id: id.0.clone() },
                ReferenceTarget::External { symbol } => Target::External {
                    symbol: symbol.clone(),
                },
                ReferenceTarget::Unresolved { name } => Target::Unresolved { name: name.clone() },
            },
        }
    }
}

impl From<&CodeGraphDiff> for GraphDifference {
    /// 传输现有 graph diff 的实际集合变化，不在前端重新猜测修改状态。
    fn from(diff: &CodeGraphDiff) -> Self {
        Self {
            nodes: diff
                .nodes
                .iter()
                .map(|node| NodeDifference {
                    id: node.id.0.clone(),
                    content: node.content.map(|kind| match kind {
                        code_graph::ChangeKind::Added => "added",
                        code_graph::ChangeKind::Removed => "removed",
                        code_graph::ChangeKind::Modified => "modified",
                    }),
                    contains: node
                        .contains
                        .iter()
                        .map(|change| project_change(change, |id| id.0.clone()))
                        .collect(),
                    references: node
                        .references
                        .iter()
                        .map(|change| {
                            project_change(change, |reference| Reference::from(reference))
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// 集合元素投影独立于 added/removed 的判定。
fn project_change<T, U>(
    change: &code_graph::SetChange<T>,
    convert: impl Fn(&T) -> U,
) -> SetChange<U> {
    match change {
        code_graph::SetChange::Added(value) => SetChange::Added(convert(value)),
        code_graph::SetChange::Removed(value) => SetChange::Removed(convert(value)),
    }
}
