//! 保留完整源码语法，以固定的归一化规则提取高层声明和自身实现内容。

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use ::syntax::{AstNode, NodeOrToken, SyntaxKind, SyntaxNode, TextRange, TextSize, WalkEvent, ast};

use super::{Result, failure};
use crate::RustProject;

use super::model::{AnalysisOptions, CodeCategory, CodeNode, CodeNodeData, LanguageId, NodeId};

/// 临时源码字节位置只用于原生关联，不进入图和节点身份。
#[derive(Clone)]
pub(super) struct Declaration {
    pub node: CodeNode,
    pub parent: Option<NodeId>,
    pub selection: TextRange,
    pub syntax: SyntaxNode,
}

/// 一个固定内容的文件及其显式声明；crate 根由后续原生语义确认，普通文件不自动建模块。
pub(super) struct Document {
    pub path: PathBuf,
    pub key: String,
    pub source: String,
    pub tree: SyntaxNode,
    pub declarations: Vec<Declaration>,
    /// 已确认的测试范围不生成对象，也不归入外层对象的代码或引用。
    excluded: Vec<TextRange>,
}

impl Document {
    /// AST 与原文均从已加载的同一个数据库快照提取，不创建第二个语法解析器。
    pub fn parse(
        repository: &Path,
        path: PathBuf,
        project: &RustProject,
        options: AnalysisOptions,
    ) -> Result<Self> {
        let file = project.file_id(&path)?;
        let source = project.source(file)?;
        let tree = project.parse(file)?.syntax().clone();
        let scope = file_scope(repository, &path)
            .ok_or_else(|| failure("无法构造不依赖绝对路径的 Rust 文件身份"))?;
        let excluded = if options.include_tests {
            Vec::new()
        } else {
            super::test_code::excluded_ranges(&tree)
        };
        let mut declarations = extract(&tree, &scope, &excluded);
        for declaration in &mut declarations {
            data_mut(&mut declaration.node).source =
                Some(source_excerpt(&source, declaration.syntax.text_range()));
        }
        Ok(Self {
            key: path
                .to_str()
                .ok_or_else(|| failure("源码路径不是 UTF-8"))?
                .to_owned(),
            path,
            source,
            tree,
            declarations,
            excluded,
        })
    }

    /// 范围按源码顺序且互不重叠，查询不随测试声明数量线性增长。
    pub fn excludes(&self, offset: TextSize) -> bool {
        let index = self
            .excluded
            .partition_point(|range| range.start() <= offset);
        index > 0 && self.excluded[index - 1].contains(offset)
    }

    /// 父节点可能从测试声明相同位置开始，必须确认整个范围被排除才能跳过子树。
    pub fn excludes_range(&self, range: TextRange) -> bool {
        let index = self
            .excluded
            .partition_point(|excluded| excluded.start() <= range.start());
        index > 0 && self.excluded[index - 1].contains_range(range)
    }

    /// 查找最内层已建模声明；闭包是普通表达式，内部关联自然归属外层对象。
    pub fn owner(&self, offset: TextSize) -> Option<&Declaration> {
        if self.excludes(offset) {
            return None;
        }
        self.declarations
            .iter()
            .filter(|declaration| declaration.syntax.text_range().contains(offset))
            .min_by_key(|declaration| declaration.syntax.text_range().len())
    }

    /// 将定义响应精确匹配到声明名，不把指向宏标注等位置误当作生成方法。
    pub fn declared_at(&self, offset: TextSize) -> Option<&Declaration> {
        self.declarations.iter().find(|declaration| {
            declaration.selection.contains(offset)
                || declaration.selection.is_empty() && declaration.selection.start() == offset
        })
    }
}

/// 按语法范围截取原文；声明独占行时保留首行缩进，不混入同行的其他声明。
fn source_excerpt(source: &str, range: TextRange) -> String {
    let start = usize::from(range.start());
    let line_start = source[..start].rfind('\n').map_or(0, |offset| offset + 1);
    let start = if source[line_start..start]
        .chars()
        .all(|character| matches!(character, ' ' | '\t'))
    {
        line_start
    } else {
        start
    };
    source[start..usize::from(range.end())].to_owned()
}

