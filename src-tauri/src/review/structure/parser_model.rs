//! 仅供结构分析内部使用的自有解析证据，不作为 IPC 或图节点模型。

use std::ops::Range;

/// 仅允许调用随应用静态编译的四种语法模式；JS 模式也支持 JSX。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SourceLanguage {
    TypeScript,
    Tsx,
    JavaScript,
    Rust,
}

impl SourceLanguage {
    /// 发现源码和验证缺侧覆盖共用同一语言范围，避免支持后缀偏离后误判增删。
    pub(crate) fn from_path(path: &str) -> Option<Self> {
        match path.rsplit_once('.')?.1 {
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "rs" => Some(Self::Rust),
            _ => None,
        }
    }
}

/// 原文件中的零基行号和 UTF-8 字节列；不能直接当成 Monaco UTF-16 列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourcePosition {
    pub line: usize,
    pub byte_column: usize,
}

/// 原文件的半开字节区间；零宽区间可以表示解析器补出的缺失 token。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceRange {
    pub bytes: Range<usize>,
    pub start: SourcePosition,
    pub end: SourcePosition,
}

/// 完整语法树的自有节点；索引只在同一 ParsedSource 中有效。
#[derive(Debug, Clone)]
pub(crate) struct SyntaxNode {
    pub native_kind: String,
    pub range: SourceRange,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub field: Option<String>,
    pub named: bool,
    pub extra: bool,
    pub is_error: bool,
    pub is_missing: bool,
    /// 当前节点或任意后代有错误；局部无错误不代表祖先作用域可信。
    pub contains_error: bool,
}

/// provider 的两层位置角色，仅用于保存证据，不代替最终声明层级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutlineRole {
    Item,
    Member,
}

/// 保留第三方可复用提取结果；原生种类与 provider 分类始终分开。
#[derive(Debug, Clone)]
pub(crate) struct OutlineEvidence {
    pub name: String,
    pub native_kind: String,
    pub provider_category: String,
    /// provider 的原始签名可能只是首行，后续 adapter 必须验证其完整性。
    pub signature: String,
    pub range: SourceRange,
    pub syntax_node: usize,
    /// 此索引指向 outline 中的父条目，而非 syntax 中的节点。
    pub parent: Option<usize>,
    pub role: OutlineRole,
    pub is_import: Option<bool>,
    pub is_exported: Option<bool>,
    pub is_public: Option<bool>,
}

/// 分别表达错误恢复区域和缺失 token，不以空 outline 表示没有声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyntaxDiagnosticKind {
    Error,
    Missing,
}

/// 诊断保留语法节点与原文位置，缺失 token 不会写入原文。
#[derive(Debug, Clone)]
pub(crate) struct SyntaxDiagnostic {
    pub kind: SyntaxDiagnosticKind,
    pub syntax_node: usize,
    pub range: SourceRange,
}

/// 一次内存解析的完整证据；语法完整不等于声明或关系分析完整。
#[derive(Debug, Clone)]
pub(crate) struct ParsedSource {
    pub language: SourceLanguage,
    pub source: String,
    pub provider: &'static str,
    pub syntax: Vec<SyntaxNode>,
    pub outline: Vec<OutlineEvidence>,
    pub diagnostics: Vec<SyntaxDiagnostic>,
}

/// 解析器不可用、规则不可用与可恢复语法诊断分开返回。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParseFailure {
    Parser(String),
    OutlineRules(String),
    InconsistentEvidence(String),
}

impl std::fmt::Display for ParseFailure {
    /// 将故障阶段保留在日志中，避免误写成“文件没有结构”。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parser(message) => write!(formatter, "结构解析器失败：{message}"),
            Self::OutlineRules(message) => write!(formatter, "结构提取规则失败：{message}"),
            Self::InconsistentEvidence(message) => write!(formatter, "结构证据不一致：{message}"),
        }
    }
}

impl std::error::Error for ParseFailure {}
