use crate::structure_review::code_graph::{
    ChangeKind, CodeCategory::*, CodeGraph, CodeNode, CodeReference, CodeRoot, LanguageId,
    NodeDiff, ReferenceTarget, SetChange,
};

use super::fixtures::{calls, data, graph, id, node};

/// 节点、根入口、包含列表和引用的排列不影响差异，重复引用也不新增事实。
#[test]
fn input_order_and_duplicate_references_do_not_create_changes() {
    let mut owner = data("owner", &["a", "b"]);
    owner.references = vec![calls("a"), calls("b")];
    let mut reordered = owner.clone();
    reordered.contains.reverse();
    reordered.references = vec![calls("b"), calls("a"), calls("a")];
    let before = graph(
        &["owner", "other"],
        vec![
            CodeNode::Container(owner),
            node(Callable, "a", &[]),
            node(Callable, "b", &[]),
            node(Type, "other", &[]),
        ],
    );
    let after = graph(
        &["other", "owner"],
        vec![
            node(Type, "other", &[]),
            node(Callable, "b", &[]),
            node(Callable, "a", &[]),
            CodeNode::Container(reordered),
        ],
    );
    assert!(before.diff(&after).unwrap().nodes.is_empty());
}

/// 方法声明修改仅命中该方法，不递归修改类、模块或未变化的被调用目标。
#[test]
fn signature_change_does_not_modify_ancestors_or_callees() {
    let mut method = data("method", &[]);
    method.references.push(calls("helper"));
    let mut changed = method.clone();
    changed.signature = Some("fn method() -> Result<Item>".into());
    let common = vec![
        node(Container, "module", &["type", "helper"]),
        node(Type, "type", &["method"]),
        node(Callable, "helper", &[]),
    ];
    let before = graph(
        &["module"],
        common
            .iter()
            .cloned()
            .chain([CodeNode::Callable(method)])
            .collect(),
    );
    let after = graph(
        &["module"],
        common
            .into_iter()
            .chain([CodeNode::Callable(changed)])
            .collect(),
    );
    assert_eq!(
        before.diff(&after).unwrap().nodes,
        vec![NodeDiff {
            id: id("method"),
            content: Some(ChangeKind::Modified),
            contains: vec![],
            references: vec![],
        }]
    );
    assert!(after.node(&id("helper")).is_some());
}

/// 子节点替换产生具体包含边的增删，父节点的自身声明仍未变化。
#[test]
fn child_additions_and_removals_do_not_modify_parent_declarations() {
    let before = graph(
        &["parent"],
        vec![
            node(Type, "parent", &["old", "stay"]),
            node(Callable, "old", &[]),
            node(Callable, "stay", &[]),
        ],
    );
    let after = graph(
        &["parent"],
        vec![
            node(Type, "parent", &["stay", "new"]),
            node(Callable, "stay", &[]),
            node(Callable, "new", &[]),
        ],
    );
    assert_eq!(
        before.diff(&after).unwrap().nodes,
        vec![
            NodeDiff {
                id: id("new"),
                content: Some(ChangeKind::Added),
                contains: vec![],
                references: vec![]
            },
            NodeDiff {
                id: id("old"),
                content: Some(ChangeKind::Removed),
                contains: vec![],
                references: vec![]
            },
            NodeDiff {
                id: id("parent"),
                content: None,
                contains: vec![SetChange::Removed(id("old")), SetChange::Added(id("new"))],
                references: vec![]
            },
        ]
    );
}

/// 整棵子树新增或删除时，缺失一侧的包含和引用集合按空集合处理。
#[test]
fn added_and_removed_nodes_include_their_relationship_deltas() {
    let empty = CodeGraph::new(CodeRoot::default(), vec![]).unwrap();
    let mut parent = data("parent", &["child"]);
    parent.references = vec![calls("child"), calls("child")];
    let populated = graph(
        &["parent"],
        vec![CodeNode::Container(parent), node(Callable, "child", &[])],
    );

    assert_eq!(
        empty.diff(&populated).unwrap().nodes,
        vec![
            NodeDiff {
                id: id("child"),
                content: Some(ChangeKind::Added),
                contains: vec![],
                references: vec![]
            },
            NodeDiff {
                id: id("parent"),
                content: Some(ChangeKind::Added),
                contains: vec![SetChange::Added(id("child"))],
                references: vec![SetChange::Added(calls("child"))]
            },
        ]
    );
    assert_eq!(
        populated.diff(&empty).unwrap().nodes,
        vec![
            NodeDiff {
                id: id("child"),
                content: Some(ChangeKind::Removed),
                contains: vec![],
                references: vec![]
            },
            NodeDiff {
                id: id("parent"),
                content: Some(ChangeKind::Removed),
                contains: vec![SetChange::Removed(id("child"))],
                references: vec![SetChange::Removed(calls("child"))]
            },
        ]
    );
}

