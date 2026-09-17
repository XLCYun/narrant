//! Oxc arena 仅存在于解析作用域；语义层消费拥有数据的文档、声明与引用位置。
use super::{failure, identity, test_code, text, Result};
use crate::{
    lsp::{file_uri, Position},
    structure_review::code_graph::{
        AnalysisOptions, CodeCategory, CodeNode, CodeNodeData, LanguageId, NodeId,
    },
};
use oxc_allocator::Allocator;
use oxc_ast_visit::Visit;
use oxc_codegen::{Codegen, CodegenOptions, CommentOptions};
use oxc_parser::{config::TokensParserConfig, ParseOptions, Parser};
use oxc_semantic::SemanticBuilder;
use oxc_span::{SourceType, Span};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
pub(crate) mod aliases;
mod content;
mod declarations;
mod occurrences;

/// 单个声明片段；重载保持全部声明和实现的原始顺序。
#[derive(Clone, Debug)]
pub(crate) struct DeclarationPart {
    pub span: Span,
    pub selection: Span,
    pub body: Option<Span>,
    /// 父内容扣除变量语句等包装时使用，精确声明定位仍使用 span。
    pub parent_exclusion: Span,
    /// 声明的定位名字；重载汇总各片段，匿名声明为空。
    pub names: Vec<Span>,
    /// 完整展示范围包含 const/export 等声明包装，不参与位置归属或比较。
    pub source_span: Span,
}

/// 稳定声明对象。完整范围可以覆盖重载间空隙，归属查询使用精确 parts。
#[derive(Clone, Debug)]
pub(crate) struct Declaration {
    pub node: CodeNode,
    pub parent: Option<NodeId>,
    pub span: Span,
    pub selections: Vec<Span>,
    pub parts: Vec<DeclarationPart>,
    /// 只有直接函数初始化器提供运行时函数值证据，普通类型注解不提供。
    pub callable_value: Option<Span>,
    /// 声明文件或显式 ambient 上下文中的具名函数引入值绑定；不包含结构调用签名。
    pub ambient_function: bool,
}

/// 语法意图只是语义解析的候选证据，不等于已确认关系。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OccurrenceKind {
    Call,
    Construct,
    Reference,
    Extends,
    Implements,
    TypeUse,
    JsxComponent,
}
impl OccurrenceKind {
    /// 将意图映射到代码图已有关系名称。
    pub fn semantic(self) -> &'static str {
        match self {
            Self::Call | Self::Construct => "code.calls",
            Self::Reference => "code.references",
            Self::Extends => "code.extends",
            Self::Implements => "code.implements",
            Self::TypeUse => "code.type_use",
            Self::JsxComponent => "jsx.uses_component",
        }
    }
}

/// 每个候选引用保留完整表达式和实际查询名；运行时调用必须再经 LSP 确认。
#[derive(Clone, Debug)]
pub(crate) struct Occurrence {
    pub owner: NodeId,
    pub kind: OccurrenceKind,
    pub span: Span,
    pub selection: Span,
    pub position: Position,
    pub text: String,
    /// 来自原始 AST 的规范引用名；普通注释和排版不能改变未解析关系身份。
    pub semantic_text: String,
    /// 调用/读取根标识符在本文件的词法声明位置，以及后续静态成员路径。
    pub root_binding: Option<Span>,
    pub members: Vec<String>,
    pub dynamic: bool,
    pub type_only: bool,
}

