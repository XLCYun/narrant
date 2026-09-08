use crate::operation::OperationContext;
use crate::review::content::{
    build_comparison, FileContent, ResolvedRevisions, SnapshotContentReader, SnapshotSide,
    SnapshotSourceError,
};
use std::path::Path;
use std::process::Command;

/// 固定两侧 tree 必须包含未修改代码与各自版本的项目配置。
#[test]
fn snapshot_tree_reads_unchanged_sources_and_each_sides_configuration() {
    let repository = repository();
    write(
        repository.path(),
        "context.ts",
        "export const unchanged = '中文';\r\n",
    );
    write(
        repository.path(),
        "tsconfig.json",
        "{\"baseUrl\":\"before\"}\n",
    );
    let before = commit(repository.path(), "base");
    write(
        repository.path(),
        "tsconfig.json",
        "{\"baseUrl\":\"after\"}\n",
    );
    let after = commit(repository.path(), "head");
    let reader = reader(repository.path(), &before, &after);
    let operation = OperationContext::background();

    for (side, revision, configuration) in [
        (SnapshotSide::Before, &before, "{\"baseUrl\":\"before\"}\n"),
        (SnapshotSide::After, &after, "{\"baseUrl\":\"after\"}\n"),
    ] {
        let tree = reader.source_tree(&operation, side).unwrap();
        assert_eq!(tree.revision(), revision);
        assert_eq!(tree.entries().len(), 2);
        assert_eq!(
            tree.read(&operation, "context.ts")
                .unwrap()
                .as_ref()
                .and_then(FileContent::text),
            Some("export const unchanged = '中文';\r\n")
        );
        assert_eq!(
            tree.read(&operation, "tsconfig.json")
                .unwrap()
                .as_ref()
                .and_then(FileContent::text),
            Some(configuration)
        );
    }
}

/// 批量读取保留原始内容边界，并让缺对象、真缺侧和可读文件独立返回。
#[test]
fn batch_sources_keep_missing_objects_distinct_from_absence_and_readable_peers() {
    let repository = repository();
    write(
        repository.path(),
        "missing.ts",
        "export const missing = true;\n",
    );
    write(
        repository.path(),
        "readable.ts",
        "export const value = `first\nsecond`;\r\n",
    );
    let binary = b"before\0after";
    write(repository.path(), "binary.ts", binary);
    let revision = commit(repository.path(), "sources");
    let reader = reader(repository.path(), &revision, &revision);
    let operation = OperationContext::background();
    let missing = git(repository.path(), &["rev-parse", "HEAD:missing.ts"]);
    std::fs::remove_file(
        repository
            .path()
            .join(".git/objects")
            .join(&missing[..2])
            .join(&missing[2..]),
    )
    .unwrap();
    let tree = reader.source_tree(&operation, SnapshotSide::After).unwrap();
    assert_eq!(
        tree.entries()
            .iter()
            .find(|entry| entry.path == "missing.ts")
            .unwrap()
            .size,
        None
    );

    let mut sources = tree
        .read_batch(
            &operation,
            &["missing.ts", "readable.ts", "absent.ts", "binary.ts"],
        )
        .unwrap()
        .into_iter();
    assert!(matches!(
        sources.next().unwrap().content,
        Err(SnapshotSourceError::ObjectUnavailable { .. })
    ));
    assert_eq!(
        sources
            .next()
            .unwrap()
            .content
            .unwrap()
            .as_ref()
            .and_then(FileContent::text),
        Some("export const value = `first\nsecond`;\r\n")
    );
    assert_eq!(sources.next().unwrap().content.unwrap(), None);
    assert_eq!(
        sources.next().unwrap().content.unwrap(),
        Some(FileContent::Binary { size: binary.len() })
    );
}

