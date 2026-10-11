use super::*;
use crate::structure_review::typescript_analyzer::{
    project::PreparedProject, tools::TypeScriptToolchain,
};
use std::{fs, sync::Arc};

/// 真实服务夹具只写临时仓库，不依赖当前应用源码与业务依赖。
fn fixture(files: &[(&str, &str)]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    for (name, content) in files {
        let file = directory.path().join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }
    directory
}

/// 固定服务测试显式执行；工具缺失会失败，绝不悄悄跳过验证。
fn start(directory: &Path, selected: &[&str]) -> TypeScriptLsp {
    let directory = directory.canonicalize().unwrap();
    let files = selected
        .iter()
        .map(|file| directory.join(file))
        .collect::<Vec<_>>();
    let project = PreparedProject::prepare(
        &directory,
        &files,
        &TypeScriptToolchain::default(),
        Duration::from_secs(30),
        &(Arc::new(|| false) as CancellationCheck),
    )
    .unwrap()
    .unwrap();
    TypeScriptLsp::start(project, Duration::from_secs(30), Arc::new(|| false)).unwrap()
}

/// 已知有效目标的夹具必须全部返回可信位置，未验证回退不能悄悄通过断言。
fn checked_definitions(definitions: Vec<DefinitionEvidence>) -> Result<Vec<CheckedLocation>> {
    definitions
        .into_iter()
        .map(|definition| match definition {
            DefinitionEvidence::Checked(location) => Ok(location),
            DefinitionEvidence::Unverified => Err(failure("夹具返回未验证定义")),
        })
        .collect()
}

/// 连目录、文件内容及链接目标一起记录，防止分析悄悄新增配置或替换真实文件。
fn filesystem_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    /// 目录和链接也纳入快照，不能只比较已知源码内容。
    fn visit(root: &Path, directory: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            let content = if kind.is_dir() {
                visit(root, &path, result);
                vec![]
            } else if kind.is_symlink() {
                fs::read_link(&path)
                    .unwrap()
                    .to_string_lossy()
                    .as_bytes()
                    .to_vec()
            } else {
                fs::read(&path).unwrap()
            };
            result.insert(path.strip_prefix(root).unwrap().to_path_buf(), content);
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

/// 真实 Git 签出是唯一源码目录；服务 URI、标准声明和清理均不依赖文件副本。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_reads_git_checkout_without_copying_or_mutating_files() {
    let repository = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"module":"Preserve","moduleResolution":"Bundler","target":"ES2022","types":[],"plugins":[{"name":"not-enabled"}]},"files":["main.ts"]}"#),
        ("main.ts", "import {work} from './worker';\nexport function run() { return Promise.resolve(work()); }"),
        ("worker.ts", "export function work() { return 1; }"),
        ("asset.css", "button { color: red; }"),
    ]);
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "fixture",
        ],
        vec!["worktree", "add", "--detach", "checkout", "HEAD"],
    ] {
        let output = std::process::Command::new("git")
            .current_dir(repository.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let root = repository.path().join("checkout").canonicalize().unwrap();
    let original = filesystem_snapshot(&root);
    let mut session = start(&root, &["main.ts"]);
    assert_eq!(session.project.workspace_root, root);
    assert_eq!(
        session.project.config_files,
        vec![root.join("tsconfig.json")]
    );
    assert!(session
        .project
        .source_paths
        .iter()
        .all(|file| file.starts_with(&root)));
    let file = root.join("main.ts");
    let name = root.join("tsconfig.json");
    let members = session.validate_project(&file, &name).unwrap();
    assert!(members.contains(&root.join("worker.ts")));
    assert!(members.iter().any(
        |file| file.starts_with(&session.project.toolchain.runtime_root)
            && file.ends_with("lib.es2015.promise.d.ts")
    ));
    let source = session.project.source(&file).unwrap();
    let position = source.position(source.rfind("work()").unwrap()).unwrap();
    let definitions = session
        .definitions(&file, position, &name)
        .and_then(checked_definitions)
        .unwrap();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].path, root.join("worker.ts"));
    let support = session.project.support_root.clone();
    let artifacts = filesystem_snapshot(&support);
    assert_eq!(
        artifacts.keys().cloned().collect::<BTreeSet<_>>(),
        [
            "config.mjs",
            "input.json",
            "output.json",
            "prepare.mjs",
            "server.mjs",
            "workspace.mjs",
            "server",
            "server/lib",
            "server/lib/tsserver.js",
            "server/package.json",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    );
    assert_eq!(filesystem_snapshot(&root), original);
    session.shutdown().unwrap();
    assert!(!support.exists());
    assert_eq!(filesystem_snapshot(&root), original);
}

