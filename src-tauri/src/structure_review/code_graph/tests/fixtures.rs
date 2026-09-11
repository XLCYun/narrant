use crate::structure_review::code_graph::{
    CodeCategory, CodeGraph, CodeNode, CodeNodeData, CodeReference, CodeRoot, ExtensionFields,
    LanguageId, NodeId, ReferenceTarget,
};

/// 创建测试语言的稳定节点身份。
pub(super) fn id(name: &str) -> NodeId {
    NodeId(format!("test:{name}"))
}

/// 创建可在不同测试场景中调整的节点公共数据。
pub(super) fn data(name: &str, children: &[&str]) -> CodeNodeData {
    CodeNodeData {
        id: id(name),
        language: LanguageId("test".into()),
        native_kind: "declaration".into(),
        name: Some(name.into()),
        signature: Some(format!("declare {name}")),
        contains: children.iter().map(|name| id(name)).collect(),
        references: Vec::new(),
        extensions: ExtensionFields::new(),
    }
}

/// 为指定通用类别构造测试节点。
pub(super) fn node(category: CodeCategory, name: &str, children: &[&str]) -> CodeNode {
    let data = data(name, children);
    match category {
        CodeCategory::Container => CodeNode::Container(data),
        CodeCategory::Type => CodeNode::Type(data),
        CodeCategory::Callable => CodeNode::Callable(data),
        CodeCategory::Value => CodeNode::Value(data),
        CodeCategory::ExecutionRegion => CodeNode::ExecutionRegion(data),
        CodeCategory::Extension => CodeNode::Extension(data),
    }
}

/// 用公开构造接口创建合法测试图。
pub(super) fn graph(roots: &[&str], nodes: Vec<CodeNode>) -> CodeGraph {
    CodeGraph::new(root(roots), nodes).expect("测试图应满足构图约束")
}

/// 创建只保存顶层节点身份的虚拟根。
pub(super) fn root(roots: &[&str]) -> CodeRoot {
    CodeRoot {
        contains: roots.iter().map(|name| id(name)).collect(),
    }
}

/// 创建一个指向图内节点的调用事实。
pub(super) fn calls(name: &str) -> CodeReference {
    CodeReference {
        semantic: "code.calls".into(),
        target: ReferenceTarget::Node(id(name)),
    }
}