/// 各种引用目标与关联语义的改变，都输出一删一增而不修改来源节点声明。
#[test]
fn reference_changes_report_removed_and_added_facts() {
    let external = |symbol: &str| CodeReference {
        semantic: "code.calls".into(),
        target: ReferenceTarget::External {
            symbol: symbol.into(),
        },
    };
    let unresolved = CodeReference {
        semantic: "code.calls".into(),
        target: ReferenceTarget::Unresolved { name: "a".into() },
    };
    let renamed_semantic = CodeReference {
        semantic: "test.delegates_to".into(),
        target: ReferenceTarget::Node(id("a")),
    };
    for (old, new) in [
        (calls("a"), calls("b")),
        (external("test:dependency#a"), external("test:dependency#b")),
        (unresolved.clone(), calls("a")),
        (external("test:dependency#a"), unresolved),
        (calls("a"), renamed_semantic),
    ] {
        let before = graph_with_reference(old.clone());
        let after = graph_with_reference(new.clone());
        assert_eq!(
            before.diff(&after).unwrap().nodes,
            vec![NodeDiff {
                id: id("owner"),
                content: None,
                contains: vec![],
                references: vec![SetChange::Removed(old), SetChange::Added(new)],
            }]
        );
    }
}

/// 同一身份的每个自身声明字段变化都应得到 Modified，包括类别变化。
#[test]
fn own_declaration_fields_are_compared_without_relationships() {
    let original = data("item", &[]);
    let before = graph(&["item"], vec![CodeNode::Callable(original.clone())]);
    let mut language = original.clone();
    language.language = LanguageId("other".into());
    let mut native_kind = original.clone();
    native_kind.native_kind = "method".into();
    let mut name = original.clone();
    name.name = None;
    let mut signature = original.clone();
    signature.signature = None;
    let mut extensions = original.clone();
    extensions
        .extensions
        .insert("test.flags".into(), serde_json::json!({"async": true}));
    for changed in [
        CodeNode::Callable(language),
        CodeNode::Callable(native_kind),
        CodeNode::Callable(name),
        CodeNode::Callable(signature),
        CodeNode::Callable(extensions),
        CodeNode::Type(original),
    ] {
        let after = graph(&["item"], vec![changed]);
        assert_eq!(
            before.diff(&after).unwrap().nodes,
            vec![NodeDiff {
                id: id("item"),
                content: Some(ChangeKind::Modified),
                contains: vec![],
                references: vec![],
            }]
        );
    }
}