/// 最小真实查询跨越准备、注册、打开、显式项目、定义和出入边全过程。
#[test]
#[ignore = "需要 pnpm setup:typescript-analyzer；固定工具验收显式运行"]
fn fixed_service_definitions_and_calls() {
    let fixture = fixture(&[
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["*.ts"]}"#,
        ),
        (
            "main.ts",
            "export function target() {}\nexport function caller() { target(); }\n",
        ),
    ]);
    let mut session = start(fixture.path(), &["main.ts"]);
    let file = session.project.files[0].file.clone();
    let name = session.project.files[0].projects[0].clone();
    let definitions = session
        .definitions(
            &file,
            Position {
                line: 1,
                character: 27,
            },
            &name,
        )
        .and_then(checked_definitions)
        .unwrap();
    assert_eq!(definitions.len(), 1);
    let source = session.project.source(&file).unwrap();
    assert_eq!(
        definitions[0].span.start_byte as usize,
        source.find("target").unwrap()
    );
    let outgoing = session
        .outgoing_calls(
            &file,
            Position {
                line: 1,
                character: 17,
            },
            &name,
        )
        .unwrap();
    assert_eq!(outgoing[0].target.name, "target");
    assert_eq!(
        outgoing[0].from_spans[0].start_byte as usize,
        source.rfind("target").unwrap()
    );
    let incoming = session
        .incoming_calls(
            &file,
            Position {
                line: 0,
                character: 17,
            },
            &name,
        )
        .unwrap();
    assert_eq!(incoming[0].target.name, "caller");
    let support = session.project.support_root.clone();
    session.shutdown().unwrap();
    assert!(!support.exists());
}

