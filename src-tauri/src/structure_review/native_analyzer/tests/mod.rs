//! 公共图校验及真实 Node helper 生命周期夹具在支持平台共用。

use super::*;
#[cfg(windows)]
use crate::child_process::test_support::{assert_completed, FixtureObservation, ObservedProcess};
use crate::child_process::test_support::{node_executable as test_node, wait_file};
use crate::structure_review::code_graph::{AnalysisOptions, NodeId, ReferenceTarget, SymbolScope};
use serde_json::{json, Value};
use std::{fs, path::Path, time::Instant};

/// 为每个测试隔离仓库、工具和平台目录，防止状态相互污染。
struct Fixture {
    _directory: tempfile::TempDir,
    repository: PathBuf,
    tools: PathBuf,
    platform: PathBuf,
}

impl Fixture {
    /// 写入合成源码和独立假 helper，绝不复制或修改真实产品源码。
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("repository");
        let tools = directory.path().join("tools");
        let platform = directory.path().join("platform");
        for path in [&repository, &tools, &platform] {
            fs::create_dir(path).unwrap();
        }
        fs::write(
            repository.join("中文.go"),
            "package fixture\nfunc Run() {}\n",
        )
        .unwrap();
        fs::write(repository.join("empty.go"), "").unwrap();
        fs::write(repository.join("go.mod"), "module fixture\ngo 1.23.8\n").unwrap();
        fs::write(
            tools.join("fake-helper.mjs"),
            include_str!("fake-helper.mjs"),
        )
        .unwrap();
        fs::write(tools.join("executable"), "fixture").unwrap();
        Self {
            _directory: directory,
            repository,
            tools,
            platform,
        }
    }

    /// 基础选集包含 Unicode 文件名和空文件，二者均要求覆盖。
    fn input(&self) -> AnalysisInput {
        AnalysisInput {
            repository_root: self.repository.clone(),
            files: vec!["中文.go".into(), "empty.go".into()],
            options: AnalysisOptions::default(),
        }
    }

    /// 使用单一明确的 Go SDK 平台验证来源关系，不运行该 SDK。
    fn analyzer(&self) -> NativeLanguageAnalyzer {
        NativeLanguageAnalyzer::new(
            HelperCommand {
                executable: self.tools.join("executable"),
                args: vec![],
                environment: BTreeMap::new(),
                trusted_roots: vec![self.tools.clone()],
            },
            HelperIdentity {
                helper_id: "narrant-go".into(),
                helper_version: "0.1.0".into(),
                projection_revision: "go-source-v1".into(),
                languages: vec!["go".into()],
                profile: "go-static-v1".into(),
            },
            vec![Platform {
                id: "sdk".into(),
                kind: "go-sdk".into(),
                version: "1.23.8".into(),
                root: self.platform.clone(),
                metadata: json!({"goos":"linux","goarch":"amd64","goamd64":"v1"}),
            }],
        )
    }

    /// 准备真实 Node 子进程，短期限使错误路径测试保持有界。
    fn executable_analyzer(&self, mode: &str, ready: &str, result: &str) -> NativeLanguageAnalyzer {
        fs::write(self.tools.join("ready"), ready).unwrap();
        fs::write(self.tools.join("result"), result).unwrap();
        let mut analyzer = self.analyzer();
        analyzer.command.executable = test_node();
        // 与生产 from_pack 相同，Windows runtime 的必需变量由调用方明确提供；
        // 宿主继续 env_clear，不为测试重新继承 PATH 或运行时注入参数。
        #[cfg(windows)]
        {
            let system_root = std::env::var_os("SystemRoot")
                .filter(|value| !value.is_empty())
                .expect("Windows Node 夹具需要明确且非空的 SystemRoot");
            analyzer
                .command
                .environment
                .insert("SystemRoot".into(), system_root);
        }
        analyzer
            .command
            .trusted_roots
            .push(analyzer.command.executable.parent().unwrap().to_path_buf());
        analyzer.command.args = vec![
            self.tools.join("fake-helper.mjs").into_os_string(),
            self.tools.clone().into_os_string(),
            mode.into(),
        ];
        analyzer.limits = AnalysisLimits {
            startup_timeout: Duration::from_secs(2),
            analysis_timeout: Duration::from_millis(600),
            cancellation_grace: Duration::from_millis(100),
            exit_timeout: Duration::from_millis(250),
            max_result_bytes: 1024 * 1024,
            max_diagnostic_bytes: 256 * 1024,
        };
        analyzer
    }

    /// 直接调用生产解码、输入校验与构图，验证来源和公共图约束。
    fn build(&self, result: Value) -> Result<CodeGraph> {
        let analyzer = self.analyzer();
        let checked = input::validate(&analyzer, &self.input())?;
        let protocol::Message::Ok(completed) =
            protocol::decode(&serde_json::to_vec(&result).unwrap())?
        else {
            panic!("fixture 不是成功结果")
        };
        graph::build(completed, &checked, &analyzer.identity)
    }
}