/// 分支移动、工作区脏内容和 Git replacement refs 都不能改变旧 Snapshot 的对象。
#[test]
fn snapshot_sources_ignore_moving_refs_dirty_files_and_replacements() {
    let repository = repository();
    write(
        repository.path(),
        "source.ts",
        "export const value = 'original';\n",
    );
    let common = commit(repository.path(), "common");
    git(repository.path(), &["checkout", "-q", "-b", "base"]);
    write(
        repository.path(),
        "source.ts",
        "export const value = 'base tip';\n",
    );
    let base_tip = commit(repository.path(), "base tip");
    git(
        repository.path(),
        &["checkout", "-q", "-b", "topic", &common],
    );
    write(
        repository.path(),
        "source.ts",
        "export const value = 'snapshot head';\n",
    );
    let head = commit(repository.path(), "snapshot head");
    let reader = reader(repository.path(), &base_tip, &head);
    write(
        repository.path(),
        "source.ts",
        "export const value = 'later';\n",
    );
    let later = commit(repository.path(), "later");
    git(repository.path(), &["branch", "-f", "base", &later]);
    let original_blob = git(
        repository.path(),
        &["rev-parse", &format!("{head}:source.ts")],
    );
    let replacement_blob = git(repository.path(), &["rev-parse", "HEAD:source.ts"]);
    git(
        repository.path(),
        &["replace", &original_blob, &replacement_blob],
    );
    write(
        repository.path(),
        "source.ts",
        "uncommitted checkout contents",
    );
    let operation = OperationContext::background();

    let before = reader
        .source_tree(&operation, SnapshotSide::Before)
        .unwrap();
    let after = reader.source_tree(&operation, SnapshotSide::After).unwrap();
    assert_eq!(before.revision(), common);
    assert_eq!(after.revision(), head);
    assert_eq!(
        before
            .read(&operation, "source.ts")
            .unwrap()
            .as_ref()
            .and_then(FileContent::text),
        Some("export const value = 'original';\n")
    );
    assert_eq!(
        after
            .read(&operation, "source.ts")
            .unwrap()
            .as_ref()
            .and_then(FileContent::text),
        Some("export const value = 'snapshot head';\n")
    );
    assert_eq!(
        reader
            .read(&operation, "source.ts")
            .unwrap()
            .after
            .as_ref()
            .and_then(FileContent::text),
        Some("export const value = 'snapshot head';\n")
    );
}

/// 创建 Snapshot 前已存在的 commit replacement 也不能改写比较祖先、File Changes 或源码。
#[test]
fn comparison_and_source_trees_ignore_commit_replacements_present_before_creation() {
    let repository = repository();
    write(
        repository.path(),
        "source.ts",
        "export const common = true;\n",
    );
    let common = commit(repository.path(), "common");
    git(repository.path(), &["checkout", "-q", "-b", "base"]);
    write(
        repository.path(),
        "source.ts",
        "export const base_tip = true;\n",
    );
    let base = commit(repository.path(), "base tip");
    git(
        repository.path(),
        &["checkout", "-q", "-b", "topic", &common],
    );
    write(
        repository.path(),
        "source.ts",
        "export const actual_head = true;\n",
    );
    let head = commit(repository.path(), "actual head");
    write(
        repository.path(),
        "source.ts",
        "export const replacement = true;\n",
    );
    let replacement = commit(repository.path(), "replacement tree");
    let replacement_tree = git(
        repository.path(),
        &["rev-parse", &format!("{replacement}^{{tree}}")],
    );
    let replacement_head = git(
        repository.path(),
        &[
            "commit-tree",
            &replacement_tree,
            "-p",
            &common,
            "-m",
            "replace head",
        ],
    );
    git(repository.path(), &["replace", &head, &replacement_head]);
    let operation = OperationContext::background();
    for replace_base_parent in [false, true] {
        if replace_base_parent {
            let base_tree = git(
                repository.path(),
                &["rev-parse", &format!("{base}^{{tree}}")],
            );
            let replacement_base = git(
                repository.path(),
                &[
                    "commit-tree",
                    &base_tree,
                    "-p",
                    &head,
                    "-m",
                    "replace base ancestry",
                ],
            );
            git(repository.path(), &["replace", &base, &replacement_base]);
        }
        let reader = reader(repository.path(), &base, &head);
        assert_eq!(reader.comparison().comparison_base, common);
        assert_eq!(reader.comparison().file_changes.len(), 1);
        let changed = reader.read(&operation, "source.ts").unwrap();
        let before_tree = reader
            .source_tree(&operation, SnapshotSide::Before)
            .unwrap();
        let after_tree = reader.source_tree(&operation, SnapshotSide::After).unwrap();
        assert_eq!(
            changed.before,
            before_tree.read(&operation, "source.ts").unwrap()
        );
        assert_eq!(
            changed.after,
            after_tree.read(&operation, "source.ts").unwrap()
        );
        assert_eq!(
            changed.before.as_ref().and_then(FileContent::text),
            Some("export const common = true;\n")
        );
        assert_eq!(
            changed.after.as_ref().and_then(FileContent::text),
            Some("export const actual_head = true;\n")
        );
    }
}

