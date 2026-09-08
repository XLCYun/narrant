use std::path::Path;
use std::process::Command;

use crate::operation::{OperationContext, TestCancellation};
use crate::review::{build_comparison, ResolvedRevisions, SnapshotContentReader};

use super::*;

/// 完整分析从固定 Snapshot 读取未变更上下文，并返回可用于原文件 Peek 的来源。
#[test]
fn analysis_includes_unchanged_callers_and_keeps_snapshot_identity() {
    let repository = repository();
    write(
        repository.path(),
        "caller.ts",
        "import { value } from './value';\nexport function caller() { return value(); }\n",
    );
    write(
        repository.path(),
        "value.ts",
        "export function value() { return 1; }\n",
    );
    let before = commit(repository.path(), "base");
    write(
        repository.path(),
        "value.ts",
        "export function value() { return 2; }\n",
    );
    let after = commit(repository.path(), "head");
    let reader = reader(repository.path(), &before, &after);
    // 未提交原文不能影响分析结果或所绑定的源码版本。
    write(
        repository.path(),
        "value.ts",
        "this checkout is deliberately invalid",
    );
    let mut progress = Vec::new();
    let result = analyze_structure(
        &reader,
        &OperationContext::background(),
        "review",
        "snapshot",
        || true,
        |event| progress.push(event),
    )
    .unwrap();
    assert_eq!(result.review_id, "review");
    assert_eq!(result.snapshot_id, "snapshot");
    assert_eq!(result.status, StructureAnalysisStatus::Completed);
    let declarations: Vec<_> = result
        .nodes
        .iter()
        .filter_map(|node| match node {
            StructureNode::Declaration(declaration) => Some(declaration),
            _ => None,
        })
        .collect();
    let named = |name: &str| {
        declarations.iter().find(|declaration| matches!(&declaration.after, StructureFact::Present { value } if value.name == name)).copied().unwrap()
    };
    let value = named("value");
    let caller = named("caller");
    assert!(value.changes.contains(&StructureChangeLabel::Modified));
    assert!(caller.changes.is_empty());
    assert!(result
        .relationships
        .iter()
        .any(|relation| relation.from_id == caller.id
            && relation.to_id == value.id
            && relation.kind == StructureRelationshipKind::Calls));
    assert!(result.initial_focus.node_ids.contains(&caller.id));
    assert!(result.sources.iter().any(|source| matches!(&source.classification, StructureSourceClassification::Unchanged { path } if path == "caller.ts")));
    assert!(progress.iter().any(|event| matches!(event, StructureEvent::Progress { progress } if progress.phase == StructurePhase::Extraction)));
    assert!(progress.iter().any(|event| matches!(event, StructureEvent::Checkpoint { result } if result.status == StructureAnalysisStatus::Completed)));
}

/// 用户取消保留明确的终态和覆盖缺口，不把尚未解析的文件当作删除。
#[test]
fn cancelled_analysis_never_turns_unprocessed_sources_into_deletions() {
    let repository = repository();
    write(
        repository.path(),
        "first.ts",
        "export function first() { return 1; }\n",
    );
    write(
        repository.path(),
        "second.ts",
        "export function second() { return 2; }\n",
    );
    let revision = commit(repository.path(), "sources");
    let reader = reader(repository.path(), &revision, &revision);
    let cancellation = TestCancellation::default();
    let operation = OperationContext::from_test_cancellation(&cancellation);
    let result = analyze_structure(&reader, &operation, "review", "snapshot", || true, |event| {
        if matches!(event, StructureEvent::Progress { progress } if progress.phase == StructurePhase::Extraction && progress.processed_files == 1) { cancellation.cancel(); }
    }).unwrap();
    assert_eq!(result.status, StructureAnalysisStatus::Cancelled);
    assert!(!result.summary.complete);
    assert!(result.nodes.iter().all(|node| match node {
        StructureNode::Declaration(value) =>
            !value.changes.contains(&StructureChangeLabel::Deleted)
                && !value.changes.contains(&StructureChangeLabel::Added),
        _ => true,
    }));
    assert!(result
        .coverage
        .files
        .iter()
        .any(|file| file.extraction != StructureCoverageStatus::Complete));
}

