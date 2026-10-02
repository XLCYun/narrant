//! 真实原生 helper 经公开 Host API 构建 CodeGraph 的端到端验收。
//!
//! 需要已固定的 helper 与目标平台 catalog；通过 `--ignored` 显式执行。

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use narrant_lib::structure_review::{
    code_graph::{
        AnalysisInput, AnalysisOptions, ChangeKind, CodeCategory, CodeGraph, ExternalSymbol,
        LanguageAnalyzer, NodeId, ReferenceTarget, SetChange, SymbolScope,
    },
    native_analyzer::{HelperCommand, HelperIdentity, NativeLanguageAnalyzer, Platform},
};
use serde::Deserialize;
use tempfile::TempDir;

/// 本地构件清单只提供受信可执行文件和目标声明，测试本身固定 profile 身份。
#[derive(Clone, Deserialize)]
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
#[derive(Clone, Deserialize)]
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
    let environment: BTreeMap<OsString, OsString> = cfg
        .environment
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect();
    #[cfg(windows)]
    let mut environment = environment;
    #[cfg(windows)]
    if !cfg
        .environment
        .keys()
        .any(|name| name.eq_ignore_ascii_case("SystemRoot"))
    {
        // 与生产支持包入口一致，只补 Windows 运行时所需的系统根，不恢复父进程环境。
        let system_root = std::env::var_os("SystemRoot")
            .filter(|value| !value.is_empty())
            .expect("Windows 真实 helper 验收需要非空 SystemRoot");
        environment.insert("SystemRoot".into(), system_root);
    }
    let command = HelperCommand {
        executable: cfg.executable.clone(),
        args: cfg.args.iter().map(OsString::from).collect(),
        environment,
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

/// 为每种模式补充未知调用、可替换调用目标和具有直接字段的结构对象。
fn extend_fixture(mode: Mode, root: &Path, variant: &str) {
    let field = if variant == "structure" {
        "Added"
    } else {
        "Removed"
    };
    let (file, old_call, new_call, prefix, declarations) = match mode {
        Mode::Go => ("api.go", "Worker()", "AlternativeTarget()", "", format!(
            "\nfunc AlternativeTarget() int {{ return 9 }}\nfunc Unknown(fn func() int) int {{ return fn() }}\ntype Shape struct {{ {field} int }}\n")),
        Mode::Java => ("src/demo/Api.java", "Worker.value()", "Alternative.AlternativeTarget()", "", format!(
            "\nclass Alternative {{ static int AlternativeTarget() {{ return 9; }} static void Unknown() {{ missing.Type.nope(); }} }}\nclass Shape {{ int {field}; }}\n")),
        Mode::Csharp => ("Api.cs", "Worker.Value()", "Alternative.AlternativeTarget()", "", format!(
            "\nstatic class Alternative {{ public static int AlternativeTarget() {{ return 9; }} static int Unknown(System.Func<int> fn) {{ return fn(); }} }}\nclass Shape {{ int {field}; }}\n")),
        Mode::C => ("api.c", "Worker()", "AlternativeTarget()", "int AlternativeTarget(void) { return 9; }\n", format!(
            "\nint Unknown(int (*fn)(void)) {{ return fn(); }}\nstruct Shape {{ int {field}; }};\n")),
        Mode::Cpp => ("api.cpp", "Worker()", "AlternativeTarget()", "int AlternativeTarget() { return 9; }\n", format!(
            "\nint Unknown(int (*fn)()) {{ return fn(); }}\nstruct Shape {{ int {field}; }};\n")),
    };
    let original = fs::read_to_string(root.join(file)).expect("无法读取待扩展 fixture");
    let source = if variant == "relations" {
        original.replace(old_call, new_call)
    } else {
        original
    };
    put(root, file, &format!("{prefix}{source}{declarations}"));
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

/// 未知调用必须保留 Unresolved，不能猜测成图内对象或 External。
fn assert_unknown(graph: &CodeGraph) {
    let (_, source) = find(graph, "Unknown", CodeCategory::Callable);
    assert!(source.data().references.iter().any(|reference| {
        reference.semantic == "code.calls"
            && matches!(&reference.target, ReferenceTarget::Unresolved { name } if !name.is_empty())
    }), "未知调用缺少具名 Unresolved：{:?}", source.data().references);
}

/// 调用改绑必须在同一个稳定调用者上报告删除旧端点和增加新端点。
fn assert_reference_diff(mode: Mode, before: &CodeGraph, after: &CodeGraph) {
    let (caller, selected, _, _) = mode.expected();
    let (caller_id, _) = find(before, caller, CodeCategory::Callable);
    let (old_target, _) = find(before, selected, CodeCategory::Callable);
    let (new_target, _) = find(after, "AlternativeTarget", CodeCategory::Callable);
    assert_eq!(
        caller_id,
        find(after, caller, CodeCategory::Callable).0,
        "调用者身份随调用端点改变"
    );
    let diff = before.diff(after).expect("关系改绑后的有效图可以比较");
    let changed = diff
        .nodes
        .iter()
        .find(|node| &node.id == caller_id)
        .expect("调用者缺少关系 diff");
    assert!(changed.references.iter().any(|change| matches!(change,
        SetChange::Removed(reference) if reference.semantic == "code.calls" && reference.target == ReferenceTarget::Node(old_target.clone()))),
        "关系 diff 没有移除原调用端点：{changed:?}");
    assert!(changed.references.iter().any(|change| matches!(change,
        SetChange::Added(reference) if reference.semantic == "code.calls" && reference.target == ReferenceTarget::Node(new_target.clone()))),
        "关系 diff 没有新增替换调用端点：{changed:?}");
}

/// 删除与新增字段须同时体现在节点内容和所属 Type 的直接 contains 变化中。
fn assert_structure_diff(before: &CodeGraph, after: &CodeGraph) {
    let (owner, _) = find(before, "Shape", CodeCategory::Type);
    let (removed, _) = find(before, "Removed", CodeCategory::Value);
    let (added, _) = find(after, "Added", CodeCategory::Value);
    assert_eq!(
        owner,
        find(after, "Shape", CodeCategory::Type).0,
        "字段变化改变了父对象身份"
    );
    assert!(after.node(removed).is_none(), "删除对象仍留在后侧图");
    assert!(before.node(added).is_none(), "新增对象提前出现在前侧图");
    let diff = before.diff(after).expect("结构改变后的有效图可以比较");
    let parent = diff
        .nodes
        .iter()
        .find(|node| &node.id == owner)
        .expect("父对象缺少 contains diff");
    assert!(
        parent
            .contains
            .contains(&SetChange::Removed(removed.clone())),
        "contains 未移除原字段：{parent:?}"
    );
    assert!(
        parent.contains.contains(&SetChange::Added(added.clone())),
        "contains 未新增后侧字段：{parent:?}"
    );
    assert_eq!(
        diff.nodes
            .iter()
            .find(|node| &node.id == removed)
            .and_then(|node| node.content),
        Some(ChangeKind::Removed)
    );
    assert_eq!(
        diff.nodes
            .iter()
            .find(|node| &node.id == added)
            .and_then(|node| node.content),
        Some(ChangeKind::Added)
    );
}

/// 对一个真实后端比较六个独立仓库快照，验收身份、三维 diff、未知关系和源码只读。
fn exercise(mode: Mode) {
    let configs = config();
    let sandbox = TempDir::new().expect("无法建立 fixture 临时根");
    let mut graphs = Vec::new();
    for (directory, variant) in [
        ("before", "before"),
        ("changed", "changed"),
        ("formatted", "formatted"),
        ("relocated", "before"),
        ("relations", "relations"),
        ("structure", "structure"),
    ] {
        let root = sandbox.path().join(directory);
        fs::create_dir(&root).unwrap();
        let files = fixture(mode, &root, variant, &configs);
        extend_fixture(mode, &root, variant);
        graphs.push(analyze(mode, &configs, &root, files));
    }
    let before = &graphs[0];
    assert_calls(mode, before);
    for graph in &graphs {
        assert_unknown(graph);
    }
    assert_reference_diff(mode, before, &graphs[4]);
    assert_structure_diff(before, &graphs[5]);
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

/// 保留单语言图中验证过的真实 ID，供合并图核对同名调用归属。
#[derive(Debug, PartialEq, Eq)]
struct MixedEvidence {
    language: &'static str,
    caller: NodeId,
    target: NodeId,
    changed: NodeId,
    external_symbol: String,
    external: ExternalSymbol,
}

/// 在尚未合并的语言图中记录身份，避免跨语言同名对象被全局名称搜索误配。
fn mixed_evidence(mode: Mode, graph: &CodeGraph) -> MixedEvidence {
    assert_calls(mode, graph);
    let (caller_name, target_name, changed_name, _) = mode.expected();
    let (caller, source) = find(graph, caller_name, CodeCategory::Callable);
    let (target, _) = find(graph, target_name, CodeCategory::Callable);
    let (changed, _) = find(graph, changed_name, CodeCategory::Callable);
    let language = match mode {
        Mode::Go => "go",
        Mode::Java => "java",
        Mode::Csharp => "csharp",
        Mode::Cpp => "cpp",
        Mode::C => unreachable!("混合 fixture 以 C++ 代表 Clang helper"),
    };
    let externals = source
        .data()
        .references
        .iter()
        .filter_map(|reference| {
            if reference.semantic != "code.calls" {
                return None;
            }
            match &reference.target {
                ReferenceTarget::External { symbol }
                    if symbol.to_ascii_lowercase().contains("hidden") =>
                {
                    Some(symbol.clone())
                }
                _ => None,
            }
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        externals.len(),
        1,
        "{language} 未选 Hidden 应恰有一个确定身份"
    );
    let external_symbol = externals.into_iter().next().unwrap();
    let external = graph
        .external_symbols()
        .get(&external_symbol)
        .expect("单语言图须提供未选目标的描述")
        .clone();
    assert_eq!(external.scope, SymbolScope::Repository);
    MixedEvidence {
        language,
        caller: caller.clone(),
        target: target.clone(),
        changed: changed.clone(),
        external_symbol,
        external,
    }
}

/// 在同一仓库侧依次运行四个真实后端，并以生产合并器形成唯一跨语言图。
fn mixed_side(
    configs: &BTreeMap<String, HelperConfig>,
    root: &Path,
    variant: &str,
) -> (CodeGraph, Vec<MixedEvidence>) {
    const MODES: [Mode; 4] = [Mode::Go, Mode::Java, Mode::Csharp, Mode::Cpp];
    let selections = MODES
        .into_iter()
        .map(|mode| {
            let files = fixture(mode, root, variant, configs);
            extend_fixture(mode, root, variant);
            (mode, files)
        })
        .collect::<Vec<_>>();
    let initial = snapshot(root);
    let mut graphs = Vec::new();
    let mut evidence = Vec::new();
    for (mode, files) in selections {
        let graph = analyze(mode, configs, root, files);
        evidence.push(mixed_evidence(mode, &graph));
        graphs.push(graph);
    }
    assert_eq!(
        initial,
        snapshot(root),
        "四个后端合并分析后仓库路径或字节改变"
    );
    let mut graphs = graphs.into_iter();
    let merged = graphs.next().expect("混合仓库必须有 Go 图");
    let merged = graphs.fold(merged, |combined, graph| {
        combined
            .merge(graph)
            .expect("真实单语言图应可合并成唯一 CodeGraph")
    });
    for entry in &evidence {
        let source = merged.node(&entry.caller).expect("合并图遗漏已验证调用者");
        assert_eq!(source.data().language.0, entry.language);
        assert_eq!(
            merged
                .node(&entry.target)
                .expect("合并图遗漏跨文件目标")
                .data()
                .language
                .0,
            entry.language
        );
        assert_eq!(
            merged
                .node(&entry.changed)
                .expect("合并图遗漏正文子对象")
                .data()
                .language
                .0,
            entry.language
        );
        assert!(
            source.data().references.iter().any(|reference| {
                reference.semantic == "code.calls"
                    && reference.target == ReferenceTarget::Node(entry.target.clone())
            }),
            "{} 同名调用在合并后改变目标",
            entry.language
        );
        assert!(
            source.data().references.iter().any(|reference| {
                reference.semantic == "code.calls"
                    && reference.target
                        == ReferenceTarget::External {
                            symbol: entry.external_symbol.clone(),
                        }
            }),
            "{} 仓内未选调用在合并后丢失",
            entry.language
        );
        assert_eq!(
            merged.external_symbols().get(&entry.external_symbol),
            Some(&entry.external),
            "{} 图外元数据在合并时冲突或丢失",
            entry.language
        );
    }
    assert_eq!(
        evidence
            .iter()
            .map(|item| &item.caller)
            .collect::<BTreeSet<_>>()
            .len(),
        4,
        "四门语言的同名调用者身份被错误归并"
    );
    assert_eq!(
        evidence
            .iter()
            .map(|item| &item.external_symbol)
            .collect::<BTreeSet<_>>()
            .len(),
        4,
        "四门语言的未选仓内目标身份发生冲突"
    );
    (merged, evidence)
}

/// 真实四 helper 对同一仓库侧构图，再验收合并图的前后侧 diff 与迁移身份。
#[test]
#[ignore = "require-real-tools: 四个真实 helper 与固定目标平台必须同时可用"]
fn mixed_repository_real_helper_graph_and_diff() {
    let configs = config();
    for key in ["go", "java", "csharp", "clang"] {
        assert!(
            configs.contains_key(key),
            "混合验收缺少 {key} 真实 helper 配置"
        );
    }
    let sandbox = TempDir::new().expect("无法建立混合 fixture 临时根");
    let mut sides = Vec::new();
    for (directory, variant) in [
        ("before", "before"),
        ("changed", "changed"),
        ("relocated", "before"),
    ] {
        let root = sandbox.path().join(directory);
        fs::create_dir(&root).expect("无法建立混合仓库侧目录");
        sides.push(mixed_side(&configs, &root, variant));
    }
    let (before, before_ids) = &sides[0];
    let (changed, changed_ids) = &sides[1];
    let (relocated, relocated_ids) = &sides[2];
    assert_eq!(
        before_ids, changed_ids,
        "正文变化改变了四门语言的对象或图外身份"
    );
    assert_eq!(
        before_ids, relocated_ids,
        "迁移仓库根改变了四门语言的对象或图外身份"
    );
    let expected = before_ids
        .iter()
        .map(|item| item.changed.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(expected.len(), 4, "四门语言的正文子对象身份必须互异");
    let diff = before.diff(changed).expect("合并后的有效图可以比较");
    assert_eq!(
        diff.nodes.len(),
        4,
        "只改四个子对象正文，不应改变其他对象：{diff:?}"
    );
    for node in &diff.nodes {
        assert!(
            expected.contains(&node.id),
            "正文变化污染了别的语言或祖先：{node:?}"
        );
        assert_eq!(node.content, Some(ChangeKind::Modified));
        assert!(
            node.contains.is_empty() && node.references.is_empty(),
            "正文变化不应改变结构或关系：{node:?}"
        );
        assert_eq!(
            before.node(&node.id).expect("前侧正文对象缺失").category(),
            CodeCategory::Callable
        );
    }
    assert_eq!(
        before.root().contains,
        relocated.root().contains,
        "迁移仓库根后混合图顶层身份改变"
    );
    assert!(
        before.diff(relocated).unwrap().nodes.is_empty(),
        "同语义迁移仓库根后混合图产生差异"
    );
}

/// 为同一份 C# 源码指定一个或两个明确 TFM；配置顺序由传入列表决定。
fn csharp_tfm_fixture(root: &Path, tfms: &[&str]) -> Vec<PathBuf> {
    put(root, "TargetProbe.cs", "public static class TargetProbe { public static void CheckTargets() { var gate = new System.Threading.Lock(); System.Console.WriteLine(\"shared\"); } }\n");
    let configurations = tfms.iter().map(|tfm| {
        let language_version = if *tfm == "net8.0" { "12.0" } else { "14.0" };
        serde_json::json!({
            "id": format!("target-{tfm}"), "files": ["TargetProbe.cs"], "tfm": tfm,
            "language_version": language_version, "define_constants": [], "implicit_usings": false
        })
    }).collect::<Vec<_>>();
    put(
        root,
        "narrant.csharp.json",
        &serde_json::json!({
            "version": "csharp-static-v1", "configurations": configurations
        })
        .to_string(),
    );
    vec!["TargetProbe.cs".into()]
}

/// 从已构建的真实图取得目标方法的调用事实，身份由单图查找而非构造。
fn csharp_tfm_calls(graph: &CodeGraph) -> (NodeId, BTreeSet<ReferenceTarget>) {
    let (id, node) = find(graph, "CheckTargets", CodeCategory::Callable);
    let calls = node
        .data()
        .references
        .iter()
        .filter(|reference| reference.semantic == "code.calls")
        .map(|reference| reference.target.clone())
        .collect::<BTreeSet<_>>();
    (id.clone(), calls)
}

/// 仅按公开的可读名称与来源描述识别目标 reference assemblies 的依赖方法。
fn dependency_calls(
    graph: &CodeGraph,
    calls: &BTreeSet<ReferenceTarget>,
    name: &str,
) -> BTreeSet<String> {
    calls
        .iter()
        .filter_map(|target| match target {
            ReferenceTarget::External { symbol }
                if graph
                    .external_symbols()
                    .get(symbol)
                    .is_some_and(|external| {
                        external.scope == SymbolScope::Dependency && external.name == name
                    }) =>
            {
                Some(symbol.clone())
            }
            _ => None,
        })
        .collect()
}

/// 同源码的 net8/net10 目标隔离与双配置并集须经真实支持包、Host 和 CodeGraph 验证。
#[test]
#[ignore = "require-real-tools: C# helper 须同时提供固定 net8.0/ref8.0.0 与 net10.0/ref10.0.0"]
fn csharp_real_target_framework_isolation_and_union() {
    let configs = config();
    let csharp = configs.get("csharp").expect("缺少 C# 真实 helper 配置");
    for (tfm, version) in [("net8.0", "8.0.0"), ("net10.0", "10.0.0")] {
        assert!(
            csharp.platforms.iter().any(|platform| {
                platform.kind == "dotnet-reference-assemblies"
                    && platform.version == version
                    && platform.metadata["tfm"] == tfm
                    && platform.metadata["reference_pack"] == "Microsoft.NETCore.App.Ref"
                    && platform.metadata["reference_pack_version"] == version
            }),
            "C# 真实支持包缺少 {tfm} / {version} reference assemblies"
        );
    }
    let sandbox = TempDir::new().expect("无法建立 C# 双目标临时根");
    let mut sides = Vec::new();
    for (directory, tfms) in [
        ("net8", vec!["net8.0"]),
        ("net10", vec!["net10.0"]),
        ("dual-8-first", vec!["net8.0", "net10.0"]),
        ("dual-10-first", vec!["net10.0", "net8.0"]),
    ] {
        let root = sandbox.path().join(directory);
        fs::create_dir(&root).expect("无法建立 C# 目标仓库侧");
        let files = csharp_tfm_fixture(&root, &tfms);
        let graph = analyze(Mode::Csharp, &configs, &root, files.clone());
        sides.push((root, files, graph));
    }
    let (id8, calls8) = csharp_tfm_calls(&sides[0].2);
    let (id10, calls10) = csharp_tfm_calls(&sides[1].2);
    let (id_both, calls_both) = csharp_tfm_calls(&sides[2].2);
    let (id_reversed, calls_reversed) = csharp_tfm_calls(&sides[3].2);
    assert_eq!(id8, id10, "TFM 改变了同一源码方法身份");
    assert_eq!(id8, id_both, "双 TFM 复制了同一源码方法");
    assert_eq!(id8, id_reversed, "配置顺序改变了源码方法身份");
    let unknown = ReferenceTarget::Unresolved {
        name: "System.Threading.Lock".into(),
    };
    assert!(
        calls8.contains(&unknown),
        "net8 必须保留 Lock 的具名未知调用：{calls8:?}"
    );
    assert!(
        dependency_calls(&sides[0].2, &calls8, "Lock").is_empty(),
        "net8 不得借用运行时或 net10 的 Lock API"
    );
    assert!(
        !calls10.contains(&unknown),
        "net10 已绑定 Lock 时不应伪造未知调用"
    );
    let lock10 = dependency_calls(&sides[1].2, &calls10, "Lock");
    assert_eq!(lock10.len(), 1, "net10 须绑定一个 Lock 构造器：{calls10:?}");
    let console8 = dependency_calls(&sides[0].2, &calls8, "WriteLine");
    let console10 = dependency_calls(&sides[1].2, &calls10, "WriteLine");
    assert_eq!(console8.len(), 1, "net8 普通标准 API 须绑定：{calls8:?}");
    assert_eq!(console10.len(), 1, "net10 普通标准 API 须绑定：{calls10:?}");
    assert_eq!(
        console8, console10,
        "普通标准 API 的确定身份不应随目标 TFM 改变"
    );
    assert_eq!(
        calls8.len(),
        2,
        "net8 应恰有 Lock 未知与 WriteLine 确定调用：{calls8:?}"
    );
    assert_eq!(
        calls10.len(),
        2,
        "net10 应恰有 Lock 与 WriteLine 两个确定调用：{calls10:?}"
    );
    assert!(
        calls_both.contains(&unknown),
        "双 TFM 并集丢失 net8 未知观测"
    );
    assert_eq!(
        calls_both.len(),
        3,
        "双 TFM 应只保留 Lock 未知、Lock 确定及 WriteLine 确定调用：{calls_both:?}"
    );
    assert_eq!(
        dependency_calls(&sides[2].2, &calls_both, "Lock"),
        lock10,
        "双 TFM 并集丢失 net10 确定的 Lock 调用"
    );
    assert_eq!(
        dependency_calls(&sides[2].2, &calls_both, "WriteLine").len(),
        1,
        "双 TFM 的普通标准 API 应归并为一个确定调用"
    );
    assert_eq!(calls_both, calls_reversed, "配置顺序改变了最终关系集合");
    assert_eq!(
        sides[2].2.external_symbols(),
        sides[3].2.external_symbols(),
        "配置顺序改变了图外符号元数据"
    );
    assert!(
        sides[2].2.diff(&sides[3].2).unwrap().nodes.is_empty(),
        "配置顺序制造了图差异"
    );
    for (index, tfm) in [(0, "net8.0"), (1, "net10.0")] {
        let mut without_target = configs.clone();
        without_target
            .get_mut("csharp")
            .unwrap()
            .platforms
            .retain(|platform| platform.metadata["tfm"] != tfm);
        let (root, files, _) = &sides[index];
        let before = snapshot(root);
        let error = analyzer(Mode::Csharp, &without_target)
            .analyze(&AnalysisInput {
                repository_root: root.clone(),
                files: files.clone(),
                options: AnalysisOptions {
                    include_tests: false,
                },
            })
            .expect_err("缺少本侧目标 reference assemblies 必须硬失败");
        assert!(
            error.message.contains("target_unavailable") && error.message.contains(tfm),
            "缺少 {tfm} 目标却未给出明确失败：{error}"
        );
        assert_eq!(before, snapshot(root), "目标缺失失败路径改写了仓库");
    }
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
