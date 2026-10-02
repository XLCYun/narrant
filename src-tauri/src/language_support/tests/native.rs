use super::*;
use crate::structure_review::{
    code_graph::{AnalysisInput, AnalysisOptions, LanguageAnalyzer},
    native_languages,
};
use httpmock::prelude::*;

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

/// 本地构建的四种归档经过真实下载校验、安装、工厂和 helper 分析后才能宣称安装可用。
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
    for id in [PackId::Go, PackId::Java, PackId::Csharp, PackId::CCpp] {
        let artifact = artifacts.join(id.slug()).join("0.1.0").join(target);
        for name in ["manifest.json", "pack.tar.gz"] {
            let bytes = fs::read(artifact.join(name)).expect("验收必须具备四种真实包");
            server.mock(|when, then| {
                when.path(format!("/{}/0.1.0/{target}/{name}", id.slug()));
                then.body(bytes);
            });
        }
        let lease = packs
            .ensure(&[id], &CancellationToken::new(), None)
            .await
            .unwrap();
        let analyzer = native_languages::from_pack(id, lease.path(id).unwrap()).unwrap();
        let fixture = tempfile::tempdir().unwrap();
        let file = match id {
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
                let control: serde_json::Value = serde_json::from_slice(
                    &fs::read(lease.path(id).unwrap().join("control/native.json")).unwrap(),
                )
                .unwrap();
                let triple = if cfg!(windows) {
                    "x86_64-pc-windows-msvc".to_owned()
                } else {
                    format!(
                        "arm64-apple-macosx{}",
                        control["target_sdk"]["version"].as_str().unwrap()
                    )
                };
                fs::write(fixture.path().join("main.cpp"), "int Value() { return 1; }").unwrap();
                fs::write(fixture.path().join("compile_commands.json"), serde_json::to_vec(&serde_json::json!([{"directory":fixture.path(),"file":"main.cpp","arguments":["clang++","-std=c++20","-target",triple,"main.cpp"]}])).unwrap()).unwrap();
                "main.cpp"
            }
            _ => unreachable!(),
        };
        let input = AnalysisInput {
            repository_root: fixture.path().into(),
            files: vec![file.into()],
            options: AnalysisOptions::default(),
        };
        let graph = tokio::task::spawn_blocking(move || analyzer.analyze(&input))
            .await
            .unwrap()
            .unwrap();
        assert!(
            !graph.root().contains.is_empty(),
            "{} 返回了空图",
            id.slug()
        );
        assert!(packs.remove(id, "0.1.0").is_err());
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
        packs.remove(id, "0.1.0").unwrap();
    }
    packs.shutdown().await;
}
