use std::collections::{BTreeSet, HashMap};

use super::compare_declarations;
use crate::review::structure::extraction_model::ExtractedFile;
use crate::review::structure::matching_model::MatchedStructure;
use crate::review::structure::model::*;
use crate::review::structure::{extract_source, parse_source, SourceLanguage};

/// 测试从实际源码进入已约定的提取/配对边界，不构造解析器内部对象。
fn extracted(path: &str, source: &str) -> ExtractedFile {
    let language = SourceLanguage::from_path(path).expect("fixture uses a supported source path");
    extract_source(path, &parse_source(language, source).unwrap())
}

/// 小型固定 Snapshot 的来源与覆盖只描述输入文件，不模拟匹配逻辑。
fn compare(before: &[ExtractedFile], after: &[ExtractedFile]) -> MatchedStructure {
    compare_with_continue(before, after, &|| true)
}

/// 取消信号通过应用比较边界注入，不依赖时钟或后台线程。
fn compare_with_continue(
    before: &[ExtractedFile],
    after: &[ExtractedFile],
    should_continue: &dyn Fn() -> bool,
) -> MatchedStructure {
    let paths: BTreeSet<_> = before
        .iter()
        .chain(after)
        .map(|file| file.path.clone())
        .collect();
    let sources: Vec<_> = paths
        .into_iter()
        .map(|path| {
            let present = |files: &[ExtractedFile], revision: &str| {
                if files.iter().any(|file| file.path == path) {
                    StructureSourceSide::Present {
                        path: path.clone(),
                        revision: revision.to_owned(),
                    }
                } else {
                    StructureSourceSide::Absent
                }
            };
            StructurePeekSource {
                id: StructureSourceId(format!("source:{path}")),
                before: present(before, "before"),
                after: present(after, "after"),
                classification: StructureSourceClassification::FileChange {
                    file_change: StructureFileChangeSummary {
                        status: if !before.iter().any(|file| file.path == path) {
                            StructureFileChangeStatus::Added
                        } else if !after.iter().any(|file| file.path == path) {
                            StructureFileChangeStatus::Deleted
                        } else {
                            StructureFileChangeStatus::Modified
                        },
                        path,
                        old_path: None,
                    },
                },
            }
        })
        .collect();
    let coverage = StructureCoverage {
        discovery: StructureDiscoveryCoverage {
            before: StructureCoverageStatus::Complete,
            after: StructureCoverageStatus::Complete,
        },
        files: [
            (StructureSide::Before, before),
            (StructureSide::After, after),
        ]
        .into_iter()
        .flat_map(|(side, files)| {
            files.iter().map(move |file| StructureFileCoverage {
                side,
                path: file.path.clone(),
                extraction: if file.complete {
                    StructureCoverageStatus::Complete
                } else {
                    StructureCoverageStatus::Partial
                },
                relationships: StructureCoverageStatus::NotStarted,
                diagnostics: file.diagnostics.clone(),
            })
        })
        .collect(),
        diagnostics: Vec::new(),
    };
    compare_declarations(
        before,
        after,
        &sources,
        &coverage,
        &HashMap::new(),
        &HashMap::new(),
        should_continue,
    )
}

/// 按展示名称选择结果，断言用户可见声明与标签而不是匹配算法步骤。
fn named<'a>(matched: &'a MatchedStructure, name: &str) -> &'a StructureDeclaration {
    matched
        .nodes
        .iter()
        .find_map(|node| {
            let StructureNode::Declaration(node) = node else {
                return None;
            };
            let side = match &node.after {
                StructureFact::Present { value } => Some(value),
                _ => match &node.before {
                    StructureFact::Present { value } => Some(value),
                    _ => None,
                },
            };
            side.filter(|value| value.name == name).map(|_| node)
        })
        .unwrap_or_else(|| panic!("missing declaration {name}"))
}

/// 成员正文改变只修改成员，父类型仅显示包含变化的去重数量。
#[test]
fn member_body_change_does_not_modify_parent_declaration() {
    let before = [extracted("box.ts", "class Box { read() { return 1; } }")];
    let after = [extracted("box.ts", "class Box { read() { return 2; } }")];
    let matched = compare(&before, &after);
    assert_eq!(
        named(&matched, "read").changes,
        [StructureChangeLabel::Modified]
    );
    assert!(named(&matched, "Box").changes.is_empty());
    assert_eq!(named(&matched, "Box").contained_changes.union, 1);
}

