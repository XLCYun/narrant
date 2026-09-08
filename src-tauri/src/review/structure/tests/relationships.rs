use super::super::matching_model::MatchedSide;
use super::*;
use crate::review::structure::{extract_source, parse_source, SourceLanguage};

/// 输入通过真实 parser/extractor，匹配身份作为本阶段已确认契约固定。
fn fixture(text: &str) -> ExtractedFile {
    extract_source(
        "src/main.ts",
        &parse_source(SourceLanguage::TypeScript, text).unwrap(),
    )
}

/// 关系阶段只消费已匹配 ID，测试按声明名字给出明确的连续身份。
fn matched(before: &[ExtractedFile], after: &[ExtractedFile]) -> MatchedStructure {
    /// 为单侧真实声明分配明确身份，保持本阶段测试只检查关系行为。
    fn side(files: &[ExtractedFile]) -> MatchedSide {
        let mut side = MatchedSide::default();
        for file in files {
            side.source_ids
                .insert(file.path.clone(), StructureSourceId(file.path.clone()));
            for (index, declaration) in file.declarations.iter().enumerate() {
                side.declarations.insert(
                    (file.path.clone(), index),
                    StructureNodeId(format!("{}:{}", file.path, declaration.qualified_name)),
                );
            }
        }
        side
    }
    MatchedStructure {
        before: side(before),
        after: side(after),
        ..MatchedStructure::default()
    }
}

/// 语法覆盖单独提供，不能被实现直接抄为语义覆盖。
fn coverage(before: &[ExtractedFile], after: &[ExtractedFile]) -> StructureCoverage {
    StructureCoverage {
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
    }
}

/// 行号和调用次数变化只更新发生位置，不能制造关系增删。
#[test]
fn repeated_calls_and_line_movement_retain_one_edge() {
    let before = vec![fixture(
        "function target() {} function caller() { target(); target(); }",
    )];
    let after = vec![fixture(
        "function target() {}\n\nfunction caller() { target(); }",
    )];
    let result = resolve_relationships(
        &before,
        &after,
        &matched(&before, &after),
        &coverage(&before, &after),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let calls: Vec<_> = result
        .relationships
        .iter()
        .filter(|edge| edge.kind == StructureRelationshipKind::Calls)
        .collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].change, StructureRelationshipChange::Retained);
    assert!(matches!(&calls[0].before, StructureFact::Present { value } if value.len() == 2));
    assert!(matches!(&calls[0].after, StructureFact::Present { value } if value.len() == 1));
}

