use super::*;
use crate::operation::TestCancellation;
use std::fs;
use tempfile::TempDir;

mod read;

/// 创建一个真实的 Review Workspace 临时目录，测试不触达私有 helper。
fn workspace() -> TempDir {
    tempfile::tempdir().expect("创建临时 Review Workspace")
}

#[test]
fn registration_exposes_the_three_read_only_repository_tools() {
    let root = workspace();
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");

    let registrations = tools.registrations();

    assert_eq!(
        registrations
            .iter()
            .map(|registration| registration.name.as_str())
            .collect::<Vec<_>>(),
        ["read", "glob", "grep"]
    );
}

#[test]
fn glob_returns_matching_workspace_relative_paths() {
    let root = workspace();
    fs::create_dir_all(root.path().join("src/nested")).expect("创建目录");
    fs::write(root.path().join("src/b.rs"), "b").expect("写入文件");
    fs::write(root.path().join("src/nested/a.rs"), "a").expect("写入文件");
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");

    let result = tools
        .execute(
            RepositoryToolRequest::Glob(GlobRequest {
                pattern: "**/*.rs".to_owned(),
            }),
            &TestCancellation::default(),
        )
        .expect("执行 Glob");

    assert_eq!(
        result,
        RepositoryToolResponse::Glob(GlobResponse {
            paths: vec!["src/b.rs".to_owned(), "src/nested/a.rs".to_owned()],
            truncated: false,
        })
    );
}

#[test]
fn glob_returns_deterministic_bounded_results_with_explicit_truncation() {
    let root = workspace();
    for index in (0..501).rev() {
        fs::write(root.path().join(format!("file-{index:03}.txt")), "x").expect("写入文件");
    }
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");

    let result = tools
        .execute(
            RepositoryToolRequest::Glob(GlobRequest {
                pattern: "*.txt".to_owned(),
            }),
            &TestCancellation::default(),
        )
        .expect("执行 Glob");

    let RepositoryToolResponse::Glob(result) = result else {
        panic!("Glob 返回了错误的响应类型");
    };
    assert_eq!(result.paths.len(), 500);
    assert!(result.truncated);
    assert_eq!(
        result.paths.first().map(String::as_str),
        Some("file-000.txt")
    );
    assert_eq!(
        result.paths.last().map(String::as_str),
        Some("file-499.txt")
    );
    assert!(result.paths.windows(2).all(|paths| paths[0] < paths[1]));
}

#[cfg(unix)]
#[test]
fn glob_proves_each_match_and_skips_symlinks_leaving_the_workspace() {
    use std::os::unix::fs::symlink;

    let root = workspace();
    let outside = workspace();
    fs::write(root.path().join("inside.txt"), "inside").expect("写入文件");
    fs::write(outside.path().join("outside.txt"), "outside").expect("写入文件");
    symlink(
        root.path().join("inside.txt"),
        root.path().join("link-inside.txt"),
    )
    .expect("创建 workspace 内 symlink");
    symlink(
        outside.path().join("outside.txt"),
        root.path().join("link-outside.txt"),
    )
    .expect("创建 workspace 外 symlink");
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");

    let result = tools
        .execute(
            RepositoryToolRequest::Glob(GlobRequest {
                pattern: "**/*.txt".to_owned(),
            }),
            &TestCancellation::default(),
        )
        .expect("执行 Glob");

    let RepositoryToolResponse::Glob(result) = result else {
        panic!("Glob 返回了错误的响应类型");
    };
    assert_eq!(
        result.paths,
        vec!["inside.txt".to_owned(), "link-inside.txt".to_owned()]
    );
    assert!(!result.truncated);
}

#[test]
fn glob_never_uses_an_absolute_pattern_as_a_host_filesystem_root() {
    let root = workspace();
    let outside = workspace();
    fs::write(root.path().join("inside.txt"), "inside").expect("写入文件");
    fs::write(outside.path().join("outside.txt"), "outside").expect("写入文件");
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");

    let result = tools
        .execute(
            RepositoryToolRequest::Glob(GlobRequest {
                pattern: format!("{}/*.txt", outside.path().display()),
            }),
            &TestCancellation::default(),
        )
        .expect("执行 Glob");

    let RepositoryToolResponse::Glob(result) = result else {
        panic!("Glob 返回了错误的响应类型");
    };
    assert!(result.paths.is_empty());
    assert!(!result.truncated);
}