/// 导出、默认导出和 ambient 声明修饰属于声明自身签名，保留身份并报告修改。
#[test]
fn declaration_wrapper_modifiers_are_signature_changes() {
    for (old_source, new_source, name) in [
        (
            "function read() { return 1; }",
            "export function read() { return 1; }",
            "read",
        ),
        (
            "export function read() { return 1; }",
            "export default function read() { return 1; }",
            "read",
        ),
        ("class Box {}", "declare class Box {}", "Box"),
    ] {
        let before = [extracted("modifiers.ts", old_source)];
        let after = [extracted("modifiers.ts", new_source)];
        let matched = compare(&before, &after);
        let declaration = named(&matched, name);
        assert_eq!(
            declaration.changes,
            [StructureChangeLabel::Modified],
            "{old_source} -> {new_source}"
        );
        assert!(matches!(declaration.before, StructureFact::Present { .. }));
        assert!(matches!(declaration.after, StructureFact::Present { .. }));
        assert_eq!(
            matched
                .nodes
                .iter()
                .filter(|node| matches!(node, StructureNode::Declaration(_)))
                .count(),
            1
        );
    }
}

/// 模板字面量的正文空白有值语义，普通模板和插值边界前的空白都不能被格式归一化吞掉。
#[test]
fn template_literal_content_whitespace_is_a_body_change() {
    for (path, old_source, new_source) in [
        (
            "templates.js",
            "function read() { return `hello world`; }",
            "function read() { return `hello  world`; }",
        ),
        (
            "templates.ts",
            "function read(value: number) { return `x ${value}`; }",
            "function read(value: number) { return `x  ${value}`; }",
        ),
    ] {
        let before = [extracted(path, old_source)];
        let after = [extracted(path, new_source)];
        let matched = compare(&before, &after);
        let declaration = named(&matched, "read");
        assert_eq!(
            declaration.changes,
            [StructureChangeLabel::Modified],
            "{old_source} -> {new_source}"
        );
        assert!(matches!(declaration.before, StructureFact::Present { .. }));
        assert!(matches!(declaration.after, StructureFact::Present { .. }));
    }
}

/// 移动、改名、正文编辑和文档编辑可同时成立，仍只有一个连续声明身份。
#[test]
fn moved_renamed_edited_function_keeps_identity_and_all_labels() {
    let before = [extracted("old.ts", "/** total */\nexport function total(values: number[]) { let sum = 0; for (const item of values) sum += item; return sum; }")];
    let after = [extracted("new.ts", "/** adjusted total */\nexport function aggregate(values: number[]) { let sum = 0; for (const item of values) sum += item; return sum + 1; }")];
    let matched = compare(&before, &after);
    let declaration = named(&matched, "aggregate");
    assert_eq!(
        declaration.changes,
        [
            StructureChangeLabel::Renamed,
            StructureChangeLabel::Moved,
            StructureChangeLabel::Modified,
            StructureChangeLabel::Documentation
        ]
    );
    assert!(matches!(declaration.before, StructureFact::Present { .. }));
    assert_eq!(
        matched
            .nodes
            .iter()
            .filter(|node| matches!(node, StructureNode::Declaration(_)))
            .count(),
        1
    );
    let StructureFact::Present { value: old } = &declaration.before else {
        panic!("missing old location")
    };
    let StructureFact::Present { value: new } = &declaration.after else {
        panic!("missing new location")
    };
    assert_ne!(old.parent_id, new.parent_id);
    assert_ne!(old.locations[0].source_id, new.locations[0].source_id);
}

