use std::collections::{BTreeMap, BTreeSet};

use super::{CodeNode, CodeReference, NodeId};

/// 按节点组织的差异，配合前后两张原图使用。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeGraphDiff {
    /// 仅包含自身内容、直接包含关系或引用关系发生变化的节点。
    pub nodes: Vec<NodeDiff>,
}

/// 一个节点的自身内容、直接包含关系与引用各自的变化。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeDiff {
    pub id: NodeId,
    /// None 表示自身内容未变，即使子节点或引用发生了变化。
    pub content: Option<ChangeKind>,
    pub contains: Vec<SetChange<NodeId>>,
    pub references: Vec<SetChange<CodeReference>>,
}

/// 节点自身数据的新增、删除或修改。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

/// 集合变化，更换关联端点或语义表示删除旧关联、增加新关联。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetChange<T> {
    Added(T),
    Removed(T),
}

/// 按稳定身份匹配节点，分别计算三个维度，省略完全未变的节点。
pub(super) fn compare(
    before: &BTreeMap<NodeId, CodeNode>,
    after: &BTreeMap<NodeId, CodeNode>,
) -> CodeGraphDiff {
    let ids: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    let mut nodes = Vec::new();
    for id in ids {
        let old = before.get(id);
        let new = after.get(id);
        let content = match (old, new) {
            (None, Some(_)) => Some(ChangeKind::Added),
            (Some(_), None) => Some(ChangeKind::Removed),
            (Some(old), Some(new)) if !same_content(old, new) => Some(ChangeKind::Modified),
            _ => None,
        };
        let contains = set_changes(
            old.map_or(&[], |node| node.data().contains.as_slice()),
            new.map_or(&[], |node| node.data().contains.as_slice()),
        );
        let references = set_changes(
            old.map_or(&[], |node| node.data().references.as_slice()),
            new.map_or(&[], |node| node.data().references.as_slice()),
        );
        if content.is_some() || !contains.is_empty() || !references.is_empty() {
            nodes.push(NodeDiff {
                id: id.clone(),
                content,
                contains,
                references,
            });
        }
    }
    CodeGraphDiff { nodes }
}

/// 只比较自身语义；展示源码的排版、注释和嵌套成员不计入父节点内容变化。
fn same_content(before: &CodeNode, after: &CodeNode) -> bool {
    let old = before.data();
    let new = after.data();
    before.category() == after.category()
        && old.language == new.language
        && old.native_kind == new.native_kind
        && old.name == new.name
        && old.signature == new.signature
        && old.code == new.code
        && old.extensions == new.extensions
}

/// 按集合而非数组比较，忽略顺序与重复事实，只复制实际增删的元素。
fn set_changes<T: Clone + Ord>(before: &[T], after: &[T]) -> Vec<SetChange<T>> {
    let old: BTreeSet<_> = before.iter().collect();
    let new: BTreeSet<_> = after.iter().collect();
    old.difference(&new)
        .copied()
        .cloned()
        .map(SetChange::Removed)
        .chain(new.difference(&old).copied().cloned().map(SetChange::Added))
        .collect()
}