/// 语法完整但动态目标未知时，旧侧确定调用不能被误标为删除。
#[test]
fn resolved_to_unknown_is_indeterminate_even_with_complete_parse() {
    let before = vec![fixture(
        "function target() {} function caller() { target(); }",
    )];
    let after = vec![fixture(
        "function target() {} function caller() { dynamic.target(); }",
    )];
    let result = resolve_relationships(
        &before,
        &after,
        &matched(&before, &after),
        &coverage(&before, &after),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let edge = result
        .relationships
        .iter()
        .find(|edge| edge.kind == StructureRelationshipKind::Calls)
        .unwrap();
    assert_eq!(edge.change, StructureRelationshipChange::Indeterminate);
    assert!(
        matches!(&edge.after, StructureFact::Unknown { diagnostics } if !diagnostics.is_empty())
    );
    assert!(result
        .file_coverage
        .iter()
        .any(|file| file.side == StructureSide::After
            && file.extraction == StructureCoverageStatus::Complete
            && file.relationships == StructureCoverageStatus::Partial));
}

/// 来源内确定的调用消失时可标记删除；其他声明里的未知调用不污染它的证明。
#[test]
fn genuine_removal_survives_unrelated_dynamic_call() {
    let before = vec![fixture(
        "function target() {} function caller() { target(); } function other() { unknown(); }",
    )];
    let after = vec![fixture(
        "function target() {} function caller() {} function other() { unknown(); }",
    )];
    let result = resolve_relationships(
        &before,
        &after,
        &matched(&before, &after),
        &coverage(&before, &after),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let edge = result
        .relationships
        .iter()
        .find(|edge| edge.kind == StructureRelationshipKind::Calls)
        .unwrap();
    assert_eq!(edge.change, StructureRelationshipChange::Deleted);
    assert!(matches!(edge.after, StructureFact::Absent));
}

/// 匿名回调的发生点保留回调执行范围，不声称在外层正文直接执行。
#[test]
fn anonymous_callback_preserves_execution_provenance() {
    let files = vec![fixture(
        "function target() {} function caller() { [1].map(() => target()); }",
    )];
    let result = resolve_relationships(
        &files,
        &files,
        &matched(&files, &files),
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let edge = result
        .relationships
        .iter()
        .find(|edge| edge.to_id.0.ends_with(":target"))
        .unwrap();
    assert!(
        matches!(&edge.after, StructureFact::Present { value } if matches!(value[0].execution, StructureExecution::AnonymousCallback { .. }))
    );
}

/// 显式接收者的 this 方法和类型限定 static 方法均指向声明节点。
#[test]
fn this_and_static_methods_resolve_without_global_basename_search() {
    let files = vec![fixture(
        "class Service { save() {} run() { this.save(); Service.create(); } static create() {} }",
    )];
    let result = resolve_relationships(
        &files,
        &files,
        &matched(&files, &files),
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let calls: Vec<_> = result
        .relationships
        .iter()
        .filter(|edge| edge.kind == StructureRelationshipKind::Calls)
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(calls
        .iter()
        .all(|edge| edge.certainty == StructureCertainty::Resolved
            && edge.change == StructureRelationshipChange::Retained));
}

/// 新旧配置独立解析，同一源码调用随 alias 改变产生真实关系变化。
#[test]
fn unchanged_caller_with_changed_alias_has_relationship_only_change() {
    let files = vec![
        extract_source(
            "src/a.ts",
            &parse_source(SourceLanguage::TypeScript, "export function work() {}").unwrap(),
        ),
        extract_source(
            "src/b.ts",
            &parse_source(SourceLanguage::TypeScript, "export function work() {}").unwrap(),
        ),
        fixture("import { work } from '@/work'; function caller() { work(); }"),
    ];
    let before_config = HashMap::from([(
        "tsconfig.json".into(),
        r#"{"compilerOptions":{"paths":{"@/work":["src/a.ts"]}}}"#.into(),
    )]);
    let after_config = HashMap::from([(
        "tsconfig.json".into(),
        r#"{"compilerOptions":{"paths":{"@/work":["src/b.ts"]}}}"#.into(),
    )]);
    let result = resolve_relationships(
        &files,
        &files,
        &matched(&files, &files),
        &coverage(&files, &files),
        &before_config,
        &after_config,
        &|| true,
    );
    let calls: Vec<_> = result
        .relationships
        .iter()
        .filter(|edge| edge.kind == StructureRelationshipKind::Calls)
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().any(|edge| edge.to_id.0 == "src/a.ts:work"
        && edge.change == StructureRelationshipChange::Deleted));
    assert!(calls
        .iter()
        .any(|edge| edge.to_id.0 == "src/b.ts:work"
            && edge.change == StructureRelationshipChange::Added));
}

/// 外部包既有 import 容器边，也有带包身份的符号节点，无虚假本地定义。
#[test]
fn external_package_nodes_are_deduplicated_against_matched_impl_nodes() {
    let files = vec![fixture(
        "import { run } from '@scope/package'; function caller() { run(); }",
    )];
    let mut matched = matched(&files, &files);
    matched
        .nodes
        .push(StructureNode::External(StructureExternalNode {
            id: external_node_id("@scope/package", "run"),
            name: "run".into(),
            package_name: "@scope/package".into(),
            definition_unavailable:
                StructureExternalDefinitionUnavailable::ExternalSourceNotIndexed,
        }));
    let result = resolve_relationships(
        &files,
        &files,
        &matched,
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    assert_eq!(result.external_nodes.len(), 1);
    assert!(result.relationships.iter().any(|edge| edge.to_id
        == external_node_id("@scope/package", "run")
        && edge.kind == StructureRelationshipKind::Calls));
    assert!(result.relationships.iter().any(|edge| edge.from_id
        == file_node_id(&StructureSourceId("src/main.ts".into()))
        && edge.kind == StructureRelationshipKind::Imports));
}

/// 同名重载候选始终保持候选状态，不能制造确定的关系新增。
#[test]
fn overload_candidates_remain_indeterminate() {
    let before = vec![fixture("function caller() {}")];
    let after = vec![fixture("function work(a: string): string; function work(a: number): number; function work(a: unknown) { return a; } function caller() { work(1); }")];
    let mut matched = matched(&before, &after);
    for (key, id) in &mut matched.after.declarations {
        if after[0].declarations[key.1].name == "work" {
            id.0.push_str(&format!(":{}", key.1));
        }
    }
    let result = resolve_relationships(
        &before,
        &after,
        &matched,
        &coverage(&before, &after),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let calls: Vec<_> = result
        .relationships
        .iter()
        .filter(|edge| edge.kind == StructureRelationshipKind::Calls)
        .collect();
    assert_eq!(calls.len(), 3);
    assert!(calls
        .iter()
        .all(|edge| edge.certainty == StructureCertainty::Candidate
            && edge.change == StructureRelationshipChange::Indeterminate));
    assert!(result
        .unresolved_relationships
        .iter()
        .any(|item| item.candidate_target_ids.len() == 3));
}

/// 取消使全部尚未处理文件保持未开始，不能从空结果判断不存在。
#[test]
fn cancellation_does_not_report_complete_relationship_coverage() {
    let files = vec![fixture(
        "function target() {} function caller() { target(); }",
    )];
    let result = resolve_relationships(
        &files,
        &files,
        &matched(&files, &files),
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| false,
    );
    assert!(result.relationships.is_empty());
    assert!(result.file_coverage.iter().all(|file| file.relationships
        == StructureCoverageStatus::NotStarted
        && file
            .diagnostics
            .iter()
            .any(|item| item.reason == StructureDiagnosticReason::Cancelled)));
}

/// 继承、接口实现和字段类型引用各自保留关系种类，并跨显式导入定位。
#[test]
fn inheritance_implementation_and_type_references_use_explicit_imports() {
    let files = vec![extract_source("src/types.ts", &parse_source(SourceLanguage::TypeScript, "export class Base {} export interface Contract {} export class Value {}").unwrap()), fixture("import { Base, Contract, Value } from './types'; class Service extends Base implements Contract { value: Value; }")];
    let result = resolve_relationships(
        &files,
        &files,
        &matched(&files, &files),
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    for (kind, target) in [
        (StructureRelationshipKind::Extends, "src/types.ts:Base"),
        (
            StructureRelationshipKind::Implements,
            "src/types.ts:Contract",
        ),
        (
            StructureRelationshipKind::TypeReference,
            "src/types.ts:Value",
        ),
    ] {
        assert!(result.relationships.iter().any(|edge| edge.kind == kind
            && edge.to_id.0 == target
            && edge.certainty == StructureCertainty::Resolved));
    }
}

/// Rust 跨文件 self 方法与 trait 实现边使用 matched 的真实聚合类型身份。
#[test]
fn rust_cross_file_self_and_trait_implementation_use_type_owner() {
    use super::super::matching_model::MatchedImplementation;
    let files = vec![
        extract_source("src/lib.rs", &parse_source(SourceLanguage::Rust, "mod model; mod worker;").unwrap()),
        extract_source("src/model.rs", &parse_source(SourceLanguage::Rust, "pub struct Service; pub trait Run { fn run(&self); }").unwrap()),
        extract_source("src/worker.rs", &parse_source(SourceLanguage::Rust, "use crate::model::{Service, Run}; impl Service { fn save(&self) {} fn call(&self) { self.save(); } } impl Run for Service { fn run(&self) {} }").unwrap()),
    ];
    let mut matched = matched(&files, &files);
    let owner = StructureNodeId("src/model.rs:Service".into());
    for side in [&mut matched.before, &mut matched.after] {
        for index in 0..files[2].impl_groups.len() {
            side.implementations.insert(
                ("src/worker.rs".into(), index),
                MatchedImplementation {
                    group_id: StructureGroupId(format!("group:{index}")),
                    owner_id: Some(owner.clone()),
                    trait_target_id: files[2].impl_groups[index]
                        .trait_name
                        .as_ref()
                        .map(|_| StructureNodeId("src/model.rs:Run".into())),
                },
            );
        }
    }
    let result = resolve_relationships(
        &files,
        &files,
        &matched,
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    assert!(result
        .relationships
        .iter()
        .any(|edge| edge.kind == StructureRelationshipKind::Calls
            && edge.to_id.0.ends_with("save")
            && edge.certainty == StructureCertainty::Resolved));
    let implementation = result
        .relationships
        .iter()
        .find(|edge| edge.kind == StructureRelationshipKind::Implements)
        .unwrap();
    assert_eq!(implementation.from_id, owner);
    assert_eq!(implementation.to_id.0, "src/model.rs:Run");
    let StructureFact::Present { value } = &implementation.after else {
        panic!("trait implementation must remain present: {implementation:?}");
    };
    assert_eq!(value.len(), 1, "one trait syntax occurrence: {value:?}");
    assert_eq!(value[0].location.source_id.0, "src/worker.rs");
    let range = &value[0].location.range;
    assert_eq!(range.start_line, 1);
    assert_eq!(range.end_line, 1);
    assert_eq!(
        range.end_column - range.start_column,
        3,
        "the occurrence locates Run itself, not the whole implementation block"
    );
}

/// 发现未结束时，即便某个文件完整，也不能断言整个聚合来源没有关系。
#[test]
fn partial_repository_discovery_cannot_prove_relation_absence() {
    let before = vec![fixture(
        "function target() {} function caller() { target(); }",
    )];
    let after = vec![fixture("function target() {} function caller() {}")];
    let mut coverage = coverage(&before, &after);
    coverage.discovery.after = StructureCoverageStatus::Partial;
    let result = resolve_relationships(
        &before,
        &after,
        &matched(&before, &after),
        &coverage,
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    assert!(result
        .relationships
        .iter()
        .filter(|edge| edge.kind == StructureRelationshipKind::Calls)
        .all(|edge| edge.change == StructureRelationshipChange::Indeterminate));
}

/// Git 确认新增文件时，其旧侧文件依赖才可标记不存在。
#[test]
fn complete_presence_catalog_proves_added_file_import_edge() {
    let before = Vec::new();
    let after = vec![fixture("import { run } from 'package';")];
    let mut matched = matched(&before, &after);
    matched.nodes.push(StructureNode::File(StructureFileNode {
        id: file_node_id(&StructureSourceId("src/main.ts".into())),
        source_id: StructureSourceId("src/main.ts".into()),
        contained_changes: StructureContainedChanges {
            before: 0,
            after: 0,
            union: 0,
        },
    }));
    let result = resolve_relationships(
        &before,
        &after,
        &matched,
        &coverage(&before, &after),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    assert_eq!(result.relationships.len(), 1);
    assert_eq!(
        result.relationships[0].change,
        StructureRelationshipChange::Added
    );
}

/// 目标绑定和证据说明都必须遵守遮蔽，不能仅因文件里有同名导入就写 ExplicitImport。
#[test]
fn local_shadowing_uses_lexical_evidence_instead_of_outer_import() {
    let files = vec![fixture(
        "import { target } from 'package'; function caller() { function target() {} target(); }",
    )];
    let result = resolve_relationships(
        &files,
        &files,
        &matched(&files, &files),
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let edge = result
        .relationships
        .iter()
        .find(|edge| edge.kind == StructureRelationshipKind::Calls)
        .unwrap();
    assert!(edge.to_id.0.ends_with("caller::target"));
    assert!(
        matches!(&edge.after, StructureFact::Present { value } if value[0].evidence[0].kind == StructureEvidenceKind::LexicalBinding)
    );
}

/// 匿名箭头继承方法的 this，同时保留独立 callback 发生范围。
#[test]
fn arrow_callback_preserves_lexical_this_method_target() {
    let files = vec![fixture(
        "class C { m() { return [1].map(() => this.other()); } other() {} }",
    )];
    let result = resolve_relationships(
        &files,
        &files,
        &matched(&files, &files),
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let edge = result
        .relationships
        .iter()
        .find(|edge| {
            edge.kind == StructureRelationshipKind::Calls && edge.to_id.0.ends_with("C::other")
        })
        .unwrap();
    assert_eq!(edge.certainty, StructureCertainty::Resolved);
    assert!(
        matches!(&edge.after, StructureFact::Present { value } if matches!(value[0].execution, StructureExecution::AnonymousCallback { .. }) && value[0].evidence[0].kind == StructureEvidenceKind::KnownReceiver)
    );
}

/// 普通回调改变 this；其中再嵌套箭头仍不能回到 class 的 this。
#[test]
fn ordinary_function_callback_and_nested_arrow_do_not_inherit_class_this() {
    for text in ["class C { m() { return [1].map(function () { this.other(); }); } other() {} }", "class C { m() { return [1].map(function () { return () => this.other(); }); } other() {} }"] {
        let files = vec![fixture(text)];
        let result = resolve_relationships(&files, &files, &matched(&files, &files), &coverage(&files, &files), &HashMap::new(), &HashMap::new(), &|| true);
        assert!(!result.relationships.iter().any(|edge| edge.kind == StructureRelationshipKind::Calls && edge.to_id.0.ends_with("C::other")));
        assert!(result.unresolved_relationships.iter().any(|item| item.written_target == "this.other"));
    }
}

/// 宏未展开时不能确认旧调用消失，语法完整也不代表宏内部关系已覆盖。
#[test]
fn replacing_a_call_with_a_macro_does_not_prove_deletion() {
    let before = vec![extract_source(
        "src/lib.rs",
        &parse_source(
            SourceLanguage::Rust,
            "fn target() {} fn caller() { target(); }",
        )
        .unwrap(),
    )];
    let after = vec![extract_source(
        "src/lib.rs",
        &parse_source(
            SourceLanguage::Rust,
            "fn target() {} fn caller() { maybe_call!(); }",
        )
        .unwrap(),
    )];
    let result = resolve_relationships(
        &before,
        &after,
        &matched(&before, &after),
        &coverage(&before, &after),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let edge = result
        .relationships
        .iter()
        .find(|edge| edge.kind == StructureRelationshipKind::Calls)
        .unwrap();
    assert_eq!(edge.change, StructureRelationshipChange::Indeterminate);
    assert!(result
        .unresolved_relationships
        .iter()
        .any(|item| item.side == StructureSide::After && item.written_target.contains('!')));
}

/// 动态 import 的模块依赖仍未知，不能将旧静态依赖误标为删除。
#[test]
fn dynamic_import_keeps_old_module_dependency_indeterminate() {
    let before = vec![fixture("import { run } from 'package';")];
    let after = vec![fixture("function caller() { return import('package'); }")];
    let result = resolve_relationships(
        &before,
        &after,
        &matched(&before, &after),
        &coverage(&before, &after),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let edge = result
        .relationships
        .iter()
        .find(|edge| edge.kind == StructureRelationshipKind::Imports)
        .unwrap();
    assert_eq!(edge.change, StructureRelationshipChange::Indeterminate);
    assert!(result
        .unresolved_relationships
        .iter()
        .any(|item| item.side == StructureSide::After
            && item.kind == StructureRelationshipKind::Imports));
}

/// impl 的归属从确定变未知时，不能将旧类型到 trait 的边误标为删除或替换为文件实现 trait。
#[test]
fn unknown_implementation_owner_does_not_fake_removed_implementation() {
    use super::super::matching_model::MatchedImplementation;
    let before = vec![extract_source(
        "src/lib.rs",
        &parse_source(
            SourceLanguage::Rust,
            "struct Item; trait Run {} impl Run for Item {}",
        )
        .unwrap(),
    )];
    let after = vec![extract_source(
        "src/lib.rs",
        &parse_source(
            SourceLanguage::Rust,
            "use missing::Item as Alias; struct Item; trait Run {} impl Run for Alias {}",
        )
        .unwrap(),
    )];
    let mut matched = matched(&before, &after);
    let owner = StructureNodeId("src/lib.rs:Item".into());
    matched.before.implementations.insert(
        ("src/lib.rs".into(), 0),
        MatchedImplementation {
            group_id: StructureGroupId("impl".into()),
            owner_id: Some(owner.clone()),
            trait_target_id: Some(StructureNodeId("src/lib.rs:Run".into())),
        },
    );
    matched.after.implementations.insert(
        ("src/lib.rs".into(), 0),
        MatchedImplementation {
            group_id: StructureGroupId("impl".into()),
            owner_id: None,
            trait_target_id: Some(StructureNodeId("src/lib.rs:Run".into())),
        },
    );
    let result = resolve_relationships(
        &before,
        &after,
        &matched,
        &coverage(&before, &after),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let edges: Vec<_> = result
        .relationships
        .iter()
        .filter(|edge| edge.kind == StructureRelationshipKind::Implements)
        .collect();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].from_id, owner);
    assert_eq!(edges[0].change, StructureRelationshipChange::Indeterminate);
    assert!(
        matches!(&edges[0].after, StructureFact::Unknown { diagnostics } if !diagnostics.is_empty())
    );
}

/// 类构造器this与实例this属于不同成员空间，静态与实例同名时按接收者选择。
#[test]
fn static_and_instance_this_do_not_cross_member_namespaces() {
    let files = vec![fixture("class C { other() {} static create() {} static run() { this.other(); this.create(); } m() { this.create(); this.other(); } }")];
    let result = resolve_relationships(
        &files,
        &files,
        &matched(&files, &files),
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let calls: Vec<_> = result
        .relationships
        .iter()
        .filter(|edge| edge.kind == StructureRelationshipKind::Calls)
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(calls
        .iter()
        .any(|edge| edge.from_id.0.ends_with("C::run") && edge.to_id.0.ends_with("C::create")));
    assert!(calls
        .iter()
        .any(|edge| edge.from_id.0.ends_with("C::m") && edge.to_id.0.ends_with("C::other")));
}

/// 重绑定使原确定调用变为未知，不能继续保留旧边或据此标记删除。
#[test]
fn assignment_changes_a_known_callee_to_an_unknown_relationship() {
    let before = vec![fixture(
        "function a() {} function b() {} function run() { a(); }",
    )];
    let after = vec![fixture(
        "function a() {} function b() {} function run() { a = b; a(); }",
    )];
    let result = resolve_relationships(
        &before,
        &after,
        &matched(&before, &after),
        &coverage(&before, &after),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    let edge = result
        .relationships
        .iter()
        .find(|edge| edge.kind == StructureRelationshipKind::Calls)
        .unwrap();
    assert_eq!(edge.change, StructureRelationshipChange::Indeterminate);
    assert!(
        matches!(&edge.after, StructureFact::Unknown { diagnostics } if !diagnostics.is_empty())
    );
    assert!(result
        .unresolved_relationships
        .iter()
        .any(|item| item.side == StructureSide::After && item.written_target == "a"));
}

/// 具名函数表达式在回调内可递归，但内部名称不会泄漏到外层调用。
#[test]
fn named_function_expression_recurses_without_leaking_inner_name() {
    let files = vec![fixture("function queue(cb: unknown) {} function outer() { queue(function retry() { retry(); }); retry(); }")];
    let result = resolve_relationships(
        &files,
        &files,
        &matched(&files, &files),
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    assert!(result
        .relationships
        .iter()
        .any(|edge| edge.kind == StructureRelationshipKind::Calls
            && edge.from_id == edge.to_id
            && edge.from_id.0.ends_with("outer::retry")
            && edge.certainty == StructureCertainty::Resolved));
    assert!(!result
        .relationships
        .iter()
        .any(|edge| edge.kind == StructureRelationshipKind::Calls
            && edge.from_id.0.ends_with(":outer")
            && edge.to_id.0.ends_with("outer::retry")));
    assert!(result
        .unresolved_relationships
        .iter()
        .any(|item| item.written_target == "retry" && item.from_id.0.ends_with(":outer")));
}

/// JS实例方法被this属性写入替换后，调用不能继续确定指向原声明。
#[test]
fn writing_this_method_invalidates_its_original_call_target() {
    let before = vec![fixture(
        "function other() {} class C { m() {} run() { this.m(); } }",
    )];
    for text in ["function other() {} class C { m() {} run() { this.m = other; this.m(); } }", "function other() {} class C { m() {} run() { const replace = () => { this.m = other; }; replace(); this.m(); } }"] {
        let after = vec![fixture(text)];
        let result = resolve_relationships(&before, &after, &matched(&before, &after), &coverage(&before, &after), &HashMap::new(), &HashMap::new(), &|| true);
        let edge = result.relationships.iter().find(|edge| edge.to_id.0.ends_with("C::m")).unwrap();
        assert_eq!(edge.change, StructureRelationshipChange::Indeterminate);
        assert!(matches!(&edge.after, StructureFact::Unknown { .. }));
    }
}

/// 命名字段写入不会污染无关方法；另一个类的同名成员也独立。
#[test]
fn unrelated_this_field_and_other_class_keep_clear_method_calls() {
    let files = vec![fixture("function other() {} class C { state = 0; m() {} run() { this.state = 1; this.m(); } } class D { m() {} run() { this.m = other; this.m(); } }")];
    let result = resolve_relationships(
        &files,
        &files,
        &matched(&files, &files),
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    assert!(result.relationships.iter().any(
        |edge| edge.to_id.0.ends_with("C::m") && edge.certainty == StructureCertainty::Resolved
    ));
    assert!(!result.relationships.iter().any(
        |edge| edge.to_id.0.ends_with("D::m") && edge.certainty == StructureCertainty::Resolved
    ));
    assert!(result
        .unresolved_relationships
        .iter()
        .any(|item| item.written_target == "this.m" && item.from_id.0.ends_with("D::run")));
}

/// 未知计算属性可能改写任意同实例方法，不能猜它只改了state。
#[test]
fn computed_this_write_invalidates_unknown_member_in_same_namespace() {
    let files = vec![fixture(
        "class C { m() {} run(key: string, other: unknown) { this[key] = other; this.m(); } }",
    )];
    let result = resolve_relationships(
        &files,
        &files,
        &matched(&files, &files),
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    assert!(!result
        .relationships
        .iter()
        .any(|edge| edge.to_id.0.ends_with("C::m")));
    assert!(result
        .unresolved_relationships
        .iter()
        .any(|item| item.written_target == "this.m"));
}

/// this写入仅影响对应static/instance空间；普通function动态this不污染词法类。
#[test]
fn this_write_respects_static_instance_and_dynamic_function_boundaries() {
    let files = vec![fixture("function other() {} class C { m() {} static m() {} static replace() { this.m = other; this.m(); } run() { function detached() { this.m = other; } this.m(); } }")];
    let mut matched = matched(&files, &files);
    for side in [&mut matched.before, &mut matched.after] {
        for (key, id) in &mut side.declarations {
            if files[0].declarations[key.1].name == "m"
                && files[0].declarations[key.1].signature.starts_with("static")
            {
                id.0.push_str(":static");
            }
        }
    }
    let result = resolve_relationships(
        &files,
        &files,
        &matched,
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    assert!(result
        .relationships
        .iter()
        .any(|edge| edge.from_id.0.ends_with("C::run")
            && edge.to_id.0.ends_with("C::m")
            && edge.certainty == StructureCertainty::Resolved));
    assert!(!result.relationships.iter().any(|edge| {
        edge.to_id.0.ends_with("C::m:static") && edge.certainty == StructureCertainty::Resolved
    }));
    assert!(result
        .unresolved_relationships
        .iter()
        .any(|item| { item.written_target == "this.m" && item.from_id.0.ends_with("C::replace") }));
}

/// Rust self字段赋值不改变名义类型的方法派发，仍解析到固有方法。
#[test]
fn rust_field_write_keeps_nominal_self_method_resolved() {
    use super::super::matching_model::MatchedImplementation;
    let files = vec![extract_source("src/lib.rs", &parse_source(SourceLanguage::Rust, "struct C { state: i32 } impl C { fn m(&self) {} fn run(&mut self) { self.state = 1; self.m(); } }").unwrap())];
    let mut matched = matched(&files, &files);
    for side in [&mut matched.before, &mut matched.after] {
        side.implementations.insert(
            ("src/lib.rs".into(), 0),
            MatchedImplementation {
                group_id: StructureGroupId("impl".into()),
                owner_id: Some(StructureNodeId("src/lib.rs:C".into())),
                trait_target_id: None,
            },
        );
    }
    let result = resolve_relationships(
        &files,
        &files,
        &matched,
        &coverage(&files, &files),
        &HashMap::new(),
        &HashMap::new(),
        &|| true,
    );
    assert!(result
        .relationships
        .iter()
        .any(|edge| edge.kind == StructureRelationshipKind::Calls
            && edge.to_id.0.ends_with("m")
            && edge.certainty == StructureCertainty::Resolved));
}