/// 已安装的业务 @types 不可进入 Program，Node 固定声明与 undici 闭包仍能解析。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_excludes_business_and_supplies_node_closure() {
    let fixture = fixture(&[
        ("package.json", r#"{"type":"module","engines":{"node":">=18"}}"#),
        ("tsconfig.json", r#"{"compilerOptions":{"module":"NodeNext","target":"ES2022","types":["node","business"],"lib":["ES2022"]},"include":["src/*.ts"]}"#),
        ("src/main.ts", "import { readFile } from 'node:fs';\nimport { hidden } from 'business';\nexport const fetcher = fetch;\nexport const reader = readFile;\n"),
        ("node_modules/@types/business/index.d.ts", "declare const hiddenGlobal: string;"),
        ("node_modules/business/package.json", r#"{"name":"business","types":"index.d.ts"}"#),
        ("node_modules/business/index.d.ts", "export declare function hidden(): void;"),
    ]);
    let mut session = start(fixture.path(), &["src/main.ts"]);
    let file = session.project.files[0].file.clone();
    let name = session.project.files[0].projects[0].clone();
    let members = session.validate_project(&file, &name).unwrap();
    assert!(members
        .iter()
        .any(|file| file.to_string_lossy().contains("@types/node/fs.d.ts")));
    assert!(members
        .iter()
        .any(|file| file.to_string_lossy().contains("undici-types/")));
    assert!(!members
        .iter()
        .any(|file| file.to_string_lossy().contains("business")));
    assert!(session
        .project
        .workspace_root
        .join("node_modules/business")
        .exists());
    let definitions = session
        .definitions(
            &file,
            Position {
                line: 3,
                character: 23,
            },
            &name,
        )
        .and_then(checked_definitions)
        .unwrap();
    assert!(definitions
        .iter()
        .any(|definition| definition.path.ends_with("@types/node/fs.d.ts")));
    session.shutdown().unwrap();
}

/// JSONC/extends/paths 与多份配置保持独立，错误项目名不能回退到默认配置。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_jsonc_paths_and_ambiguous_projects() {
    let fixture = fixture(&[
        ("config/base.json", "{ // JSONC\n\"compilerOptions\":{\"target\":\"ES2022\",\"module\":\"Preserve\",\"moduleResolution\":\"Bundler\",\"types\":[],},}"),
        ("a/tsconfig.json", r#"{"extends":"../config/base.json","compilerOptions":{"baseUrl":"..","paths":{"@impl":["a/impl.ts"]}},"files":["../shared.ts","impl.ts"]}"#),
        ("b/tsconfig.json", r#"{"extends":"../config/base.json","compilerOptions":{"baseUrl":"..","paths":{"@impl":["b/impl.ts"]}},"files":["../shared.ts","impl.ts"]}"#),
        ("a/impl.ts", "export function work() { return 'A'; }"),
        ("b/impl.ts", "export function work() { return 'B'; }"),
        ("shared.ts", "import { work } from '@impl';\nexport function caller() { return work(); }"),
    ]);
    let mut session = start(fixture.path(), &["shared.ts"]);
    let file = session.project.files[0].file.clone();
    let names = session.project.files[0].projects.clone();
    assert_eq!(names.len(), 2);
    let mut targets = BTreeSet::new();
    for name in &names {
        let definitions = session
            .definitions(
                &file,
                Position {
                    line: 1,
                    character: 35,
                },
                name,
            )
            .and_then(checked_definitions)
            .unwrap();
        targets.extend(definitions.into_iter().map(|target| target.path));
    }
    assert_eq!(targets.len(), 2);
    assert!(targets.iter().any(|target| target.ends_with("a/impl.ts")));
    assert!(targets.iter().any(|target| target.ends_with("b/impl.ts")));
    let invalid = session
        .project
        .workspace_root
        .join(".narrant-registration-0");
    assert!(session
        .validate_project(&file, &invalid)
        .unwrap_err()
        .to_string()
        .contains("未知项目"));
    assert!(session.client.is_none());
}

/// 未构建 composite 包仍通过配置引用重定向到第一方源文件，不伪造 exports 源入口。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_workspace_reference_without_dist() {
    let fixture = fixture(&[
        (
            "package.json",
            r#"{"private":true,"workspaces":["packages/*"]}"#,
        ),
        (
            "tsconfig.json",
            r#"{"files":[],"references":[{"path":"packages/lib"},{"path":"packages/app"}]}"#,
        ),
        (
            "packages/lib/package.json",
            r#"{"name":"@sample/lib","version":"1.0.0","type":"module","exports":{".":{"types":"./dist/index.d.ts","default":"./dist/index.js"}}}"#,
        ),
        (
            "packages/lib/tsconfig.json",
            r#"{"compilerOptions":{"composite":true,"module":"NodeNext","target":"ES2022","rootDir":"src","outDir":"dist","types":[]},"include":["src"]}"#,
        ),
        (
            "packages/lib/src/index.ts",
            "export function service() { return 1; }",
        ),
        (
            "packages/app/package.json",
            r#"{"name":"@sample/app","version":"1.0.0","type":"module","dependencies":{"@sample/lib":"workspace:*"}}"#,
        ),
        (
            "packages/app/tsconfig.json",
            r#"{"compilerOptions":{"composite":true,"module":"NodeNext","target":"ES2022","rootDir":"src","types":[]},"references":[{"path":"../lib"}],"include":["src"]}"#,
        ),
        (
            "packages/app/src/main.ts",
            "import { service } from '@sample/lib';\nexport function main() { return service(); }",
        ),
    ]);
    let mut session = start(fixture.path(), &["packages/app/src/main.ts"]);
    assert!(!session
        .project
        .workspace_root
        .join("packages/lib/dist")
        .exists());
    let file = session.project.files[0].file.clone();
    let name = session.project.files[0].projects[0].clone();
    let definitions = session
        .definitions(
            &file,
            Position {
                line: 1,
                character: 34,
            },
            &name,
        )
        .and_then(checked_definitions)
        .unwrap();
    assert!(definitions
        .iter()
        .any(|target| target.path.ends_with("packages/lib/src/index.ts")));
    session.shutdown().unwrap();
}

/// 配置外 JS 使用显式独立推断项目，不被邻近配置的 exclude 静默丢弃。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_unconfigured_js_and_unicode_paths() {
    let fixture = fixture(&[
        ("package.json", r#"{"type":"commonjs"}"#),
        ("tsconfig.json", r#"{"files":["included.ts"],"compilerOptions":{"types":[],"noLib":true}}"#),
        ("included.ts", "export const included = 1;"),
        ("中文 space/main.js", "const path = require('node:path');\nfunction make() { return path.join('a', 'b'); }\nmodule.exports = make;"),
    ]);
    let mut session = start(fixture.path(), &["中文 space/main.js"]);
    let file = session.project.files[0].file.clone();
    let name = session.project.files[0].projects[0].clone();
    assert!(name.to_string_lossy().contains(".narrant-inferred-"));
    let members = session.validate_project(&file, &name).unwrap();
    assert!(members
        .iter()
        .any(|file| file.to_string_lossy().contains("@types/node/path.d.ts")));
    let definitions = session
        .definitions(
            &file,
            Position {
                line: 1,
                character: 31,
            },
            &name,
        )
        .and_then(checked_definitions)
        .unwrap();
    assert!(definitions
        .iter()
        .any(|target| target.path.ends_with("@types/node/path.d.ts")));
    session.shutdown().unwrap();
}

/// 缺失业务 extends 时仍沿用实际 noLib 和 types 配置。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_missing_extends_and_authoritative_libraries() {
    let fixture = fixture(&[
        (
            "tsconfig.json",
            r#"{"extends":"@business/tsconfig","compilerOptions":{"noLib":true,"types":[],"module":"Preserve","moduleResolution":"Bundler"},"files":["main.ts"]}"#,
        ),
        ("main.ts", "export function work() {}\nwork();"),
    ]);
    let mut session = start(fixture.path(), &["main.ts"]);
    let file = session.project.files[0].file.clone();
    let name = session.project.files[0].projects[0].clone();
    assert_eq!(
        *session.validate_project(&file, &name).unwrap(),
        BTreeSet::from([file])
    );
    session.shutdown().unwrap();
}

/// 只枚举本测试进程启动的 TLS 与同进程组 tsserver，避免与别的开发会话混淆。
#[cfg(unix)]
fn owned_service_processes() -> Vec<i32> {
    let output = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,pgid=,command="])
        .output()
        .unwrap();
    let listing = String::from_utf8(output.stdout).unwrap();
    let rows = listing
        .lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            Some((
                fields.first()?.parse::<i32>().ok()?,
                fields.get(1)?.parse::<i32>().ok()?,
                fields.get(2)?.parse::<i32>().ok()?,
                line.to_owned(),
            ))
        })
        .collect::<Vec<_>>();
    let groups = rows
        .iter()
        .filter(|(_, parent, _, command)| {
            *parent == std::process::id() as i32
                && command.contains("typescript-language-server/lib/cli.mjs")
        })
        .map(|(_, _, group, _)| *group)
        .collect::<Vec<_>>();
    rows.iter()
        .filter(|(_, _, group, _)| groups.contains(group))
        .map(|(pid, _, _, _)| *pid)
        .collect()
}

