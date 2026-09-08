//! 将静态 grammar 和现成 outline 提取器封装为不依赖 provider 类型的证据。

use std::cell::RefCell;
use std::collections::HashMap;

use ast_grep_core::tree_sitter::StrDoc;
use ast_grep_core::{AstGrep, Node};
use ast_grep_language::SupportLang;
use ast_grep_outline::combined_extractor::CombinedExtractors;
use ast_grep_outline::extractor::parse_outline_rules;
use ast_grep_outline::model::{EntryRole, OutlineEntry};
use ast_grep_outline::DEFAULT_OUTLINE_RULES;

use super::parser_model::*;

thread_local! {
    /// 每个分析线程仅编译一次各语言规则；provider 句柄不离开本模块。
    static EXTRACTORS: RefCell<HashMap<SourceLanguage, Result<CombinedExtractors<SupportLang>, ParseFailure>>> = RefCell::new(HashMap::new());
}

/// 从 Snapshot 提供的内存原文解析一次，保存完整语法和现成提取证据。
pub(crate) fn parse_source(
    language: SourceLanguage,
    source: &str,
) -> Result<ParsedSource, ParseFailure> {
    let root = AstGrep::<StrDoc<SupportLang>>::try_new(source, provider_language(language))
        .map_err(ParseFailure::Parser)?;
    let syntax = collect_syntax(root.root());
    let diagnostics = syntax
        .iter()
        .enumerate()
        .filter_map(|(syntax_node, node)| {
            let kind = if node.is_missing {
                SyntaxDiagnosticKind::Missing
            } else if node.is_error {
                SyntaxDiagnosticKind::Error
            } else {
                return None;
            };
            Some(SyntaxDiagnostic {
                kind,
                syntax_node,
                range: node.range.clone(),
            })
        })
        .collect();
    let outline = EXTRACTORS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let extractor = cache
            .entry(language)
            .or_insert_with(|| compile_rules(language));
        let extractor = extractor.as_ref().map_err(Clone::clone)?;
        collect_outline(extractor, root.root(), &syntax)
    })?;
    Ok(ParsedSource {
        language,
        source: source.to_owned(),
        provider: "ast-grep-language/ast-grep-outline 0.45.1",
        syntax,
        outline,
        diagnostics,
    })
}

/// 只映射 Cargo 中已显式启用的 grammar，避免误调用未打包的语言。
fn provider_language(language: SourceLanguage) -> SupportLang {
    match language {
        SourceLanguage::TypeScript => SupportLang::TypeScript,
        SourceLanguage::Tsx => SupportLang::Tsx,
        SourceLanguage::JavaScript => SupportLang::JavaScript,
        SourceLanguage::Rust => SupportLang::Rust,
    }
}

/// 过滤后才编译规则，其他语言的 grammar 无须安装或下载。
fn compile_rules(
    language: SourceLanguage,
) -> Result<CombinedExtractors<SupportLang>, ParseFailure> {
    let rules = parse_outline_rules::<SupportLang>(DEFAULT_OUTLINE_RULES)
        .map_err(|error| ParseFailure::OutlineRules(error.to_string()))?
        .into_iter()
        .filter(|rule| rule.common().language == provider_language(language))
        .collect();
    CombinedExtractors::try_from(rules, &Default::default())
        .map_err(|error| ParseFailure::OutlineRules(error.to_string()))
}

/// 使用显式游标遍历所有节点，保留匿名 token、字段关系和错误祖先信息。
fn collect_syntax(root: Node<'_, StrDoc<SupportLang>>) -> Vec<SyntaxNode> {
    let mut cursor = root.get_inner_node().walk();
    let mut nodes: Vec<SyntaxNode> = Vec::new();
    let mut parents = Vec::new();
    loop {
        let node = cursor.node();
        let id = nodes.len();
        let parent = parents.last().copied();
        nodes.push(SyntaxNode {
            native_kind: node.kind().to_owned(),
            range: SourceRange {
                bytes: node.byte_range(),
                start: SourcePosition {
                    line: node.start_position().row,
                    byte_column: node.start_position().column,
                },
                end: SourcePosition {
                    line: node.end_position().row,
                    byte_column: node.end_position().column,
                },
            },
            parent,
            children: Vec::new(),
            field: cursor.field_name().map(str::to_owned),
            named: node.is_named(),
            extra: node.is_extra(),
            is_error: node.is_error(),
            is_missing: node.is_missing(),
            contains_error: node.has_error(),
        });
        if let Some(parent) = parent {
            nodes[parent].children.push(id);
        }
        if cursor.goto_first_child() {
            parents.push(id);
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return nodes;
            }
            parents.pop();
        }
    }
}

/// 保留 item/member 及标记，并以原生种类和精确区间关联到同一次语法树。
fn collect_outline(
    extractor: &CombinedExtractors<SupportLang>,
    root: Node<'_, StrDoc<SupportLang>>,
    syntax: &[SyntaxNode],
) -> Result<Vec<OutlineEvidence>, ParseFailure> {
    let index: HashMap<_, _> = syntax
        .iter()
        .enumerate()
        .map(|(id, node)| {
            (
                (
                    node.range.bytes.start,
                    node.range.bytes.end,
                    node.native_kind.as_str(),
                ),
                id,
            )
        })
        .collect();
    let mut outline = Vec::new();
    for item in extractor.extract(root) {
        let parent = outline.len();
        let mut evidence = own_entry(item.entry, syntax, &index, None)?;
        evidence.is_import = Some(item.is_import);
        evidence.is_exported = Some(item.is_exported);
        outline.push(evidence);
        for member in item.members {
            let mut evidence = own_entry(member.entry, syntax, &index, Some(parent))?;
            evidence.is_public = Some(member.is_public);
            outline.push(evidence);
        }
    }
    Ok(outline)
}

/// 复制 provider 的可用字段；位置统一为可在原文切片的 UTF-8 字节范围。
fn own_entry(
    entry: OutlineEntry<'_>,
    syntax: &[SyntaxNode],
    index: &HashMap<(usize, usize, &str), usize>,
    parent: Option<usize>,
) -> Result<OutlineEvidence, ParseFailure> {
    let key = (
        entry.range.byte_offset.start,
        entry.range.byte_offset.end,
        entry.ast_kind.as_ref(),
    );
    let syntax_node = index.get(&key).copied().ok_or_else(|| {
        ParseFailure::InconsistentEvidence(format!(
            "{} {:?}",
            entry.ast_kind, entry.range.byte_offset
        ))
    })?;
    Ok(OutlineEvidence {
        name: entry.name.into_owned(),
        native_kind: entry.ast_kind.into_owned(),
        provider_category: format!("{:?}", entry.symbol_type),
        signature: entry.signature.into_owned(),
        range: syntax[syntax_node].range.clone(),
        syntax_node,
        parent,
        role: match entry.role {
            EntryRole::Item => OutlineRole::Item,
            EntryRole::Member => OutlineRole::Member,
        },
        is_import: None,
        is_exported: None,
        is_public: None,
    })
}
