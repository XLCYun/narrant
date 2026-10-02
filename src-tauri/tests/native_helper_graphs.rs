//! 真实原生 helper 经公开 Host API 构建 CodeGraph 的端到端验收。
//!
//! 需要已固定的 helper 与目标平台 catalog；通过 `--ignored` 显式执行。

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use narrant_lib::structure_review::{
    code_graph::{
        AnalysisInput, AnalysisOptions, ChangeKind, CodeCategory, CodeGraph, LanguageAnalyzer,
        NodeId, ReferenceTarget, SymbolScope,
    },
    native_analyzer::{HelperCommand, HelperIdentity, NativeLanguageAnalyzer, Platform},
};
use serde::Deserialize;
use tempfile::TempDir;

/// 本地构件清单只提供受信可执行文件和目标声明，测试本身固定 profile 身份。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HelperConfig {
    executable: PathBuf,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
    trusted_roots: Vec<PathBuf>,
    platforms: Vec<PlatformConfig>,
}

/// 与公开 Host Platform 字段一一对应的 JSON 输入。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlatformConfig {
    id: String,
    kind: String,
    version: String,
    root: PathBuf,
    fingerprint: String,
    metadata: serde_json::Value,
}

/// 一个 fixture 的语言模式；C 与 C++ 共用同一个 Clang helper 配置。
#[derive(Clone, Copy)]
enum Mode {
    Go,
    Java,
    Csharp,
    C,
    Cpp,
}

impl Mode {
    /// 返回配置键、协议语言及公开 profile 所需的固定身份。
    fn identity(self) -> (&'static str, HelperIdentity) {
        let (key, helper, languages, profile, revision) = match self {
            Self::Go => (
                "go",
                "narrant-go",
                vec!["go"],
                "go-static-v1",
                "go-source-v1",
            ),
            Self::Java => (
                "java",
                "narrant-java",
                vec!["java"],
                "java-static-v1",
                "java-source-v1",
            ),
            Self::Csharp => (
                "csharp",
                "narrant-csharp",
                vec!["csharp"],
                "csharp-static-v1",
                "csharp-source-v1",
            ),
            Self::C | Self::Cpp => (
                "clang",
                "narrant-clang",
                vec!["c", "cpp"],
                "clang-static-v1",
                "clang-source-v1",
            ),
        };
        (
            key,
            HelperIdentity {
                helper_id: helper.into(),
                helper_version: "0.1.0".into(),
                projection_revision: revision.into(),
                languages: languages.into_iter().map(str::to_owned).collect(),
                profile: profile.into(),
            },
        )
    }

    /// 返回已知调用者、跨文件目标、变更子对象及其父对象名称。
    fn expected(
        self,
    ) -> (
        &'static str,
        &'static str,
        &'static str,
        Option<&'static str>,
    ) {
        match self {
            Self::Go => ("Entry", "Worker", "Worker", Some("worker.go")),
            Self::Java => ("entry", "value", "value", Some("Worker")),
            Self::Csharp => ("Entry", "Value", "Value", Some("Worker")),
            Self::C => ("Entry", "Worker", "Worker", None),
            Self::Cpp => ("Entry", "Worker", "Value", Some("Box")),
        }
    }
}

/// 在显式执行时读取固定构件清单；没有清单是验收失败，不转换成跳过。
fn config() -> BTreeMap<String, HelperConfig> {
    let path = std::env::var_os("NARRANT_NATIVE_HELPERS_CONFIG")
        .expect("真实 helper 验收需要 NARRANT_NATIVE_HELPERS_CONFIG=<JSON 文件绝对路径>");
    let bytes = fs::read(path).expect("无法读取真实 helper 配置");
    serde_json::from_slice(&bytes).expect("真实 helper 配置 JSON 无效")
}

/// 将固定清单装配为公开 Host；执行命令与目标根仍由 Host 自行校验。
fn analyzer(mode: Mode, configs: &BTreeMap<String, HelperConfig>) -> NativeLanguageAnalyzer {
    let (key, identity) = mode.identity();
    let cfg = configs
        .get(key)
        .unwrap_or_else(|| panic!("缺少 {key} 真实 helper 配置"));
    let command = HelperCommand {
        executable: cfg.executable.clone(),
        args: cfg.args.iter().map(OsString::from).collect(),
        environment: cfg
            .environment
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect(),
        trusted_roots: cfg.trusted_roots.clone(),
    };
    let platforms = cfg
        .platforms
        .iter()
        .map(|p| Platform {
            id: p.id.clone(),
            kind: p.kind.clone(),
            version: p.version.clone(),
            root: p.root.clone(),
            fingerprint: p.fingerprint.clone(),
            metadata: p.metadata.clone(),
        })
        .collect();
    NativeLanguageAnalyzer::new(command, identity, platforms)
}

