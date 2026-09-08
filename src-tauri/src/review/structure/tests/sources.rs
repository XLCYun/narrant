use super::{read_structure_source, SourceCatalog};
use crate::operation::{OperationContext, TestCancellation};
use crate::review::content::{
    build_comparison, ChangeComparison, FileChange, FileChangeStatus, FileContentRef,
    ResolvedRevisions, SnapshotContentReader, SnapshotSide,
};
use crate::review::structure::model::*;
use std::path::Path;
use std::process::Command;

/// 单侧 tree 失败不使未修改文件缺侧；全部 File Changes 仍能进入来源目录。
#[test]
fn catalogue_preserves_rename_deletion_and_context_when_one_tree_is_unavailable() {
    let repository = repository();
    write(
        repository.path(),
        "old.ts",
        "export function renamed() {}\n",
    );
    write(repository.path(), "deleted.rs", "fn deleted() {}\n");
    write(
        repository.path(),
        "context.ts",
        "export const context = true;\n",
    );
    let base = commit(repository.path(), "base");
    std::fs::rename(
        repository.path().join("old.ts"),
        repository.path().join("new.ts"),
    )
    .unwrap();
    std::fs::remove_file(repository.path().join("deleted.rs")).unwrap();
    let head = commit(repository.path(), "head");
    let reader = reader(repository.path(), &base, &head);
    let tree = reader
        .source_tree(&OperationContext::background(), SnapshotSide::After)
        .unwrap();
    let catalogue = SourceCatalog::build(&reader, None, Some(&tree));
    let context = catalogue
        .lookup(StructureSide::Before, "context.ts")
        .unwrap();
    assert_eq!(
        context.before,
        StructureSourceSide::Present {
            path: "context.ts".into(),
            revision: base
        }
    );
    assert!(matches!(context.after, StructureSourceSide::Present { .. }));
    assert!(matches!(
        context.classification,
        StructureSourceClassification::Unchanged { .. }
    ));
    assert_eq!(
        catalogue
            .lookup(StructureSide::Before, "old.ts")
            .unwrap()
            .id
            .0,
        "source:new.ts"
    );
    assert_eq!(
        catalogue
            .lookup(StructureSide::After, "new.ts")
            .unwrap()
            .id
            .0,
        "source:new.ts"
    );
    assert!(catalogue.lookup(StructureSide::After, "old.ts").is_none());
    assert!(matches!(
        catalogue
            .lookup(StructureSide::Before, "deleted.rs")
            .unwrap()
            .after,
        StructureSourceSide::Absent
    ));
    let no_trees = SourceCatalog::build(&reader, None, None);
    assert_eq!(no_trees.sources.len(), 2);
    write(
        repository.path(),
        "future.ts",
        "export const future = true;\n",
    );
    let future = commit(repository.path(), "later snapshot");
    let future_reader = SnapshotContentReader::restore(
        repository.path(),
        comparison(repository.path(), &head, &future),
    );
    let future_tree = future_reader
        .source_tree(&OperationContext::background(), SnapshotSide::After)
        .unwrap();
    let stale_discovery = SourceCatalog::build(&reader, None, Some(&future_tree));
    assert!(stale_discovery
        .lookup(StructureSide::After, "future.ts")
        .is_none());
    assert_eq!(stale_discovery.sources.len(), 2);
}

