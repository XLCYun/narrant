use super::*;
use flate2::{write::GzEncoder, Compression};
use httpmock::prelude::*;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

/// 提供实际 tar.gz 和对应清单，让服务测试经过真实下载及解压边界。
fn fixture() -> (Vec<u8>, serde_json::Value) {
    let content = b"test rust-analyzer executable";
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(&mut header, "bin/rust-analyzer", content.as_slice())
        .unwrap();
    let archive = tar.into_inner().unwrap().finish().unwrap();
    let manifest = serde_json::json!({
        "schemaVersion": 1, "id": "rust", "version": "1.0.0", "target": "aarch64-apple-darwin",
        "archiveSize": archive.len(), "archiveSha256": format!("{:x}", Sha256::digest(&archive)),
        "files": [{"path": "bin/rust-analyzer", "size": content.len(), "sha256": format!("{:x}", Sha256::digest(content)), "executable": true}]
    });
    (archive, manifest)
}

#[tokio::test]
async fn installs_complete_pack_and_reuses_it_after_restart() {
    let server = MockServer::start();
    let (archive, manifest) = fixture();
    let metadata = server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/manifest.json");
        then.json_body(manifest);
    });
    let download = server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/pack.tar.gz");
        then.body(archive);
    });
    let directory = tempfile::tempdir().unwrap();
    let packs = LanguageSupportPacks::with_origin(
        directory.path().to_owned(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    let lease = packs
        .ensure(&[PackId::Rust], &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(lease.path(PackId::Rust).unwrap().join("bin/rust-analyzer")).unwrap(),
        b"test rust-analyzer executable"
    );
    assert_eq!(packs.snapshot().packs[0].phase, PackPhase::Ready);
    drop(lease);
    packs.shutdown().await;
    drop(packs);
    let reopened = LanguageSupportPacks::with_origin(
        directory.path().to_owned(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    let _lease = reopened
        .ensure(&[PackId::Rust], &CancellationToken::new())
        .await
        .unwrap();
    metadata.assert_hits(1);
    download.assert_hits(1);
}

#[tokio::test]
async fn reviews_share_download_and_cancelling_one_waiter_keeps_other_review_running() {
    let server = MockServer::start();
    let (archive, manifest) = fixture();
    server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/manifest.json");
        then.json_body(manifest);
    });
    let download = server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/pack.tar.gz");
        then.delay(std::time::Duration::from_millis(150))
            .body(archive);
    });
    let directory = tempfile::tempdir().unwrap();
    let packs = LanguageSupportPacks::with_origin(
        directory.path().to_owned(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    let cancelled = CancellationToken::new();
    let other = CancellationToken::new();
    let (first, second, ()) = tokio::join!(
        packs.ensure(&[PackId::Rust], &cancelled),
        packs.ensure(&[PackId::Rust], &other),
        async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            cancelled.cancel();
        }
    );
    assert!(first.is_err());
    let lease = second.unwrap();
    download.assert_hits(1);
    assert!(packs.snapshot().packs[0].in_use);
    assert!(packs.remove(PackId::Rust, "1.0.0").is_err());
    // 第二个应用实例也不能删除当前分析持有的版本。
    let other_instance = LanguageSupportPacks::with_origin(
        directory.path().to_owned(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    assert!(other_instance.remove(PackId::Rust, "1.0.0").is_err());
    drop(lease);
    packs.remove(PackId::Rust, "1.0.0").unwrap();
    assert_eq!(packs.snapshot().packs[0].phase, PackPhase::NotInstalled);
}

#[tokio::test]
async fn failed_checksum_is_never_installed_and_explicit_retry_starts_fresh() {
    let server = MockServer::start();
    let (archive, manifest) = fixture();
    server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/manifest.json");
        then.json_body(manifest);
    });
    let mut corrupt = server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/pack.tar.gz");
        then.body(vec![0; archive.len()]);
    });
    let directory = tempfile::tempdir().unwrap();
    let packs = LanguageSupportPacks::with_origin(
        directory.path().to_owned(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    assert!(packs
        .ensure(&[PackId::Rust], &CancellationToken::new())
        .await
        .is_err());
    let failed = packs.snapshot();
    assert_eq!(failed.packs[0].phase, PackPhase::Failed);
    assert_eq!(failed.packs[0].installed_bytes, 0);
    assert!(!directory
        .path()
        .join("installed/rust/1.0.0/aarch64-apple-darwin")
        .exists());
    assert_eq!(
        std::fs::read_dir(
            directory
                .path()
                .join("staging/rust/1.0.0/aarch64-apple-darwin")
        )
        .unwrap()
        .count(),
        0
    );
    corrupt.delete();
    let good = server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/pack.tar.gz");
        then.body(archive);
    });
    packs.retry(PackId::Rust).unwrap();
    let _lease = packs
        .ensure(&[PackId::Rust], &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(packs.snapshot().packs[0].phase, PackPhase::Ready);
    assert!(packs.snapshot().packs[0].error.is_none());
    good.assert_hits(1);
}