/// 正常退出后不仅直接 TLS PID，连实际 tsserver 后代都必须已消失。
#[test]
#[cfg(unix)]
#[ignore = "需要固定工具"]
fn fixed_service_reclaims_descendants_on_shutdown_and_cancel() {
    let fixture = fixture(&[("main.ts", "export function work() {}\nwork();")]);
    let session = start(fixture.path(), &["main.ts"]);
    let pids = owned_service_processes();
    assert!(pids.len() >= 2, "未观察到 TLS 与 tsserver：{pids:?}");
    let support = session.project.support_root.clone();
    session.shutdown().unwrap();
    assert!(!support.exists());
    for pid in pids {
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "正常退出残留 PID {pid}");
    }
    let root = fixture.path().canonicalize().unwrap();
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = cancelled.clone();
    let check: CancellationCheck = Arc::new(move || flag.load(std::sync::atomic::Ordering::SeqCst));
    let project = PreparedProject::prepare(
        &root,
        &[root.join("main.ts")],
        &TypeScriptToolchain::default(),
        Duration::from_secs(30),
        &check,
    )
    .unwrap()
    .unwrap();
    let mut session = TypeScriptLsp::start(project, Duration::from_secs(30), check).unwrap();
    let pids = owned_service_processes();
    let file = session.project.files[0].file.clone();
    let name = session.project.files[0].projects[0].clone();
    cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(session
        .definitions(
            &file,
            Position {
                line: 1,
                character: 1
            },
            &name
        )
        .and_then(checked_definitions)
        .unwrap_err()
        .is_cancelled());
    assert!(session.client.is_none());
    for pid in pids {
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "取消残留 PID {pid}");
    }
}