/// 固定 ready 与支持包身份完全一致。
fn ready() -> Value {
    json!({"protocol":protocol::PROTOCOL,"type":"ready","helper_id":"narrant-go","helper_version":"0.1.0","projection_revision":"go-source-v1","languages":["go"],"profiles":["go-static-v1"]})
}

/// 创建完整的规范节点，含转义换行、中文及外部/未知关系。
fn node(id: &str) -> Value {
    json!({"category":"callable","id":id,"language":"go","native_kind":"function","name":"函数🙂","signature":"func Run()","code":"print(\"中文\\n\")\n","source":"func Run() {}\n","contains":[],"references":[],"extensions":{"go.receiver":null}})
}

/// 成功 DTO 覆盖空文件且每个节点都具有真实选集出处。
fn result() -> Value {
    json!({"protocol":protocol::PROTOCOL,"type":"completed","id":protocol::REQUEST_ID,"outcome":"ok","graph":{"root_contains":["go:Run"],"nodes":[node("go:Run")],"external_symbols":[]},"evidence":{"configurations":[{"id":"main","source_paths":["go.mod"],"platform_ids":["sdk"]}],"files":[{"path":"中文.go","disposition":"analyzed","configuration_ids":["main"]},{"path":"empty.go","disposition":"analyzed","configuration_ids":["main"]}],"origins":[{"node_id":"go:Run","selected_paths":["中文.go"]}]}})
}

/// 一行一个 JSON，确保测试协议含末尾换行。
fn line(value: Value) -> String {
    format!("{value}\n")
}

/// 同步协议入口返回后立即检查原始根 HANDLE，不用 PID 重查或观察线程 join 补等。
fn analyze(analyzer: &NativeLanguageAnalyzer, input: &AnalysisInput) -> Result<CodeGraph> {
    #[cfg(windows)]
    {
        use crate::child_process::windows_validation::{with_validation, ValidationOptions};
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        with_validation(ValidationOptions::default(), |observations| {
            let result = analyzer.analyze(input);
            let statuses = observations.root_statuses();
            assert_eq!(statuses, [WAIT_OBJECT_0], "实际启动的根进程必须同步完成");
            result
        })
    }
    #[cfg(not(windows))]
    {
        analyzer.analyze(input)
    }
}

/// 生产校验保留 Unicode、空文件覆盖和引用环，并映射图外来源供 UI 筛选。
#[test]
fn graph_accepts_complete_coverage_reference_cycles_and_external_descriptors() {
    let fixture = Fixture::new();
    let mut value = result();
    value["graph"]["nodes"][0]["references"] = json!([
        {"semantic":"code.calls","target":{"kind":"node","id":"go:Run"}},
        {"semantic":"code.calls","target":{"kind":"external","symbol":"go:fmt#Println"}},
        {"semantic":"code.calls","target":{"kind":"unresolved","name":"Missing.Call"}}
    ]);
    value["graph"]["external_symbols"] =
        json!([{"symbol":"go:fmt#Println","name":"fmt.Println","scope":"dependency"}]);
    let graph = fixture.build(value).unwrap();
    let data = graph.node(&NodeId("go:Run".into())).unwrap().data();
    assert_eq!(data.name.as_deref(), Some("函数🙂"));
    assert!(data
        .references
        .iter()
        .any(|r| matches!(r.target, ReferenceTarget::Node(_))));
    assert_eq!(
        graph.external_symbols()["go:fmt#Println"].scope,
        SymbolScope::Dependency
    );
}