/// 在临时仓库写入静态配置、选中文件和未选上下文，返回选集与目标节点名。
fn fixture(
    mode: Mode,
    root: &Path,
    variant: &str,
    configs: &BTreeMap<String, HelperConfig>,
) -> Vec<PathBuf> {
    let body = if variant == "changed" { "2" } else { "1" };
    let formatted = variant == "formatted";
    match mode {
        Mode::Go => {
            put(root, "go.mod", "module example.test/native\ngo 1.23.0\n");
            put(
                root,
                "api.go",
                "package native\nfunc Entry() int { return Worker() + Hidden() }\n",
            );
            put(
                root,
                "hidden.go",
                "package native\nfunc Hidden() int { return 3 }\n",
            );
            put(
                root,
                "worker.go",
                &if formatted {
                    "package native\n// Ordinary comment.\nfunc Worker() int {\n    return 1\n}\n"
                        .to_owned()
                } else {
                    format!("package native\nfunc Worker() int {{ return {body} }}\n")
                },
            );
            vec!["api.go".into(), "worker.go".into()]
        }
        Mode::Java => {
            put(
                root,
                "narrant-java.json",
                r#"{"schema":"java-static-v1","release":"17","source_roots":[{"path":"src","role":"main"}]}"#,
            );
            put(root, "src/demo/Api.java", "package demo; public class Api { public static int entry() { return Worker.value() + Hidden.get(); } }\n");
            put(
                root,
                "src/demo/Hidden.java",
                "package demo; public class Hidden { public static int get() { return 3; } }\n",
            );
            put(
                root,
                "src/demo/Worker.java",
                &if formatted {
                    "package demo;\n// Ordinary comment.\npublic class Worker {\n  public static int value() {\n    return 1;\n  }\n}\n".to_owned()
                } else {
                    format!("package demo; public class Worker {{ public static int value() {{ return {body}; }} }}\n")
                },
            );
            vec!["src/demo/Api.java".into(), "src/demo/Worker.java".into()]
        }
        Mode::Csharp => {
            put(
                root,
                "narrant.csharp.json",
                r#"{"version":"csharp-static-v1","configurations":[{"id":"main","files":["Api.cs","Worker.cs","Hidden.cs"],"tfm":"net10.0","language_version":"14.0","define_constants":[],"implicit_usings":false}]}"#,
            );
            put(root, "Api.cs", "public static class Api { public static int Entry() { return Worker.Value() + Hidden.Get(); } }\n");
            put(
                root,
                "Hidden.cs",
                "public static class Hidden { public static int Get() { return 3; } }\n",
            );
            put(
                root,
                "Worker.cs",
                &if formatted {
                    "// Ordinary comment.\npublic static class Worker\n{\n    public static int Value()\n    {\n        return 1;\n    }\n}\n".to_owned()
                } else {
                    format!("public static class Worker {{ public static int Value() {{ return {body}; }} }}\n")
                },
            );
            vec!["Api.cs".into(), "Worker.cs".into()]
        }
        Mode::C | Mode::Cpp => {
            let ext = if matches!(mode, Mode::C) { "c" } else { "cpp" };
            let header = if matches!(mode, Mode::C) {
                "int Worker(void); int Hidden(void);\n"
            } else {
                "int Worker(); int Hidden();\n"
            };
            put(root, "api.h", header);
            let api = if matches!(mode, Mode::C) {
                "#include \"api.h\"\nint Entry(void) { return Worker() + Hidden(); }\n"
            } else {
                "#include \"api.h\"\nint Entry() { return Worker() + Hidden(); }\n"
            };
            put(root, &format!("api.{ext}"), api);
            let hidden = if matches!(mode, Mode::C) {
                "#include \"api.h\"\nint Hidden(void) { return 3; }\n"
            } else {
                "#include \"api.h\"\nint Hidden() { return 3; }\n"
            };
            put(root, &format!("hidden.{ext}"), hidden);
            let worker = match mode {
                Mode::C if formatted => "#include \"api.h\"\n// Ordinary comment.\nint Worker(void)\n{\n return 1;\n}\n".to_owned(),
                Mode::C => format!("#include \"api.h\"\nint Worker(void) {{ return {body}; }}\n"),
                Mode::Cpp if formatted => "#include \"api.h\"\n// Ordinary comment.\nstruct Box { static int Value() {\n return 1;\n} };\nint Worker() { return Box::Value(); }\n".to_owned(),
                Mode::Cpp => format!("#include \"api.h\"\nstruct Box {{ static int Value() {{ return {body}; }} }};\nint Worker() {{ return Box::Value(); }}\n"),
                _ => unreachable!(),
            };
            put(root, &format!("worker.{ext}"), &worker);
            let triple = configs
                .get("clang")
                .and_then(|cfg| cfg.platforms.iter().find(|p| p.kind == "clang-target"))
                .and_then(|p| p.metadata.get("target_triple"))
                .and_then(serde_json::Value::as_str)
                .expect("Clang 配置缺少 target_triple");
            let entries = ["api", "worker", "hidden"].into_iter().map(|stem| {
                let file = format!("{stem}.{ext}");
                serde_json::json!({"directory": root, "file": file,
                    "arguments": ["clang", format!("-std={}", if matches!(mode, Mode::C) { "c17" } else { "c++20" }),
                        "-target", triple, file]})
            }).collect::<Vec<_>>();
            put(
                root,
                "compile_commands.json",
                &serde_json::to_string(&entries).unwrap(),
            );
            vec![format!("api.{ext}").into(), format!("worker.{ext}").into()]
        }
    }
}

