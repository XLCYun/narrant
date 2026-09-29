use super::*;
use flate2::{write::GzEncoder, Compression};
use httpmock::prelude::*;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

/// 完整 Python 包包括原生工具、空控制入口与固定标准库许可证。
fn fixture(missing_control: bool, executable: bool) -> (Vec<u8>, serde_json::Value) {
    let files = [
        ("bin/ty", "native ty fixture"),
        ("control/ty.toml", ""),
        ("licenses/ty-LICENSE", "ty license"),
        ("licenses/ruff-LICENSE", "ruff license"),
        ("licenses/typeshed-LICENSE", "typeshed license"),
    ];
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let mut metadata = Vec::new();
    for (name, source) in files {
        if missing_control && name == "control/ty.toml" {
            continue;
        }
        let mut header = tar::Header::new_gnu();
        header.set_size(source.len() as u64);
        header.set_mode(if name.starts_with("bin/") {
            0o755
        } else {
            0o644
        });
        header.set_cksum();
        tar.append_data(&mut header, name, source.as_bytes())
            .unwrap();
        metadata.push(serde_json::json!({"path":name,"size":source.len(),"sha256":format!("{:x}",Sha256::digest(source.as_bytes())),"executable":name.starts_with("bin/") && executable}));
    }
    let bytes = tar.into_inner().unwrap().finish().unwrap();
    let manifest = serde_json::json!({"schemaVersion":1,"id":"python","version":"1.0.0","target":"aarch64-apple-darwin","archiveSize":bytes.len(),"archiveSha256":format!("{:x}",Sha256::digest(&bytes)),"files":metadata});
    (bytes, manifest)
}

/// 失败可重试，使用租约期间不可删除；安装后重启识别同一固定版本。
#[tokio::test]
async fn python_pack_retries_then_holds_its_installation_until_the_lease_ends() {
    let server = MockServer::start();
    let (bytes, manifest) = fixture(false, true);
    let mut failed = server.mock(|when, then| {
        when.path("/python/1.0.0/aarch64-apple-darwin/manifest.json");
        then.status(503);
    });
    let root = tempfile::tempdir().unwrap();
    let packs = LanguageSupportPacks::with_origin(
        root.path().into(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    assert!(packs
        .ensure(&[PackId::Python], &CancellationToken::new(), None)
        .await
        .is_err());
    failed.delete();
    let metadata = server.mock(|when, then| {
        when.path("/python/1.0.0/aarch64-apple-darwin/manifest.json");
        then.json_body(manifest);
    });
    server.mock(|when, then| {
        when.path("/python/1.0.0/aarch64-apple-darwin/pack.tar.gz");
        then.body(bytes);
    });
    packs.retry(PackId::Python).unwrap();
    let lease = packs
        .ensure(
            &[PackId::Python],
            &CancellationToken::new(),
            Some("python-review".into()),
        )
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(lease.path(PackId::Python).unwrap().join("control/ty.toml")).unwrap(),
        b""
    );
    assert!(packs.remove(PackId::Python, "1.0.0").is_err());
    drop(lease);
    packs.shutdown().await;
    drop(packs);
    let reopened = LanguageSupportPacks::with_origin(
        root.path().into(),
        server.base_url(),
        "aarch64-apple-darwin".into(),
    )
    .unwrap();
    assert_eq!(
        reopened
            .snapshot()
            .packs
            .iter()
            .find(|pack| pack.id == PackId::Python)
            .unwrap()
            .phase,
        PackPhase::Ready
    );
    let lease = reopened
        .ensure(&[PackId::Python], &CancellationToken::new(), None)
        .await
        .unwrap();
    metadata.assert_hits(1);
    drop(lease);
    assert!(reopened.remove(PackId::Python, "1.0.0").is_ok());
    reopened.shutdown().await;
}

/// 必需控制文件、可执行标志与摘要失败都必须阻止安装，不能留下 ready 状态。
#[tokio::test]
async fn python_pack_rejects_incomplete_or_corrupted_payloads() {
    for (missing, executable, corrupt) in [
        (true, true, false),
        (false, false, false),
        (false, true, true),
    ] {
        let server = MockServer::start();
        let (mut bytes, manifest) = fixture(missing, executable);
        if corrupt {
            bytes[0] ^= 1;
        }
        server.mock(|when, then| {
            when.path("/python/1.0.0/aarch64-apple-darwin/manifest.json");
            then.json_body(manifest);
        });
        server.mock(|when, then| {
            when.path("/python/1.0.0/aarch64-apple-darwin/pack.tar.gz");
            then.body(bytes);
        });
        let root = tempfile::tempdir().unwrap();
        let packs = LanguageSupportPacks::with_origin(
            root.path().into(),
            server.base_url(),
            "aarch64-apple-darwin".into(),
        )
        .unwrap();
        assert!(packs
            .ensure(&[PackId::Python], &CancellationToken::new(), None)
            .await
            .is_err());
        assert_eq!(
            packs
                .snapshot()
                .packs
                .iter()
                .find(|pack| pack.id == PackId::Python)
                .unwrap()
                .phase,
            PackPhase::Failed
        );
        packs.shutdown().await;
    }
}

/// 真实平台支持包经过应用安装服务后，可独立提供源码和标准库导航。
#[tokio::test]
#[ignore = "需要当前平台真实 Python 包和 NARRANT_LANGUAGE_PACK_FIXTURE"]
async fn real_installed_python_pack_analyzes_without_user_python() {
    use crate::structure_review::{
        code_graph::{AnalysisInput, LanguageAnalyzer, ReferenceTarget},
        python_analyzer::PythonLanguageAnalyzer,
    };
    let target = if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else {
        "aarch64-apple-darwin"
    };
    let source =
        PathBuf::from(std::env::var_os("NARRANT_LANGUAGE_PACK_FIXTURE").expect("真实包目录"));
    let package = source.join("python/1.0.0").join(target);
    let server = MockServer::start();
    for name in ["manifest.json", "pack.tar.gz"] {
        let bytes = std::fs::read(package.join(name)).unwrap();
        server.mock(|when, then| {
            when.path(format!("/python/1.0.0/{target}/{name}"));
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
        .ensure(&[PackId::Python], &CancellationToken::new(), None)
        .await
        .unwrap();
    let executable = lease.path(PackId::Python).unwrap().join(if cfg!(windows) {
        "bin/ty.exe"
    } else {
        "bin/ty"
    });
    let repository = directory.path().join("repository");
    std::fs::create_dir(&repository).unwrap();
    std::fs::write(repository.join("main.py"), "def run(): return len([])\n").unwrap();
    let graph = tokio::task::spawn_blocking(move || {
        PythonLanguageAnalyzer::new(executable).analyze(&AnalysisInput {
            repository_root: repository,
            files: vec!["main.py".into()],
            options: Default::default(),
        })
    })
    .await
    .unwrap()
    .unwrap();
    let mut pending = graph.root().contains.clone();
    let mut found = false;
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap();
        pending.extend(node.data().contains.clone());
        found |= node.data().references.iter().any(|reference| matches!(&reference.target, ReferenceTarget::External { symbol } if graph.external_symbols()[symbol].name == "len"));
    }
    assert!(found);
    assert!(packs.remove(PackId::Python, "1.0.0").is_err());
    drop(lease);
    packs.remove(PackId::Python, "1.0.0").unwrap();
    packs.shutdown().await;
}