/// 无需分析就能阅读完整未修改上下文，原始 CRLF、路径和 object ID 全部保留。
#[test]
fn peek_reads_unchanged_complete_source_without_an_analysis_result() {
    let repository = repository();
    let text = "// 中文上下文\r\nexport const untouched = true;\r\n// 文件结尾\r\n";
    write(repository.path(), "context.ts", text);
    write(
        repository.path(),
        "changed.ts",
        "export const version = 1;\n",
    );
    let base = commit(repository.path(), "base");
    write(
        repository.path(),
        "changed.ts",
        "export const version = 2;\n",
    );
    let head = commit(repository.path(), "head");
    let reader = reader(repository.path(), &base, &head);
    let result = peek(&reader, StructureSide::After, "context.ts");
    assert_eq!(result.source_id.0, "source:context.ts");
    assert_eq!(result.review_id, "review");
    assert_eq!(result.snapshot_id, "snapshot");
    assert!(matches!(
        result.source.classification,
        StructureSourceClassification::Unchanged { .. }
    ));
    for (content, revision) in [(&result.before, base), (&result.after, head)] {
        let StructureSourceContent::Available {
            path,
            revision: actual,
            object_id,
            text: actual_text,
            size,
        } = content
        else {
            panic!("expected complete text")
        };
        assert_eq!(path, "context.ts");
        assert_eq!(actual, &revision);
        assert_eq!(actual_text, text);
        assert_eq!(*size, text.len() as u64);
        assert_eq!(
            object_id,
            &git(
                repository.path(),
                &["rev-parse", &format!("{revision}:context.ts")]
            )
        );
    }
}

/// 已知 File Change 的 blob 即使 commit 消失也能读取；缺 blob 不吞掉另一个完整侧。
#[test]
fn peek_reads_fixed_change_blobs_without_trees_and_keeps_unavailable_sides_explicit() {
    let repository = repository();
    write(
        repository.path(),
        "source.ts",
        "export const before = true;\n",
    );
    let base = commit(repository.path(), "base");
    write(
        repository.path(),
        "source.ts",
        "export const after = true;\n",
    );
    let head = commit(repository.path(), "head");
    let reader = reader(repository.path(), &base, &head);
    let before_blob = reader.comparison().file_changes[0]
        .before
        .as_ref()
        .unwrap()
        .object_id
        .clone();
    remove_object(repository.path(), &base);
    let result = peek(&reader, StructureSide::After, "source.ts");
    assert_text(&result.before, "export const before = true;\n");
    assert_text(&result.after, "export const after = true;\n");
    remove_object(repository.path(), &before_blob);
    let result = peek(&reader, StructureSide::After, "source.ts");
    assert!(matches!(
        result.before,
        StructureSourceContent::Unavailable {
            reason: StructureSourceUnavailableReason::GitObjectUnavailable,
            ..
        }
    ));
    assert_text(&result.after, "export const after = true;\n");
}

/// 未修改来源的发现失败必须保持不可用，而不是把该侧改成已删除。
#[test]
fn peek_keeps_unchanged_side_unavailable_when_its_commit_is_missing() {
    let repository = repository();
    write(
        repository.path(),
        "context.ts",
        "export const context = true;\n",
    );
    write(
        repository.path(),
        "version.ts",
        "export const version = 1;\n",
    );
    let base = commit(repository.path(), "base");
    write(
        repository.path(),
        "version.ts",
        "export const version = 2;\n",
    );
    let head = commit(repository.path(), "head");
    let reader = reader(repository.path(), &base, &head);
    remove_object(repository.path(), &base);
    let result = peek(&reader, StructureSide::Before, "context.ts");
    assert!(matches!(
        result.source.before,
        StructureSourceSide::Present { .. }
    ));
    assert!(matches!(
        result.before,
        StructureSourceContent::Unavailable {
            reason: StructureSourceUnavailableReason::GitObjectUnavailable,
            ..
        }
    ));
    assert_text(&result.after, "export const context = true;\n");
}

