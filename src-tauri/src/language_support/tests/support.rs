//! 安装测试只用公开发布资产或独立伪包，不接触 helper 私有布局。
use super::*;
use httpmock::MockServer;
use serde_json::Value;

/// 描述符与 manifest 使用同一固定公共身份。
pub(crate) fn descriptor(id: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"schema_version":1,"pack_id":id,"pack_version":"0.1.1","engine_version":"0.1.1","protocol":"codegraph.analyzer/1"})).unwrap()
}

/// 将伪包清单绑定到实际返回的字节；清单校验失败不能阻止后续归档边界测试。
pub(crate) fn metadata(server: &MockServer, manifest: &Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(manifest).unwrap();
    let asset = format!(
        "{}-{}",
        manifest["id"].as_str().unwrap(),
        manifest["target"].as_str().unwrap()
    );
    let hashes = format!(
        "{:x}  {asset}-manifest.json\n{}  {asset}-pack.tar.gz\n",
        Sha256::digest(&bytes),
        manifest["archiveSha256"].as_str().unwrap()
    );
    server.mock(|when, then| {
        when.path("/v0.1.1/SHA256SUMS");
        then.body(hashes);
    });
    bytes
}

/// 本机候选的真实归档按公共发布 URL 提供给真实安装器；缺少选中资产会明确失败。
pub(crate) fn serve_artifacts(server: &MockServer, source: &Path, target: &str) {
    let mut hashes = String::new();
    for id in PackId::SUPPORTED {
        let package = source.join(id.slug()).join("0.1.1").join(target);
        if !package.join("manifest.json").is_file() {
            continue;
        }
        for name in ["manifest.json", "pack.tar.gz"] {
            let bytes = std::fs::read(package.join(name)).expect("真实候选必须有完整归档");
            let asset = format!("{}-{target}-{name}", id.slug());
            hashes.push_str(&format!("{:x}  {asset}\n", Sha256::digest(&bytes)));
            server.mock(|when, then| {
                when.path(format!("/v0.1.1/{asset}"));
                then.body(bytes);
            });
        }
    }
    assert!(!hashes.is_empty(), "缺少真实 CodeGraph 产物");
    server.mock(|when, then| {
        when.path("/v0.1.1/SHA256SUMS");
        then.body(hashes);
    });
}