#[test]
fn glob_honors_cancellation_before_traversal() {
    let root = workspace();
    fs::write(root.path().join("inside.txt"), "inside").expect("写入文件");
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");
    let cancellation = TestCancellation::default();
    cancellation.cancel();

    let error = tools
        .execute(
            RepositoryToolRequest::Glob(GlobRequest {
                pattern: "**/*.txt".to_owned(),
            }),
            &cancellation,
        )
        .expect_err("已取消的 Glob 应失败");

    assert_eq!(error, RepositoryToolError::Cancelled);
    assert!(!error
        .to_string()
        .contains(&root.path().display().to_string()));
}

#[test]
fn grep_searches_a_directory_and_returns_relative_paths_and_one_based_lines() {
    let root = workspace();
    fs::create_dir_all(root.path().join("src")).expect("创建目录");
    fs::write(
        root.path().join("src/lib.rs"),
        "unrelated\nneedle first\nneedle second\n",
    )
    .expect("写入文件");
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");

    let result = tools
        .execute(
            RepositoryToolRequest::Grep(GrepRequest {
                pattern: "needle".to_owned(),
                path: "src".to_owned(),
            }),
            &TestCancellation::default(),
        )
        .expect("执行 Grep");

    assert_eq!(
        result,
        RepositoryToolResponse::Grep(GrepResponse {
            matches: vec![
                GrepMatch {
                    path: "src/lib.rs".to_owned(),
                    line_number: 2,
                    line: "needle first".to_owned(),
                },
                GrepMatch {
                    path: "src/lib.rs".to_owned(),
                    line_number: 3,
                    line: "needle second".to_owned(),
                },
            ],
            truncated: false,
        })
    );
}

#[test]
fn grep_accepts_absolute_and_parent_targets_only_when_they_resolve_inside() {
    let root = workspace();
    fs::create_dir_all(root.path().join("src")).expect("创建目录");
    let file = root.path().join("src/lib.rs");
    fs::write(&file, "needle\n").expect("写入文件");
    let outside = workspace();
    let outside_file = outside.path().join("outside.rs");
    fs::write(&outside_file, "needle\n").expect("写入文件");
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");

    for path in [
        "src/lib.rs".to_owned(),
        file.display().to_string(),
        "src/../src/lib.rs".to_owned(),
    ] {
        let result = tools
            .execute(
                RepositoryToolRequest::Grep(GrepRequest {
                    pattern: "needle".to_owned(),
                    path,
                }),
                &TestCancellation::default(),
            )
            .expect("执行 Grep");
        assert_eq!(
            result,
            RepositoryToolResponse::Grep(GrepResponse {
                matches: vec![GrepMatch {
                    path: "src/lib.rs".to_owned(),
                    line_number: 1,
                    line: "needle".to_owned(),
                }],
                truncated: false,
            })
        );
    }

    let error = tools
        .execute(
            RepositoryToolRequest::Grep(GrepRequest {
                pattern: "needle".to_owned(),
                path: outside_file.display().to_string(),
            }),
            &TestCancellation::default(),
        )
        .expect_err("workspace 外目标应失败");
    assert_eq!(error, RepositoryToolError::OutsideWorkspace);
    assert!(!error
        .to_string()
        .contains(&root.path().display().to_string()));
    assert!(!error
        .to_string()
        .contains(&outside.path().display().to_string()));
}

#[test]
fn grep_skips_binary_and_invalid_utf8_files_without_partial_matches() {
    let root = workspace();
    fs::write(root.path().join("text.rs"), "needle text\n").expect("写入文本文件");
    fs::write(
        root.path().join("binary.dat"),
        b"needle before nul\0needle after nul",
    )
    .expect("写入二进制文件");
    fs::write(root.path().join("invalid.dat"), [0xff, 0xfe, 0xfd]).expect("写入非 UTF-8 文件");
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");

    let result = tools
        .execute(
            RepositoryToolRequest::Grep(GrepRequest {
                pattern: "needle".to_owned(),
                path: ".".to_owned(),
            }),
            &TestCancellation::default(),
        )
        .expect("执行 Grep");

    assert_eq!(
        result,
        RepositoryToolResponse::Grep(GrepResponse {
            matches: vec![GrepMatch {
                path: "text.rs".to_owned(),
                line_number: 1,
                line: "needle text".to_owned(),
            }],
            truncated: false,
        })
    );
}

