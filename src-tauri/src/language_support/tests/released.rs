use super::*;
use crate::structure_review::{
    code_graph::{AnalysisInput, AnalysisOptions, CodeCategory, LanguageAnalyzer},
    codegraph_client::CodeGraphAnalyzer,
};

/// 从空的隔离目录按需下载固定公开 Python 包，再通过该包返回真实源码图。
#[tokio::test]
#[ignore = "需要公开 CodeGraph v0.1.0 及 GitHub 网络；不接受本机候选或源覆盖"]
async fn real_released_python_pack_downloads_on_demand_and_analyzes() {
    let root = tempfile::tempdir().unwrap();
    let packs = LanguageSupportPacks::new(root.path().into()).unwrap();
    assert_eq!(packs.inner.origin, ORIGIN);
    assert!(packs
        .snapshot()
        .packs
        .iter()
        .all(|pack| pack.phase == PackPhase::NotInstalled));
    let lease = packs
        .ensure(&[PackId::Python], &CancellationToken::new(), None)
        .await
        .unwrap();
    assert_eq!(lease.held.len(), 1);
    assert_eq!(lease.held[0].0.id, PackId::Python);
    let manifest = packs.inner.storage.inspect(&lease.held[0].0).unwrap();
    assert_eq!(
        manifest.source_commit,
        "0552cc97f217d772b6d883ba75cc9007d3fc8309"
    );
    assert!(!manifest.source_dirty);
    println!(
        "Released Python source: {}; archive SHA256: {}",
        manifest.source_commit, manifest.archive_sha256
    );
    let installed = lease.path(PackId::Python).unwrap().to_owned();
    assert!(installed.starts_with(root.path().canonicalize().unwrap()));
    assert_eq!(
        fs::read_dir(root.path().join("installed"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsString::from("python")]
    );
    for pack in packs.snapshot().packs {
        assert_eq!(
            pack.phase,
            if pack.id == PackId::Python {
                PackPhase::Ready
            } else {
                PackPhase::NotInstalled
            }
        );
    }
    let descriptor: serde_json::Value =
        serde_json::from_slice(&fs::read(installed.join("control/codegraph.json")).unwrap())
            .unwrap();
    assert_eq!(
        descriptor,
        serde_json::json!({"schema_version": 1, "pack_id": "python", "pack_version": "0.1.0",
            "engine_version": "0.1.0", "protocol": "codegraph.analyzer/1"})
    );
    let fixture = tempfile::tempdir().unwrap();
    let source = "def released_value():\n    return len([1, 2, 3])\n";
    fs::write(fixture.path().join("main.py"), source).unwrap();
    let input = AnalysisInput {
        repository_root: fixture.path().into(),
        files: vec!["main.py".into()],
        options: AnalysisOptions::default(),
    };
    let graph = tokio::task::spawn_blocking(move || {
        CodeGraphAnalyzer::from_pack("python", &installed)
            .unwrap()
            .analyze(&input)
    })
    .await
    .unwrap()
    .unwrap();
    assert!(!graph.root().contains.is_empty());
    let mut pending = graph.root().contains.clone();
    let mut functions = Vec::new();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap();
        pending.extend(node.data().contains.iter().cloned());
        if node.category() == CodeCategory::Callable
            && node.data().name.as_deref() == Some("released_value")
        {
            functions.push(node);
        }
    }
    assert_eq!(functions.len(), 1);
    let returned_source = functions[0].data().source.as_deref().unwrap();
    assert!(!returned_source.is_empty() && source.contains(returned_source));
    println!("Released production-origin proof: python 0.1.0 codegraph.analyzer/1; requested Python only; nonempty source graph");
    drop(lease);
    packs.shutdown().await;
}