/// rename 和删除由实际两侧 tree 确认；特殊路径分隔符不会拆开文件名。
#[test]
fn snapshot_trees_preserve_real_rename_paths_and_deleted_file_absence() {
    let repository = repository();
    let old_path = "src/旧\t名字\npart.ts";
    let new_path = "src/new\t名字\npart.ts";
    write(repository.path(), old_path, "export function moved() {}\n");
    write(repository.path(), "deleted.rs", "fn deleted() {}\n");
    let before = commit(repository.path(), "base");
    std::fs::rename(
        repository.path().join(old_path),
        repository.path().join(new_path),
    )
    .unwrap();
    std::fs::remove_file(repository.path().join("deleted.rs")).unwrap();
    let after = commit(repository.path(), "head");
    let reader = reader(repository.path(), &before, &after);
    let operation = OperationContext::background();
    let before = reader
        .source_tree(&operation, SnapshotSide::Before)
        .unwrap();
    let after = reader.source_tree(&operation, SnapshotSide::After).unwrap();

    assert!(before.read(&operation, old_path).unwrap().is_some());
    assert_eq!(before.read(&operation, new_path).unwrap(), None);
    assert_eq!(after.read(&operation, old_path).unwrap(), None);
    assert!(after.read(&operation, new_path).unwrap().is_some());
    assert!(before.read(&operation, "deleted.rs").unwrap().is_some());
    assert_eq!(after.read(&operation, "deleted.rs").unwrap(), None);
    let change = reader.read(&operation, new_path).unwrap();
    assert_eq!(change.old_path.as_deref(), Some(old_path));
    assert_eq!(change.before, change.after);
}

/// 无效或丢失的单侧 revision 不产生空 tree，也不会妨碍另一侧读取。
#[test]
fn unavailable_and_invalid_revisions_do_not_establish_absence() {
    use crate::core_git::CoreGitFailureKind;
    let repository = repository();
    write(repository.path(), "context.rs", "fn context() {}\n");
    let revision = commit(repository.path(), "source");
    let operation = OperationContext::background();
    for (before, kind) in [
        ("HEAD".to_owned(), CoreGitFailureKind::InvalidData),
        ("a".repeat(41), CoreGitFailureKind::InvalidData),
        (
            "0".repeat(40),
            CoreGitFailureKind::Exited { code: Some(128) },
        ),
    ] {
        let comparison = crate::review::content::ChangeComparison {
            base_ref: "base".into(),
            head_ref: "topic".into(),
            base_revision: revision.clone(),
            comparison_base: before,
            head_revision: revision.clone(),
            file_changes: vec![],
        };
        let reader = SnapshotContentReader::restore(repository.path(), comparison);
        let error = reader
            .source_tree(&operation, SnapshotSide::Before)
            .err()
            .unwrap();
        assert_eq!(error.kind(), kind);
        assert!(reader
            .source_tree(&operation, SnapshotSide::After)
            .unwrap()
            .read(&operation, "context.rs")
            .unwrap()
            .is_some());
    }
}