/// 持久化 Snapshot 的真实 rename 文件对与复用旧路径的新文件必须按侧分别定位。
#[test]
fn peek_does_not_confuse_rename_origin_with_a_new_file_reusing_that_path() {
    let repository = repository();
    write(
        repository.path(),
        "old.ts",
        "export function original() {}\n",
    );
    let base = commit(repository.path(), "base");
    write(
        repository.path(),
        "new.ts",
        "export function original() {}\n",
    );
    write(
        repository.path(),
        "old.ts",
        "export const replacement = true;\n",
    );
    let head = commit(repository.path(), "head");
    let mut comparison = comparison(repository.path(), &base, &head);
    // 对象与路径均取自真实 tree；恢复明确配对的 Snapshot，不依赖 Git 的 rename 启发式。
    comparison.file_changes = vec![
        FileChange {
            path: "new.ts".into(),
            old_path: Some("old.ts".into()),
            status: FileChangeStatus::Renamed,
            before: Some(reference(repository.path(), &base, "old.ts")),
            after: Some(reference(repository.path(), &head, "new.ts")),
        },
        FileChange {
            path: "old.ts".into(),
            old_path: None,
            status: FileChangeStatus::Added,
            before: None,
            after: Some(reference(repository.path(), &head, "old.ts")),
        },
    ];
    let reader = SnapshotContentReader::restore(repository.path(), comparison);
    let original = peek(&reader, StructureSide::Before, "old.ts");
    let replacement = peek(&reader, StructureSide::After, "old.ts");
    assert_eq!(original.source_id.0, "source:new.ts");
    assert_text(&original.before, "export function original() {}\n");
    assert_text(&original.after, "export function original() {}\n");
    assert_eq!(replacement.source_id.0, "source:old.ts");
    assert!(matches!(
        replacement.before,
        StructureSourceContent::Absent { .. }
    ));
    assert_text(&replacement.after, "export const replacement = true;\n");
    let catalogue = SourceCatalog::build(&reader, None, None);
    assert_eq!(
        catalogue
            .lookup(StructureSide::Before, "old.ts")
            .unwrap()
            .id,
        original.source_id
    );
    assert_eq!(
        catalogue.lookup(StructureSide::After, "old.ts").unwrap().id,
        replacement.source_id
    );
}

/// 声明跨文件移动仍打开两个真实完整文件对，不把旧 A 与新 B 合成虚拟文件。
#[test]
fn moved_declarations_keep_each_real_files_complete_before_and_after() {
    let repository = repository();
    let old_a = "export function moved() {}\nexport function staysA() {}\n";
    let new_a = "export function staysA() {}\n";
    let old_b = "export function staysB() {}\n";
    let new_b = "export function staysB() {}\nexport function moved() {}\n";
    write(repository.path(), "a.ts", old_a);
    write(repository.path(), "b.ts", old_b);
    let base = commit(repository.path(), "base");
    write(repository.path(), "a.ts", new_a);
    write(repository.path(), "b.ts", new_b);
    let head = commit(repository.path(), "head");
    let reader = reader(repository.path(), &base, &head);
    let original_location = peek(&reader, StructureSide::Before, "a.ts");
    let new_location = peek(&reader, StructureSide::After, "b.ts");
    assert_eq!(original_location.source_id.0, "source:a.ts");
    assert_eq!(new_location.source_id.0, "source:b.ts");
    assert_text(&original_location.before, old_a);
    assert_text(&original_location.after, new_a);
    assert_text(&new_location.before, old_b);
    assert_text(&new_location.after, new_b);
}

/// 二进制、链接、子模块和确认缺侧分别展示；取消不会转换成完成的 Peek。
#[test]
fn peek_distinguishes_nontext_absence_and_cancellation() {
    let repository = repository();
    write(repository.path(), "context.rs", "fn context() {}\n");
    let base = commit(repository.path(), "base");
    write(repository.path(), "binary.ts", b"binary\0source");
    git(repository.path(), &["add", "-A"]);
    let target = reference(repository.path(), &base, "context.rs").object_id;
    git(
        repository.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{target},link.ts"),
        ],
    );
    git(
        repository.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{base},dependency"),
        ],
    );
    git(repository.path(), &["commit", "-q", "-m", "head"]);
    let head = git(repository.path(), &["rev-parse", "HEAD"]);
    let reader = reader(repository.path(), &base, &head);
    for (path, expected) in [
        ("binary.ts", StructureSourceUnavailableReason::Binary),
        ("link.ts", StructureSourceUnavailableReason::Symlink),
        ("dependency", StructureSourceUnavailableReason::Submodule),
    ] {
        let result = peek(&reader, StructureSide::After, path);
        assert!(matches!(
            result.before,
            StructureSourceContent::Absent { .. }
        ));
        assert!(
            matches!(result.after, StructureSourceContent::Unavailable { reason, .. } if reason == expected)
        );
    }
    let cancellation = TestCancellation::default();
    let operation = OperationContext::from_test_cancellation(&cancellation);
    cancellation.cancel();
    assert!(read_structure_source(
        &reader,
        &operation,
        "review",
        "snapshot",
        &StructureSourceRequest {
            side: StructureSide::After,
            path: "binary.ts".into()
        }
    )
    .unwrap_err()
    .is_cancelled());
}