/// 跳过测试范围后提取声明，并生成身份、包含关系和父对象自身内容。
fn extract(tree: &SyntaxNode, scope: &str, excluded: &[TextRange]) -> Vec<Declaration> {
    let mut declarations = Vec::new();
    collect(tree, scope, None, excluded, &mut declarations);
    let children: BTreeMap<_, Vec<_>> = declarations
        .iter()
        .map(|declaration| {
            let children = declarations
                .iter()
                .filter(|child| child.parent.as_ref() == Some(&declaration.node.data().id))
                .map(|child| (child.syntax.text_range(), child.node.data().id.clone()))
                .collect();
            (declaration.node.data().id.clone(), children)
        })
        .collect();
    for declaration in &mut declarations {
        let own_children = &children[&declaration.node.data().id];
        let mut ignored: Vec<_> = own_children
            .iter()
            .map(|(range, _)| *range)
            .chain(
                excluded
                    .iter()
                    .copied()
                    .filter(|range| declaration.syntax.text_range().contains_range(*range)),
            )
            .collect();
        ignored.sort_unstable_by_key(range_key);
        ignored.dedup();
        let body = implementation(&declaration.syntax);
        let data = data_mut(&mut declaration.node);
        data.signature = Some(signature(&declaration.syntax, &ignored));
        data.code = body.as_ref().map(|body| normalize(body, &ignored));
        data.contains = own_children.iter().map(|(_, id)| id.clone()).collect();
    }
    declarations
}

/// 仅为仓库内源码构造稳定命名空间，不再提取标准库或第三方源码的声明身份。
pub(super) fn file_scope(repository: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(repository).ok()?;
    Some(format!("rust:repo:{}", component(relative.to_str()?)))
}