/// 多个完全相同函数体是歧义证据，不能通过排序强行匹配改名。
#[test]
fn identical_bodies_with_multiple_candidates_remain_additions_and_deletions() {
    let before = [extracted(
        "same.ts",
        "function first() { return 10; } function second() { return 10; }",
    )];
    let after = [extracted(
        "same.ts",
        "function third() { return 10; } function fourth() { return 10; }",
    )];
    let matched = compare(&before, &after);
    assert_eq!(
        named(&matched, "first").changes,
        [StructureChangeLabel::Deleted]
    );
    assert_eq!(
        named(&matched, "second").changes,
        [StructureChangeLabel::Deleted]
    );
    assert_eq!(
        named(&matched, "third").changes,
        [StructureChangeLabel::Added]
    );
    assert_eq!(
        named(&matched, "fourth").changes,
        [StructureChangeLabel::Added]
    );
}

/// 同名嵌套函数按真实父声明匹配，不串到另一个外层函数。
#[test]
fn duplicate_names_in_different_scopes_keep_distinct_parents() {
    let before = [extracted("scopes.ts", "function first() { function same() { return 1; } } function second() { function same() { return 1; } }")];
    let after = [extracted("scopes.ts", "function first() { function same() { return 2; } } function second() { function same() { return 1; } }")];
    let matched = compare(&before, &after);
    let changed: Vec<_> = matched
        .nodes
        .iter()
        .filter_map(|node| match node {
            StructureNode::Declaration(node) if !node.changes.is_empty() => Some(node),
            _ => None,
        })
        .collect();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].changes, [StructureChangeLabel::Modified]);
    assert_eq!(named(&matched, "first").contained_changes.union, 1);
    assert_eq!(named(&matched, "second").contained_changes.union, 0);
}

/// 重载按签名区分，即使位置重排也不把保留重载标为增删。
#[test]
fn overload_reordering_preserves_each_signature() {
    let before = [extracted("overload.ts", "function read(value: string): string; function read(value: number): number; function read(value: any) { return value; }")];
    let after = [extracted("overload.ts", "function read(value: number): number; function read(value: string): string; function read(value: any) { return value; }")];
    let matched = compare(&before, &after);
    let declarations: Vec<_> = matched
        .nodes
        .iter()
        .filter_map(|node| match node {
            StructureNode::Declaration(node) => Some(node),
            _ => None,
        })
        .collect();
    assert_eq!(declarations.len(), 3);
    assert!(declarations.iter().all(|node| node.changes.is_empty()));
}

/// 一个重载移除时，剩余重载仍有连续身份，而非全部回退为增删。
#[test]
fn removing_one_overload_preserves_the_other_signature() {
    let before = [extracted(
        "overload.ts",
        "function read(value: string): string; function read(value: number): number;",
    )];
    let after = [extracted(
        "overload.ts",
        "function read(value: string): string;",
    )];
    let matched = compare(&before, &after);
    let declarations: Vec<_> = matched
        .nodes
        .iter()
        .filter_map(|node| match node {
            StructureNode::Declaration(node) => Some(node),
            _ => None,
        })
        .collect();
    assert_eq!(declarations.len(), 2);
    assert_eq!(
        declarations
            .iter()
            .filter(|node| node.changes.is_empty())
            .count(),
        1
    );
    assert_eq!(
        declarations
            .iter()
            .filter(|node| node.changes == [StructureChangeLabel::Deleted])
            .count(),
        1
    );
}

/// 部分语法恢复不提供声明不存在的证明。
#[test]
fn damaged_opposite_side_never_manufactures_deletion() {
    let before = [extracted("broken.ts", "function previous() { return 1; }")];
    let after = [extracted("broken.ts", "function broken(")];
    let matched = compare(&before, &after);
    let previous = named(&matched, "previous");
    assert!(previous.changes.is_empty());
    assert_eq!(previous.comparison, StructureComparison::Incomplete);
    assert!(matches!(previous.after, StructureFact::Unknown { .. }));
}

/// 取消后已观察声明仍可呈现，但所有未完成缺侧结论保持未知。
#[test]
fn cancellation_retains_observed_nodes_without_add_delete_labels() {
    let before = [extracted("before.ts", "function previous() { return 1; }")];
    let after = [extracted("after.ts", "function next() { return 2; }")];
    let matched = compare_with_continue(&before, &after, &|| false);
    assert!(named(&matched, "previous").changes.is_empty());
    assert!(named(&matched, "next").changes.is_empty());
    assert!(matched
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.reason == StructureDiagnosticReason::Cancelled));
}

