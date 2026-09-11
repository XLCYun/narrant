use std::collections::BTreeMap;

use super::{diff, validation, CodeGraphDiff, CodeNode, CompareError, InvalidGraph, NodeId};

/// 当前源码版本的代码图，统一持有节点并提供只读访问。
#[derive(Clone, Debug)]
pub struct CodeGraph {
    root: CodeRoot,
    nodes: BTreeMap<NodeId, CodeNode>,
}

/// 图的唯一入口，只关联顶层声明，自身不是源码中的包或模块。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeRoot {
    pub contains: Vec<NodeId>,
}

impl CodeGraph {
    /// 建立唯一节点索引，校验包含关系及图内引用目标。
    ///
    /// 每个节点只能有一个声明位置，全部节点必须通过无环包含关系从根到达。
    /// 引用允许成环；重复的相同引用在比较时视为同一个事实。
    pub fn new(root: CodeRoot, nodes: Vec<CodeNode>) -> Result<Self, InvalidGraph> {
        let mut index = BTreeMap::new();
        for node in nodes {
            let id = node.data().id.clone();
            if index.insert(id.clone(), node).is_some() {
                return Err(InvalidGraph {
                    message: format!("节点 ID 重复：{}。", id.0),
                });
            }
        }
        validation::validate(&root, &index)?;
        Ok(Self { root, nodes: index })
    }

    /// 从唯一的虚拟根开始遍历。
    pub fn root(&self) -> &CodeRoot {
        &self.root
    }

    /// 根据 ID 返回节点的 Rust 借用，不要求调用方维护索引。
    pub fn node(&self, id: &NodeId) -> Option<&CodeNode> {
        self.nodes.get(id)
    }

    /// 按稳定身份分别比较自身声明、直接包含关系和引用。
    ///
    /// 两侧须使用相同提取和规范化约定；此约定由调用方保证。
    /// 当前已验证的内存图可以直接比较，不产生额外的比较错误。
    /// 返回值按节点 ID 排序，关系变化先列删除、再列新增，各组按值排序。
    pub fn diff(&self, after: &CodeGraph) -> Result<CodeGraphDiff, CompareError> {
        Ok(diff::compare(&self.nodes, &after.nodes))
    }
}
