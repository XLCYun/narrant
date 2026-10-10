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

/// 完整安装在重启后复用，不重复访问下载源。
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
        .ensure(&[PackId::Rust], &CancellationToken::new(), None)
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
        .ensure(&[PackId::Rust], &CancellationToken::new(), None)
        .await
        .unwrap();
    metadata.assert_hits(1);
    download.assert_hits(1);
}

/// 同包请求合并，单个取消与跨进程删除均不干扰持有者。
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
        packs.ensure(&[PackId::Rust], &cancelled, Some("cancelled".into())),
        packs.ensure(&[PackId::Rust], &other, Some("current".into())),
        async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            cancelled.cancel();
        }
    );
    assert!(first.is_err());
    let lease = second.unwrap();
    assert_eq!(
        packs.snapshot().requirements.get("current"),
        Some(&vec![PackId::Rust])
    );
    assert!(!packs.snapshot().requirements.contains_key("cancelled"));
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
    assert!(packs.snapshot().requirements.is_empty());
    packs.remove(PackId::Rust, "1.0.0").unwrap();
    assert_eq!(packs.snapshot().packs[0].phase, PackPhase::NotInstalled);
}

/// 校验失败清理暂存，显式重试重新下载并成功安装。
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
        .ensure(&[PackId::Rust], &CancellationToken::new(), None)
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
        .ensure(&[PackId::Rust], &CancellationToken::new(), None)
        .await
        .unwrap();
    assert_eq!(packs.snapshot().packs[0].phase, PackPhase::Ready);
    assert!(packs.snapshot().packs[0].error.is_none());
    good.assert_hits(1);
}

/// 无支持语言时不下载，启动回收没有持有者的临时残留。
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
    let lease = packs
        .ensure(&[], &CancellationToken::new(), None)
        .await
        .unwrap();
    assert!(lease.path(PackId::Rust).is_none());
    assert!(!leftover.exists());
    assert!(packs
        .snapshot()
        .packs
        .iter()
        .all(|pack| pack.phase == PackPhase::NotInstalled));
    network.assert_hits(0);
}

/// 越界清单在归档下载之前被拒绝。
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
            .ensure(&[PackId::Rust], &CancellationToken::new(), None)
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
        native_languages,
    };
    let target = if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else {
        "aarch64-apple-darwin"
    };
    let source =
        PathBuf::from(std::env::var_os("NARRANT_LANGUAGE_PACK_FIXTURE").expect("真实包目录"));
    let version = &versions()[&PackId::TypeScriptJavascript];
    let package = source
        .join("typescript-javascript")
        .join(version)
        .join(target);
    let server = MockServer::start();
    for name in ["manifest.json", "pack.tar.gz"] {
        let bytes = std::fs::read(package.join(name)).unwrap();
        server.mock(|when, then| {
            when.path(format!("/typescript-javascript/{version}/{target}/{name}"));
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
        .ensure(
            &[PackId::TypeScriptJavascript],
            &CancellationToken::new(),
            None,
        )
        .await
        .unwrap();
    let installed = lease.path(PackId::TypeScriptJavascript).unwrap();
    let analyzer = native_languages::from_pack(PackId::TypeScriptJavascript, installed).unwrap();
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

/// 正常退出停止下载并清理临时内容，下次不会误认为已经安装。
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

/// 真实 rust-analyzer 通过完整安装路径发现子项目；仅仓库范围不依赖 rust-src。
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
        .ensure(&[PackId::Rust], &CancellationToken::new(), None)
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
    std::fs::create_dir_all(repository.join("src-tauri/src")).unwrap();
    std::fs::write(
        repository.join("src-tauri/Cargo.toml"),
        "[package]\nname = \"pack_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        repository.join("src-tauri/src/lib.rs"),
        "pub fn answer() -> u32 { 42 }",
    )
    .unwrap();
    let graph = tokio::task::spawn_blocking(move || {
        analyzer.analyze(&AnalysisInput {
            repository_root: repository,
            files: vec!["src-tauri/src/lib.rs".into()],
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

/// 存储故障只成为包失败状态，修复目录后无需重启应用即可重新准备。
#[tokio::test]
async fn storage_initialization_failure_is_visible_and_retryable() {
    let server = MockServer::start();
    let (archive, manifest) = fixture();
    server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/manifest.json");
        then.json_body(manifest);
    });
    server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/pack.tar.gz");
        then.body(archive);
    });
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("packs");
    std::fs::write(&root, b"directory blocked by file").unwrap();
    let packs = LanguageSupportPacks::with_origin(
        root.clone(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    assert!(packs
        .snapshot()
        .packs
        .iter()
        .all(|pack| pack.phase == PackPhase::Failed && pack.error.is_some()));
    std::fs::remove_file(root).unwrap();
    let _lease = packs
        .ensure(&[PackId::Rust], &CancellationToken::new(), None)
        .await
        .unwrap();
    assert_eq!(packs.snapshot().packs[0].phase, PackPhase::Ready);
    assert!(packs.snapshot().packs[0].error.is_none());
}

/// 两个独立管理器通过操作系统锁合并安装，第二个实例等待并复用完整提交。
#[tokio::test]
async fn independent_instances_wait_for_the_same_installation() {
    let server = MockServer::start();
    let (archive, manifest) = fixture();
    server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/manifest.json");
        then.json_body(manifest);
    });
    let download = server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/pack.tar.gz");
        then.delay(std::time::Duration::from_millis(100))
            .body(archive);
    });
    let root = tempfile::tempdir().unwrap();
    let first = LanguageSupportPacks::with_origin(
        root.path().to_owned(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    let second = LanguageSupportPacks::with_origin(
        root.path().to_owned(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    let first_cancel = CancellationToken::new();
    let second_cancel = CancellationToken::new();
    let (first_lease, second_lease) =
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::join!(
                first.ensure(&[PackId::Rust], &first_cancel, None),
                second.ensure(&[PackId::Rust], &second_cancel, None)
            )
        })
        .await
        .unwrap();
    assert!(first_lease.is_ok());
    assert!(second_lease.is_ok());
    download.assert_hits(1);
}

/// 清单通过而文件摘要错误时，同样不能提交任何可用安装。
#[tokio::test]
async fn extraction_integrity_failure_cleans_staging() {
    let server = MockServer::start();
    let (archive, mut manifest) = fixture();
    manifest["files"][0]["sha256"] = serde_json::json!("0".repeat(64));
    server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/manifest.json");
        then.json_body(manifest);
    });
    server.mock(|when, then| {
        when.path("/rust/1.0.0/aarch64-apple-darwin/pack.tar.gz");
        then.body(archive);
    });
    let root = tempfile::tempdir().unwrap();
    let packs = LanguageSupportPacks::with_origin(
        root.path().to_owned(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    let failure = packs
        .ensure(
            &[PackId::Rust],
            &CancellationToken::new(),
            Some("review".into()),
        )
        .await
        .err()
        .unwrap();
    assert!(failure.contains("文件摘要不匹配"));
    assert!(packs.snapshot().requirements.is_empty());
    assert!(!root
        .path()
        .join("installed/rust/1.0.0/aarch64-apple-darwin")
        .exists());
    assert_eq!(
        std::fs::read_dir(root.path().join("staging/rust/1.0.0/aarch64-apple-darwin"))
            .unwrap()
            .count(),
        0
    );
}