/// 纯缩进不算结构修改，字符串内部空白和 ASI 敏感换行仍按真实语义比较。
#[test]
fn formatting_literals_and_asi_sensitive_newlines_are_distinct() {
    let before = [extracted(
        "format.ts",
        "function read() { return \"a b\"; }",
    )];
    let formatted = [extracted(
        "format.ts",
        "function read() {\n  return \"a b\";\n}\n",
    )];
    assert!(named(&compare(&before, &formatted), "read")
        .changes
        .is_empty());
    let changed = [extracted("format.ts", "function read() { return \"ab\"; }")];
    assert_eq!(
        named(&compare(&before, &changed), "read").changes,
        [StructureChangeLabel::Modified]
    );
    let asi_before = [extracted(
        "asi.ts",
        "function read() { return\n  \"value\"; }",
    )];
    let asi_after = [extracted("asi.ts", "function read() { return \"value\"; }")];
    assert_eq!(
        named(&compare(&asi_before, &asi_after), "read").changes,
        [StructureChangeLabel::Modified]
    );
}

/// 仅关联注释改变时保留专门标签，不把文档变化折算为正文修改。
#[test]
fn documentation_only_change_is_separate() {
    let before = [extracted("docs.rs", "/// before\nfn read() -> i32 { 1 }")];
    let after = [extracted("docs.rs", "/// after\nfn read() -> i32 { 1 }")];
    assert_eq!(
        named(&compare(&before, &after), "read").changes,
        [StructureChangeLabel::Documentation]
    );
}

/// 跨文件 impl 按 use 路径归入类型，成员正文变化不修改类型自身。
#[test]
fn cross_file_implementation_preserves_type_and_member_sources() {
    let before = [
        extracted("src/lib.rs", "mod model; mod implementation;"),
        extracted("src/model.rs", "pub struct Box { pub value: i32 }"),
        extracted(
            "src/implementation.rs",
            "use crate::model::Box; impl Box { pub fn read(&self) -> i32 { self.value } }",
        ),
    ];
    let after = [
        extracted("src/lib.rs", "mod model; mod implementation;"),
        extracted("src/model.rs", "pub struct Box { pub value: i32 }"),
        extracted(
            "src/implementation.rs",
            "use crate::model::Box; impl Box { pub fn read(&self) -> i32 { self.value + 1 } }",
        ),
    ];
    let matched = compare(&before, &after);
    let parent = named(&matched, "Box");
    let member = named(&matched, "read");
    let StructureFact::Present { value } = &member.after else {
        panic!("missing member")
    };
    assert_eq!(value.parent_id, parent.id);
    assert_eq!(
        value.locations[0].source_id.0,
        "source:src/implementation.rs"
    );
    assert!(parent.changes.is_empty());
    assert_eq!(parent.contained_changes.union, 1);
    assert_eq!(matched.implementation_groups.len(), 1);
}

