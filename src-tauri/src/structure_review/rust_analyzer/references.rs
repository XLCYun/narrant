//! 使用真实定义位置连接对象；名字相同不构成已解析目标的证据。

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use ra_ap_syntax::{ast, AstNode, SyntaxKind, SyntaxNode, TextSize};
use url::Url;

use super::{
    failure,
    lsp::RustLsp,
    syntax::{self, child, path_name, type_name, Document},
    Result,
};
use crate::lsp::{file_uri, Location, Position};
use crate::structure_review::code_graph::{
    CodeCategory, CodeGraph, CodeNode, CodeReference, CodeRoot, NodeId, ReferenceTarget,
};

/// 单次构图的临时解析材料；图外文件只用来确认身份，不扩大输出范围。
struct Resolver<'a> {
    client: &'a mut RustLsp,
    repository: &'a Path,
    documents: &'a [Document],
    external: BTreeMap<String, Document>,
    selected: BTreeSet<NodeId>,
    categories: BTreeMap<NodeId, CodeCategory>,
    definitions: BTreeMap<(String, u32), Vec<Location>>,
}

/// 组装词法包含和非包含关联，最后通过公共构造函数校验全部图约束。
pub(super) fn analyze(
    client: &mut RustLsp,
    repository: &Path,
    documents: &[Document],
) -> Result<CodeGraph> {
    let mut nodes: BTreeMap<NodeId, CodeNode> = documents
        .iter()
        .flat_map(|document| &document.declarations)
        .map(|declaration| (declaration.node.data().id.clone(), declaration.node.clone()))
        .collect();
    let mut resolver = Resolver {
        client,
        repository,
        documents,
        external: BTreeMap::new(),
        selected: nodes.keys().cloned().collect(),
        categories: nodes
            .iter()
            .map(|(id, node)| (id.clone(), node.category()))
            .collect(),
        definitions: BTreeMap::new(),
    };
    let mut root = CodeRoot::default();
    for document in documents {
        let parents = resolver.module_parents(&document.uri)?;
        let local_parents: Vec<_> = parents
            .iter()
            .filter_map(|target| match target {
                ReferenceTarget::Node(id) => Some(id.clone()),
                _ => None,
            })
            .collect();
        for declaration in document
            .declarations
            .iter()
            .filter(|declaration| declaration.parent.is_none())
        {
            let id = declaration.node.data().id.clone();
            if parents.len() == 1 && local_parents.len() == 1 && local_parents[0] != id {
                syntax::data_mut(
                    nodes
                        .get_mut(&local_parents[0])
                        .ok_or_else(|| failure("模块父节点不存在"))?,
                )
                .contains
                .push(id.clone());
            } else {
                root.contains.push(id.clone());
            }
            // 同一源码被多个模块复用时保留全部归属；不任取一个父节点或复制源码对象。
            if parents.len() != 1 || local_parents.is_empty() {
                for target in &parents {
                    add(&mut nodes, &id, "code.member_of", target.clone());
                }
            }
        }
        for declaration in &document.declarations {
            if let Some(implementation) = ast::Impl::cast(declaration.syntax.clone()) {
                if let Some(name) = implementation
                    .self_ty()
                    .and_then(|ty| type_name(ty.syntax()))
                {
                    for target in resolver.definition(&document.uri, &name, "code.applies_to")? {
                        add(
                            &mut nodes,
                            &declaration.node.data().id,
                            "code.applies_to",
                            target.clone(),
                        );
                        for member in document.declarations.iter().filter(|member| {
                            member.parent.as_ref() == Some(&declaration.node.data().id)
                        }) {
                            add(
                                &mut nodes,
                                &member.node.data().id,
                                "code.member_of",
                                target.clone(),
                            );
                        }
                    }
                }
                if let Some(name) = implementation
                    .trait_()
                    .and_then(|ty| type_name(ty.syntax()))
                {
                    for target in resolver.definition(&document.uri, &name, "code.implements")? {
                        add(
                            &mut nodes,
                            &declaration.node.data().id,
                            "code.implements",
                            target,
                        );
                    }
                }
            }
        }
        for node in document.tree.descendants() {
            // 宏 token tree 没有常规表达式结构；调用层级在下方补充可解析的展开内调用。
            if node
                .ancestors()
                .skip(1)
                .any(|parent| matches!(parent.kind(), SyntaxKind::ATTR | SyntaxKind::TOKEN_TREE))
            {
                continue;
            }
            if node.kind() == SyntaxKind::CALL_EXPR {
                if let Some(mut callee) = node
                    .children()
                    .find(|child| ast::Expr::can_cast(child.kind()))
                {
                    if callee.kind() != SyntaxKind::PATH_EXPR {
                        while callee.kind() == SyntaxKind::PAREN_EXPR {
                            let Some(inner) = callee
                                .children()
                                .find(|child| ast::Expr::can_cast(child.kind()))
                            else {
                                break;
                            };
                            callee = inner;
                        }
                        if let Some(owner) = document.owner(node.text_range().start()) {
                            let target = document
                                .declarations
                                .iter()
                                .find(|declaration| declaration.syntax == callee)
                                .map(|declaration| {
                                    resolver.reference(declaration.node.data().id.clone())
                                })
                                .unwrap_or_else(|| unknown("<dynamic callable>"));
                            add(&mut nodes, &owner.node.data().id, "code.calls", target);
                        }
                    }
                }
            }
            for (semantic, name) in reference_sites(&node) {
                let Some(owner) = document.owner(name.text_range().start()) else {
                    continue;
                };
                for target in resolver.definition(&document.uri, &name, semantic)? {
                    let semantic = resolver.relation_semantic(semantic, &target);
                    add(&mut nodes, &owner.node.data().id, semantic, target);
                }
            }
        }
        // 标准调用层级能补充宏内调用；fromRanges 决定最内层 owner，不能一律归到外层函数。
        for declaration in document
            .declarations
            .iter()
            .filter(|declaration| declaration.syntax.kind() == SyntaxKind::FN)
        {
            let position = syntax::position(&document.source, declaration.selection.start());
            for call in resolver.client.outgoing_calls(&document.uri, position)? {
                let targets = resolver.target(
                    &call.to.uri,
                    &call.to.selection_range.start,
                    &call.to.name,
                    "code.calls",
                )?;
                for range in &call.from_ranges {
                    let offset = syntax::byte_offset(&document.source, &range.start)?;
                    let Some(owner) = document.owner(offset) else {
                        continue;
                    };
                    for target in &targets {
                        add(
                            &mut nodes,
                            &owner.node.data().id,
                            "code.calls",
                            target.clone(),
                        );
                    }
                }
            }
        }
    }
    for node in nodes.values_mut() {
        let data = syntax::data_mut(node);
        data.contains.sort();
        data.contains.dedup();
        data.references.sort();
        data.references.dedup();
    }
    root.contains.sort();
    root.contains.dedup();
    CodeGraph::new(root, nodes.into_values().collect()).map_err(failure)
}