/// selected 文件仅通过 import 加入项目时，仍优先采用真实 configured 上下文。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_imported_selection_and_json_context() {
    let fixture = fixture(&[
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","resolveJsonModule":true,"types":[]},"files":["main.ts"]}"#,
        ),
        ("main.ts", "import { worker } from './worker';\nworker();"),
        (
            "worker.ts",
            "import data from './data.json';\nexport function worker() { return data.name; }",
        ),
        ("data.json", r#"{"name":"sample"}"#),
    ]);
    let mut session = start(fixture.path(), &["worker.ts"]);
    let file = session.project.files[0].file.clone();
    let names = session.project.files[0].projects.clone();
    assert_eq!(
        names,
        vec![session.project.workspace_root.join("tsconfig.json")]
    );
    assert!(session
        .validate_project(&file, &names[0])
        .unwrap()
        .iter()
        .any(|path| path.ends_with("data.json")));
    let prepared = session
        .prepare_hierarchy(
            &file,
            Position {
                line: 1,
                character: 17,
            },
            &names[0],
        )
        .unwrap();
    assert_eq!(prepared[0].name, "worker");
    assert!(prepared[0].full_span.end_byte >= prepared[0].location.span.end_byte);
    assert_eq!(
        session
            .project()
            .checked_path(&fixture.path().canonicalize().unwrap().join("worker.ts"))
            .unwrap(),
        file
    );
    session.shutdown().unwrap();
}

/// 原始侧外的源码不能通过三斜线引用混入成功项目；纯配置路径也拒绝逃逸。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_rejects_external_source_and_configuration_paths() {
    let outside = fixture(&[("outside.ts", "export const secret = 1;")]);
    let fixture = fixture(&[
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"types":[]},"files":["main.ts"]}"#,
        ),
        (
            "main.ts",
            &format!(
                "/// <reference path=\"{}\" />\nexport const local = 1;",
                outside
                    .path()
                    .canonicalize()
                    .unwrap()
                    .join("outside.ts")
                    .display()
            ),
        ),
    ]);
    let root = fixture.path().canonicalize().unwrap();
    let check: CancellationCheck = Arc::new(|| false);
    let tools = TypeScriptToolchain::default();
    let project = PreparedProject::prepare(
        &root,
        &[root.join("main.ts")],
        &tools,
        Duration::from_secs(30),
        &check,
    )
    .unwrap()
    .unwrap();
    let support = project.support_root.clone();
    let error = TypeScriptLsp::start(project, Duration::from_secs(30), check.clone())
        .err()
        .unwrap();
    assert!(error.to_string().contains("未授权源码"));
    assert!(!support.exists());
    fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":"../../outside"},"files":["main.ts"]}"#,
    )
    .unwrap();
    let error = PreparedProject::prepare(
        &root,
        &[root.join("main.ts")],
        &tools,
        Duration::from_secs(30),
        &check,
    )
    .unwrap_err();
    assert!(error.to_string().contains("仓库外路径"));
}

