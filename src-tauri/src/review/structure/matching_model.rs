//! 配对与关系阶段共享的内部身份映射，不进入 IPC。

use std::collections::HashMap;

use super::model::{
    StructureDiagnostic, StructureFileCoverage, StructureGroupId, StructureImplementationGroup,
    StructureNode, StructureNodeId, StructureRelationship, StructureSourceId,
    StructureUnresolvedRelationship,
};

/// 文件路径和提取器本地声明编号共同确定一侧声明。
pub(crate) type DeclarationKey = (String, usize);

/// 文件路径和本地 impl 编号共同确定一侧实现组。
pub(crate) type ImplementationKey = (String, usize);

/// 已绑定的实现组元数据；归属未知时不猜测同名全局类型。
#[derive(Clone, Debug)]
pub(crate) struct MatchedImplementation {
    pub group_id: StructureGroupId,
    pub owner_id: Option<StructureNodeId>,
    pub trait_target_id: Option<StructureNodeId>,
}

/// 单侧的身份映射供关系阶段把词法证据映射为已配对图端点。
#[derive(Clone, Debug, Default)]
pub(crate) struct MatchedSide {
    pub declarations: HashMap<DeclarationKey, StructureNodeId>,
    pub implementations: HashMap<ImplementationKey, MatchedImplementation>,
    pub source_ids: HashMap<String, StructureSourceId>,
}

/// 声明配对结果拥有完整节点与分组，内部映射只服务后续关系解析。
#[derive(Clone, Debug, Default)]
pub(crate) struct MatchedStructure {
    pub nodes: Vec<StructureNode>,
    pub implementation_groups: Vec<StructureImplementationGroup>,
    pub before: MatchedSide,
    pub after: MatchedSide,
    pub diagnostics: Vec<StructureDiagnostic>,
}

/// 关系阶段返回新增外部节点及更新后的逐文件关系覆盖。
#[derive(Clone, Debug, Default)]
pub(crate) struct ResolvedRelationships {
    pub relationships: Vec<StructureRelationship>,
    pub unresolved_relationships: Vec<StructureUnresolvedRelationship>,
    pub external_nodes: Vec<StructureNode>,
    pub file_coverage: Vec<StructureFileCoverage>,
}

/// 文件节点身份与真实文件对保持一致，真实 rename 两侧共享节点。
pub(crate) fn file_node_id(source_id: &StructureSourceId) -> StructureNodeId {
    StructureNodeId(format!("file:{}", source_id.0))
}

/// 长度前缀避免包名和限定名中的分隔符造成外部身份碰撞。
pub(crate) fn external_node_id(package_name: &str, name: &str) -> StructureNodeId {
    StructureNodeId(format!(
        "external:{}:{}:{}",
        package_name.len(),
        package_name,
        name
    ))
}
