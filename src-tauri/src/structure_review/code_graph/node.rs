use std::collections::BTreeMap;

/// 代码节点的六种通用类别，具体语言种类由 native_kind 保留。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeNode {
    /// 包、模块、命名空间等组织单元。
    Container(CodeNodeData),
    /// 类、结构体、接口、trait、枚举、类型别名。
    Type(CodeNodeData),
    /// 函数、方法、构造函数、访问器、闭包。
    Callable(CodeNodeData),
    /// 字段、属性、常量、全局变量、枚举成员。
    Value(CodeNodeData),
    /// 独立建模的非 Callable 执行区域，例如顶层语句或类静态块。
    ExecutionRegion(CodeNodeData),
    /// 无法合理归入前五类的语言特有实体。
    Extension(CodeNodeData),
}

/// 当前六类节点共用的数据，类别由外层枚举变体唯一确定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeNodeData {
    pub id: NodeId,
    pub language: LanguageId,
    /// 原语言中的具体种类，例如 class、struct、interface、getter。
    pub native_kind: String,
    /// 声明名称，匿名对象可以没有名称。
    pub name: Option<String>,
    /// 语言实现生成的规范化声明代码，不包含函数体或嵌套子对象。
    pub signature: Option<String>,
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
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CodeReference {
    /// 有约定方向和含义的名称，例如 code.calls、code.member_of。
    pub semantic: String,
    pub target: ReferenceTarget,
}

/// 区分已解析目标的位置与目标尚未解析的情况。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReferenceTarget {
    /// 已解析，且目标节点保存在当前图中。
    Node(NodeId),
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
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub String);

/// 节点的语言标识，不限制在固定语言枚举中。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageId(pub String);