/// 一个固定源码文件，不将 Oxc 的 arena 生命周期泄漏到语言服务层。
#[derive(Debug)]
pub(crate) struct Document {
    pub uri: String,
    pub source: String,
    pub declarations: Vec<Declaration>,
    pub occurrences: Vec<Occurrence>,
    pub excluded: bool,
    pub aliases: Vec<aliases::Alias>,
    /// 被省略的 const 仍可作为内部别名，不需要为续查创建数据节点。
    pub value_aliases: Vec<aliases::ValueAlias>,
}
impl Document {
    /// 先解析原文，再用 Oxc 生成无注释规范代码；两次 typed AST 遍历按声明配对。
    pub fn parse(
        repository: &Path,
        path: PathBuf,
        source: String,
        options: AnalysisOptions,
    ) -> Result<Self> {
        let language = test_code::language(&path)
            .ok_or_else(|| failure("不支持的 TypeScript / JavaScript 文件扩展名"))?;
        let scope = identity::file_scope(repository, &path, language)?;
        if source.len() > u32::MAX as usize {
            return Err(failure("TypeScript 源码超过字节范围上限"));
        }
        let source_type = SourceType::from_path(&path)
            .map_err(failure)?
            .with_jsx(language == "javascript" || path.extension().is_some_and(|s| s == "tsx"));
        let arena = Allocator::default();
        let parsed = Parser::new(&arena, &source, source_type)
            .with_options(ParseOptions {
                parse_regular_expression: true,
                ..ParseOptions::default()
            })
            .parse();
        if parsed.fatal_error || !parsed.diagnostics.is_empty() {
            return Err(failure(format!(
                "TypeScript 源文件无法解析 {}：{:?}",
                path.display(),
                parsed.diagnostics
            )));
        }
        let checked = SemanticBuilder::new()
            .with_check_syntax_error(true)
            .build(&parsed.program);
        if !checked.diagnostics.is_empty() {
            return Err(failure(format!(
                "TypeScript 源文件语法无效 {}：{:?}",
                path.display(),
                checked.diagnostics
            )));
        }
        // file_scope 已校验词法/真实边界；分类只看仓库内部分，忽略 checkout 祖先名称。
        let relative = path.strip_prefix(repository).map_err(failure)?;
        let excluded = !options.include_tests && test_code::is_test(relative);
        let mut document = Self {
            uri: file_uri(&path).map_err(failure)?,
            source: source.clone(),
            declarations: vec![],
            occurrences: vec![],
            excluded,
            aliases: vec![],
            value_aliases: vec![],
        };
        if excluded {
            return Ok(document);
        }
        let mut original = declarations::Collector::new();
        original.visit_program(&parsed.program);
        let normalized = Codegen::new()
            .with_options(CodegenOptions {
                comments: CommentOptions::disabled(),
                ..CodegenOptions::default()
            })
            .build(&parsed.program)
            .code;
        let normalized_arena = Allocator::default();
        let normalized_parse = Parser::new(&normalized_arena, &normalized, source_type)
            .with_config(TokensParserConfig)
            .parse();
        if normalized_parse.fatal_error || !normalized_parse.diagnostics.is_empty() {
            return Err(failure("Oxc 规范代码无法重新解析"));
        }
        let content = content::Content::new(
            &normalized,
            &normalized_parse.tokens,
            &normalized_parse.program,
        );
        let mut canonical = declarations::Collector::new();
        canonical.visit_program(&normalized_parse.program);
        if original.items.len() != canonical.items.len() {
            return Err(failure("Oxc 规范化改变了声明数量"));
        }
        let mut ids = Vec::<NodeId>::new();
        let mut counts = BTreeMap::<String, usize>::new();
        for raw in &original.items {
            let owner = raw.parent.map_or(scope.as_str(), |i| ids[i].0.as_str());
            let path = raw
                .identity_path
                .iter()
                .map(|name| format!("::binding:{}", identity::component(name)))
                .collect::<String>();
            let base = format!(
                "{owner}{path}::{}:{}",
                raw.kind,
                identity::component(raw.name.as_deref().unwrap_or("anonymous"))
            );
            let count = counts.entry(base.clone()).or_default();
            *count += 1;
            ids.push(NodeId(if *count == 1 {
                base
            } else {
                format!("{base}#{count}")
            }));
        }
        for (index, (raw, norm)) in original.items.iter().zip(&canonical.items).enumerate() {
            if raw.kind != norm.kind
                || raw.name != norm.name
                || raw.parts.len() != norm.parts.len()
                || raw.parent != norm.parent
                || raw.identity_path != norm.identity_path
            {
                return Err(failure("Oxc 规范化改变了声明结构"));
            }
            let children: Vec<_> = canonical
                .items
                .iter()
                .enumerate()
                .filter(|(_, c)| c.parent == Some(index))
                .map(|(i, _)| ids[i].clone())
                .collect();
            let mut signature = Vec::new();
            let mut bodies = Vec::new();
            for part in &norm.parts {
                // 所有独立建模的内部声明均排除；export 包装声明保持词法透明。
                let nested: Vec<_> = canonical
                    .items
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != index)
                    .flat_map(|(_, c)| c.parts.iter().map(|p| p.parent_exclusion))
                    .filter(|s| contains(part.span, *s) && *s != part.span)
                    .collect();
                let mut omitted = nested.clone();
                if let Some(body) = part.body {
                    omitted.push(body);
                    bodies.push(content.own_text(body, &nested));
                }
                signature.push(format!(
                    "{}{}",
                    norm.prefix,
                    content.own_text(part.span, &omitted)
                ));
            }
            let source_text = raw
                .parts
                .iter()
                .map(|p| source_excerpt(&source, p.source_span, &parsed.program.comments))
                .collect::<Vec<_>>()
                .join("\n");
            let data = CodeNodeData {
                id: ids[index].clone(),
                language: LanguageId(language.into()),
                native_kind: raw.kind.into(),
                name: raw.name.clone(),
                signature: Some(signature.join("\n")),
                code: (!bodies.is_empty()).then(|| bodies.join("\n")),
                source: Some(source_text),
                contains: children,
                references: vec![],
                extensions: BTreeMap::new(),
            };
            let node = match raw.category {
                CodeCategory::Container => CodeNode::Container(data),
                CodeCategory::Type => CodeNode::Type(data),
                CodeCategory::Callable => CodeNode::Callable(data),
                CodeCategory::Value => CodeNode::Value(data),
                CodeCategory::ExecutionRegion => CodeNode::ExecutionRegion(data),
                CodeCategory::Extension => CodeNode::Extension(data),
            };
            document.declarations.push(Declaration {
                node,
                parent: raw.parent.map(|i| ids[i].clone()),
                span: Span::new(
                    raw.parts.first().unwrap().span.start,
                    raw.parts.last().unwrap().span.end,
                ),
                selections: raw
                    .parts
                    .iter()
                    .flat_map(|p| p.names.iter().copied())
                    .collect(),
                parts: raw.parts.clone(),
                callable_value: raw.callable_value,
                ambient_function: raw.ambient_function
                    || source_type.is_typescript_definition()
                        && raw.kind == "function"
                        && raw.name.is_some(),
            });
        }
        let mut aliases = aliases::Collector::new(checked.semantic.scoping(), parsed.program.span);
        aliases.visit_program(&parsed.program);
        document.aliases = aliases.aliases;
        document.value_aliases = aliases.value_aliases;
        let mut references = occurrences::Collector::new(&document, checked.semantic.scoping());
        references.visit_program(&parsed.program);
        document.occurrences = references.finish()?;
        document.attach_file_container(NodeId(scope), language, relative);
        Ok(document)
    }

    /// 文件容器只组织现有对象，不代表语言模块；路径统一用斜线，不参与子对象内容比较。
    fn attach_file_container(&mut self, id: NodeId, language: &str, relative: &Path) {
        if self.declarations.is_empty() {
            return;
        }
        let mut children = Vec::new();
        for declaration in &mut self.declarations {
            if declaration.parent.is_none() {
                children.push(declaration.node.data().id.clone());
                declaration.parent = Some(id.clone());
            }
        }
        self.declarations.push(Declaration {
            node: CodeNode::Container(CodeNodeData {
                id,
                language: LanguageId(language.into()),
                native_kind: "file".into(),
                name: Some(
                    relative
                        .iter()
                        .map(|part| part.to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                ),
                signature: None,
                code: None,
                source: Some(self.source.clone()),
                contains: children,
                references: vec![],
                extensions: BTreeMap::new(),
            }),
            parent: None,
            span: Span::new(0, self.source.len() as u32),
            selections: vec![],
            parts: vec![],
            callable_value: None,
            ambient_function: false,
        });
    }

    /// 查找最内层精确声明；匿名回调的出现位置仍落入具名词法所有者。
    pub fn owner(&self, offset: u32) -> Option<&Declaration> {
        self.declarations
            .iter()
            .filter_map(|d| {
                d.parts
                    .iter()
                    .filter(|p| at(p.span, offset))
                    .map(|p| (p.span.size(), d))
                    .min_by_key(|(len, _)| *len)
            })
            .min_by_key(|(len, _)| *len)
            .map(|(_, d)| d)
    }

    /// 定义响应必须命中声明名或某个重载名，不按相邻声明猜测。
    pub fn declared_at(&self, offset: u32) -> Option<&Declaration> {
        self.declarations
            .iter()
            .find(|d| d.selections.iter().any(|s| at(*s, offset)))
    }
}