/// Review 已失效时不应再发布进度或检查点，即使对象仍然可读。
#[test]
fn invalidated_runtime_does_not_publish_analysis_events() {
    let repository = repository();
    write(repository.path(), "file.ts", "export const value = 1;\n");
    let revision = commit(repository.path(), "source");
    let reader = reader(repository.path(), &revision, &revision);
    let mut events = Vec::new();
    let result = analyze_structure(
        &reader,
        &OperationContext::background(),
        "review",
        "snapshot",
        || false,
        |event| events.push(event),
    );
    assert!(result.unwrap_err().is_cancelled());
    assert!(events.is_empty());
}

/// 父声明自身和子方法同时变化时，各声明只计一次，文档标签也不重复计数。
#[test]
fn parent_and_child_changes_are_counted_once_per_declaration() {
    let repository = repository();
    write(
        repository.path(),
        "box.ts",
        "/** Original box. */\nexport class Box<T> {\n  value() { return 1; }\n}\n",
    );
    let before = commit(repository.path(), "base");
    write(
        repository.path(),
        "box.ts",
        "/** Updated box. */\nexport class Box<T = string> {\n  value() { return 2; }\n}\n",
    );
    let after = commit(repository.path(), "head");
    let result = analyze_structure(
        &reader(repository.path(), &before, &after),
        &OperationContext::background(),
        "review",
        "snapshot",
        || true,
        |_| {},
    )
    .unwrap();
    let declaration = |name: &str| {
        result.nodes.iter().find_map(|node| match node {
            StructureNode::Declaration(node)
                if matches!(&node.after, StructureFact::Present { value } if value.name == name) =>
            {
                Some(node)
            }
            _ => None,
        }).unwrap()
    };
    let parent = declaration("Box");
    let child = declaration("value");
    assert!(parent.changes.contains(&StructureChangeLabel::Modified));
    assert!(parent
        .changes
        .contains(&StructureChangeLabel::Documentation));
    assert!(child.changes.contains(&StructureChangeLabel::Modified));
    assert_eq!(parent.contained_changes.union, 1);
    assert_eq!(result.summary.changed_declarations, 2);
}