/// 业务配置只在内存去除插件，workspace 原文件和依赖保持不变，业务声明不进入项目。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_business_config_metadata_without_plugins() {
    let fixture = fixture(&[
        (
            "tsconfig.json",
            r#"{"extends":"@business/config","files":["main.ts"]}"#,
        ),
        ("main.ts", "export function ready() {}\nready();"),
        (
            "node_modules/@business/config/package.json",
            r#"{"name":"@business/config","tsconfig":"base.json"}"#,
        ),
        (
            "node_modules/@business/config/base.json",
            r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[],"plugins":[{"name":"danger-plugin"}]}}"#,
        ),
        (
            "node_modules/@business/config/index.d.ts",
            "declare const hidden: string;",
        ),
        (
            "node_modules/danger-plugin/index.js",
            "throw new Error('must not execute plugin');",
        ),
    ]);
    let mut session = start(fixture.path(), &["main.ts"]);
    let file = session.project.files[0].file.clone();
    let name = session.project.files[0].projects[0].clone();
    let members = session.validate_project(&file, &name).unwrap();
    assert!(!members
        .iter()
        .any(|path| path.to_string_lossy().contains("@business")));
    let config = session
        .project
        .workspace_root
        .join("node_modules/@business/config/base.json");
    assert!(config.exists());
    assert!(fs::read_to_string(config)
        .unwrap()
        .contains("danger-plugin"));
    assert!(session
        .project
        .workspace_root
        .join("node_modules/@business/config/index.d.ts")
        .exists());
    assert!(session
        .project
        .workspace_root
        .join("node_modules/danger-plugin")
        .exists());
    session.shutdown().unwrap();
}

/// 浏览器未配置源码采用 Preserve/Bundler 与 DOM，未定义符号返回真实空列表。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_browser_inference_and_unresolved() {
    let fixture = fixture(&[(
        "main.ts",
        "export const title = document.title;\nmissing();",
    )]);
    let mut session = start(fixture.path(), &["main.ts"]);
    let file = session.project.files[0].file.clone();
    let name = session.project.files[0].projects[0].clone();
    let members = session.validate_project(&file, &name).unwrap();
    assert!(members.iter().any(|file| file.ends_with("lib.dom.d.ts")));
    assert!(!members
        .iter()
        .any(|file| file.to_string_lossy().contains("@types/node")));
    let definitions = session
        .definitions(
            &file,
            Position {
                line: 0,
                character: 32,
            },
            &name,
        )
        .and_then(checked_definitions)
        .unwrap();
    assert!(definitions
        .iter()
        .any(|target| target.path.ends_with("lib.dom.d.ts")));
    assert!(session
        .definitions(
            &file,
            Position {
                line: 1,
                character: 3
            },
            &name
        )
        .and_then(checked_definitions)
        .unwrap()
        .is_empty());
    session.shutdown().unwrap();
}

/// 原生类型误用证据按源码字节位置保留，合法运行时调用不能被同名别名误伤。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_preserves_native_runtime_type_errors() {
    let source = "// 多字节前缀验证诊断位置转换。\nimport type { Model } from './target';\nimport { action, runtimeAction } from './barrel';\ninterface Contract {}\nnew Model();\naction();\nnew Contract();\nruntimeAction();\n";
    let fixture = fixture(&[
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"module":"Preserve","moduleResolution":"Bundler","target":"ES2022","types":[]},"files":["main.ts"]}"#,
        ),
        ("target.ts", "export class Model {}\nexport function action() {}"),
        (
            "barrel.ts",
            "export type { action } from './target';\nexport { action as runtimeAction } from './target';",
        ),
        ("main.ts", source),
    ]);
    let session = start(fixture.path(), &["main.ts"]);
    let file = session.project.files[0].file.clone();
    let name = session.project.files[0].projects[0].clone();
    // 分别覆盖类型专用导入、类型专用导出和只存在于类型空间的声明。
    for (identifier, code) in [("Model", 1361), ("action", 1362), ("Contract", 2693)] {
        let offset = source.rfind(identifier).unwrap() as u32;
        assert!(
            session.runtime_type_error(&file, offset, &name),
            "TS{code} 应保留 {identifier} 的运行时类型误用位置"
        );
        assert!(!session.runtime_type_error(&file, offset + identifier.len() as u32, &name));
    }
    let allowed = source.rfind("runtimeAction()").unwrap() as u32;
    assert!(!session.runtime_type_error(&file, allowed, &name));
    session.shutdown().unwrap();
}

