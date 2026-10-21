use super::*;
use crate::structure_review::{
    code_graph::{AnalysisInput, AnalysisOptions, CodeCategory, LanguageAnalyzer},
    codegraph_client::CodeGraphAnalyzer,
};
use httpmock::prelude::*;

/// 每个目标通过已租用的公开入口分析，验证描述符身份、源码图与分析前后的删除保护。
async fn assert_installed_target(
    id: PackId,
    packs: &LanguageSupportPacks,
    lease: &PackLease,
    repository: &Path,
    file: &str,
) {
    let installed = lease.path(id).expect("真实分析必须持有包租约");
    let descriptor: serde_json::Value =
        serde_json::from_slice(&fs::read(installed.join("control/codegraph.json")).unwrap())
            .unwrap();
    assert_eq!(
        descriptor,
        serde_json::from_slice::<serde_json::Value>(&super::test_support::descriptor(id.slug()))
            .unwrap(),
        "已安装公开描述符必须保留固定包、引擎与协议身份"
    );
    let version = &versions()[&id];
    assert!(packs.snapshot().packs.iter().any(|pack| {
        pack.id == id && pack.version == *version && pack.phase == PackPhase::Ready && pack.in_use
    }));
    assert!(packs.remove(id, version).is_err());
    let source = fs::read_to_string(repository.join(file)).unwrap();
    let installed = installed.to_owned();
    let input = AnalysisInput {
        repository_root: repository.into(),
        files: vec![file.into()],
        options: AnalysisOptions::default(),
    };
    let graph = tokio::task::spawn_blocking(move || {
        CodeGraphAnalyzer::from_pack(id.slug(), &installed)
            .unwrap()
            .analyze(&input)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(
        !graph.root().contains.is_empty(),
        "{} 返回了空图",
        id.slug()
    );
    let mut pending = graph.root().contains.clone();
    let mut values = Vec::new();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap();
        pending.extend(node.data().contains.iter().cloned());
        if node.category() == CodeCategory::Callable
            && matches!(node.data().name.as_deref(), Some("value" | "Value"))
        {
            values.push(node);
        }
    }
    assert_eq!(values.len(), 1, "真实目标图必须包含唯一的源码函数");
    let returned_source = values[0].data().source.as_deref().unwrap();
    assert!(
        !returned_source.is_empty() && source.contains(returned_source),
        "公开图必须保留当前目标样本的真实源码"
    );
    assert!(packs.remove(id, version).is_err());
}

/// 未发布能力的请求保留包身份与明确原因，不能尝试访问虚构远端版本。
#[tokio::test]
async fn unpublished_native_pack_reports_explicit_error() {
    let root = tempfile::tempdir().unwrap();
    let packs = LanguageSupportPacks::with_origin(
        root.path().into(),
        ORIGIN.into(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    let error = packs
        .ensure(
            &[PackId::Go],
            &CancellationToken::new(),
            Some("native".into()),
        )
        .await
        .err()
        .unwrap();
    assert!(error.contains("尚未发布"));
    assert!(packs
        .snapshot()
        .packs
        .iter()
        .any(|pack| pack.id == PackId::Go && pack.phase == PackPhase::Failed));
    packs.shutdown().await;
}

/// 七种分析器在两个平台都必须提供自身固定入口，旧总入口和其他语言不能替代。
#[test]
fn manifest_requires_public_cli_and_descriptor_on_both_targets() {
    for (id, entry) in [
        (PackId::Rust, "bin/codegraph-rust"),
        (
            PackId::TypeScriptJavascript,
            "bin/codegraph-typescript-javascript",
        ),
        (PackId::Python, "bin/codegraph-python"),
        (PackId::Java, "bin/codegraph-java"),
        (PackId::Go, "bin/codegraph-go"),
        (PackId::Csharp, "bin/codegraph-csharp"),
        (PackId::CCpp, "bin/codegraph-c-cpp"),
    ] {
        let key = PackKey {
            id,
            version: versions()[&id].clone(),
        };
        for target in ["aarch64-apple-darwin", "x86_64-pc-windows-msvc"] {
            let suffix = if target == "x86_64-pc-windows-msvc" {
                ".exe"
            } else {
                ""
            };
            let manifest = Manifest {
                schema_version: 1,
                id,
                version: key.version.clone(),
                target: target.into(),
                engine_version: "0.1.0".into(),
                protocol: "codegraph.analyzer/1".into(),
                source_commit: "0".repeat(40),
                source_dirty: false,
                archive_size: 1,
                archive_sha256: "0".repeat(64),
                files: [format!("{entry}{suffix}"), "control/codegraph.json".into()]
                    .into_iter()
                    .map(|path| model::ManifestFile {
                        executable: path.starts_with("bin/"),
                        path,
                        size: 1,
                        sha256: "0".repeat(64),
                    })
                    .collect(),
            };
            assert!(
                validate_manifest(&manifest, &key, target).is_ok(),
                "{id:?} {target}"
            );
            for wrong in [
                format!("bin/codegraph{suffix}"),
                format!(
                    "bin/codegraph-{}{suffix}",
                    if id == PackId::Rust { "python" } else { "rust" }
                ),
                format!("{entry}{}", if suffix.is_empty() { ".exe" } else { "" }),
            ] {
                let mut invalid = manifest.clone();
                invalid.files[0].path = wrong;
                assert!(
                    validate_manifest(&invalid, &key, target).is_err(),
                    "{id:?} {target}"
                );
            }
            let mut invalid = manifest.clone();
            invalid.files[0].executable = false;
            assert!(validate_manifest(&invalid, &key, target).is_err());
            invalid = manifest.clone();
            invalid.files.pop();
            assert!(validate_manifest(&invalid, &key, target).is_err());
        }
    }
}

/// 所选真实归档经过下载校验、安装租约和公共 CLI 的目标矩阵后才能宣称安装可用。
#[tokio::test]
#[ignore = "需要 NARRANT_CODEGRAPH_ARTIFACTS 指向真实构建产物；显式执行且不得静默跳过缺包"]
async fn real_codegraph_packs_install_and_analyze_through_public_cli() {
    let artifacts =
        PathBuf::from(std::env::var_os("NARRANT_CODEGRAPH_ARTIFACTS").expect("缺少真实原生包目录"));
    let target = if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else {
        "aarch64-apple-darwin"
    };
    let server = MockServer::start();
    super::test_support::serve_artifacts(&server, &artifacts, target);
    let root = tempfile::tempdir().unwrap();
    let packs =
        LanguageSupportPacks::with_origin(root.path().into(), server.base_url(), target.into())
            .unwrap();
    let language = std::env::var("NARRANT_CODEGRAPH_LANGUAGE").unwrap_or_else(|_| "all".into());
    assert!(
        language == "all" || PackId::SUPPORTED.iter().any(|id| id.slug() == language),
        "未完成或未知的 Native 迁移：{language}"
    );
    for &id in PackId::SUPPORTED
        .iter()
        .filter(|id| language == "all" || id.slug() == language)
    {
        let version = &versions()[&id];
        let lease = packs
            .ensure(&[id], &CancellationToken::new(), None)
            .await
            .unwrap();
        let fixture = tempfile::tempdir().unwrap();
        let file = match id {
            PackId::Rust => {
                fs::write(fixture.path().join("Cargo.toml"), "[package]\nname=\"native_fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"lib.rs\"\n").unwrap();
                fs::write(fixture.path().join("lib.rs"), "pub fn value() -> u32 { 1 }").unwrap();
                "lib.rs"
            }
            PackId::TypeScriptJavascript => {
                fs::write(
                    fixture.path().join("main.ts"),
                    "export function value() { return 1; }",
                )
                .unwrap();
                "main.ts"
            }
            PackId::Python => {
                fs::write(
                    fixture.path().join("main.py"),
                    "def value(): return len([])\n",
                )
                .unwrap();
                "main.py"
            }
            PackId::Go => {
                fs::write(
                    fixture.path().join("go.mod"),
                    "module example.test/native\ngo 1.23.0\n",
                )
                .unwrap();
                fs::write(
                    fixture.path().join("main.go"),
                    "package native\nfunc Value() int { return 1 }\n",
                )
                .unwrap();
                "main.go"
            }
            PackId::Java => {
                fs::write(fixture.path().join("narrant-java.json"), r#"{"schema":"java-static-v1","release":"17","source_roots":[{"path":"src","role":"main"}]}"#).unwrap();
                fs::create_dir(fixture.path().join("src")).unwrap();
                fs::write(
                    fixture.path().join("src/Main.java"),
                    "public class Main { public static int value() { return 1; } }",
                )
                .unwrap();
                "src/Main.java"
            }
            PackId::Csharp => {
                fs::write(fixture.path().join("narrant.csharp.json"), r#"{"version":"csharp-static-v1","configurations":[{"id":"main","files":["Main.cs"],"tfm":"net10.0","language_version":"14.0","define_constants":[],"implicit_usings":false}]}"#).unwrap();
                fs::write(
                    fixture.path().join("Main.cs"),
                    "public static class Main { public static int Value() { return 1; } }",
                )
                .unwrap();
                "Main.cs"
            }
            PackId::CCpp => {
                fs::write(fixture.path().join("main.cpp"), "int Value() { return 1; }").unwrap();
                fs::write(fixture.path().join("compile_commands.json"), serde_json::to_vec(&serde_json::json!([{"directory":fixture.path(),"file":"main.cpp","arguments":["clang++","-std=c++20","main.cpp"]}])).unwrap()).unwrap();
                "main.cpp"
            }
        };
        assert_installed_target(id, &packs, &lease, fixture.path(), file).await;
        if id == PackId::Csharp {
            // 同一公开包切换项目声明的目标；net9 样本不能依赖消费者内置目标表。
            for (tfm, language) in [("net8.0", "12.0"), ("net9.0", "13.0")] {
                fs::write(fixture.path().join("narrant.csharp.json"), serde_json::to_vec(&serde_json::json!({
                    "version":"csharp-static-v1","configurations":[{"id":"main","files":["Main.cs"],
                    "tfm":tfm,"language_version":language,"define_constants":[],"implicit_usings":false}]
                })).unwrap()).unwrap();
                fs::write(fixture.path().join("Main.cs"), "public static class Main { public static int Value() { return System.Math.Abs(-1); } }").unwrap();
                assert_installed_target(id, &packs, &lease, fixture.path(), file).await;
            }
        }
        if id == PackId::Java {
            for (release, expression) in [
                ("8", "java.util.Collections.emptyList().size()"),
                ("25", "java.util.List.of(1).getFirst()"),
            ] {
                fs::write(fixture.path().join("narrant-java.json"), serde_json::to_vec(&serde_json::json!({"schema":"java-static-v1","release":release,"source_roots":[{"path":"src","role":"main"}]})).unwrap()).unwrap();
                fs::write(fixture.path().join(file), format!("public class Main {{ public static int value() {{ return {expression}; }} }}")).unwrap();
                assert_installed_target(id, &packs, &lease, fixture.path(), file).await;
            }
        }
        if id == PackId::Go {
            fs::write(
                fixture.path().join("go.mod"),
                "module example.test/native\ngo 1.27.0\n",
            )
            .unwrap();
            fs::write(fixture.path().join(file), "package native\nimport \"strings\"\nfunc Value() { for range strings.SplitSeq(\"a,b\", \",\") {} }\n").unwrap();
            assert_installed_target(id, &packs, &lease, fixture.path(), file).await;
        }
        if id == PackId::CCpp {
            // 产品目录不再指定统一语言标准，C 和 C++ 各自采用编译数据库的 -std。
            for (standard, file, source) in [
                (
                    "c11",
                    "other.c",
                    "_Static_assert(sizeof(int) > 0, \"int\"); int Value(void) { return 1; }",
                ),
                (
                    "c++17",
                    "other.cpp",
                    "int Value() { if constexpr (true) { return 1; } else { return 2; } }",
                ),
            ] {
                fs::write(fixture.path().join(file), source).unwrap();
                fs::write(fixture.path().join("compile_commands.json"), serde_json::to_vec(&serde_json::json!([{"directory":fixture.path(),"file":file,"arguments":["clang",format!("-std={standard}"),file]}])).unwrap()).unwrap();
                assert_installed_target(id, &packs, &lease, fixture.path(), file).await;
            }
        }
        assert!(packs.remove(id, version).is_err());
        drop(lease);
        // 已验证安装在未发布状态下仍可经生产源重新打开，且不需要再次下载。
        let reopened =
            LanguageSupportPacks::with_origin(root.path().into(), ORIGIN.into(), target.into())
                .unwrap();
        let installed = reopened
            .ensure(&[id], &CancellationToken::new(), None)
            .await
            .unwrap();
        drop(installed);
        reopened.shutdown().await;
        packs.remove(id, version).unwrap();
    }
    packs.shutdown().await;
}