/// TSX 命名箭头函数保留原生种类，未修改调用方使用两侧各自的路径别名解析真实调用。
#[test]
fn tsx_arrow_alias_calls_follow_each_fixed_snapshot_configuration() {
    let repository = repository();
    let panel_text = "import { value as getValue } from '@/value';\nexport const Panel = () => <div>{getValue()}</div>;\n";
    write(repository.path(), "src/Panel.tsx", panel_text);
    for (path, text) in [
        (
            "src/legacy/value.ts",
            "export function value() { return 'before'; }\n",
        ),
        (
            "src/current/value.ts",
            "export function value() { return 'after'; }\n",
        ),
    ] {
        write(repository.path(), path, text);
    }
    write(
        repository.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/legacy/*"]},"jsx":"react-jsx"}}"#,
    );
    let before = commit(repository.path(), "legacy alias");
    write(
        repository.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/current/*"]},"jsx":"react-jsx"}}"#,
    );
    let after = commit(repository.path(), "current alias");
    let reader = reader(repository.path(), &before, &after);
    // HEAD 再次推进也不能改变已捕获的源码和配置。
    write(
        repository.path(),
        "src/Panel.tsx",
        "export const unrelated = 0;\n",
    );
    write(repository.path(), "tsconfig.json", "{}");
    commit(repository.path(), "later checkout");
    let operation = OperationContext::background();
    let result = analyze_structure(
        &reader,
        &operation,
        "tsx-review",
        "tsx-snapshot",
        || true,
        |_| {},
    )
    .unwrap();
    assert_eq!(result.status, StructureAnalysisStatus::Completed);
    assert_eq!(result.review_id, "tsx-review");
    assert_eq!(result.snapshot_id, "tsx-snapshot");
    let source = |path: &str| {
        result.sources.iter().find(|source| matches!(&source.after, StructureSourceSide::Present { path: actual, .. } if actual == path)).unwrap()
    };
    let declaration = |path: &str, name: &str| {
        result.nodes.iter().find_map(|node| match node {
        StructureNode::Declaration(node) if matches!(&node.after, StructureFact::Present { value } if value.name == name && value.locations.iter().any(|location| location.source_id == source(path).id)) => Some(node),
        _ => None,
    }).unwrap()
    };
    let panel = declaration("src/Panel.tsx", "Panel");
    assert!(panel.changes.is_empty());
    assert_eq!(panel.comparison, StructureComparison::Complete);
    for (fact, side) in [
        (&panel.before, StructureSide::Before),
        (&panel.after, StructureSide::After),
    ] {
        let StructureFact::Present { value } = fact else {
            panic!("命名箭头函数必须在两侧存在")
        };
        assert_eq!(value.native_kind.language, StructureLanguage::TypeScript);
        assert_eq!(value.native_kind.category, StructureCategory::Function);
        assert_eq!(value.native_kind.name, StructureNativeName::ArrowFunction);
        assert_eq!(value.locations[0].side, side);
        assert_eq!(value.locations[0].range.start_line, 2);
    }
    for (path, side, change) in [
        (
            "src/legacy/value.ts",
            StructureSide::Before,
            StructureRelationshipChange::Deleted,
        ),
        (
            "src/current/value.ts",
            StructureSide::After,
            StructureRelationshipChange::Added,
        ),
    ] {
        let target = declaration(path, "value");
        assert!(target.changes.is_empty());
        let edges: Vec<_> = result
            .relationships
            .iter()
            .filter(|edge| {
                edge.from_id == panel.id
                    && edge.to_id == target.id
                    && edge.kind == StructureRelationshipKind::Calls
            })
            .collect();
        assert_eq!(edges.len(), 1);
        let edge = edges[0];
        assert_eq!(edge.certainty, StructureCertainty::Resolved);
        assert_eq!(edge.change, change);
        assert!(result.initial_focus.relationship_ids.contains(&edge.id));
        assert!(result.initial_focus.node_ids.contains(&panel.id));
        assert!(result.initial_focus.node_ids.contains(&target.id));
        let (present, absent) = if side == StructureSide::Before {
            (&edge.before, &edge.after)
        } else {
            (&edge.after, &edge.before)
        };
        assert!(matches!(absent, StructureFact::Absent));
        let StructureFact::Present { value: occurrences } = present else {
            panic!("实际调用必须保留出现位置")
        };
        assert_eq!(occurrences.len(), 1);
        assert_eq!(
            occurrences[0].location.source_id,
            source("src/Panel.tsx").id
        );
        assert_eq!(occurrences[0].location.side, side);
        assert_eq!(occurrences[0].location.range.start_line, 2);
    }
    assert_eq!(result.summary.changed_declarations, 0);
    assert!(result.summary.relationship_changes >= 2);
    let peek = read_structure_source(
        &reader,
        &operation,
        "tsx-review",
        "tsx-snapshot",
        &StructureSourceRequest {
            side: StructureSide::After,
            path: "src/Panel.tsx".into(),
        },
    )
    .unwrap();
    assert_eq!(&peek.source, source("src/Panel.tsx"));
    assert!(
        matches!(&peek.source.classification, StructureSourceClassification::Unchanged { path } if path == "src/Panel.tsx")
    );
    for (content, revision) in [(&peek.before, &before), (&peek.after, &after)] {
        assert!(
            matches!(content, StructureSourceContent::Available { path, revision: actual, text, .. } if path == "src/Panel.tsx" && actual == revision && text == panel_text)
        );
    }
}

/// Rust 两个显式 crate 根经真实模块图聚合跨文件 impl，并为类型和修改方法提供各自完整原文。
#[test]
fn rust_module_graph_aggregates_impl_changes_and_preserves_real_peek_sources() {
    let repository = repository();
    let model_text = "pub struct Counter { pub value: i32 }\n";
    let implementation_before = "use crate::model::Counter;\nimpl Counter {\n    pub fn new() -> Self { Self { value: 1 } }\n    pub fn read(&self) -> i32 { self.value }\n    pub fn run(&self) -> i32 { self.read() }\n}\n";
    let implementation_after =
        implementation_before.replace("{ self.value }", "{ self.value + 1 }");
    for (path, text) in [
        ("Cargo.toml", "[package]\nname = \"engine-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[lib]\npath = \"src/lib.rs\"\n[[bin]]\nname = \"engine-fixture\"\npath = \"src/main.rs\"\n"),
        ("src/lib.rs", "pub mod model;\nmod implementation;\npub fn create() { model::Counter::new(); }\n"),
        ("src/main.rs", "mod cli;\nfn main() { cli::start(); }\n"),
        ("src/cli.rs", "pub fn start() {}\n"),
        ("src/model.rs", model_text),
        ("src/implementation.rs", implementation_before),
    ] {
        write(repository.path(), path, text);
    }
    let before = commit(repository.path(), "base modules");
    write(
        repository.path(),
        "src/implementation.rs",
        &implementation_after,
    );
    let after = commit(repository.path(), "modified impl method");
    let reader = reader(repository.path(), &before, &after);
    write(
        repository.path(),
        "src/model.rs",
        "pub struct CheckoutOnly;\n",
    );
    write(
        repository.path(),
        "src/implementation.rs",
        "invalid checkout",
    );
    let operation = OperationContext::background();
    let result = analyze_structure(
        &reader,
        &operation,
        "rust-review",
        "rust-snapshot",
        || true,
        |_| {},
    )
    .unwrap();
    assert_eq!(result.status, StructureAnalysisStatus::Completed);
    let declaration = |name: &str| {
        result.nodes.iter().find_map(|node| match node {
        StructureNode::Declaration(node) if matches!(&node.after, StructureFact::Present { value } if value.name == name) => Some(node),
        _ => None,
    }).unwrap()
    };
    let counter = declaration("Counter");
    let read = declaration("read");
    assert!(counter.changes.is_empty());
    assert_eq!(counter.contained_changes.before, 1);
    assert_eq!(counter.contained_changes.after, 1);
    assert_eq!(counter.contained_changes.union, 1);
    assert_eq!(read.changes, vec![StructureChangeLabel::Modified]);
    assert_eq!(read.comparison, StructureComparison::Complete);
    assert_eq!(result.summary.changed_declarations, 1);
    let source = |path: &str| {
        result.sources.iter().find(|source| matches!(&source.after, StructureSourceSide::Present { path: actual, .. } if actual == path)).unwrap()
    };
    let model_source = source("src/model.rs");
    let impl_source = source("src/implementation.rs");
    assert_ne!(model_source.id, impl_source.id);
    assert!(matches!(
        &model_source.classification,
        StructureSourceClassification::Unchanged { .. }
    ));
    assert!(
        matches!(&impl_source.classification, StructureSourceClassification::FileChange { file_change } if file_change.status == StructureFileChangeStatus::Modified)
    );
    assert_eq!(result.implementation_groups.len(), 1);
    let group = &result.implementation_groups[0];
    for (owner, method, implementation, side) in [
        (
            &counter.before,
            &read.before,
            &group.before,
            StructureSide::Before,
        ),
        (
            &counter.after,
            &read.after,
            &group.after,
            StructureSide::After,
        ),
    ] {
        let StructureFact::Present { value: owner } = owner else {
            panic!("类型必须在两侧存在")
        };
        let StructureFact::Present { value: method } = method else {
            panic!("修改方法必须在两侧存在")
        };
        let StructureFact::Present {
            value: implementation,
        } = implementation
        else {
            panic!("跨文件 impl 必须有确定归属")
        };
        assert_eq!(owner.native_kind.name, StructureNativeName::Struct);
        assert_eq!(owner.locations[0].source_id, model_source.id);
        assert_eq!(owner.locations[0].side, side);
        assert_eq!(owner.locations[0].range.start_line, 1);
        assert_eq!(method.parent_id, counter.id);
        assert_eq!(method.implementation_group_id.as_ref(), Some(&group.id));
        assert_eq!(method.locations[0].source_id, impl_source.id);
        assert_eq!(method.locations[0].side, side);
        assert_eq!(method.locations[0].range.start_line, 4);
        assert_eq!(implementation.owner_id, counter.id);
        assert_eq!(implementation.locations[0].source_id, impl_source.id);
        assert_eq!(implementation.locations[0].side, side);
        assert_eq!(implementation.locations[0].range.start_line, 2);
    }
    for (caller, callee, path, line) in [
        ("create", "new", "src/lib.rs", 3),
        ("run", "read", "src/implementation.rs", 5),
        ("main", "start", "src/main.rs", 2),
    ] {
        let edges: Vec<_> = result
            .relationships
            .iter()
            .filter(|edge| {
                edge.from_id == declaration(caller).id
                    && edge.to_id == declaration(callee).id
                    && edge.kind == StructureRelationshipKind::Calls
            })
            .collect();
        assert_eq!(edges.len(), 1, "{caller} -> {callee}");
        let edge = edges[0];
        assert_eq!(edge.certainty, StructureCertainty::Resolved);
        assert_eq!(edge.change, StructureRelationshipChange::Retained);
        for (fact, side) in [
            (&edge.before, StructureSide::Before),
            (&edge.after, StructureSide::After),
        ] {
            let StructureFact::Present { value: occurrences } = fact else {
                panic!("两侧必须保留真实调用")
            };
            assert_eq!(occurrences.len(), 1);
            assert_eq!(occurrences[0].location.source_id, source(path).id);
            assert_eq!(occurrences[0].location.side, side);
            assert_eq!(occurrences[0].location.range.start_line, line);
        }
    }
    // 聚合类型的声明和实现各打开真实文件对，不能拼接成虚构的源码。
    for (path, expected_before, expected_after) in [
        ("src/model.rs", model_text, model_text),
        (
            "src/implementation.rs",
            implementation_before,
            implementation_after.as_str(),
        ),
    ] {
        let peek = read_structure_source(
            &reader,
            &operation,
            "rust-review",
            "rust-snapshot",
            &StructureSourceRequest {
                side: StructureSide::After,
                path: path.into(),
            },
        )
        .unwrap();
        assert_eq!(&peek.source, source(path));
        assert_eq!(peek.source_id, source(path).id);
        assert_eq!(peek.review_id, "rust-review");
        assert_eq!(peek.snapshot_id, "rust-snapshot");
        for (content, expected, revision) in [
            (&peek.before, expected_before, &before),
            (&peek.after, expected_after, &after),
        ] {
            assert!(
                matches!(content, StructureSourceContent::Available { path: actual_path, revision: actual_revision, text, .. } if actual_path == path && actual_revision == revision && text == expected)
            );
        }
    }
}

/// 文档单独变化会进入初始图焦点，且仍能读取未裁剪的固定文件 Diff。
#[test]
fn documentation_only_change_is_discoverable_in_initial_focus() {
    let repository = repository();
    let original = "/** Original greeting. */\nexport function greet() { return 'hello'; }\n";
    let updated = original.replace("Original", "Documented");
    write(repository.path(), "greet.ts", original);
    let before = commit(repository.path(), "base");
    write(repository.path(), "greet.ts", &updated);
    let after = commit(repository.path(), "head");
    let reader = reader(repository.path(), &before, &after);
    let result = analyze_structure(
        &reader,
        &OperationContext::background(),
        "review",
        "snapshot",
        || true,
        |_| {},
    )
    .unwrap();
    let declaration = result.nodes.iter().find_map(|node| match node {
        StructureNode::Declaration(node)
            if matches!(&node.after, StructureFact::Present { value } if value.name == "greet") => Some(node),
        _ => None,
    }).unwrap();
    assert_eq!(
        declaration.changes,
        vec![StructureChangeLabel::Documentation]
    );
    assert_eq!(result.summary.changed_declarations, 1);
    assert!(result.initial_focus.node_ids.contains(&declaration.id));
    let peek = read_structure_source(
        &reader,
        &OperationContext::background(),
        "review",
        "snapshot",
        &StructureSourceRequest {
            side: StructureSide::After,
            path: "greet.ts".into(),
        },
    )
    .unwrap();
    assert!(
        matches!(peek.before, StructureSourceContent::Available { text, .. } if text == original)
    );
    assert!(
        matches!(peek.after, StructureSourceContent::Available { text, .. } if text == updated)
    );
}

/// 真实语法错误和超出 Git 输出预算的文件分别保留未知，不污染其他可信声明。
#[test]
fn syntax_and_resource_gaps_preserve_unknown_facts_and_original_source() {
    let repository = repository();
    for (path, text) in [
        ("clean.ts", "export function clean() { return 1; }\n"),
        ("broken.ts", "export function broken() { return 2; }\n"),
        ("limited.ts", "export function limited() { return 3; }\n"),
    ] {
        write(repository.path(), path, text);
    }
    let before = commit(repository.path(), "base");
    let broken = "export function broken(\n";
    write(repository.path(), "broken.ts", broken);
    // 样例显式超过现有单命令 16 MiB 输出预算；不会交给解析器吞掉内存预算。
    write(
        repository.path(),
        "limited.ts",
        &format!("/*{}*/", "x".repeat(17 * 1024 * 1024)),
    );
    let after = commit(repository.path(), "head");
    let reader = reader(repository.path(), &before, &after);
    let result = analyze_structure(
        &reader,
        &OperationContext::background(),
        "review",
        "snapshot",
        || true,
        |_| {},
    )
    .unwrap();
    assert_eq!(result.status, StructureAnalysisStatus::Completed);
    assert!(!result.summary.complete);
    for name in ["broken", "limited"] {
        let declaration = result.nodes.iter().find_map(|node| match node {
            StructureNode::Declaration(node)
                if matches!(&node.before, StructureFact::Present { value } if value.name == name) => Some(node),
            _ => None,
        }).unwrap();
        assert!(matches!(declaration.after, StructureFact::Unknown { .. }));
        assert!(!declaration.changes.contains(&StructureChangeLabel::Deleted));
    }
    assert!(result
        .nodes
        .iter()
        .any(|node| matches!(node, StructureNode::Declaration(node)
        if node.comparison == StructureComparison::Complete && node.changes.is_empty()
        && matches!(&node.after, StructureFact::Present { value } if value.name == "clean"))));
    let limited = result
        .coverage
        .files
        .iter()
        .find(|file| file.side == StructureSide::After && file.path == "limited.ts")
        .unwrap();
    assert_eq!(limited.extraction, StructureCoverageStatus::Partial);
    assert!(limited
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.reason == StructureDiagnosticReason::ResourceLimit));
    let peek = read_structure_source(
        &reader,
        &OperationContext::background(),
        "review",
        "snapshot",
        &StructureSourceRequest {
            side: StructureSide::After,
            path: "broken.ts".into(),
        },
    )
    .unwrap();
    assert!(matches!(peek.after, StructureSourceContent::Available { text, .. } if text == broken));
    let peek = read_structure_source(
        &reader,
        &OperationContext::background(),
        "review",
        "snapshot",
        &StructureSourceRequest {
            side: StructureSide::After,
            path: "limited.ts".into(),
        },
    )
    .unwrap();
    assert!(matches!(
        peek.before,
        StructureSourceContent::Available { .. }
    ));
    assert!(matches!(
        peek.after,
        StructureSourceContent::Unavailable {
            reason: StructureSourceUnavailableReason::ResourceLimit,
            ..
        }
    ));
}