#[cfg(unix)]
#[test]
fn grep_reads_internal_symlinks_and_skips_external_symlinks() {
    use std::os::unix::fs::symlink;

    let root = workspace();
    let outside = workspace();
    fs::write(root.path().join("inside.rs"), "needle inside\n").expect("写入文件");
    fs::write(outside.path().join("outside.rs"), "needle outside\n").expect("写入文件");
    symlink(
        root.path().join("inside.rs"),
        root.path().join("link-inside.rs"),
    )
    .expect("创建 workspace 内 symlink");
    symlink(
        outside.path().join("outside.rs"),
        root.path().join("link-outside.rs"),
    )
    .expect("创建 workspace 外 symlink");
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");

    let result = tools
        .execute(
            RepositoryToolRequest::Grep(GrepRequest {
                pattern: "needle".to_owned(),
                path: ".".to_owned(),
            }),
            &TestCancellation::default(),
        )
        .expect("执行 Grep");

    let RepositoryToolResponse::Grep(result) = result else {
        panic!("Grep 返回了错误的响应类型");
    };
    assert_eq!(
        result.matches,
        vec![
            GrepMatch {
                path: "inside.rs".to_owned(),
                line_number: 1,
                line: "needle inside".to_owned(),
            },
            GrepMatch {
                path: "link-inside.rs".to_owned(),
                line_number: 1,
                line: "needle inside".to_owned(),
            },
        ]
    );
    assert!(!result.truncated);
}

#[test]
fn grep_returns_deterministic_bounded_matching_lines_with_explicit_truncation() {
    let root = workspace();
    for index in (0..501).rev() {
        fs::write(
            root.path().join(format!("file-{index:03}.rs")),
            format!("needle {index}\n"),
        )
        .expect("写入文件");
    }
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");

    let result = tools
        .execute(
            RepositoryToolRequest::Grep(GrepRequest {
                pattern: "needle".to_owned(),
                path: ".".to_owned(),
            }),
            &TestCancellation::default(),
        )
        .expect("执行 Grep");

    let RepositoryToolResponse::Grep(result) = result else {
        panic!("Grep 返回了错误的响应类型");
    };
    assert_eq!(result.matches.len(), 500);
    assert!(result.truncated);
    assert_eq!(
        result.matches.first().map(|item| item.path.as_str()),
        Some("file-000.rs")
    );
    assert_eq!(
        result.matches.last().map(|item| item.path.as_str()),
        Some("file-499.rs")
    );
    assert!(result.matches.windows(2).all(|items| {
        (items[0].path.as_str(), items[0].line_number)
            < (items[1].path.as_str(), items[1].line_number)
    }));
}

#[test]
fn grep_bounds_a_single_extreme_line_and_reports_truncation() {
    let root = workspace();
    fs::write(
        root.path().join("long.txt"),
        format!("needle{}", "x".repeat(20_000)),
    )
    .expect("写入极长行");
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");

    let RepositoryToolResponse::Grep(result) = tools
        .execute(
            RepositoryToolRequest::Grep(GrepRequest {
                pattern: "needle".to_owned(),
                path: ".".to_owned(),
            }),
            &TestCancellation::default(),
        )
        .expect("执行 Grep")
    else {
        panic!("Grep 返回了错误的响应类型");
    };

    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].line.len(), 8 * 1024);
    assert!(result.truncated);
}

#[test]
fn grep_honors_cancellation_before_search() {
    let root = workspace();
    fs::write(root.path().join("inside.rs"), "needle\n").expect("写入文件");
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");
    let cancellation = TestCancellation::default();
    cancellation.cancel();

    let error = tools
        .execute(
            RepositoryToolRequest::Grep(GrepRequest {
                pattern: "needle".to_owned(),
                path: ".".to_owned(),
            }),
            &cancellation,
        )
        .expect_err("已取消的 Grep 应失败");

    assert_eq!(error, RepositoryToolError::Cancelled);
    assert!(!error
        .to_string()
        .contains(&root.path().display().to_string()));
}

#[test]
fn grep_reports_stable_redacted_errors_for_invalid_pattern_and_missing_target() {
    let root = workspace();
    let tools = RepositoryTools::new(root.path()).expect("初始化工具");

    let invalid_pattern = tools
        .execute(
            RepositoryToolRequest::Grep(GrepRequest {
                pattern: "[".to_owned(),
                path: ".".to_owned(),
            }),
            &TestCancellation::default(),
        )
        .expect_err("无效模式应失败");
    assert_eq!(invalid_pattern, RepositoryToolError::InvalidPattern);
    assert!(!invalid_pattern
        .to_string()
        .contains(&root.path().display().to_string()));

    let missing_target_path = root.path().join("missing.rs");
    let missing_target = tools
        .execute(
            RepositoryToolRequest::Grep(GrepRequest {
                pattern: "needle".to_owned(),
                path: missing_target_path.display().to_string(),
            }),
            &TestCancellation::default(),
        )
        .expect_err("缺失目标应失败");
    assert_eq!(missing_target, RepositoryToolError::TargetUnavailable);
    assert!(!missing_target
        .to_string()
        .contains(&missing_target_path.display().to_string()));
}