/// 左闭右开的位置包含检查，匿名声明仅允许精确起点。
fn at(span: Span, offset: u32) -> bool {
    span.start <= offset && (offset < span.end || span.start == span.end && offset == span.start)
}
/// 判断完整子范围是否属于外层范围。
fn contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// 展示原文包括相邻前导注释；图比较只读取规范签名和代码。
fn source_excerpt(source: &str, span: Span, comments: &[oxc_ast::ast::Comment]) -> String {
    let mut start = span.start;
    for comment in comments.iter().rev().filter(|c| c.span.end <= span.start) {
        if source[comment.span.end as usize..start as usize]
            .trim()
            .is_empty()
        {
            start = comment.span.start;
        } else {
            break;
        }
    }
    source[start as usize..span.end as usize].to_owned()
}

/// 组装阶段更新公共节点数据，图建成后保持只读。
pub(crate) fn data_mut(node: &mut CodeNode) -> &mut CodeNodeData {
    match node {
        CodeNode::Container(d)
        | CodeNode::Type(d)
        | CodeNode::Callable(d)
        | CodeNode::Value(d)
        | CodeNode::ExecutionRegion(d)
        | CodeNode::Extension(d) => d,
    }
}

#[cfg(test)]
#[path = "tests/syntax.rs"]
mod tests;