/// 保留路径与原始字节，证明分析没有改写配置、源文件或产生仓内文件。
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .map(|entry| {
            let entry = entry.expect("无法遍历 fixture 仓库");
            entry.path().strip_prefix(root).unwrap().to_path_buf()
        })
        .filter(|path| root.join(path).is_file())
        .map(|path| {
            let bytes = fs::read(root.join(&path)).expect("无法读取 fixture 字节");
            (path, bytes)
        })
        .collect()
}

/// 写入一份固定 fixture，失败时直接中止测试。
fn put(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).expect("无法建立 fixture 目录");
    fs::write(path, content).expect("无法写入 fixture");
}

/// 通过公开分析器执行完整协议，并核对源仓库路径及字节完全未变。
fn analyze(
    mode: Mode,
    configs: &BTreeMap<String, HelperConfig>,
    root: &Path,
    files: Vec<PathBuf>,
) -> CodeGraph {
    let before = snapshot(root);
    let graph = analyzer(mode, configs)
        .analyze(&AnalysisInput {
            repository_root: root.to_path_buf(),
            files,
            options: AnalysisOptions {
                include_tests: false,
            },
        })
        .unwrap_or_else(|error| panic!("{} 真实 Host 分析失败：{error}", mode.identity().0));
    assert_eq!(before, snapshot(root), "helper 修改了 fixture 路径或字节");
    graph
}

/// 从唯一根递归查找节点，避免按 helper 私有 ID 编码构造预期图。
fn find<'a>(
    graph: &'a CodeGraph,
    name: &str,
    category: CodeCategory,
) -> (
    &'a NodeId,
    &'a narrant_lib::structure_review::code_graph::CodeNode,
) {
    let mut matches = Vec::new();
    let mut pending = graph.root().contains.iter().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        let node = graph.node(id).expect("根可达节点必须可读");
        if node.category() == category && node.data().name.as_deref() == Some(name) {
            matches.push((id, node));
        }
        pending.extend(node.data().contains.iter());
    }
    assert_eq!(matches.len(), 1, "应恰有一个 {name:?} {category:?} 节点");
    matches.pop().unwrap()
}

/// 验证本侧真实绑定指向已选跨文件节点和仓内未选目标。
fn assert_calls(mode: Mode, graph: &CodeGraph) {
    let (caller, selected_target, _, _) = mode.expected();
    let (_, source) = find(graph, caller, CodeCategory::Callable);
    let (target_id, _) = find(graph, selected_target, CodeCategory::Callable);
    assert!(
        source
            .data()
            .references
            .iter()
            .any(|reference| reference.semantic == "code.calls"
                && reference.target == ReferenceTarget::Node(target_id.clone())),
        "跨文件调用未解析成图内 Node"
    );
    assert!(
        source.data().references.iter().any(|reference| {
            if reference.semantic != "code.calls" {
                return false;
            }
            match &reference.target {
                ReferenceTarget::External { symbol } => {
                    symbol.to_ascii_lowercase().contains("hidden")
                        && graph.external_symbols().get(symbol).is_some_and(|entry| {
                            entry.scope == SymbolScope::Repository && !entry.name.is_empty()
                        })
                }
                _ => false,
            }
        }),
        "仓内未选目标缺少 Repository External 描述"
    );
}