/// 对身份分段转义，防止 Rust 名称、路径和类型语法与层级分隔符碰撞。
fn component(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

/// 语法树中的六类映射；顶层宏调用和 use 声明保留为 Rust 扩展对象。
fn category(syntax: &SyntaxNode) -> Option<CodeCategory> {
    use SyntaxKind::*;
    Some(match syntax.kind() {
        MODULE => CodeCategory::Container,
        STRUCT | ENUM | TRAIT | TYPE_ALIAS | UNION => CodeCategory::Type,
        FN => CodeCategory::Callable,
        RECORD_FIELD | TUPLE_FIELD | VARIANT | CONST | STATIC => CodeCategory::Value,
        IMPL | MACRO_RULES | MACRO_DEF | USE | EXTERN_CRATE | EXTERN_BLOCK => {
            CodeCategory::Extension
        }
        MACRO_CALL
            if syntax.parent().is_some_and(|parent| {
                matches!(
                    parent.kind(),
                    SOURCE_FILE | ITEM_LIST | ASSOC_ITEM_LIST | EXTERN_ITEM_LIST
                )
            }) =>
        {
            CodeCategory::Extension
        }
        _ => return None,
    })
}

/// 收集分析范围内的显式声明；未知 cfg 分支保留，明确排除的测试子树不提取。
fn collect(
    syntax: &SyntaxNode,
    scope: &str,
    parent: Option<NodeId>,
    excluded: &[TextRange],
    output: &mut Vec<Declaration>,
) {
    if excluded
        .binary_search_by_key(&range_key(&syntax.text_range()), range_key)
        .is_ok()
    {
        return;
    }
    let mut next_parent = parent.clone();
    if let Some(category) = category(syntax) {
        let name_node = child(syntax, SyntaxKind::NAME);
        let name = name_node.as_ref().map(|name| name.text().to_string());
        let native_kind = match syntax.kind() {
            SyntaxKind::MACRO_CALL => "macro_invocation".to_owned(),
            kind => format!("{kind:?}").to_lowercase(),
        };
        let signature = signature(syntax, excluded);
        let key = name
            .clone()
            .unwrap_or_else(|| identity_key(syntax, &signature, excluded));
        let owner = parent.as_ref().map_or(scope, |id| id.0.as_str());
        let base = format!(
            "{owner}::{native_kind}:{}",
            component(key.strip_prefix("r#").unwrap_or(&key))
        );
        let count = output
            .iter()
            .filter(|declaration| {
                declaration.node.data().id.0 == base
                    || declaration
                        .node
                        .data()
                        .id
                        .0
                        .strip_prefix(&base)
                        .is_some_and(|suffix| suffix.starts_with('#') && !suffix.contains("::"))
            })
            .count();
        let id = NodeId(if count == 0 {
            base
        } else {
            format!("{base}#{}", count + 1)
        });
        let data = CodeNodeData {
            id: id.clone(),
            language: LanguageId("rust".into()),
            native_kind,
            name,
            signature: Some(signature),
            code: None,
            source: None,
            contains: Vec::new(),
            references: Vec::new(),
            extensions: BTreeMap::new(),
        };
        let node = match category {
            CodeCategory::Container => CodeNode::Container(data),
            CodeCategory::Type => CodeNode::Type(data),
            CodeCategory::Callable => CodeNode::Callable(data),
            CodeCategory::Value => CodeNode::Value(data),
            CodeCategory::Extension => CodeNode::Extension(data),
            CodeCategory::ExecutionRegion => unreachable!(),
        };
        output.push(Declaration {
            node,
            parent,
            selection: name_node.map_or_else(
                || TextRange::empty(syntax.text_range().start()),
                |name| name.text_range(),
            ),
            syntax: syntax.clone(),
        });
        next_parent = Some(id);
    }
    for child in syntax.children() {
        collect(&child, scope, next_parent.clone(), excluded, output);
    }
}

/// 匿名对象使用词法归属和结构锚点，不把行列或实现代码当成身份。
fn identity_key(syntax: &SyntaxNode, signature: &str, excluded: &[TextRange]) -> String {
    if let Some(implementation) = ast::Impl::cast(syntax.clone()) {
        return [implementation.trait_(), implementation.self_ty()]
            .into_iter()
            .flatten()
            .map(|ty| normalize(ty.syntax(), excluded))
            .collect::<Vec<_>>()
            .join(" for ");
    }
    if syntax.kind() == SyntaxKind::MACRO_CALL {
        return child(syntax, SyntaxKind::PATH)
            .map_or_else(|| "macro".into(), |path| normalize(&path, excluded));
    }
    if matches!(
        syntax.kind(),
        SyntaxKind::TUPLE_FIELD | SyntaxKind::CONST | SyntaxKind::EXTERN_BLOCK
    ) {
        return "anonymous".into();
    }
    signature.to_owned()
}

/// 提取直接属于当前对象的实现区域，普通函数无需额外 ExecutionRegion。
fn implementation(syntax: &SyntaxNode) -> Option<SyntaxNode> {
    use SyntaxKind::*;
    match syntax.kind() {
        FN => child(syntax, BLOCK_EXPR),
        CONST | STATIC => syntax
            .children()
            .find(|node| ast::Expr::can_cast(node.kind())),
        VARIANT => child(syntax, CONST_ARG),
        MACRO_RULES | MACRO_DEF | MACRO_CALL => child(syntax, TOKEN_TREE),
        _ => None,
    }
}

/// 声明签名排除实现和成员内容，但保留括号形态、修饰符及语言特有标注。
fn signature(syntax: &SyntaxNode, children: &[TextRange]) -> String {
    let body = implementation(syntax);
    let mut tokens = Vec::new();
    for element in syntax.children_with_tokens() {
        match element {
            NodeOrToken::Node(node) if body.as_ref() == Some(&node) => {}
            NodeOrToken::Node(node)
                if matches!(
                    node.kind(),
                    SyntaxKind::RECORD_FIELD_LIST
                        | SyntaxKind::TUPLE_FIELD_LIST
                        | SyntaxKind::VARIANT_LIST
                        | SyntaxKind::ASSOC_ITEM_LIST
                        | SyntaxKind::ITEM_LIST
                        | SyntaxKind::EXTERN_ITEM_LIST
                ) =>
            {
                for token in node
                    .children_with_tokens()
                    .filter_map(NodeOrToken::into_token)
                {
                    if matches!(
                        token.kind(),
                        SyntaxKind::L_CURLY
                            | SyntaxKind::R_CURLY
                            | SyntaxKind::L_PAREN
                            | SyntaxKind::R_PAREN
                    ) {
                        tokens.push(token.text().to_string());
                    }
                }
            }
            NodeOrToken::Node(node) => tokens.push(normalize(&node, children)),
            NodeOrToken::Token(token) if !token.kind().is_trivia() => {
                if body.is_some() && token.kind() == SyntaxKind::EQ {
                    continue;
                }
                tokens.push(token.text().to_string());
            }
            _ => {}
        }
    }
    tokens
        .into_iter()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 输出可读的规范化代码；闭包内容原样参与外层比较，独立的具名子声明整体排除。
fn normalize(syntax: &SyntaxNode, children: &[TextRange]) -> String {
    let mut tokens = Vec::<(String, TextRange, bool)>::new();
    normalized_tokens(syntax, children, &mut tokens);
    let mut output = String::new();
    let mut previous: Option<(TextRange, bool)> = None;
    for (text, range, punctuation) in tokens {
        let joint = previous.is_some_and(|(left, was_punctuation)| {
            was_punctuation && punctuation && left.end() == range.start()
        });
        if !output.is_empty() && !joint {
            output.push(' ');
        }
        output.push_str(&text);
        previous = Some((range, punctuation));
    }
    output
}

/// 只删除真正的空白/注释 token；字面量和宏中有意义的标点粘连保持原样。
fn normalized_tokens(
    syntax: &SyntaxNode,
    children: &[TextRange],
    output: &mut Vec<(String, TextRange, bool)>,
) {
    if children
        .binary_search_by_key(&range_key(&syntax.text_range()), range_key)
        .is_ok()
    {
        return;
    }
    for element in syntax.children_with_tokens() {
        match element {
            NodeOrToken::Node(node) => normalized_tokens(&node, children, output),
            NodeOrToken::Token(token) if !token.kind().is_trivia() => {
                let in_macro = token
                    .parent_ancestors()
                    .any(|parent| parent.kind() == SyntaxKind::TOKEN_TREE);
                output.push((
                    token.text().to_string(),
                    token.text_range(),
                    in_macro
                        && token.kind().is_punct()
                        && !matches!(
                            token.kind(),
                            SyntaxKind::L_CURLY
                                | SyntaxKind::R_CURLY
                                | SyntaxKind::L_PAREN
                                | SyntaxKind::R_PAREN
                                | SyntaxKind::L_BRACK
                                | SyntaxKind::R_BRACK
                        ),
                ));
            }
            _ => {}
        }
    }
}

/// TextRange 没有全序，明确按起止偏移排序以便二分查询排除范围。
fn range_key(range: &TextRange) -> (TextSize, TextSize) {
    (range.start(), range.end())
}

/// 获取直接语法子节点，避免混入更深层对象的名称或类型。
pub(super) fn child(syntax: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    syntax.children().find(|node| node.kind() == kind)
}

/// PATH 的最后一段才是本次类型/函数使用的直接目标。
pub(super) fn path_name(path: &SyntaxNode) -> Option<SyntaxNode> {
    let segment = path
        .children()
        .filter(|node| node.kind() == SyntaxKind::PATH_SEGMENT)
        .last()?;
    child(&segment, SyntaxKind::NAME_REF)
}

/// 取得实现目标或 trait 类型表达式的主体名字，泛型实参不单独生成类型使用关系。
pub(super) fn type_name(node: &SyntaxNode) -> Option<SyntaxNode> {
    if node.kind() == SyntaxKind::PATH_TYPE {
        return child(node, SyntaxKind::PATH).and_then(|path| path_name(&path));
    }
    node.children().find_map(|child| type_name(&child))
}

/// 只收集字段声明类型中的路径，包括泛型实参；常量表达式、宏内容和其他声明不参与。
pub(super) fn field_type_names(node: &SyntaxNode) -> Vec<SyntaxNode> {
    let ty = match node.kind() {
        SyntaxKind::RECORD_FIELD => {
            ast::RecordField::cast(node.clone()).and_then(|field| field.ty())
        }
        SyntaxKind::TUPLE_FIELD => ast::TupleField::cast(node.clone()).and_then(|field| field.ty()),
        _ => None,
    };
    let Some(ty) = ty else { return Vec::new() };
    let mut names = Vec::new();
    let mut walk = ty.syntax().preorder();
    while let Some(event) = walk.next() {
        let WalkEvent::Enter(node) = event else {
            continue;
        };
        if ast::Expr::can_cast(node.kind())
            || ast::Item::can_cast(node.kind())
            || node.kind() == SyntaxKind::MACRO_TYPE
        {
            walk.skip_subtree();
        } else if node.kind() == SyntaxKind::PATH_TYPE
            && let Some(name) = child(&node, SyntaxKind::PATH).and_then(|path| path_name(&path))
        {
            names.push(name);
        }
    }
    names
}

/// 组装期修改公共数据，完成后图仍只提供只读查询。
pub(super) fn data_mut(node: &mut CodeNode) -> &mut CodeNodeData {
    match node {
        CodeNode::Container(data)
        | CodeNode::Type(data)
        | CodeNode::Callable(data)
        | CodeNode::Value(data)
        | CodeNode::ExecutionRegion(data)
        | CodeNode::Extension(data) => data,
    }
}