/// 已取消操作连纯目录查询也必须返回取消，不能把未执行误报为文件缺失。
#[test]
fn cancelled_snapshot_reads_never_return_absence_or_completion() {
    use crate::operation::TestCancellation;
    let repository = repository();
    write(repository.path(), "source.rs", "fn source() {}\n");
    let revision = commit(repository.path(), "source");
    let reader = reader(repository.path(), &revision, &revision);
    let tree = reader
        .source_tree(&OperationContext::background(), SnapshotSide::After)
        .unwrap();
    let cancellation = TestCancellation::default();
    let operation = OperationContext::from_test_cancellation(&cancellation);
    cancellation.cancel();
    assert!(reader
        .source_tree(&operation, SnapshotSide::After)
        .err()
        .unwrap()
        .is_cancelled());
    assert!(tree.read_batch(&operation, &[]).unwrap_err().is_cancelled());
    assert!(tree
        .read_batch(&operation, &["source.rs"])
        .unwrap_err()
        .is_cancelled());
    assert!(
        matches!(tree.read(&operation, "absent.rs"), Err(SnapshotSourceError::Git(error)) if error.is_cancelled())
    );
}

/// blob 的二进制、编码、可执行位、链接和 gitlink 语义与 File Changes 一致。
#[test]
fn snapshot_sources_keep_git_modes_and_text_decoding_semantics() {
    let repository = repository();
    write(repository.path(), "empty.ts", "");
    write(repository.path(), "invalid.ts", [0xff, 0xfe]);
    write(
        repository.path(),
        "executable.js",
        "console.log('hello');\n",
    );
    let base = commit(repository.path(), "blobs");
    git(
        repository.path(),
        &["update-index", "--chmod=+x", "executable.js"],
    );
    let target_file = repository.path().join("target.txt");
    std::fs::write(&target_file, "../target.ts").unwrap();
    let link_blob = git(repository.path(), &["hash-object", "-w", "target.txt"]);
    git(
        repository.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{link_blob},link.ts"),
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
    git(repository.path(), &["commit", "-q", "-m", "modes"]);
    let head = git(repository.path(), &["rev-parse", "HEAD"]);
    let reader = reader(repository.path(), &base, &head);
    let operation = OperationContext::background();
    let tree = reader.source_tree(&operation, SnapshotSide::After).unwrap();

    assert_eq!(
        tree.read(&operation, "empty.ts").unwrap(),
        Some(FileContent::Text {
            size: 0,
            text: String::new()
        })
    );
    assert_eq!(
        tree.read(&operation, "invalid.ts").unwrap(),
        Some(FileContent::Binary { size: 2 })
    );
    assert_eq!(
        tree.read(&operation, "link.ts").unwrap(),
        Some(FileContent::Symlink {
            size: 12,
            target: "../target.ts".into()
        })
    );
    assert_eq!(
        tree.read(&operation, "dependency").unwrap(),
        Some(FileContent::Submodule { commit_id: base })
    );
    assert_eq!(
        tree.entries()
            .iter()
            .find(|entry| entry.path == "executable.js")
            .unwrap()
            .mode,
        0o100755
    );
    assert_eq!(
        tree.read(&operation, "link.ts").unwrap(),
        reader.read(&operation, "link.ts").unwrap().after
    );
}

/// 大于常见管道容量的输入和输出须并发消费，跨批次仍逐项保持请求顺序。
#[test]
fn batch_source_reads_handle_repository_scale_and_duplicate_requests() {
    let repository = repository();
    let body = "// source context\n".repeat(600);
    let paths = (0..800)
        .map(|index| format!("sources/context-{index}.ts"))
        .collect::<Vec<_>>();
    for (index, path) in paths.iter().enumerate() {
        write(
            repository.path(),
            path,
            format!("export const value = {index};\n{body}"),
        );
    }
    let revision = commit(repository.path(), "many sources");
    let reader = reader(repository.path(), &revision, &revision);
    let operation = OperationContext::background();
    let tree = reader.source_tree(&operation, SnapshotSide::After).unwrap();
    let mut requests = paths.iter().map(String::as_str).collect::<Vec<_>>();
    requests.push(&paths[0]);
    let results = tree.read_batch(&operation, &requests).unwrap();
    assert_eq!(results.len(), 801);
    for (index, result) in results.iter().enumerate() {
        assert_eq!(result.path, requests[index]);
        let expected_index = if index == 800 { 0 } else { index };
        assert_eq!(
            result
                .content
                .as_ref()
                .unwrap()
                .as_ref()
                .and_then(FileContent::text),
            Some(format!("export const value = {expected_index};\n{body}").as_str())
        );
    }
}

/// 超过单命令输出上限的文件保留资源失败，后续可读来源仍能显示。
#[test]
fn oversized_source_reports_a_limit_without_hiding_readable_files() {
    use crate::core_git::CoreGitFailureKind;
    let repository = repository();
    write(repository.path(), "large.ts", vec![b'x'; 17 * 1024 * 1024]);
    write(
        repository.path(),
        "small.ts",
        "export const small = true;\n",
    );
    let revision = commit(repository.path(), "source limit");
    let reader = reader(repository.path(), &revision, &revision);
    let operation = OperationContext::background();
    let tree = reader.source_tree(&operation, SnapshotSide::After).unwrap();
    let mut results = tree
        .read_batch(&operation, &["large.ts", "small.ts"])
        .unwrap()
        .into_iter();
    assert!(matches!(results.next().unwrap().content,
        Err(SnapshotSourceError::Git(error)) if matches!(error.kind(), CoreGitFailureKind::OutputLimitExceeded { .. })
    ));
    assert_eq!(
        results
            .next()
            .unwrap()
            .content
            .unwrap()
            .as_ref()
            .and_then(FileContent::text),
        Some("export const small = true;\n")
    );
}

/// cat-file 遇到损坏 blob 中途退出时，单对象隔离仍能保留其后的可读源码。
#[test]
fn corrupt_blob_does_not_hide_readable_sources_later_in_the_batch() {
    let repository = repository();
    write(
        repository.path(),
        "corrupt.ts",
        "const original = true;\n".repeat(20_000),
    );
    write(
        repository.path(),
        "readable.ts",
        "export const readable = true;\n",
    );
    let revision = commit(repository.path(), "sources");
    let reader = reader(repository.path(), &revision, &revision);
    let operation = OperationContext::background();
    let tree = reader.source_tree(&operation, SnapshotSide::After).unwrap();
    let object = &tree
        .entries()
        .iter()
        .find(|entry| entry.path == "corrupt.ts")
        .unwrap()
        .object_id;
    let path = repository
        .path()
        .join(".git/objects")
        .join(&object[..2])
        .join(&object[2..]);
    let compressed = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, &compressed[..compressed.len() / 2]).unwrap();
    let mut results = tree
        .read_batch(&operation, &["corrupt.ts", "readable.ts"])
        .unwrap()
        .into_iter();
    assert!(results.next().unwrap().content.is_err());
    assert_eq!(
        results
            .next()
            .unwrap()
            .content
            .unwrap()
            .as_ref()
            .and_then(FileContent::text),
        Some("export const readable = true;\n")
    );
}