impl Resolver<'_> {
    /// 优先读取选中快照，图外文档只载入一次以获得相同的身份生成约定。
    fn document(&self, uri: &str) -> Option<&Document> {
        self.documents
            .iter()
            .find(|document| document.uri == uri)
            .or_else(|| self.external.get(uri))
    }

    /// 无法稳定命名的虚拟/外部源码返回 false，由目标解析显式保留 Unresolved。
    fn load(&mut self, uri: &str) -> Result<bool> {
        if self.document(uri).is_some() {
            return Ok(true);
        }
        let Some(path) = Url::parse(uri).ok().and_then(|uri| uri.to_file_path().ok()) else {
            return Ok(false);
        };
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs")
            || syntax::file_scope(self.repository, &path).is_none()
        {
            return Ok(false);
        }
        let source = fs::read_to_string(&path).map_err(failure)?;
        let document = Document::parse(self.repository, path, source)?;
        self.categories
            .extend(document.declarations.iter().map(|declaration| {
                (
                    declaration.node.data().id.clone(),
                    declaration.node.category(),
                )
            }));
        self.external.insert(uri.to_owned(), document);
        Ok(true)
    }

    /// 由是否在选中文件范围内决定 Node 或 External，二者使用同一声明身份。
    fn reference(&self, id: NodeId) -> ReferenceTarget {
        if self.selected.contains(&id) {
            ReferenceTarget::Node(id)
        } else {
            ReferenceTarget::External { symbol: id.0 }
        }
    }

    /// 把已解析的函数值或类型使用与数据读取区分开，保留非调用的高层关联。
    fn relation_semantic(&self, semantic: &'static str, target: &ReferenceTarget) -> &'static str {
        let category = match target {
            ReferenceTarget::Node(id) => self.categories.get(id),
            ReferenceTarget::External { symbol } => self.categories.get(&NodeId(symbol.clone())),
            ReferenceTarget::Unresolved { .. } => None,
        };
        match (semantic, category) {
            ("code.reads", Some(CodeCategory::Value)) => "code.reads",
            ("code.reads", Some(CodeCategory::Type)) => "code.type_use",
            ("code.reads", _) => "code.references",
            ("code.type_use", Some(CodeCategory::Value)) => "code.references",
            _ => semantic,
        }
    }

    /// 父模块信息来自 rust-analyzer，覆盖 #[path]，不根据文件名伪造模块层次。
    fn module_parents(&mut self, uri: &str) -> Result<Vec<ReferenceTarget>> {
        let mut targets = Vec::new();
        for location in self.client.parent_modules(uri)? {
            if location.uri == uri || !self.load(&location.uri)? {
                continue;
            }
            let document = self
                .document(&location.uri)
                .ok_or_else(|| failure("父模块文档不可用"))?;
            let offset = syntax::byte_offset(&document.source, &location.position)?;
            if let Some(parent) = document
                .declarations
                .iter()
                .filter(|declaration| {
                    declaration.syntax.kind() == SyntaxKind::MODULE
                        && declaration.syntax.text_range().contains(offset)
                })
                .min_by_key(|declaration| declaration.syntax.text_range().len())
            {
                targets.push(self.reference(parent.node.data().id.clone()));
            }
        }
        targets.sort();
        targets.dedup();
        Ok(targets)
    }

    /// 在引用发生处查询定义；成功的空响应保留未知目标，失败响应向外传播。
    fn definition(
        &mut self,
        uri: &str,
        name: &SyntaxNode,
        semantic: &str,
    ) -> Result<Vec<ReferenceTarget>> {
        let text = name.text().to_string();
        if semantic == "code.type_use"
            && [
                "bool", "str", "char", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16",
                "i32", "i64", "i128", "isize", "f32", "f64",
            ]
            .contains(&text.as_str())
        {
            return Ok(Vec::new());
        }
        if text == "Self" {
            if let Some(enclosing) = name.ancestors().find(|node| {
                matches!(
                    node.kind(),
                    SyntaxKind::IMPL
                        | SyntaxKind::TRAIT
                        | SyntaxKind::STRUCT
                        | SyntaxKind::ENUM
                        | SyntaxKind::UNION
                )
            }) {
                if let Some(implementation) = ast::Impl::cast(enclosing.clone()) {
                    if let Some(concrete) = implementation
                        .self_ty()
                        .and_then(|ty| type_name(ty.syntax()))
                        .filter(|concrete| concrete.text() != "Self")
                    {
                        return self.definition(uri, &concrete, semantic);
                    }
                } else if let Some(declaration) = self.document(uri).and_then(|document| {
                    document
                        .declarations
                        .iter()
                        .find(|declaration| declaration.syntax == enclosing)
                }) {
                    return Ok(vec![if semantic == "code.calls" {
                        unknown(&text)
                    } else {
                        self.reference(declaration.node.data().id.clone())
                    }]);
                }
            }
        }
        let key = (uri.to_owned(), u32::from(name.text_range().start()));
        if !self.definitions.contains_key(&key) {
            let document = self
                .document(uri)
                .ok_or_else(|| failure("引用来源文档不存在"))?;
            let position = syntax::position(&document.source, name.text_range().start());
            let locations = self.client.definitions(uri, position)?;
            self.definitions.insert(key.clone(), locations);
        }
        let locations = self.definitions[&key].clone();
        if locations.is_empty() {
            return Ok(vec![unknown(&text)]);
        }
        let mut targets = Vec::new();
        for location in locations {
            targets.extend(self.target(&location.uri, &location.position, &text, semantic)?);
        }
        targets.sort();
        targets.dedup();
        Ok(targets)
    }

    /// 将定义位置映射成高层目标，不把局部变量或派生标注误当作生成方法。
    fn target(
        &mut self,
        uri: &str,
        position: &Position,
        observed: &str,
        semantic: &str,
    ) -> Result<Vec<ReferenceTarget>> {
        let uri = Url::parse(uri)
            .ok()
            .and_then(|uri| uri.to_file_path().ok())
            .and_then(|path| path.canonicalize().ok())
            .and_then(|path| file_uri(&path).ok())
            .unwrap_or_else(|| uri.to_owned());
        if !self.load(&uri)? {
            return Ok(vec![unknown(observed)]);
        }
        let document = self
            .document(&uri)
            .ok_or_else(|| failure("定义目标文档不存在"))?;
        let offset = syntax::byte_offset(&document.source, position)?;
        if let Some(declaration) = document.declared_at(offset) {
            let category = declaration.node.category();
            if semantic == "code.calls" && category != CodeCategory::Callable {
                return Ok(vec![unknown(observed)]);
            }
            if semantic == "code.writes" && category != CodeCategory::Value {
                return Ok(Vec::new());
            }
            return Ok(vec![self.reference(declaration.node.data().id.clone())]);
        }
        // 对局部闭包绑定的调用，定义指向绑定名；由其实际初始化表达式定位闭包对象。
        let token = document.tree.token_at_offset(offset).right_biased();
        if let Some(token) = token {
            let ancestors: Vec<_> = token.parent_ancestors().collect();
            if semantic == "code.calls" {
                if let Some(statement) = ancestors
                    .iter()
                    .find(|node| node.kind() == SyntaxKind::LET_STMT)
                {
                    if let Some(closure) =
                        child(statement, SyntaxKind::CLOSURE_EXPR).and_then(|closure| {
                            document
                                .declarations
                                .iter()
                                .find(|declaration| declaration.syntax == closure)
                        })
                    {
                        return Ok(vec![self.reference(closure.node.data().id.clone())]);
                    }
                }
            }
            if semantic != "code.calls"
                && ancestors
                    .iter()
                    .take_while(|node| {
                        !matches!(
                            node.kind(),
                            SyntaxKind::FN
                                | SyntaxKind::STRUCT
                                | SyntaxKind::IMPL
                                | SyntaxKind::TRAIT
                        )
                    })
                    .any(|node| {
                        matches!(
                            node.kind(),
                            SyntaxKind::TYPE_PARAM
                                | SyntaxKind::CONST_PARAM
                                | SyntaxKind::LIFETIME_PARAM
                                | SyntaxKind::IDENT_PAT
                                | SyntaxKind::SELF_PARAM
                        )
                    })
            {
                return Ok(Vec::new());
            }
        }
        if offset == TextSize::from(0) {
            let path = document.path.clone();
            let modules = self.module_parents(&uri)?;
            if !modules.is_empty() {
                return Ok(modules);
            }
            if let Some(scope) = syntax::file_scope(self.repository, &path) {
                return Ok(vec![ReferenceTarget::External {
                    symbol: format!("{scope}::crate"),
                }]);
            }
        }
        Ok(vec![unknown(observed)])
    }
}