/// 用显式固定 revisions 测量真实仓库；默认测试不读取开发者当前 checkout 或产生性能承诺。
#[test]
#[ignore = "仅在最终 Narrant 验收时提供固定仓库、revisions 和输出路径后运行"]
fn measure_fixed_narrant_snapshot() {
    let repository =
        std::env::var("NARRANT_STRUCTURE_BENCH_REPOSITORY").expect("fixed repository path");
    let before = std::env::var("NARRANT_STRUCTURE_BENCH_BEFORE").expect("fixed before revision");
    let after = std::env::var("NARRANT_STRUCTURE_BENCH_AFTER").expect("fixed after revision");
    let output = std::env::var("NARRANT_STRUCTURE_BENCH_OUTPUT").expect("measurement output path");
    let reader = reader(Path::new(&repository), &before, &after);
    let mut runs = Vec::new();
    // 两轮在同一线程执行；第二轮只复用实际存在的 grammar 规则/系统缓存，没有结果缓存。
    for label in ["cold-analysis", "warm-analysis"] {
        let started = std::time::Instant::now();
        let mut first_result_ms = None;
        let mut phases = Vec::new();
        let result = analyze_structure(
            &reader,
            &OperationContext::background(),
            "narrant-benchmark",
            "fixed-benchmark-snapshot",
            || true,
            |event| match event {
                StructureEvent::Progress { progress } => {
                    if phases
                        .last()
                        .is_none_or(|(phase, _)| phase != &progress.phase)
                    {
                        phases.push((progress.phase, started.elapsed().as_millis()));
                    }
                }
                StructureEvent::Checkpoint { .. } => {
                    first_result_ms.get_or_insert(started.elapsed().as_millis());
                }
            },
        )
        .unwrap();
        // 分析计时在结果返回时结束，不把可选 JSON 导出和文件写入算入 warm 轮。
        let analysis_elapsed_ms = started.elapsed().as_millis();
        assert_eq!(result.status, StructureAnalysisStatus::Completed);
        if label == "warm-analysis" {
            if let Ok(result_output) = std::env::var("NARRANT_STRUCTURE_BENCH_RESULT_OUTPUT") {
                std::fs::write(result_output, serde_json::to_vec(&result).unwrap()).unwrap();
            }
        }
        runs.push(serde_json::json!({
            "label": label,
            "elapsedMs": analysis_elapsed_ms,
            "firstUsableResultMs": first_result_ms,
            "phaseStarts": phases,
            "nodes": result.nodes.len(),
            "implementationGroups": result.implementation_groups.len(),
            "relationships": result.relationships.len(),
            "summary": result.summary,
            "coverage": result.coverage,
        }));
    }
    let cancellation = TestCancellation::default();
    let operation = OperationContext::from_test_cancellation(&cancellation);
    let started = std::time::Instant::now();
    let mut cancellation_requested = None;
    // 固定在首个文件提取完成后取消；报告请求到终态的延迟，不把启动耗时当取消耗时。
    let cancelled = analyze_structure(
        &reader,
        &operation,
        "narrant-benchmark",
        "fixed-benchmark-snapshot",
        || true,
        |event| {
            if cancellation_requested.is_none()
                && matches!(event, StructureEvent::Progress { progress }
                    if progress.phase == StructurePhase::Extraction && progress.processed_files == 1)
            {
                cancellation_requested = Some(std::time::Instant::now());
                cancellation.cancel();
            }
        },
    )
    .unwrap();
    assert_eq!(cancelled.status, StructureAnalysisStatus::Cancelled);
    let cancellation_latency = cancellation_requested
        .expect("Narrant contains supported source files")
        .elapsed();
    let measurement = serde_json::json!({
        "repository": repository,
        "beforeRevision": before,
        "comparisonBase": reader.comparison().comparison_base,
        "afterRevision": after,
        "build": "cargo test debug; incremental and sccache enabled",
        "cacheMeaning": "Second analysis reuses thread-local parser rules and OS caches, not cached analysis results.",
        "runs": runs,
        "cancellation": {
            "trigger": "after the first extracted source file",
            "totalElapsedMs": started.elapsed().as_millis(),
            "requestToTerminalMs": cancellation_latency.as_secs_f64() * 1000.0,
            "status": cancelled.status,
            "summary": cancelled.summary,
        },
    });
    std::fs::write(&output, serde_json::to_vec_pretty(&measurement).unwrap()).unwrap();
    println!("Narrant structure measurement: {output}");
}