/// 在公开来源请求边界读取 fixture，而不接触语言解析状态。
fn peek(reader: &SnapshotContentReader, side: StructureSide, path: &str) -> StructurePeekContent {
    read_structure_source(
        reader,
        &OperationContext::background(),
        "review",
        "snapshot",
        &StructureSourceRequest {
            side,
            path: path.into(),
        },
    )
    .unwrap()
}

/// 校验完整来源文本，不能由片段或空文本占位满足。
fn assert_text(content: &StructureSourceContent, expected: &str) {
    let StructureSourceContent::Available { text, .. } = content else {
        panic!("expected readable source: {content:?}")
    };
    assert_eq!(text, expected);
}

/// 从真实 tree 获取固定 blob 引用来恢复 Snapshot 元数据。
fn reference(repository: &Path, revision: &str, path: &str) -> FileContentRef {
    FileContentRef {
        object_id: git(repository, &["rev-parse", &format!("{revision}:{path}")]),
        mode: 0o100644,
    }
}

/// 仅删除本 fixture 的 loose object，模拟历史 Git 对象被回收。
fn remove_object(repository: &Path, object_id: &str) {
    std::fs::remove_file(
        repository
            .join(".git/objects")
            .join(&object_id[..2])
            .join(&object_id[2..]),
    )
    .unwrap();
}

/// 建立禁用换行转换的真实仓库，固定测试分支命名。
fn repository() -> tempfile::TempDir {
    let repository = tempfile::TempDir::new().unwrap();
    git(
        repository.path(),
        &["init", "-q", "--initial-branch=fixture"],
    );
    git(repository.path(), &["config", "core.autocrlf", "false"]);
    git(repository.path(), &["config", "user.name", "Narrant Test"]);
    git(
        repository.path(),
        &["config", "user.email", "test@narrant.invalid"],
    );
    repository
}

/// 写入用于已跟踪源码的 fixture 内容。
fn write(repository: &Path, path: &str, content: impl AsRef<[u8]>) {
    std::fs::write(repository.join(path), content).unwrap();
}

/// 提交 fixture 所有变化并返回完整 revision。
fn commit(repository: &Path, message: &str) -> String {
    git(repository, &["add", "-A"]);
    git(repository, &["commit", "-q", "-m", message]);
    git(repository, &["rev-parse", "HEAD"])
}

/// 通过既有精确比较接口构造 Snapshot 元数据。
fn comparison(repository: &Path, before: &str, after: &str) -> ChangeComparison {
    build_comparison(
        &OperationContext::background(),
        repository,
        ResolvedRevisions {
            base_ref: "base".into(),
            head_ref: "topic".into(),
            base_revision: before.into(),
            head_revision: after.into(),
        },
    )
    .unwrap()
}

/// 恢复 fixture 的精确 Snapshot reader。
fn reader(repository: &Path, before: &str, after: &str) -> SnapshotContentReader {
    SnapshotContentReader::restore(repository, comparison(repository, before, after))
}

/// 执行 fixture Git 命令并读取成功输出。
fn git(repository: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