/// 建立真实 Git 仓库，避免以模拟对象库掩盖协议问题。
fn repository() -> tempfile::TempDir {
    let repository = tempfile::TempDir::new().unwrap();
    git(
        repository.path(),
        &["init", "-q", "--initial-branch=fixture"],
    );
    git(repository.path(), &["config", "core.autocrlf", "false"]);
    git(
        repository.path(),
        &["config", "user.email", "test@narrant.invalid"],
    );
    git(repository.path(), &["config", "user.name", "Narrant Test"]);
    repository
}

/// 写入测试用的可跟踪内容。
fn write(repository: &Path, path: &str, content: impl AsRef<[u8]>) {
    let file = repository.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, content).unwrap();
}

/// 提交当前全部 fixture 文件并返回精确 commit ID。
fn commit(repository: &Path, message: &str) -> String {
    git(repository, &["add", "-A"]);
    git(repository, &["commit", "-q", "-m", message]);
    git(repository, &["rev-parse", "HEAD"])
}

/// 从真实精确 revisions 构建可恢复的 Snapshot reader。
fn reader(repository: &Path, before: &str, after: &str) -> SnapshotContentReader {
    let comparison = build_comparison(
        &OperationContext::background(),
        repository,
        ResolvedRevisions {
            base_ref: "refs/heads/base".into(),
            head_ref: "refs/heads/topic".into(),
            base_revision: before.into(),
            head_revision: after.into(),
        },
    )
    .unwrap();
    SnapshotContentReader::restore(repository, comparison)
}

/// 执行 fixture 命令并读取机器格式输出。
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
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