#[tokio::test]
async fn empty_scope_does_not_download_and_startup_removes_abandoned_staging() {
    let server = MockServer::start();
    let network = server.mock(|_, then| {
        then.status(500);
    });
    let directory = tempfile::tempdir().unwrap();
    let leftover = directory
        .path()
        .join("staging/rust/1.0.0/aarch64-apple-darwin/download-interrupted");
    std::fs::create_dir_all(&leftover).unwrap();
    std::fs::write(leftover.join("download.part"), b"partial bytes").unwrap();
    let packs = LanguageSupportPacks::with_origin(
        directory.path().to_owned(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    let lease = packs.ensure(&[], &CancellationToken::new()).await.unwrap();
    assert!(lease.path(PackId::Rust).is_none());
    assert!(!leftover.exists());
    assert!(packs
        .snapshot()
        .packs
        .iter()
        .all(|pack| pack.phase == PackPhase::NotInstalled));
    network.assert_hits(0);
}

#[tokio::test]
async fn invalid_manifest_cannot_escape_staging_or_request_another_archive() {
    for invalid in [
        "../outside",
        "/outside",
        "bin/../outside",
        "bin/node.exe:stream",
    ] {
        let server = MockServer::start();
        let (_, mut manifest) = fixture();
        manifest["files"][0]["path"] = serde_json::json!(invalid);
        server.mock(|when, then| {
            when.path("/rust/1.0.0/aarch64-apple-darwin/manifest.json");
            then.json_body(manifest);
        });
        let archive = server.mock(|when, then| {
            when.path("/rust/1.0.0/aarch64-apple-darwin/pack.tar.gz");
            then.status(500);
        });
        let directory = tempfile::tempdir().unwrap();
        let packs = LanguageSupportPacks::with_origin(
            directory.path().to_owned(),
            server.base_url(),
            "aarch64-apple-darwin".into(),
        )
        .unwrap();
        assert!(packs
            .ensure(&[PackId::Rust], &CancellationToken::new())
            .await
            .is_err());
        archive.assert_hits(0);
    }
}

/// 显式使用本地构建的真实支持包，验证安装目录能独立运行 TS/JS 分析。
#[tokio::test]
#[ignore = "需要先构建当前平台语言支持包，并设置 NARRANT_LANGUAGE_PACK_FIXTURE"]
async fn real_installed_typescript_pack_analyzes_without_development_tools() {
    use crate::structure_review::{
        code_graph::{AnalysisInput, AnalysisOptions, LanguageAnalyzer},
        typescript_analyzer::TypeScriptLanguageAnalyzer,
    };
    let target = if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else {
        "aarch64-apple-darwin"
    };
    let source =
        PathBuf::from(std::env::var_os("NARRANT_LANGUAGE_PACK_FIXTURE").expect("真实包目录"));
    let package = source.join("typescript-javascript/1.0.0").join(target);
    let server = MockServer::start();
    for name in ["manifest.json", "pack.tar.gz"] {
        let bytes = std::fs::read(package.join(name)).unwrap();
        server.mock(|when, then| {
            when.path(format!("/typescript-javascript/1.0.0/{target}/{name}"));
            then.body(bytes);
        });
    }
    let directory = tempfile::tempdir().unwrap();
    let packs = LanguageSupportPacks::with_origin(
        directory.path().join("packs"),
        server.base_url(),
        target.into(),
    )
    .unwrap();
    let lease = packs
        .ensure(&[PackId::TypeScriptJavascript], &CancellationToken::new())
        .await
        .unwrap();
    let installed = lease.path(PackId::TypeScriptJavascript).unwrap();
    let analyzer = TypeScriptLanguageAnalyzer::new(
        installed.join(if cfg!(windows) {
            "bin/node.exe"
        } else {
            "bin/node"
        }),
        installed.join("runtime"),
    );
    let repository = directory.path().join("project");
    std::fs::create_dir(&repository).unwrap();
    std::fs::write(
        repository.join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"es2022","strict":true},"files":["client.ts"]}"#,
    )
    .unwrap();
    std::fs::write(
        repository.join("client.ts"),
        "export class Counter { value = 1; inc() { return this.value + 1; } }",
    )
    .unwrap();
    let graph = tokio::task::spawn_blocking(move || {
        analyzer.analyze(&AnalysisInput {
            repository_root: repository,
            files: vec!["client.ts".into()],
            options: AnalysisOptions {
                include_tests: false,
            },
        })
    })
    .await
    .unwrap()
    .unwrap();
    assert!(!graph.root().contains.is_empty());
    let mut pending = graph.root().contains.clone();
    let mut names = Vec::new();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap().data();
        names.extend(node.name.iter().cloned());
        pending.extend(node.contains.iter().cloned());
    }
    for expected in ["Counter", "value", "inc"] {
        assert!(
            names.iter().any(|name| name == expected),
            "missing {expected}: {names:?}"
        );
    }
}

