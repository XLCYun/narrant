//! 单侧源码提取结果；只保存应用拥有的声明与词法证据，不泄露 parser 节点。

use std::ops::Range;

use super::{StructureDiagnostic, StructureNativeKind, StructureRange, StructureRelationshipKind};

/// 文件内声明编号；跨文件及跨版本身份由仓库索引与匹配阶段建立。
pub(crate) type DeclarationIndex = usize;

/// Rust 模块源码路径来自当前侧属性；条件属性不能猜成默认物理路径。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExtractedModulePath {
    Default,
    Explicit(String),
    Unknown,
}

/// 源码范围同时保留精确字节与编辑器使用的 UTF-16 坐标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExtractedRange {
    pub bytes: Range<usize>,
    pub position: StructureRange,
}

/// 原生声明拥有的正文和签名不包含具名后代，避免父子重复计算修改。
#[derive(Clone, Debug)]
pub(crate) struct ExtractedDeclaration {
    pub name: String,
    pub qualified_name: String,
    pub native_kind: StructureNativeKind,
    pub parent: Option<DeclarationIndex>,
    pub impl_group: Option<usize>,
    pub scope: usize,
    pub range: ExtractedRange,
    pub name_range: ExtractedRange,
    pub signature: String,
    /// AST 结构和精确 token 内容的无歧义编码；仅排除本声明名称及注释。
    pub signature_tokens: String,
    /// 具名后代整体排除，匿名表达式和回调仍属于当前声明。
    pub body_tokens: String,
    pub documentation: String,
    pub documentation_range: Option<ExtractedRange>,
    pub exported: bool,
    pub module_path: Option<ExtractedModulePath>,
    /// 归属或声明边界受语法恢复影响时为 false，不可据此证明缺失。
    pub reliable: bool,
}

/// Rust 实现块的原始限定目标；跨文件绑定后才能归入类型，不能只按短名聚合。
#[derive(Clone, Debug)]
pub(crate) struct ExtractedImplGroup {
    pub owner: String,
    pub trait_name: Option<String>,
    pub scope: usize,
    pub parent: Option<DeclarationIndex>,
    pub range: ExtractedRange,
    pub signature: String,
    pub header_tokens: String,
    pub documentation: String,
    pub documentation_range: Option<ExtractedRange>,
    pub members: Vec<DeclarationIndex>,
    pub reliable: bool,
}

/// 词法作用域包括文件、块和匿名函数；普通局部变量只进入绑定表。
#[derive(Clone, Debug)]
pub(crate) struct ExtractedScope {
    pub parent: Option<usize>,
    pub declaration: Option<DeclarationIndex>,
    pub range: ExtractedRange,
    pub reliable: bool,
}

/// 绑定种类用于防止将被参数或局部变量遮蔽的名称连到同名全局函数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtractedBindingKind {
    Declaration,
    Parameter,
    Local,
    Import,
}

/// 单侧词法绑定；没有 declaration 的变量仍会遮蔽外层同名目标。
#[derive(Clone, Debug)]
pub(crate) struct ExtractedBinding {
    pub name: String,
    pub scope: usize,
    pub kind: ExtractedBindingKind,
    pub declaration: Option<DeclarationIndex>,
    pub range: ExtractedRange,
    pub type_name: Option<String>,
}

/// 导入/重导出的显式别名；通配符保留为星号，不假设其具体导出。
#[derive(Clone, Debug)]
pub(crate) struct ExtractedImport {
    pub module: String,
    pub imported: String,
    pub local: String,
    pub scope: usize,
    pub range: ExtractedRange,
    pub reexport: bool,
    pub type_only: bool,
}

/// 关系发生点与执行上下文分开；匿名回调不会冒充外层函数直接执行。
#[derive(Clone, Debug)]
pub(crate) struct ExtractedOccurrence {
    pub kind: StructureRelationshipKind,
    pub written_target: String,
    pub declaration: Option<DeclarationIndex>,
    pub impl_group: Option<usize>,
    pub scope: usize,
    pub range: ExtractedRange,
    pub callback_range: Option<ExtractedRange>,
    /// 普通 JS 函数重新绑定 this；箭头函数保留外层 this，二者不能混为一谈。
    pub dynamic_this: bool,
    pub reliable: bool,
}

/// 名称或接收者已有写入时，原声明绑定不足以证明后续调用目标。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ExtractedMutationMember {
    Named(String),
    Computed,
}

/// 绑定写入与特定成员写入分别保留，避免无关字段变化污染所有方法。
#[derive(Clone, Debug)]
pub(crate) struct ExtractedMutation {
    pub name: String,
    pub member: Option<ExtractedMutationMember>,
    pub dynamic_this: bool,
    pub scope: usize,
    pub range: ExtractedRange,
}

/// 单文件完整提取结果；错误位置和作用域可靠性保留给缺失证明与关系绑定。
#[derive(Clone, Debug)]
pub(crate) struct ExtractedFile {
    pub path: String,
    pub declarations: Vec<ExtractedDeclaration>,
    pub impl_groups: Vec<ExtractedImplGroup>,
    pub scopes: Vec<ExtractedScope>,
    pub bindings: Vec<ExtractedBinding>,
    pub imports: Vec<ExtractedImport>,
    pub occurrences: Vec<ExtractedOccurrence>,
    pub mutations: Vec<ExtractedMutation>,
    pub diagnostics: Vec<StructureDiagnostic>,
    pub complete: bool,
    /// 仅表达当前源码中的显式禁用；Rust crate 根的继承由仓库索引处理。
    pub implicit_prelude: bool,
}
