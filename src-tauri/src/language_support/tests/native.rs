use super::*;
use crate::structure_review::{
    code_graph::{AnalysisInput, AnalysisOptions, LanguageAnalyzer},
    native_languages,
};
use httpmock::prelude::*;

/// 每个目标都经过安装工厂和真实 helper，确保本机发现与打包入口共同生效。
async fn assert_installed_target(id: PackId, installed: &Path, repository: &Path, file: &str) {
    let installed = installed.to_owned();
    let input = AnalysisInput {
        repository_root: repository.into(),
        files: vec![file.into()],
        options: AnalysisOptions::default(),
    };
    let graph = tokio::task::spawn_blocking(move || {
        if id == PackId::Rust {
            native_languages::RustPackAnalyzer::from_pack(&installed)
                .unwrap()
                .analyze(
                    &input,
                    std::sync::Arc::new(|| false),
                    crate::structure_review::code_graph::RustDependencyScope::RepositoryOnly,
                )
        } else {
            native_languages::from_pack(id, &installed)
                .unwrap()
                .analyze(&input)
        }
    })
    .await
    .unwrap()
    .unwrap();
    assert!(
        !graph.root().contains.is_empty(),
        "{} 返回了空图",
        id.slug()
    );
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

/// 新增的无条件导入模块必须在下载前通过清单入口校验，不能等 Node 启动后才失败。
#[test]
fn typescript_manifest_requires_syntax_module_on_both_targets() {
    let id = PackId::TypeScriptJavascript;
    let key = PackKey {
        id,
        version: versions()[&id].clone(),
    };
    let syntax = "runtime/helper/syntax.mjs";
    for target in ["aarch64-apple-darwin", "x86_64-pc-windows-msvc"] {
        let mut manifest = Manifest {
            schema_version: 1,
            id,
            version: key.version.clone(),
            target: target.into(),
            archive_size: 1,
            archive_sha256: "0".repeat(64),
            files: id
                .required(target)
                .into_iter()
                .filter(|path| *path != syntax)
                .chain(std::iter::once(syntax))
                .map(|path| model::ManifestFile {
                    path: path.into(),
                    size: 1,
                    sha256: "0".repeat(64),
                    executable: path.starts_with("bin/"),
                })
                .collect(),
        };
        assert!(
            validate_manifest(&manifest, &key, target).is_ok(),
            "{target}"
        );
        assert_eq!(manifest.files.pop().unwrap().path, syntax);
        assert_eq!(
            validate_manifest(&manifest, &key, target),
            Err("语言支持包缺少必需入口".into()),
            "{target}"
        );
    }
}

/// 所选已迁移归档经过真实下载校验、安装、工厂和 helper 分析后才能宣称安装可用。
#[tokio::test]
#[ignore = "需要 NARRANT_NATIVE_PACKS 指向真实构建产物；显式执行且不得静默跳过缺包"]
async fn real_native_packs_install_and_analyze_through_production_factory() {
    let artifacts =
        PathBuf::from(std::env::var_os("NARRANT_NATIVE_PACKS").expect("缺少真实原生包目录"));
    let target = if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else {
        "aarch64-apple-darwin"
    };
    let server = MockServer::start();
    let root = tempfile::tempdir().unwrap();
    let packs =
        LanguageSupportPacks::with_origin(root.path().into(), server.base_url(), target.into())
            .unwrap();
    let language = std::env::var("NARRANT_NATIVE_LANGUAGE").unwrap_or_else(|_| "all".into());
    assert!(
        language == "all"
            || PackId::NATIVE_MIGRATED
                .iter()
                .any(|id| id.slug() == language),
        "未完成或未知的 Native 迁移：{language}"
    );
    for &id in PackId::NATIVE_MIGRATED
        .iter()
        .filter(|id| language == "all" || id.slug() == language)
    {
        let version = &versions()[&id];
        let artifact = artifacts.join(id.slug()).join(version).join(target);
        for name in ["manifest.json", "pack.tar.gz"] {
            let bytes = fs::read(artifact.join(name)).expect("验收必须具备所选真实支持包");
            server.mock(|when, then| {
                when.path(format!("/{}/{version}/{target}/{name}", id.slug()));
                then.body(bytes);
            });
        }
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
        if let Some(kind) = match id {
            PackId::Go => Some("go"),
            PackId::Java => Some("java"),
            PackId::Csharp => Some("dotnet"),
            _ => None,
        } {
            // 目标声明由本机发现；Java 运行 JDK 可作为自身真实 major 的候选，不再复制另一套17。
            let installed = lease.path(id).unwrap();
            let control: serde_json::Value =
                serde_json::from_slice(&fs::read(installed.join("control/native.json")).unwrap())
                    .unwrap();
            assert_eq!(control["target_sdk"]["kind"], kind);
            assert!(control["platforms"].as_array().unwrap().is_empty());
            assert!(!installed.join("runtime/platform").exists());
        }
        assert_installed_target(id, lease.path(id).unwrap(), fixture.path(), file).await;
        if id == PackId::Csharp {
            // 同一私有运行时切换本机发现的目标；新增 net9 样本不能依赖产品内置目标表。
            for (tfm, language) in [("net8.0", "12.0"), ("net9.0", "13.0")] {
                fs::write(fixture.path().join("narrant.csharp.json"), serde_json::to_vec(&serde_json::json!({
                    "version":"csharp-static-v1","configurations":[{"id":"main","files":["Main.cs"],
                    "tfm":tfm,"language_version":language,"define_constants":[],"implicit_usings":false}]
                })).unwrap()).unwrap();
                fs::write(fixture.path().join("Main.cs"), "public static class Main { public static int Value() { return System.Math.Abs(-1); } }").unwrap();
                assert_installed_target(id, lease.path(id).unwrap(), fixture.path(), file).await;
            }
        }
        if id == PackId::Java {
            for (release, expression) in [
                ("8", "java.util.Collections.emptyList().size()"),
                ("25", "java.util.List.of(1).getFirst()"),
            ] {
                fs::write(fixture.path().join("narrant-java.json"), serde_json::to_vec(&serde_json::json!({"schema":"java-static-v1","release":release,"source_roots":[{"path":"src","role":"main"}]})).unwrap()).unwrap();
                fs::write(fixture.path().join(file), format!("public class Main {{ public static int value() {{ return {expression}; }} }}")).unwrap();
                assert_installed_target(id, lease.path(id).unwrap(), fixture.path(), file).await;
            }
        }
        if id == PackId::Go {
            fs::write(
                fixture.path().join("go.mod"),
                "module example.test/native\ngo 1.27.0\n",
            )
            .unwrap();
            fs::write(fixture.path().join(file), "package native\nimport \"strings\"\nfunc Value() { for range strings.SplitSeq(\"a,b\", \",\") {} }\n").unwrap();
            assert_installed_target(id, lease.path(id).unwrap(), fixture.path(), file).await;
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
                assert_installed_target(id, lease.path(id).unwrap(), fixture.path(), file).await;
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