/// 不推断身份的引用目标，保留观察到的名称以便两侧作确定性比较。
fn unknown(name: &str) -> ReferenceTarget {
    ReferenceTarget::Unresolved {
        name: name.to_owned(),
    }
}

/// 写入源节点的一个高层关联，调用方稍后统一排序去重。
fn add(
    nodes: &mut BTreeMap<NodeId, CodeNode>,
    owner: &NodeId,
    semantic: &str,
    target: ReferenceTarget,
) {
    if let Some(node) = nodes.get_mut(owner) {
        syntax::data_mut(node).references.push(CodeReference {
            semantic: semantic.into(),
            target,
        });
    }
}

/// 提取有明确源码位置的类型、调用、导入、数据访问和宏使用。
fn reference_sites(node: &SyntaxNode) -> Vec<(&'static str, SyntaxNode)> {
    use SyntaxKind::*;
    let site = match node.kind() {
        PATH_TYPE => type_name(node).map(|name| ("code.type_use", name)),
        RECORD_EXPR | RECORD_PAT => child(node, PATH)
            .and_then(|path| path_name(&path))
            .map(|name| ("code.type_use", name)),
        USE_TREE if child(node, USE_TREE_LIST).is_none() => child(node, PATH)
            .and_then(|path| path_name(&path))
            .map(|name| ("code.imports", name)),
        METHOD_CALL_EXPR => child(node, NAME_REF).map(|name| ("code.calls", name)),
        CALL_EXPR => node
            .children()
            .find(|child| ast::Expr::can_cast(child.kind()))
            .filter(|expression| expression.kind() == PATH_EXPR)
            .and_then(|expression| child(&expression, PATH))
            .and_then(|path| path_name(&path))
            .map(|name| ("code.calls", name)),
        MACRO_CALL => child(node, PATH)
            .and_then(|path| path_name(&path))
            .map(|name| ("rust.macro_use", name)),
        PATH_EXPR
            if node
                .parent()
                .is_none_or(|parent| parent.kind() != CALL_EXPR) =>
        {
            child(node, PATH)
                .and_then(|path| path_name(&path))
                .map(|name| ("code.reads", name))
        }
        FIELD_EXPR => child(node, NAME_REF).map(|name| ("code.reads", name)),
        _ => None,
    };
    let Some((semantic, name)) = site else {
        return Vec::new();
    };
    if semantic == "code.reads" {
        if let Some(parent) = node.parent().filter(|parent| parent.kind() == BIN_EXPR) {
            let is_left = parent
                .children()
                .find(|child| ast::Expr::can_cast(child.kind()))
                .as_ref()
                == Some(node);
            if is_left {
                let operator = parent
                    .children_with_tokens()
                    .filter_map(|element| element.into_token())
                    .find(|token| {
                        matches!(
                            token.text(),
                            "=" | "+="
                                | "-="
                                | "*="
                                | "/="
                                | "%="
                                | "&="
                                | "|="
                                | "^="
                                | "<<="
                                | ">>="
                        )
                    });
                if let Some(operator) = operator {
                    let mut result = vec![("code.writes", name.clone())];
                    if operator.text() != "=" {
                        result.push(("code.reads", name));
                    }
                    return result;
                }
            }
        }
    }
    vec![(semantic, name)]
}
