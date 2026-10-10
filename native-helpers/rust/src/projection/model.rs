use std::collections::BTreeMap;

/// 代码节点的六种通用类别，具体语言种类由 native_kind 保留。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "category", rename_all = "snake_case")]
pub enum CodeNode {
    /// 包、模块、命名空间等组织单元。
    Container(CodeNodeData),
    /// 类、结构体、接口、trait、枚举、类型别名。
    Type(CodeNodeData),
    /// 独立建模的函数、方法、构造函数、访问器等可调用对象。
    Callable(CodeNodeData),
    /// 字段、属性、常量、全局变量、枚举成员。
    Value(CodeNodeData),
    /// 独立建模的非 Callable 执行区域，例如顶层语句或类静态块。
    #[allow(dead_code)] // 公共 DTO 保留六类，当前 Rust 规则不生成独立执行区。
    ExecutionRegion(CodeNodeData),
    /// 无法合理归入前五类的语言特有实体。
    Extension(CodeNodeData),
}

/// 当前六类节点共用的数据，类别由外层枚举变体唯一确定。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CodeNodeData {
    pub id: NodeId,
    pub language: LanguageId,
    /// 原语言中的具体种类，例如 class、struct、interface、getter。
    pub native_kind: String,
    /// 声明名称，匿名对象可以没有名称。
    pub name: Option<String>,
    /// 语言实现生成的规范化声明代码，不包含函数体或嵌套子对象。
    pub signature: Option<String>,
    /// 自身实现内容的规范化代码，例如函数体或初始化表达式；不重复嵌入子对象内容。
    /// None 表示没有实现内容，不能用来掩盖提取失败；子表达式可由语言实现投影为身份占位。
    /// 未单独建模的表达式（如当前 Rust 映射中的闭包）仍保留在所属对象的代码中。
    pub code: Option<String>,
    /// 节点对应的原始源码，仅供展示，不参与身份和结构差异比较。
    /// 保留声明、实现、注释、排版及嵌套成员；无对应源码时为 None。
    pub source: Option<String>,
    /// 直接包含的声明节点，分类从目标节点的枚举变体读取。
    pub contains: Vec<NodeId>,
    /// 当前节点发出的非包含关联，允许指向自身或形成引用环。
    pub references: Vec<CodeReference>,
    /// 仅保存参与比较的语言特有语义，不包含时间戳或展示数据。
    pub extensions: ExtensionFields,
}

impl CodeNode {
    /// 读取任意类别节点的公共数据。
    pub fn data(&self) -> &CodeNodeData {
        match self {
            Self::Container(data)
            | Self::Type(data)
            | Self::Callable(data)
            | Self::Value(data)
            | Self::ExecutionRegion(data)
            | Self::Extension(data) => data,
        }
    }

    /// 从枚举变体推导分类，供调用方筛选节点。
    pub fn category(&self) -> CodeCategory {
        match self {
            Self::Container(_) => CodeCategory::Container,
            Self::Type(_) => CodeCategory::Type,
            Self::Callable(_) => CodeCategory::Callable,
            Self::Value(_) => CodeCategory::Value,
            Self::ExecutionRegion(_) => CodeCategory::ExecutionRegion,
            Self::Extension(_) => CodeCategory::Extension,
        }
    }
}

/// 用于筛选节点的分类值，始终由 CodeNode 变体推导。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeCategory {
    Container,
    Type,
    Callable,
    Value,
    ExecutionRegion,
    Extension,
}

/// 按命名空间保存语言特有语义，例如 rust.layout，不包含版本封套。
pub type ExtensionFields = BTreeMap<String, serde_json::Value>;

/// 当前节点发出的非包含关联，来源由所属节点确定。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct CodeReference {
    /// 有约定方向和含义的名称，例如 code.calls、code.member_of。
    pub semantic: String,
    pub target: ReferenceTarget,
}

/// 区分已解析目标的位置与目标尚未解析的情况。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReferenceTarget {
    /// 已解析，且目标节点保存在当前图中。
    Node { id: NodeId },
    /// 已解析，但目标不在当前图中，可能仍位于同一仓库。
    External {
        /// 带语言命名空间的稳定符号身份，不依赖 checkout 绝对路径。
        symbol: String,
    },
    /// 尚不能确定目标身份，也无法确定目标是否在当前图中。
    Unresolved {
        /// 观测到的引用名称，不代表已经确定的符号身份。
        name: String,
    },
}

/// 整图唯一的稳定身份，包含语言命名空间，不依赖 checkout 的绝对路径。
///
/// 身份的具体编码由语言实现约定，公共模块不推断重命名或移动。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct NodeId(pub String);

/// 节点的语言标识，不限制在固定语言枚举中。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct LanguageId(pub String);

/// 语言提取阶段的测试选项。
#[derive(Clone, Copy, Default)]
pub struct AnalysisOptions {
    pub include_tests: bool,
}

/// 仅序列化统一协议的候选数据，公共 Host 负责唯一的最终构图与校验。
#[derive(serde::Serialize)]
pub struct CodeGraph {
    pub root_contains: Vec<NodeId>,
    pub nodes: Vec<CodeNode>,
    pub external_symbols: Vec<ExternalSymbol>,
}
/// 虚拟根的直接子节点。
#[derive(Default)]
pub struct CodeRoot {
    pub contains: Vec<NodeId>,
}
/// 已解析图外目标的显示名称和真实范围。
#[derive(serde::Serialize)]
pub struct ExternalSymbol {
    pub symbol: String,
    pub name: String,
    pub scope: SymbolScope,
}
/// 来源相对当前仓库划分。
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolScope {
    Repository,
    Dependency,
}