/// JSON 对象键序不产生差异，扩展数组的顺序仍保留其语义。
#[test]
fn extensions_ignore_object_key_order_but_preserve_array_order() {
    let mut before = data("item", &[]);
    before.extensions.insert(
        "test.options".into(),
        serde_json::from_str(r#"{"a":1,"b":[1,2]}"#).unwrap(),
    );
    let mut reordered = data("item", &[]);
    reordered.extensions.insert(
        "test.options".into(),
        serde_json::from_str(r#"{"b":[1,2],"a":1}"#).unwrap(),
    );
    let mut changed = reordered.clone();
    changed.extensions.insert(
        "test.options".into(),
        serde_json::json!({"a": 1, "b": [2, 1]}),
    );
    let before = graph(&["item"], vec![CodeNode::Extension(before)]);
    let reordered = graph(&["item"], vec![CodeNode::Extension(reordered)]);
    let changed = graph(&["item"], vec![CodeNode::Extension(changed)]);
    assert!(before.diff(&reordered).unwrap().nodes.is_empty());
    assert_eq!(
        before.diff(&changed).unwrap().nodes[0].content,
        Some(ChangeKind::Modified)
    );
}

/// 核心比较不能盲删空格，字符串字面量中的空格变化仍有意义。
#[test]
fn significant_whitespace_inside_signature_literals_is_preserved() {
    let mut before = data("text", &[]);
    before.signature = Some(r#"const TEXT: &str = "a b";"#.into());
    let mut after = before.clone();
    after.signature = Some(r#"const TEXT: &str = "ab";"#.into());
    let before = graph(&["text"], vec![CodeNode::Value(before)]);
    let after = graph(&["text"], vec![CodeNode::Value(after)]);
    assert_eq!(
        before.diff(&after).unwrap().nodes[0].content,
        Some(ChangeKind::Modified)
    );
}

/// 身份改变即使内容相同也按增删处理，不偷偷进行重命名匹配。
#[test]
fn different_identity_is_removed_and_added_even_with_identical_content() {
    let original = data("old", &[]);
    let mut renamed = original.clone();
    renamed.id = id("new");
    let before = graph(&["old"], vec![CodeNode::Callable(original)]);
    let after = graph(&["new"], vec![CodeNode::Callable(renamed)]);
    assert_eq!(
        before.diff(&after).unwrap().nodes,
        vec![
            NodeDiff {
                id: id("new"),
                content: Some(ChangeKind::Added),
                contains: vec![],
                references: vec![]
            },
            NodeDiff {
                id: id("old"),
                content: Some(ChangeKind::Removed),
                contains: vec![],
                references: vec![]
            },
        ]
    );
}

/// 新语言节点的加入不构成整张图不兼容，也不修改原有语言节点。
#[test]
fn adding_a_language_is_a_normal_node_addition() {
    let before = graph(&["existing"], vec![node(Callable, "existing", &[])]);
    let mut added = data("new_language", &[]);
    added.language = LanguageId("new_language".into());
    added.references.push(calls("existing"));
    let after = graph(
        &["existing", "new_language"],
        vec![node(Callable, "existing", &[]), CodeNode::Callable(added)],
    );
    assert_eq!(
        before.diff(&after).unwrap().nodes,
        vec![NodeDiff {
            id: id("new_language"),
            content: Some(ChangeKind::Added),
            contains: vec![],
            references: vec![SetChange::Added(calls("existing"))],
        }]
    );
}

/// 已匹配节点改变声明位置只影响真实父节点的包含边，虚拟根不产生差异记录。
#[test]
fn placement_changes_keep_node_declarations_unchanged() {
    let before = graph(
        &["child", "parent"],
        vec![node(Callable, "child", &[]), node(Container, "parent", &[])],
    );
    let after = graph(
        &["parent"],
        vec![
            node(Container, "parent", &["child"]),
            node(Callable, "child", &[]),
        ],
    );
    assert_ne!(before.root().contains, after.root().contains);
    assert_eq!(
        before.diff(&after).unwrap().nodes,
        vec![NodeDiff {
            id: id("parent"),
            content: None,
            contains: vec![SetChange::Added(id("child"))],
            references: vec![],
        }]
    );
}

/// 真实差异结果的排序不受提取顺序影响，新增节点和关系都稳定输出。
#[test]
fn difference_output_is_deterministic() {
    let empty = CodeGraph::new(CodeRoot::default(), vec![]).unwrap();
    let mut parent = data("parent", &["b", "a"]);
    parent.references = vec![calls("b"), calls("a"), calls("a")];
    let first = graph(
        &["parent"],
        vec![
            node(Callable, "b", &[]),
            CodeNode::Container(parent.clone()),
            node(Callable, "a", &[]),
        ],
    );
    parent.contains.reverse();
    parent.references.reverse();
    let second = graph(
        &["parent"],
        vec![
            node(Callable, "a", &[]),
            node(Callable, "b", &[]),
            CodeNode::Container(parent),
        ],
    );
    let diff = empty.diff(&first).unwrap();
    assert_eq!(diff, empty.diff(&second).unwrap());
    assert_eq!(
        diff.nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>(),
        vec![id("a"), id("b"), id("parent")]
    );
    assert_eq!(
        diff.nodes[2].contains,
        vec![SetChange::Added(id("a")), SetChange::Added(id("b"))]
    );
    assert_eq!(
        diff.nodes[2].references,
        vec![SetChange::Added(calls("a")), SetChange::Added(calls("b"))]
    );
}

/// 为引用差异场景构造具有相同上下文节点的图。
fn graph_with_reference(reference: CodeReference) -> CodeGraph {
    let mut owner = data("owner", &[]);
    owner.references.push(reference);
    graph(
        &["owner", "a", "b"],
        vec![
            CodeNode::Callable(owner),
            node(Callable, "a", &[]),
            node(Callable, "b", &[]),
        ],
    )
}
/// 自身实现变化独立于签名与关联，不把子对象的代码递归比较到父节点。
#[test]
fn body_only_changes_modify_only_the_owning_node() {
    let make = |code: &str| {
        let mut callable = data("method", &[]);
        callable.code = Some(code.into());
        graph(
            &["type"],
            vec![
                node(Type, "type", &["method"]),
                CodeNode::Callable(callable),
            ],
        )
    };
    let diff = make("{ 1 }").diff(&make("{ 2 }")).unwrap();
    assert_eq!(
        diff.nodes,
        vec![NodeDiff {
            id: id("method"),
            content: Some(ChangeKind::Modified),
            contains: vec![],
            references: vec![]
        }]
    );
}

/// 调用替换可以同时改变自身代码和引用集合，两个维度并不互斥。
#[test]
fn call_replacement_changes_content_and_references() {
    let make = |callee: &str| {
        let mut caller = data("caller", &[]);
        caller.code = Some(format!("{{ {callee} ( ) }}"));
        caller.references = vec![calls(callee)];
        graph(
            &["caller", "a", "b"],
            vec![
                CodeNode::Callable(caller),
                node(Callable, "a", &[]),
                node(Callable, "b", &[]),
            ],
        )
    };
    let diff = make("a").diff(&make("b")).unwrap();
    assert_eq!(diff.nodes.len(), 1);
    assert_eq!(diff.nodes[0].content, Some(ChangeKind::Modified));
    assert_eq!(
        diff.nodes[0].references,
        vec![SetChange::Removed(calls("a")), SetChange::Added(calls("b"))]
    );
}