/// 真实服务器初始化超时与命令错误都失败，返回前释放进程及工具文件，保留 workspace。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_timeout_and_rpc_errors() {
    let fixture = fixture(&[("main.ts", "export function ready() {}")]);
    let root = fixture.path().canonicalize().unwrap();
    let check: CancellationCheck = Arc::new(|| false);
    let project = PreparedProject::prepare(
        &root,
        &[root.join("main.ts")],
        &TypeScriptToolchain::default(),
        Duration::from_secs(30),
        &check,
    )
    .unwrap()
    .unwrap();
    let support = project.support_root.clone();
    let error = TypeScriptLsp::start(project, Duration::from_nanos(1), check)
        .err()
        .unwrap();
    assert!(error.to_string().contains("超时"));
    assert!(!support.exists());
    #[cfg(unix)]
    assert!(owned_service_processes().is_empty());
    let mut session = start(fixture.path(), &["main.ts"]);
    let error = session
        .guarded(|session| session.raw("narrantInvalidCommand", json!({}), true))
        .unwrap_err();
    assert!(
        error.to_string().contains("narrantInvalidCommand")
            || error.to_string().contains("Unrecognized")
    );
    assert!(session.client.is_none());
    #[cfg(unix)]
    assert!(owned_service_processes().is_empty());
}

/// jsconfig 的 paths 与普通 target 源目录完整保留，不能误当作 Rust 构建输出删除。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_jsconfig_and_first_party_layout() {
    let fixture = fixture(&[
        (
            "jsconfig.json",
            r#"{"compilerOptions":{"module":"Preserve","moduleResolution":"Bundler","target":"ES2022","baseUrl":".","paths":{"@code/*":["src/target/*"]},"types":[]},"include":["src"]}"#,
        ),
        ("src/main.js", "import { run } from '@code/worker';\nrun();"),
        ("src/target/worker.js", "export function run() {}"),
    ]);
    let mut session = start(fixture.path(), &["src/main.js"]);
    let file = session.project.files[0].file.clone();
    let name = session.project.files[0].projects[0].clone();
    assert!(name.ends_with("jsconfig.json"));
    let definitions = session
        .definitions(
            &file,
            Position {
                line: 1,
                character: 1,
            },
            &name,
        )
        .and_then(checked_definitions)
        .unwrap();
    assert!(definitions
        .iter()
        .any(|target| target.path.ends_with("src/target/worker.js")));
    session.shutdown().unwrap();
}

/// 以固定 TS 配置/模块解析器确认原始侧目标，再要求受控 LSP 返回同一目标。
fn assert_inherited_alias(files: &[(&str, &str)], expected: &str) {
    let fixture = fixture(files);
    let root = fixture.path().canonicalize().unwrap();
    let tools = TypeScriptToolchain::default();
    let mut command = tools.command();
    command.args(["-e", r#"const ts = require(process.argv[1]); const root = process.argv[2];
const parsed = ts.getParsedCommandLineOfConfigFile(root + '/app/tsconfig.json', {}, {...ts.sys, onUnRecoverableConfigFileDiagnostic: error => { throw new Error(String(error.code)); }});
const target = ts.resolveModuleName('@impl', root + '/app/main.ts', parsed.options, ts.sys).resolvedModule;
if (!target) throw new Error('original alias unresolved'); process.stdout.write(target.resolvedFileName);"#])
        .arg(tools.runtime_root.join("typescript/lib/typescript.js")).arg(&root);
    let check: CancellationCheck = Arc::new(|| false);
    let original = crate::structure_review::typescript_analyzer::runtime::run(
        command,
        Duration::from_secs(10),
        &check,
    )
    .unwrap();
    assert_eq!(Path::new(&original), root.join(expected));
    let mut session = start(&root, &["app/main.ts"]);
    let file = session.project.files[0].file.clone();
    let name = session.project.workspace_root.join("app/tsconfig.json");
    let source = session.project.source(&file).unwrap();
    let position = source.position(source.rfind("value").unwrap()).unwrap();
    let definitions = session
        .definitions(&file, position, &name)
        .and_then(checked_definitions)
        .unwrap();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].path, root.join(expected));
    session.shutdown().unwrap();
}

