//! 语言 adapter 向共同提取流程提供的语法定位，所有索引仅在当前解析结果内有效。

use super::{ParsedSource, StructureCategory, StructureNativeName};

/// 原生声明规则只确定边界和名称；归属、指纹和范围由共同流程统一产生。
#[derive(Debug)]
pub(super) struct DeclarationSeed {
    pub node: usize,
    pub name_node: Option<usize>,
    pub name: String,
    pub category: StructureCategory,
    pub native_name: StructureNativeName,
    /// 正文/成员容器/初始化表达式；这些节点不进入签名指纹。
    pub bodies: Vec<usize>,
    pub exported: bool,
}

/// Rust impl 的类型与 trait 保留原始限定路径，暂不做仓库级解析。
#[derive(Debug)]
pub(super) struct ImplSeed {
    pub node: usize,
    pub owner: String,
    pub trait_name: Option<String>,
    pub body: Option<usize>,
}

/// 导入语法展开为逐个显式别名；scope 由共同遍历确定。
#[derive(Debug)]
pub(super) struct ImportSeed {
    pub node: usize,
    pub binding_node: Option<usize>,
    pub module: String,
    pub imported: String,
    pub local: String,
    pub reexport: bool,
    pub type_only: bool,
}

/// 单种语言给共同流程的原生声明与导入补充。
#[derive(Debug, Default)]
pub(super) struct AdapterOutput {
    pub declarations: Vec<DeclarationSeed>,
    pub impls: Vec<ImplSeed>,
    pub imports: Vec<ImportSeed>,
}

/// 从真实字段取语法节点，不依赖源码正则猜测语言结构。
pub(super) fn field(parsed: &ParsedSource, node: usize, name: &str) -> Option<usize> {
    parsed.syntax[node]
        .children
        .iter()
        .copied()
        .find(|child| parsed.syntax[*child].field.as_deref() == Some(name))
}

/// 使用解析器给出的半开字节区间读取原文。
pub(super) fn text(parsed: &ParsedSource, node: usize) -> &str {
    &parsed.source[parsed.syntax[node].range.bytes.clone()]
}

/// 查找最近的祖先语法种类，保留原生层级判断。
pub(super) fn ancestor(parsed: &ParsedSource, node: usize, kinds: &[&str]) -> Option<usize> {
    let mut current = parsed.syntax[node].parent;
    while let Some(index) = current {
        if kinds.contains(&parsed.syntax[index].native_kind.as_str()) {
            return Some(index);
        }
        current = parsed.syntax[index].parent;
    }
    None
}
