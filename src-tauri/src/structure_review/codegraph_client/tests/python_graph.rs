//! 迁移旧 graph.rs 的业务断言，经实际 helper 与公共 CodeGraph/diff 验证。
use crate::structure_review::{
    code_graph::{AnalysisInput, ChangeKind, CodeGraph, CodeNode, LanguageAnalyzer},
    codegraph_client::CodeGraphAnalyzer,
};
use std::fs;

/// 显式开发 executable 与内嵌标准声明来源，不注册生产工厂。
pub(super) fn analyzer() -> CodeGraphAnalyzer {
    CodeGraphAnalyzer::from_pack("python", &super::candidate::root("python")).unwrap()
}

/// 无引用的源码也通过实际公共 CLI 边界。
fn analyze(source: &str) -> CodeGraph {
    let repository = tempfile::tempdir().unwrap();
    fs::write(repository.path().join("main.py"), source).unwrap();
    analyzer()
        .analyze(&AnalysisInput {
            repository_root: repository.path().into(),
            files: vec!["main.py".into()],
            options: Default::default(),
        })
        .unwrap()
}
/// 仅通过公共包含图找到源码对象，不依赖适配器的身份编码和内部 AST。
pub(super) fn named<'a>(graph: &'a CodeGraph, name: &str) -> &'a CodeNode {
    let mut pending = graph.root().contains.clone();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap();
        if node.data().name.as_deref() == Some(name) {
            return node;
        }
        pending.extend(node.data().contains.iter().cloned());
    }
    panic!("缺少代码对象：{name}")
}

/// 函数体修改只修改函数，模块的声明锚点和文件容器保持不变。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn function_body_change_belongs_to_callable() {
    let before = analyze("def calculate():\n    return 1\n");
    let after = analyze("def calculate():\n    return 2\n");
    let function = named(&before, "calculate");
    assert!(matches!(function, CodeNode::Callable(_)));
    let difference = before.diff(&after).unwrap();
    assert_eq!(difference.nodes.len(), 1, "{difference:?}");
    assert_eq!(difference.nodes[0].id, function.data().id);
    assert_eq!(difference.nodes[0].content, Some(ChangeKind::Modified));
}

/// 类方法独立归属，局部类仍完整属于外层方法。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn class_method_body_and_local_class_do_not_change_parent_content() {
    let before = analyze("class Service:\n    token: str = 'a'\n    def run(this):\n        class Local:\n            value = 1\n        return Local\n");
    let after = analyze("class Service:\n    token: str = 'a'\n    def run(this):\n        class Local:\n            value = 2\n        return Local\n");
    assert!(matches!(named(&before, "Service"), CodeNode::Type(_)));
    assert!(matches!(named(&before, "token"), CodeNode::Value(_)));
    let difference = before.diff(&after).unwrap();
    assert_eq!(difference.nodes.len(), 1, "{difference:?}");
    assert_eq!(difference.nodes[0].id, named(&before, "run").data().id);
    assert!(named(&before, "run").data().contains.is_empty());
}

/// 条件中的声明保留控制结构与锚点；只改子体不修改模块，移动声明则修改模块。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn conditional_declarations_preserve_execution_order_without_copying_child_content() {
    let before = analyze("n = 1\nif n:\n    def run():\n        return 1\nn = 2\n");
    let body = analyze("n = 1\nif n:\n    def run():\n        return 2\nn = 2\n");
    let moved = analyze("n = 1\nn = 2\nif n:\n    def run():\n        return 1\n");
    let body_diff = before.diff(&body).unwrap();
    assert_eq!(body_diff.nodes.len(), 1);
    assert_eq!(body_diff.nodes[0].id, named(&before, "run").data().id);
    let moved_diff = before.diff(&moved).unwrap();
    assert_eq!(moved_diff.nodes.len(), 1);
    assert_eq!(
        moved_diff.nodes[0].id,
        named(&before, "module_body").data().id
    );
}

/// 实例字段以参数绑定确认，合并类声明和多处赋值，运行时 RHS 仍归方法。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn fields_deduplicate_receiver_bindings_without_stealing_method_content() {
    let source = "class Service:\n    value: int\n    def run(receiver):\n        receiver.value = 1\n        receiver.extra: str = 'x'\n        receiver.value = 2\n    def invalid(self):\n        self = 3\n        self.fake = 1\n    @staticmethod\n    def static(self):\n        self.fake_static = 1\n";
    let before = analyze(source);
    let after = analyze(&source.replace("receiver.value = 2", "receiver.value = 3"));
    let class = named(&before, "Service");
    let members = class
        .data()
        .contains
        .iter()
        .map(|id| before.node(id).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        members
            .iter()
            .filter(|node| matches!(node, CodeNode::Value(_)))
            .count(),
        2
    );
    assert!(matches!(named(&before, "extra"), CodeNode::Value(_)));
    let difference = before.diff(&after).unwrap();
    assert_eq!(difference.nodes.len(), 1, "{difference:?}");
    assert_eq!(difference.nodes[0].id, named(&before, "run").data().id);
}

/// AST 规范化忽略排版和普通注释，但保存文档字符串与有序初始化语义。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn normalization_and_field_order_distinguish_semantic_content() {
    let before = analyze("class Record:\n    first = 1\n    second = 2\n    def read(self, default=1):\n        'doc'\n        return default\n");
    let formatting = analyze("# comment\nclass Record:\n  first=1 # comment\n  second=2\n  def read(self, default = 1):\n    \"doc\"\n    return default\n");
    assert!(before.diff(&formatting).unwrap().nodes.is_empty());
    let swapped = analyze("class Record:\n    second = 2\n    first = 1\n    def read(self, default=1):\n        'doc'\n        return default\n");
    let diff = before.diff(&swapped).unwrap();
    assert_eq!(diff.nodes.len(), 1);
    assert_eq!(diff.nodes[0].id, named(&before, "Record").data().id);
    let document = analyze("class Record:\n    first = 1\n    second = 2\n    def read(self, default=1):\n        'changed doc'\n        return default\n");
    assert_eq!(
        before.diff(&document).unwrap().nodes[0].id,
        named(&before, "read").data().id
    );
}

/// 多目标共享初始化保持在类内，重复字段只保留一个对象且不丢失初始化顺序。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn shared_initializers_and_repeated_fields_are_not_duplicated() {
    let before = analyze("class Pair:\n    left = right = 1\n    repeated = 1\n    repeated = 2\n");
    let after = analyze("class Pair:\n    left = right = 2\n    repeated = 1\n    repeated = 2\n");
    assert_eq!(named(&before, "Pair").data().contains.len(), 3);
    assert_eq!(named(&before, "left").data().code, None);
    let diff = before.diff(&after).unwrap();
    assert_eq!(diff.nodes.len(), 1);
    assert_eq!(diff.nodes[0].id, named(&before, "Pair").data().id);
    assert_eq!(
        named(&before, "repeated").data().code.as_deref(),
        Some("1\n2")
    );
}