/// 每个测试使用真实独立对象库，禁用用户配置造成的提交和换行差异。
fn repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), &["init", "-b", "main"]);
    git(
        directory.path(),
        &["config", "user.name", "Structure fixture"],
    );
    git(
        directory.path(),
        &["config", "user.email", "structure@example.invalid"],
    );
    git(directory.path(), &["config", "commit.gpgsign", "false"]);
    git(directory.path(), &["config", "core.autocrlf", "false"]);
    directory
}

/// 写入代表源码；目录创建只作用于当前临时仓库。
fn write(repository: &Path, path: &str, source: &str) {
    let path = repository.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

/// 提交当前样例并返回不可变 revision。
fn commit(repository: &Path, message: &str) -> String {
    git(repository, &["add", "."]);
    git(repository, &["commit", "-m", message]);
    git(repository, &["rev-parse", "HEAD"])
}

/// 运行样例仓库的 Git 操作，失败信息保留完整 stderr。
fn git(repository: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// 从两个固定 commit 建立与真实应用相同的 Snapshot reader。
fn reader(repository: &Path, before: &str, after: &str) -> SnapshotContentReader {
    let comparison = build_comparison(
        &OperationContext::background(),
        repository,
        ResolvedRevisions {
            base_ref: "main".into(),
            head_ref: "head".into(),
            base_revision: before.into(),
            head_revision: after.into(),
        },
    )
    .unwrap();
    SnapshotContentReader::restore(repository, comparison)
}