/// 子配置的相对 paths 使用继承的 baseUrl，不能错误绑定到子配置目录的竞争文件。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_inherited_base_url_with_child_paths() {
    assert_inherited_alias(
        &[
            (
                "base.json",
                r#"{"compilerOptions":{"baseUrl":"./src","module":"Preserve","moduleResolution":"Bundler","types":[]}}"#,
            ),
            (
                "app/tsconfig.json",
                r#"{"extends":"../base.json","compilerOptions":{"paths":{"@impl":["impl.ts"]}},"files":["main.ts"]}"#,
            ),
            (
                "app/main.ts",
                "import { value } from '@impl';\nexport const result = value;",
            ),
            (
                "src/impl.ts",
                "export const value = 'correct inherited base';",
            ),
            (
                "app/impl.ts",
                "export const value = 'wrong child directory';",
            ),
        ],
        "src/impl.ts",
    );
}

/// 子配置覆盖 baseUrl 时继承的 paths 必须重新按有效 baseUrl 解析。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_inherited_paths_with_overridden_base_url() {
    assert_inherited_alias(
        &[
            (
                "base.json",
                r#"{"compilerOptions":{"paths":{"@impl":["impl.ts"]},"module":"Preserve","moduleResolution":"Bundler","types":[]}}"#,
            ),
            (
                "app/tsconfig.json",
                r#"{"extends":"../base.json","compilerOptions":{"baseUrl":"../other"},"files":["main.ts"]}"#,
            ),
            (
                "app/main.ts",
                "import { value } from '@impl';\nexport const result = value;",
            ),
            (
                "other/impl.ts",
                "export const value = 'correct overridden base';",
            ),
            (
                "impl.ts",
                "export const value = 'wrong base config directory';",
            ),
        ],
        "other/impl.ts",
    );
}

/// 保留相对 paths 后仍按最终继承上下文拒绝越界，不能让新的基准绕过范围验证。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_inherited_paths_reject_effective_escape() {
    let fixture = fixture(&[
        (
            "config/deep/base.json",
            r#"{"compilerOptions":{"paths":{"@impl":["../../outside/impl.ts"]}}}"#,
        ),
        (
            "app/tsconfig.json",
            r#"{"extends":"../config/deep/base.json","compilerOptions":{"baseUrl":".."},"files":["main.ts"]}"#,
        ),
        ("app/main.ts", "import { value } from '@impl';\nvalue();"),
    ]);
    let root = fixture.path().canonicalize().unwrap();
    let check: CancellationCheck = Arc::new(|| false);
    let error = PreparedProject::prepare(
        &root,
        &[root.join("app/main.ts")],
        &TypeScriptToolchain::default(),
        Duration::from_secs(30),
        &check,
    )
    .unwrap_err();
    assert!(error.to_string().contains("有效 paths 映射越界"));
}

/// 原生配置成员必须注册为 configured 项目，模板别名需返回消费目录中的精确声明。
#[test]
#[ignore = "需要固定工具"]
fn fixed_service_config_dir_templates_resolve_consumer_declaration() {
    for inherited in [false, true] {
        let fixture = crate::structure_review::config_dir_fixture::fixture(inherited);
        let root = fixture.path().canonicalize().unwrap();
        let mut session = start(&root, &["app/src/main.ts", "app/src/dep.ts"]);
        let name = session.project.workspace_root.join("app/tsconfig.json");
        for association in &session.project.files {
            assert_eq!(association.projects, vec![name.clone()]);
        }
        let file = session.project.workspace_root.join("app/src/main.ts");
        let source = session.project.source(&file).unwrap();
        let position = source.position(source.rfind("target").unwrap()).unwrap();
        let definitions = session
            .definitions(&file, position, &name)
            .and_then(checked_definitions)
            .unwrap();
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].path, root.join("app/src/dep.ts"));
        session.shutdown().unwrap();
    }
}