/// G1 与 S3 的每个非法变体都必须失败，不能交付部分图或空图。
#[test]
fn rejects_invalid_graphs_and_cross_evidence() {
    let fixture = Fixture::new();
    for case in 0..19 {
        let mut value = result();
        match case {
            0 => {
                value["graph"]["nodes"]
                    .as_array_mut()
                    .unwrap()
                    .push(node("go:Run"));
            }
            1 => {
                value["graph"]["nodes"][0]["references"] =
                    json!([{"semantic":"code.calls","target":{"kind":"node","id":"go:missing"}}])
            }
            2 => value["graph"]["root_contains"] = json!(["go:Run", "go:Run"]),
            3 => value["graph"]["nodes"][0]["contains"] = json!(["go:Run"]),
            4 => value["graph"]["root_contains"] = json!([]),
            5 => {
                let duplicate = value["evidence"]["configurations"][0].clone();
                value["evidence"]["configurations"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            6 => value["evidence"]["configurations"][0]["platform_ids"] = json!(["unapproved"]),
            7 => value["evidence"]["files"][0]["disposition"] = json!("excluded_test"),
            8 => {
                value["evidence"]["files"].as_array_mut().unwrap().pop();
            }
            9 => value["evidence"]["origins"][0]["selected_paths"] = json!(["outside.go"]),
            10 => value["evidence"]["origins"] = json!([]),
            11 => value["evidence"]["files"][0]["configuration_ids"] = json!([]),
            12 => value["evidence"]["configurations"][0]["source_paths"] = json!(["missing.mod"]),
            13 => value["graph"]["nodes"][0]["language"] = json!("java"),
            14 => value["graph"]["nodes"][0]["extensions"] = json!({"unscoped":1}),
            15 => {
                value["graph"]["external_symbols"] =
                    json!([{"symbol":"go:extra","name":"extra","scope":"repository"}])
            }
            16 => {
                value["graph"]["nodes"][0]["references"] = json!([{"semantic":"code.calls","target":{"kind":"external","symbol":"go:missing"}}])
            }
            17 => value["evidence"]["origins"][0]["selected_paths"] = json!(["中文.go", "中文.go"]),
            18 => {
                value["graph"]["nodes"][0]["references"] = json!([{"semantic":"code.calls","target":{"kind":"external","symbol":"go:Run"}}]);
                value["graph"]["external_symbols"] =
                    json!([{"symbol":"go:Run","name":"Run","scope":"repository"}]);
            }
            _ => unreachable!(),
        }
        assert!(fixture.build(value).is_err(), "非法变体 {case} 被接受");
    }
}

/// 所有嵌套对象拒绝重复键，nullable 字段仍必须出现。
#[test]
fn protocol_rejects_duplicates_unknown_fields_types_and_missing_nullable() {
    assert!(protocol::decode(br#"{"protocol":"narrant.native-analyzer/1","protocol":"narrant.native-analyzer/1","type":"ready"}"#).is_err());
    let mut value = result();
    value["graph"]["nodes"][0]
        .as_object_mut()
        .unwrap()
        .remove("name");
    assert!(protocol::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    for mut value in [result(), ready()] {
        value["unexpected"] = json!(true);
        assert!(protocol::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let duplicate = line(result()).replace(
        "\"go.receiver\":null",
        "\"go.receiver\":null,\"go.receiver\":1",
    );
    assert!(protocol::decode(duplicate.as_bytes()).is_err());
    let mut value = result();
    value["graph"]["nodes"][0]["contains"] = json!(false);
    assert!(protocol::decode(&serde_json::to_vec(&value).unwrap()).is_err());
}

/// 路径与工具校验在启动之前发生，不信任字符串前缀或宿主平台路径语法。
#[test]
fn input_rejects_unsafe_paths_platforms_and_tool_roots() {
    let fixture = Fixture::new();
    for path in [
        "../outside.go",
        "/absolute.go",
        "C:/drive.go",
        "\\\\server\\share",
        "a//b",
        "./中文.go",
        "中文.go\0",
    ] {
        let mut input = fixture.input();
        input.files = vec![path.into()];
        assert!(
            input::validate(&fixture.analyzer(), &input).is_err(),
            "{path:?}"
        );
    }
    let mut input = fixture.input();
    input.files.push("中文.go".into());
    assert!(input::validate(&fixture.analyzer(), &input).is_err());
    let mut analyzer = fixture.analyzer();
    analyzer.platforms.push(analyzer.platforms[0].clone());
    assert!(input::validate(&analyzer, &fixture.input()).is_err());
    let mut analyzer = fixture.analyzer();
    analyzer.platforms[0].metadata["command"] = json!("malicious");
    assert!(input::validate(&analyzer, &fixture.input()).is_err());
    let mut analyzer = fixture.analyzer();
    analyzer.command.trusted_roots = vec![fixture.repository.clone()];
    assert!(input::validate(&analyzer, &fixture.input()).is_err());
}

/// 分离的 Windows SDK 与 MSVC 标准头目录必须逐根批准，不能通过任意路径或错误 kind 扩大来源。
#[test]
fn clang_catalog_requires_exact_approved_additional_header_ids() {
    let fixture = Fixture::new();
    let mut analyzer = fixture.analyzer();
    let root = analyzer.platforms[0].root.clone();
    analyzer.platforms = vec![
        Platform {
            id: "target".into(),
            kind: "clang-target".into(),
            version: "21.1.8".into(),
            root: root.clone(),
            metadata: json!({"target_triple":"x86_64-pc-windows-msvc","system_include_directories":[],"resource_platform_id":"resource","additional_system_platform_ids":["msvc","sdk"]}),
        },
        Platform {
            id: "resource".into(),
            kind: "clang-resource".into(),
            version: "21.1.8".into(),
            root: root.clone(),
            metadata: json!({}),
        },
        Platform {
            id: "msvc".into(),
            kind: "clang-system-headers".into(),
            version: "14".into(),
            root: root.clone(),
            metadata: json!({}),
        },
        Platform {
            id: "sdk".into(),
            kind: "clang-system-headers".into(),
            version: "10".into(),
            root,
            metadata: json!({}),
        },
    ];
    assert!(input::validate(&analyzer, &fixture.input()).is_ok());
    for invalid in [
        json!(["msvc", "msvc"]),
        json!([""]),
        json!(["missing"]),
        json!(["target"]),
        json!(["resource"]),
        json!(["C:/Program Files"]),
        json!([1]),
        json!("msvc"),
    ] {
        let mut invalid_analyzer = analyzer.clone();
        invalid_analyzer.platforms[0].metadata["additional_system_platform_ids"] = invalid;
        assert!(input::validate(&invalid_analyzer, &fixture.input()).is_err());
    }
    analyzer.platforms[2].metadata = json!({"flags":["-include","project-plugin.h"]});
    assert!(input::validate(&analyzer, &fixture.input()).is_err());
}

/// 仓外链接和同一选中文件的链接别名必须在启动前拒绝。
#[cfg(unix)]
#[test]
fn rejects_outbound_symlinks_and_duplicate_aliases() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink(
        fixture.tools.join("executable"),
        fixture.repository.join("outside.go"),
    )
    .unwrap();
    let mut input = fixture.input();
    input.files.push("outside.go".into());
    assert!(input::validate(&fixture.analyzer(), &input).is_err());
    std::os::unix::fs::symlink(
        fixture.repository.join("中文.go"),
        fixture.repository.join("alias.go"),
    )
    .unwrap();
    input.files.pop();
    input.files.push("alias.go".into());
    assert!(input::validate(&fixture.analyzer(), &input).is_err());
}

/// P1 极小分片与 stderr 管道填满仍能完成，传入环境不继承宿主注入值。
#[test]
fn subprocess_handles_unicode_fragments_and_drains_stderr() {
    for mode in ["fragment", "stderr_ok", "ok"] {
        let fixture = Fixture::new();
        let analyzer = fixture.executable_analyzer(mode, &line(ready()), &line(result()));
        let graph = analyze(&analyzer, &fixture.input()).unwrap();
        assert_eq!(graph.root().contains, vec![NodeId("go:Run".into())]);
        let request: Value =
            serde_json::from_slice(&fs::read(fixture.tools.join("request")).unwrap()).unwrap();
        assert_eq!(request["input"]["files"], json!(["empty.go", "中文.go"]));
        assert!(
            !Path::new(request["environment"]["scratch_root"].as_str().unwrap()).exists(),
            "返回前 scratch 必须释放"
        );
    }
}

/// 同一真实 Session 验证 runtime 随机源可用、显式环境已传入且未恢复宿主注入环境。
#[test]
fn subprocess_initializes_crypto_with_explicit_minimal_runtime_environment() {
    let fixture = Fixture::new();
    let mut analyzer =
        fixture.executable_analyzer("runtime_environment", &line(ready()), &line(result()));
    assert_eq!(
        analyzer.command.environment.len(),
        usize::from(cfg!(windows))
    );
    analyzer.command.environment.insert(
        "NARRANT_NATIVE_ENV_SENTINEL".into(),
        "explicit-fixture-value".into(),
    );
    let graph = analyze(&analyzer, &fixture.input()).unwrap();
    assert_eq!(graph.root().contains, vec![NodeId("go:Run".into())]);
    let observed: Value = serde_json::from_slice(
        &fs::read(fixture.tools.join("runtime-environment-status")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        observed,
        json!({
            "crypto_ready": true,
            "explicit_marker_received": true,
            "system_root_present": cfg!(windows),
            "runtime_injection_absent": true,
            "scratch_matches_request": true,
        })
    );
    #[cfg(unix)]
    assert_process_gone(&fixture.tools.join("pid"));
}

/// stdout 先关闭时仍保留随后到达的运行时启动诊断及原始协议错误。
#[test]
fn subprocess_preserves_early_eof_diagnostics() {
    for mode in ["startup_diagnostic", "startup_stderr_first"] {
        let fixture = Fixture::new();
        let error = analyze(
            &fixture.executable_analyzer(mode, &line(ready()), &line(result())),
            &fixture.input(),
        )
        .unwrap_err();
        assert!(error.message.contains("完整 completed"), "{error}");
        assert!(
            error
                .message
                .contains("unique-runtime-initialization-failure"),
            "{error}"
        );
        #[cfg(unix)]
        assert_process_gone(&fixture.tools.join("pid"));
    }
}

/// P2 错误阶段、双终结、半行、异常退出和无取消的 cancelled 均是硬失败。
#[test]
fn subprocess_rejects_invalid_protocol_lifecycle() {
    for mode in [
        "premature",
        "eof",
        "half",
        "duplicate_ready",
        "double",
        "nonzero",
    ] {
        let fixture = Fixture::new();
        let error = analyze(
            &fixture.executable_analyzer(mode, &line(ready()), &line(result())),
            &fixture.input(),
        )
        .unwrap_err();
        assert!(!error.is_cancelled(), "{mode}: {error}");
    }
    for mutation in 0..5 {
        let fixture = Fixture::new();
        let mut value = result();
        match mutation {
            0 => value["id"] = json!("wrong"),
            1 => value["protocol"] = json!("narrant.native-analyzer/2"),
            2 => value["type"] = json!("log"),
            3 => {
                value = json!({"protocol":protocol::PROTOCOL,"type":"completed","id":protocol::REQUEST_ID,"outcome":"cancelled"})
            }
            4 => {
                value["error"] = json!({"code":"internal_failure","message":"conflict","paths":[]})
            }
            _ => unreachable!(),
        }
        assert!(analyze(
            &fixture.executable_analyzer("ok", &line(ready()), &line(value)),
            &fixture.input()
        )
        .is_err());
    }
}

/// P3 资源、启动、分析及退出超时都在有界时间内杀死真实 helper。
#[test]
fn subprocess_bounds_resources_and_all_timeout_stages() {
    for mode in [
        "startup_stall",
        "stall",
        "exit_stall",
        "stderr",
        "oversized",
    ] {
        let fixture = Fixture::new();
        let mut analyzer = fixture.executable_analyzer(mode, &line(ready()), &line(result()));
        if mode == "oversized" {
            analyzer.limits.max_result_bytes = 256;
        }
        let started = Instant::now();
        assert!(analyze(&analyzer, &fixture.input()).is_err(), "{mode}");
        assert!(started.elapsed() < Duration::from_secs(5), "{mode}");
        #[cfg(unix)]
        assert_process_gone(&fixture.tools.join("pid"));
    }
}

/// 取消请求真实写到 stdin，不合作 helper 也必须回收后才返回取消。
#[test]
fn subprocess_cooperative_and_forced_cancellation() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for mode in ["cancel", "stall", "cancel_exit", "cancel_exit_zero"] {
        let fixture = Fixture::new();
        let value = json!({"protocol":protocol::PROTOCOL,"type":"completed","id":protocol::REQUEST_ID,"outcome":"cancelled"});
        let cancellation = Arc::new(AtomicBool::new(false));
        let observed = cancellation.clone();
        let analyzer = fixture
            .executable_analyzer(mode, &line(ready()), &line(value))
            .with_cancellation(Arc::new(move || observed.load(Ordering::Acquire)));
        let marker = fixture.tools.join("request");
        let cancelling = std::thread::spawn(move || {
            wait_file(&marker);
            cancellation.store(true, Ordering::Release);
        });
        let error = analyze(&analyzer, &fixture.input()).unwrap_err();
        cancelling.join().unwrap();
        assert!(error.is_cancelled(), "{mode}: {error}");
        if mode == "cancel" {
            let cancel: Value =
                serde_json::from_slice(&fs::read(fixture.tools.join("cancel")).unwrap()).unwrap();
            assert_eq!(cancel["type"], "cancel");
            assert_eq!(cancel["id"], protocol::REQUEST_ID);
        }
        #[cfg(unix)]
        assert_process_gone(&fixture.tools.join("pid"));
    }
}

/// helper 父进程已退出时，有或没有继承管道的遗留后代都必须在返回前消失。
#[test]
fn subprocess_reaps_descendants_after_parent_exit() {
    for mode in ["descendant", "descendant_closed"] {
        let fixture = Fixture::new();
        #[cfg(windows)]
        let mut observation =
            FixtureObservation::start(&fixture.tools, ["pid", "descendant", "observed"], |_, _| {
                Ok(())
            });
        let result = fixture
            .executable_analyzer(mode, &line(ready()), &line(result()))
            .analyze(&fixture.input());
        #[cfg(windows)]
        let completion = observation.completion();
        #[cfg(windows)]
        assert_completed(completion);
        if mode == "descendant" {
            let error = result.unwrap_err();
            assert!(error.message.contains("退出超时"), "{error}");
        } else {
            result.unwrap();
        }
        #[cfg(unix)]
        assert_process_gone(&fixture.tools.join("pid"));
        #[cfg(unix)]
        assert_process_gone(&fixture.tools.join("descendant"));
        #[cfg(windows)]
        observation.join();
    }
}

/// 真正的后端错误优先于紧随其后的非零退出与退出超时。
#[test]
fn subprocess_preserves_backend_failure_over_exit_failure() {
    for mode in ["nonzero", "exit_stall"] {
        let fixture = Fixture::new();
        let value = json!({"protocol":protocol::PROTOCOL,"type":"completed","id":protocol::REQUEST_ID,"outcome":"error","error":{"code":"union_conflict","message":"真实归并冲突","paths":["中文.go"]}});
        let error = analyze(
            &fixture.executable_analyzer(mode, &line(ready()), &line(value)),
            &fixture.input(),
        )
        .unwrap_err();
        assert!(error.message.contains("真实归并冲突"), "{error}");
        assert!(!error.is_cancelled());
    }
}

/// 返回即检查进程已消失，测试自身不额外轮询以免掩盖生命周期缺口。
#[cfg(unix)]
fn assert_process_gone(marker: &Path) {
    let pid = fs::read_to_string(marker).unwrap().parse::<i32>().unwrap();
    // SAFETY: PID 来自本次独立夹具，信号 0 只查询而不终止进程。
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "进程 {pid} 仍存活");
}

/// panic 展开和显式 Drop 都先杀死并回收真实后代，再归还同步调用。
#[test]
fn session_drop_and_panic_reap_all_children() {
    for panic in [false, true] {
        let fixture = Fixture::new();
        #[cfg(windows)]
        let mut observation =
            FixtureObservation::start(&fixture.tools, ["pid", "descendant", "observed"], |_, _| {
                Ok(())
            });
        let analyzer = fixture.executable_analyzer("descendant", &line(ready()), &line(result()));
        let checked = input::validate(&analyzer, &fixture.input()).unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let request = checked.request(&analyzer, scratch.path()).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let session = process::Session::start(&analyzer, &checked, scratch.path()).unwrap();
            session.send(&request).unwrap();
            wait_file(&fixture.tools.join("descendant"));
            #[cfg(windows)]
            wait_file(&fixture.tools.join("observed"));
            assert!(!panic, "测试期望的宿主 panic");
            drop(session);
        }));
        #[cfg(windows)]
        let completion = observation.completion();
        #[cfg(windows)]
        assert_completed(completion);
        assert_eq!(result.is_err(), panic);
        #[cfg(unix)]
        assert_process_gone(&fixture.tools.join("pid"));
        #[cfg(unix)]
        assert_process_gone(&fixture.tools.join("descendant"));
        #[cfg(windows)]
        observation.join();
    }
}

/// 终结前 Host 关闭 stdin，合作 helper 必须自行停止，不等分析超时。
#[test]
fn helper_observes_host_stdin_eof_before_terminal() {
    let fixture = Fixture::new();
    let analyzer = fixture.executable_analyzer("stall", &line(ready()), &line(result()));
    let checked = input::validate(&analyzer, &fixture.input()).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let mut session = process::Session::start(&analyzer, &checked, scratch.path()).unwrap();
    session
        .send(&checked.request(&analyzer, scratch.path()).unwrap())
        .unwrap();
    wait_file(&fixture.tools.join("request"));
    #[cfg(windows)]
    let root = ObservedProcess::marker(
        &fixture.tools.join("pid"),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    session.close_input();
    wait_file(&fixture.tools.join("host-eof"));
    drop(session);
    #[cfg(windows)]
    let immediate = root.status();
    #[cfg(windows)]
    assert_eq!(immediate, windows_sys::Win32::Foundation::WAIT_OBJECT_0);
    #[cfg(unix)]
    assert_process_gone(&fixture.tools.join("pid"));
}

/// helper 完全不读 stdin 时，大请求也不能卡住主线程或阻止超时回收。
#[test]
fn subprocess_bounds_blocked_stdin_writer() {
    let fixture = Fixture::new();
    let analyzer = fixture.executable_analyzer("no_read", &line(ready()), &line(result()));
    let mut input = fixture.input();
    for index in 0..800 {
        let name = format!("{:0140}-{index}.go", 0);
        fs::write(fixture.repository.join(&name), "").unwrap();
        input.files.push(name.into());
    }
    let started = Instant::now();
    let error = analyze(&analyzer, &input).unwrap_err();
    assert!(error.message.contains("分析超时"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
    #[cfg(unix)]
    assert_process_gone(&fixture.tools.join("pid"));
}

#[path = "migration.rs"]
mod migration;

mod rust_graph;

/// 即使已发送 cancel，实际后端错误或无效协议也不能改称取消成功。
#[test]
fn subprocess_cancel_preserves_observed_backend_and_protocol_failures() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for mode in ["cancel_failure", "cancel_invalid"] {
        let fixture = Fixture::new();
        let value = json!({"protocol":protocol::PROTOCOL,"type":"completed","id":protocol::REQUEST_ID,"outcome":"error","error":{"code":"union_conflict","message":"真实归并冲突","paths":["中文.go"]}});
        let cancellation = Arc::new(AtomicBool::new(false));
        let observed = cancellation.clone();
        let analyzer = fixture
            .executable_analyzer(mode, &line(ready()), &line(value))
            .with_cancellation(Arc::new(move || observed.load(Ordering::Acquire)));
        let marker = fixture.tools.join("request");
        let trigger = std::thread::spawn(move || {
            wait_file(&marker);
            cancellation.store(true, Ordering::Release);
        });
        let error = analyze(&analyzer, &fixture.input()).unwrap_err();
        trigger.join().unwrap();
        assert!(!error.is_cancelled(), "{mode}: {error}");
        if mode == "cancel_failure" {
            assert!(error.message.contains("真实归并冲突"), "{error}");
        }
        #[cfg(unix)]
        assert_process_gone(&fixture.tools.join("pid"));
    }
}