/// 不同 trait 的同名成员保持独立身份，不能通过相同方法名或正文合并。
#[test]
fn same_named_methods_in_distinct_traits_stay_separate() {
    let before = [extracted("src/lib.rs", "struct Item; trait A { fn read(&self) -> i32; } trait B { fn read(&self) -> i32; } impl A for Item { fn read(&self) -> i32 { 1 } } impl B for Item { fn read(&self) -> i32 { 1 } }")];
    let after = [extracted("src/lib.rs", "struct Item; trait A { fn read(&self) -> i32; } trait B { fn read(&self) -> i32; } impl A for Item { fn read(&self) -> i32 { 2 } } impl B for Item { fn read(&self) -> i32 { 1 } }")];
    let matched = compare(&before, &after);
    let methods: Vec<_> = matched
        .nodes
        .iter()
        .filter_map(|node| match node {
            StructureNode::Declaration(node) => match &node.after {
                StructureFact::Present { value } if value.name == "read" => Some(node),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(methods.len(), 4);
    assert_eq!(
        methods
            .iter()
            .filter(|node| !node.changes.is_empty())
            .count(),
        1
    );
    assert!(named(&matched, "Item").changes.is_empty());
}

/// impl 头部自身改变可修改聚合类型，成员正文仍不参与此比较。
#[test]
fn implementation_header_and_documentation_belong_to_the_type() {
    let before = [extracted(
        "src/lib.rs",
        "struct Item<T>(T);\n/// before\nimpl<T> Item<T> { fn read(&self) {} }",
    )];
    let after = [extracted(
        "src/lib.rs",
        "struct Item<T>(T);\n/// after\nimpl<T: Clone> Item<T> { fn read(&self) {} }",
    )];
    let matched = compare(&before, &after);
    let parent = named(&matched, "Item");
    assert!(parent.changes.contains(&StructureChangeLabel::Modified));
    assert!(parent
        .changes
        .contains(&StructureChangeLabel::Documentation));
    assert!(!named(&matched, "read")
        .changes
        .contains(&StructureChangeLabel::Modified));
}

/// 未分析到的实现不是被删除的实现，不能据此给仍可识别的类型加修改标签。
#[test]
fn damaged_implementation_does_not_modify_the_aggregate_type() {
    let before = [extracted(
        "src/lib.rs",
        "struct Item; impl Item { fn read(&self) {} }",
    )];
    let after = [extracted(
        "src/lib.rs",
        "struct Item; impl Item { fn broken(",
    )];
    let matched = compare(&before, &after);
    assert!(named(&matched, "Item").changes.is_empty());
    assert_eq!(
        named(&matched, "Item").comparison,
        StructureComparison::Incomplete
    );
    assert!(!named(&matched, "read")
        .changes
        .contains(&StructureChangeLabel::Deleted));
}

/// 缺少导入的 impl 不得绑到另一模块的同名类型，也不能因无法绑定就报方法增删。
#[test]
fn unbound_implementation_owner_does_not_use_global_basename() {
    let before = [
        extracted("src/lib.rs", "mod model; mod implementation;"),
        extracted("src/model.rs", "pub struct Item;"),
        extracted("src/implementation.rs", "impl Item { fn read(&self) {} }"),
    ];
    let after = before.clone();
    let matched = compare(&before, &after);
    let parent = named(&matched, "Item");
    for node in &matched.nodes {
        let StructureNode::Declaration(node) = node else {
            continue;
        };
        let part = match &node.after {
            StructureFact::Present { value } => value,
            _ => continue,
        };
        if part.name == "read" {
            assert_ne!(part.parent_id, parent.id);
            assert!(node.changes.is_empty());
            assert_eq!(node.comparison, StructureComparison::Incomplete);
        }
    }
}

/// 真实 rename 到不支持语言时，文件仍存在这一事实阻止声明删除推断。
#[test]
fn renamed_file_with_unsupported_after_content_preserves_unknown() {
    let before = [extracted("read.ts", "function read() { return 1; }")];
    let sources = [StructurePeekSource {
        id: StructureSourceId("source:read.md".into()),
        before: StructureSourceSide::Present {
            path: "read.ts".into(),
            revision: "old".into(),
        },
        after: StructureSourceSide::Present {
            path: "read.md".into(),
            revision: "new".into(),
        },
        classification: StructureSourceClassification::FileChange {
            file_change: StructureFileChangeSummary {
                path: "read.md".into(),
                old_path: Some("read.ts".into()),
                status: StructureFileChangeStatus::Renamed,
            },
        },
    }];
    let coverage = StructureCoverage {
        discovery: StructureDiscoveryCoverage {
            before: StructureCoverageStatus::Complete,
            after: StructureCoverageStatus::Complete,
        },
        files: vec![StructureFileCoverage {
            side: StructureSide::After,
            path: "read.md".into(),
            extraction: StructureCoverageStatus::Partial,
            relationships: StructureCoverageStatus::NotStarted,
            diagnostics: vec![],
        }],
        diagnostics: vec![],
    };
    let matched = compare_declarations(
        &before,
        &[],
        &sources,
        &coverage,
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    assert!(named(&matched, "read").changes.is_empty());
    assert!(matches!(
        named(&matched, "read").after,
        StructureFact::Unknown { .. }
    ));
}

/// 方法文档属于方法，不属于包围它的 impl/type 文档片段。
#[test]
fn member_documentation_does_not_duplicate_parent_documentation_change() {
    let before = [extracted(
        "src/lib.rs",
        "struct Item; impl Item {\n/// old method\nfn read(&self) {} }",
    )];
    let after = [extracted(
        "src/lib.rs",
        "struct Item; impl Item {\n/// new method\nfn read(&self) {} }",
    )];
    let matched = compare(&before, &after);
    assert_eq!(
        named(&matched, "read").changes,
        [StructureChangeLabel::Documentation]
    );
    assert!(named(&matched, "Item").changes.is_empty());
    assert_eq!(named(&matched, "Item").contained_changes.union, 1);
}

/// Rust raw string 内部空白是实际正文内容，不能当作格式删除。
#[test]
fn rust_raw_string_whitespace_is_a_body_change() {
    let before = [extracted(
        "src/lib.rs",
        "fn read() -> &'static str { r#\"a b\"# }",
    )];
    let after = [extracted(
        "src/lib.rs",
        "fn read() -> &'static str { r#\"ab\"# }",
    )];
    assert_eq!(
        named(&compare(&before, &after), "read").changes,
        [StructureChangeLabel::Modified]
    );
}

/// 同一名义类型的不同泛型特化 impl 不能把同名同正文成员互相配错。
#[test]
fn generic_implementation_headers_disambiguate_identical_members() {
    let before = [extracted("src/lib.rs", "struct Item<T>(T); impl Item<u8> { fn read(&self) -> i32 { 1 } } impl Item<u16> { fn read(&self) -> i32 { 1 } }")];
    let after = [extracted("src/lib.rs", "struct Item<T>(T); impl Item<u16> { fn read(&self) -> i32 { 1 } } impl Item<u8> { fn read(&self) -> i32 { 1 } }")];
    let matched = compare(&before, &after);
    let methods: Vec<_> = matched
        .nodes
        .iter()
        .filter_map(|node| match node {
            StructureNode::Declaration(node) => match &node.after {
                StructureFact::Present { value } if value.name == "read" => Some(node),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(methods.len(), 2);
    assert!(methods.iter().all(
        |node| node.changes.is_empty() && matches!(node.before, StructureFact::Present { .. })
    ));
}

/// 同名 interface 声明各自的成员签名可区分连续身份，不把未修改重声明变成增删。
#[test]
fn same_named_interface_fragments_keep_their_distinct_members() {
    let before = [extracted(
        "declarations.ts",
        "interface Options { first: string } interface Options { second: number }",
    )];
    let after = [extracted(
        "declarations.ts",
        "interface Options { second: number } interface Options { first: string }",
    )];
    let matched = compare(&before, &after);
    assert!(matched
        .nodes
        .iter()
        .filter_map(|node| match node {
            StructureNode::Declaration(node) => Some(node),
            _ => None,
        })
        .all(|node| node.changes.is_empty()));
}

/// 精确未修改来源可证明相同位置的重复声明连续性，不制造上下文中的假变化。
#[test]
fn unchanged_source_keeps_duplicate_identical_declarations_as_context() {
    let before = [extracted(
        "same.ts",
        "function same() { return 1; } function same() { return 1; }",
    )];
    let after = before.clone();
    let sources = [StructurePeekSource {
        id: StructureSourceId("source:same.ts".into()),
        before: StructureSourceSide::Present {
            path: "same.ts".into(),
            revision: "old".into(),
        },
        after: StructureSourceSide::Present {
            path: "same.ts".into(),
            revision: "new".into(),
        },
        classification: StructureSourceClassification::Unchanged {
            path: "same.ts".into(),
        },
    }];
    let coverage = StructureCoverage {
        discovery: StructureDiscoveryCoverage {
            before: StructureCoverageStatus::Complete,
            after: StructureCoverageStatus::Complete,
        },
        files: vec![],
        diagnostics: vec![],
    };
    let matched = compare_declarations(
        &before,
        &after,
        &sources,
        &coverage,
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let declarations: Vec<_> = matched
        .nodes
        .iter()
        .filter_map(|node| match node {
            StructureNode::Declaration(node) => Some(node),
            _ => None,
        })
        .collect();
    assert_eq!(declarations.len(), 2);
    assert!(declarations.iter().all(|node| node.changes.is_empty()
        && matches!(node.before, StructureFact::Present { .. })
        && matches!(node.after, StructureFact::Present { .. })));
}
