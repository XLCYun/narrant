//! 受控真实候选只记录公共包身份与绝对包根；不接受私有 helper 配置。
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf};

/// 发布消费或本机候选的公开定位信息，root 只存在于测试配置。
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Candidate {
    pub id: String,
    pub version: String,
    pub engine_version: String,
    pub protocol: String,
    pub target: String,
    pub root: PathBuf,
}

/// 显式验收缺少配置或包必须失败，不能降级成跳过。
pub fn candidates() -> BTreeMap<String, Candidate> {
    let file = std::env::var_os("NARRANT_CODEGRAPH_PACKS")
        .expect("真实 CLI 验收需要 NARRANT_CODEGRAPH_PACKS=<候选 JSON 绝对路径>");
    let values: Vec<Candidate> =
        serde_json::from_slice(&std::fs::read(file).expect("读取候选配置"))
            .expect("候选配置必须只含公共身份字段");
    let target = if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else {
        "aarch64-apple-darwin"
    };
    let mut candidates = BTreeMap::new();
    for value in values {
        assert_eq!(value.version, "0.1.0");
        assert_eq!(value.engine_version, "0.1.0");
        assert_eq!(value.protocol, "codegraph.analyzer/1");
        assert_eq!(value.target, target);
        assert!(value.root.is_absolute() && value.root.is_dir());
        assert!(
            candidates.insert(value.id.clone(), value).is_none(),
            "重复候选包 ID"
        );
    }
    candidates
}

/// 精确选中一个包根，描述符和可执行入口继续由消费适配校验。
pub fn root(id: &str) -> PathBuf {
    candidates()
        .remove(id)
        .unwrap_or_else(|| panic!("缺少 {id} 真实 CodeGraph 包"))
        .root
}