/// 对一个真实后端比较四个独立仓库快照，验收身份、内容投影和源码只读。
fn exercise(mode: Mode) {
    let configs = config();
    let sandbox = TempDir::new().expect("无法建立 fixture 临时根");
    let mut graphs = Vec::new();
    for (directory, variant) in [
        ("before", "before"),
        ("changed", "changed"),
        ("formatted", "formatted"),
        ("relocated", "before"),
    ] {
        let root = sandbox.path().join(directory);
        fs::create_dir(&root).unwrap();
        let files = fixture(mode, &root, variant, &configs);
        graphs.push(analyze(mode, &configs, &root, files));
    }
    let before = &graphs[0];
    assert_calls(mode, before);
    let (_, _, child, parent) = mode.expected();
    let (child_id, _) = find(before, child, CodeCategory::Callable);
    let changed = before.diff(&graphs[1]).expect("有效图可以比较");
    let child_diff = changed
        .nodes
        .iter()
        .find(|node| &node.id == child_id)
        .expect("正文变化必须到达子对象");
    assert_eq!(
        child_diff.content,
        Some(ChangeKind::Modified),
        "正文变化必须是自身内容变化"
    );
    assert!(
        changed
            .nodes
            .iter()
            .all(|node| node.content.is_none() || &node.id == child_id),
        "正文变化污染了祖先或相邻对象的自身内容"
    );
    if let Some(parent) = parent {
        let category = if matches!(mode, Mode::Go) {
            CodeCategory::Container
        } else {
            CodeCategory::Type
        };
        let (parent_id, _) = find(before, parent, category);
        assert!(
            changed
                .nodes
                .iter()
                .find(|node| &node.id == parent_id)
                .is_none_or(|node| node.content.is_none()),
            "子对象正文修改了父对象自身内容"
        );
    }
    let formatting_diff = before.diff(&graphs[2]).unwrap();
    assert!(
        formatting_diff.nodes.is_empty(),
        "仅排版和普通注释产生了语义 diff：{formatting_diff:?}"
    );
    assert_eq!(
        before.root().contains,
        graphs[3].root().contains,
        "迁移仓库根后顶层 ID 改变"
    );
    assert!(
        before.diff(&graphs[3]).unwrap().nodes.is_empty(),
        "迁移仓库根后图语义或 ID 改变"
    );
    assert_calls(mode, &graphs[3]);
}

/// 完整验收调用必须同时提供四个固定 helper，Clang 覆盖 C 与 C++ 两种语言模式。
#[test]
#[ignore = "require-real-tools: NARRANT_NATIVE_HELPERS_CONFIG 指向四个真实 helper 和固定目标平台"]
fn all_required_helpers_configured() {
    let configs = config();
    for key in ["go", "java", "csharp", "clang"] {
        assert!(configs.contains_key(key), "真实 helper 配置缺少 {key}");
    }
}

/// 真实 Go 后端经 Host 构图和 diff 的验收。
#[test]
#[ignore = "require-real-tools: 固定 Go helper 和 Go SDK"]
fn go_real_helper_graph_and_diff() {
    exercise(Mode::Go);
}

/// 真实 JDT 后端经 Host 构图和 diff 的验收。
#[test]
#[ignore = "require-real-tools: 固定 Java helper 和目标 JDK"]
fn java_real_helper_graph_and_diff() {
    exercise(Mode::Java);
}

/// 真实 Roslyn 后端经 Host 构图和 diff 的验收。
#[test]
#[ignore = "require-real-tools: 固定 C# helper 和 reference assemblies"]
fn csharp_real_helper_graph_and_diff() {
    exercise(Mode::Csharp);
}

/// 真实 Clang C17 后端经 Host 构图和 diff 的验收。
#[test]
#[ignore = "require-real-tools: 固定 Clang helper 和目标 resource/headers"]
fn c_real_helper_graph_and_diff() {
    exercise(Mode::C);
}

/// 真实 Clang C++20 后端经 Host 构图和 diff 的验收。
#[test]
#[ignore = "require-real-tools: 固定 Clang helper 和目标 resource/headers"]
fn cpp_real_helper_graph_and_diff() {
    exercise(Mode::Cpp);
}