#[tokio::test]
async fn shutdown_cleans_pending_install_and_next_start_is_not_ready() {
    let server = MockServer::start();
    let (archive, manifest) = fixture();
    server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/manifest.json");
        then.json_body(manifest);
    });
    server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/pack.tar.gz");
        then.delay(std::time::Duration::from_secs(5)).body(archive);
    });
    let directory = tempfile::tempdir().unwrap();
    let packs = LanguageSupportPacks::with_origin(
        directory.path().to_owned(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    packs.retry(PackId::Rust).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    tokio::time::timeout(std::time::Duration::from_secs(2), packs.shutdown())
        .await
        .unwrap();
    assert!(!directory
        .path()
        .join("installed/rust/1.0.0/aarch64-apple-darwin")
        .exists());
    let staging = directory
        .path()
        .join("staging/rust/1.0.0/aarch64-apple-darwin");
    assert!(!staging.exists() || std::fs::read_dir(staging).unwrap().next().is_none());
    let reopened = LanguageSupportPacks::with_origin(
        directory.path().to_owned(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    assert!(reopened
        .snapshot()
        .packs
        .iter()
        .all(|pack| pack.phase == PackPhase::NotInstalled));
}

/// 真实 rust-analyzer 通过完整安装路径运行；仅仓库范围不依赖 rust-src。
#[tokio::test]
#[ignore = "需要构建当前平台语言支持包并已有 Cargo/rustc"]
async fn real_installed_rust_pack_analyzes_with_existing_toolchain() {
    use crate::structure_review::{
        code_graph::{AnalysisInput, AnalysisOptions, LanguageAnalyzer},
        rust_analyzer::{RustDependencyScope, RustLanguageAnalyzer},
    };
    let target = if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else {
        "aarch64-apple-darwin"
    };
    let source =
        PathBuf::from(std::env::var_os("NARRANT_LANGUAGE_PACK_FIXTURE").expect("真实包目录"));
    let package = source.join("rust/1.0.0").join(target);
    let server = MockServer::start();
    for name in ["manifest.json", "pack.tar.gz"] {
        let bytes = std::fs::read(package.join(name)).unwrap();
        server.mock(|when, then| {
            when.path(format!("/rust/1.0.0/{target}/{name}"));
            then.body(bytes);
        });
    }
    let directory = tempfile::tempdir().unwrap();
    let packs = LanguageSupportPacks::with_origin(
        directory.path().join("packs"),
        server.base_url(),
        target.into(),
    )
    .unwrap();
    let lease = packs
        .ensure(&[PackId::Rust], &CancellationToken::new())
        .await
        .unwrap();
    let analyzer =
        RustLanguageAnalyzer::new(lease.path(PackId::Rust).unwrap().join(if cfg!(windows) {
            "bin/rust-analyzer.exe"
        } else {
            "bin/rust-analyzer"
        }))
        .with_dependency_scope(RustDependencyScope::RepositoryOnly);
    let repository = directory.path().join("project");
    std::fs::create_dir_all(repository.join("src")).unwrap();
    std::fs::write(
        repository.join("Cargo.toml"),
        "[package]\nname = \"pack_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        repository.join("src/lib.rs"),
        "pub fn answer() -> u32 { 42 }",
    )
    .unwrap();
    let graph = tokio::task::spawn_blocking(move || {
        analyzer.analyze(&AnalysisInput {
            repository_root: repository,
            files: vec!["src/lib.rs".into()],
            options: AnalysisOptions {
                include_tests: false,
            },
        })
    })
    .await
    .unwrap()
    .unwrap();
    let mut pending = graph.root().contains.clone();
    let mut found = false;
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap().data();
        found |= node.name.as_deref() == Some("answer");
        pending.extend(node.contains.iter().cloned());
    }
    assert!(found, "真实安装后的 Rust 支持包应能识别函数");
}
