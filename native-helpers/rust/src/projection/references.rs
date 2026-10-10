//! 使用真实定义位置连接对象；名字相同不构成已解析目标的证据。

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use ::syntax::{AstNode, SyntaxKind, SyntaxNode, TextSize, WalkEvent, ast};

use super::Navigation;
use super::model::{
    AnalysisOptions, CodeCategory, CodeGraph, CodeNode, CodeNodeData, CodeReference, CodeRoot,
    LanguageId, NodeId, ReferenceTarget,
};
use super::{
    Result, failure,
    syntax::{self, Document, child, path_name, type_name},
};
use crate::{ModuleParent, RustProject};

/// 单次构图的临时解析材料；图外文件只用来确认身份，不扩大输出范围。
struct Resolver<'a> {
    project: &'a RustProject,
    repository: &'a Path,
    documents: &'a [Document],
    external: BTreeMap<String, Document>,
    selected: BTreeSet<NodeId>,
    categories: BTreeMap<NodeId, CodeCategory>,
    definitions: BTreeMap<(String, u32), Vec<Navigation>>,
    crate_roots: BTreeMap<NodeId, CodeNode>,
    options: AnalysisOptions,
    test_files: BTreeMap<String, bool>,
    visiting_files: BTreeSet<String>,
}

/// 组装词法包含和非包含关联，最后通过公共构造函数校验全部图约束。
pub(super) fn analyze(
    project: &RustProject,
    repository: &Path,
    documents: &[Document],
    options: AnalysisOptions,
) -> Result<(CodeGraph, BTreeMap<String, bool>)> {
    let mut resolver = Resolver {
        project,
        repository,
        documents,
        external: BTreeMap::new(),
        selected: BTreeSet::new(),
        categories: BTreeMap::new(),
        definitions: BTreeMap::new(),
        crate_roots: BTreeMap::new(),
        options,
        test_files: BTreeMap::new(),
        visiting_files: BTreeSet::new(),
    };
    // 先确认文件的所有模块归属，再创建图节点，避免测试文件内的辅助声明逃逸到根。
    for document in documents {
        resolver.test_only_file(&document.key)?;
    }
    let mut nodes: BTreeMap<NodeId, CodeNode> = documents
        .iter()
        .filter(|document| {
            !resolver
                .test_files
                .get(&document.key)
                .copied()
                .unwrap_or(false)
        })
        .flat_map(|document| &document.declarations)
        .map(|declaration| (declaration.node.data().id.clone(), declaration.node.clone()))
        .collect();
    resolver.selected = nodes.keys().cloned().collect();
    resolver
        .categories
        .extend(nodes.iter().map(|(id, node)| (id.clone(), node.category())));
    let mut root = CodeRoot::default();
    for document in documents {
        if resolver.test_files.get(&document.key) == Some(&true) {
            continue;
        }
        let parents = resolver.module_parents(&document.key)?;
        let local_parents: Vec<_> = parents
            .iter()
            .filter_map(|target| match target {
                ReferenceTarget::Node { id } => Some(id.clone()),
                _ => None,
            })
            .collect();
        for parent in &local_parents {
            if let Some(node) = resolver.crate_roots.get(parent) {
                nodes.entry(parent.clone()).or_insert_with(|| node.clone());
            }
        }
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
            for name in syntax::field_type_names(&declaration.syntax) {
                for target in resolver.definition(&document.key, &name, "code.type_use")? {
                    if resolver.includes_reference("code.type_use", &target) {
                        add(
                            &mut nodes,
                            &declaration.node.data().id,
                            "code.type_use",
                            target,
                        );
                    }
                }
            }
            if let Some(implementation) = ast::Impl::cast(declaration.syntax.clone()) {
                if let Some(name) = implementation
                    .self_ty()
                    .and_then(|ty| type_name(ty.syntax()))
                {
                    for target in resolver.definition(&document.key, &name, "code.applies_to")? {
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
                    for target in resolver.definition(&document.key, &name, "code.implements")? {
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
        let mut walk = document.tree.preorder();
        while let Some(event) = walk.next() {
            let WalkEvent::Enter(node) = event else {
                continue;
            };
            if document.excludes_range(node.text_range()) {
                walk.skip_subtree();
                continue;
            }
            // 宏 token tree 没有常规表达式结构；调用层级在下方补充可解析的展开内调用。
            if node
                .ancestors()
                .skip(1)
                .any(|parent| matches!(parent.kind(), SyntaxKind::ATTR | SyntaxKind::TOKEN_TREE))
            {
                continue;
            }
            if node.kind() == SyntaxKind::CALL_EXPR {
                // 直接执行闭包不生成目标节点；其内部调用仍由下方的语法遍历提取。
                let dynamic_target = node
                    .children()
                    .find(|child| ast::Expr::can_cast(child.kind()))
                    .is_some_and(|callee| {
                        callee.kind() != SyntaxKind::PATH_EXPR && !is_closure_expression(&callee)
                    });
                if dynamic_target && let Some(owner) = document.owner(node.text_range().start()) {
                    add(
                        &mut nodes,
                        &owner.node.data().id,
                        "code.calls",
                        unknown("<dynamic callable>"),
                    );
                }
            }
            if let Some((semantic, name)) = reference_site(&node) {
                let Some(owner) = document.owner(name.text_range().start()) else {
                    continue;
                };
                for target in resolver.definition(&document.key, &name, semantic)? {
                    if resolver.includes_reference(semantic, &target) {
                        add(&mut nodes, &owner.node.data().id, semantic, target);
                    }
                }
            }
        }
        // 标准调用层级能补充宏内调用；fromRanges 决定最内层 owner，不能一律归到外层函数。
        for declaration in document
            .declarations
            .iter()
            .filter(|declaration| declaration.syntax.kind() == SyntaxKind::FN)
        {
            let position = u32::from(declaration.selection.start());
            let self_file = resolver.project.file_id(&document.path)?;
            for call in resolver
                .project
                .outgoing_calls(resolver.project.file_id(&document.path)?, position)?
            {
                let owners: Vec<_> = call
                    .ranges
                    .iter()
                    .filter(|range| self_file == range.file_id)
                    .map(|range| range.range.start())
                    .filter_map(|offset| document.owner(offset))
                    .map(|owner| owner.node.data().id.clone())
                    .collect();
                if owners.is_empty() {
                    continue;
                }
                let targets = resolver.target(
                    &resolver
                        .project
                        .file_path(call.target.file_id)?
                        .to_string_lossy(),
                    &call.target.focus_or_full_range().start(),
                    call.target.name.as_str(),
                    "code.calls",
                )?;
                for owner in &owners {
                    for target in &targets {
                        add(&mut nodes, owner, "code.calls", target.clone());
                    }
                }
            }
        }
    }
    for (id, node) in &resolver.crate_roots {
        nodes.entry(id.clone()).or_insert_with(|| node.clone());
        root.contains.push(id.clone());
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
    let referenced = nodes
        .values()
        .flat_map(|node| &node.data().references)
        .filter_map(|reference| match &reference.target {
            ReferenceTarget::External { symbol } => Some(symbol.as_str()),
            _ => None,
        })
        .collect();
    let symbols: BTreeMap<_, _> = resolver
        .documents
        .iter()
        .chain(resolver.external.values())
        .flat_map(|document| super::symbols::describe(repository, document, &referenced))
        .collect();
    Ok((
        CodeGraph {
            root_contains: root.contains,
            nodes: nodes.into_values().collect(),
            external_symbols: symbols.into_values().collect(),
        },
        resolver.test_files,
    ))
}

impl Resolver<'_> {
    /// 原生导航使用 VFS 路径和 UTF-8 字节范围，绝不经过 LSP 行列转换。
    fn navigation(&self, target: ide::NavigationTarget) -> Result<Navigation> {
        Ok(Navigation {
            key: self
                .project
                .file_path(target.file_id)?
                .to_string_lossy()
                .into_owned(),
            offset: target.focus_or_full_range().start(),
        })
    }

    /// 保留 crate 根与每个词法模块声明，不猜测文件名归属。
    fn parents(&self, key: &str) -> Result<Vec<Option<Navigation>>> {
        self.project
            .parent_modules(self.project.file_id(Path::new(key))?)?
            .into_iter()
            .map(|parent| match parent {
                ModuleParent::CrateRoot => Ok(None),
                ModuleParent::Declaration(target) => self.navigation(target).map(Some),
            })
            .collect()
    }

    /// 多个模块复用同一文件时，只有所有已知归属都明确为测试专用才排除整个文件。
    fn test_only_file(&mut self, key: &str) -> Result<bool> {
        if self.options.include_tests {
            return Ok(false);
        }
        if let Some(test) = self.test_files.get(key) {
            return Ok(*test);
        }
        if !self.visiting_files.insert(key.to_owned()) {
            return Ok(false);
        }
        let test = self.classify_test_file(key)?;
        self.visiting_files.remove(key);
        self.test_files.insert(key.to_owned(), test);
        Ok(test)
    }

    /// 父模块可在选中文件之外；只加载上下文和属性，不把这些文件加入输出图。
    fn classify_test_file(&mut self, key: &str) -> Result<bool> {
        if !self.load(key)? {
            return Ok(false);
        }
        let document = self
            .document(key)
            .ok_or_else(|| failure("测试范围文档不存在"))?;
        if super::test_code::is_test_only(&document.tree) {
            return Ok(true);
        }
        let parents = self.parents(key)?;
        if parents.is_empty() {
            return Ok(false);
        }
        for parent in parents {
            let Some(location) = parent else {
                return Ok(false);
            };
            if !self.load(&location.key)? {
                return Ok(false);
            }
            let document = self
                .document(&location.key)
                .ok_or_else(|| failure("测试父模块文档不存在"))?;
            let offset = location.offset;
            if !document.excludes(offset) && !self.test_only_file(&location.key)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// 优先读取选中快照，图外文档只载入一次以获得相同的身份生成约定。
    fn document(&self, key: &str) -> Option<&Document> {
        self.documents
            .iter()
            .find(|document| document.key == key)
            .or_else(|| self.external.get(key))
    }

    /// 仅读取仓库内源码；仓库外目标和越界符号链接显式保留为 Unresolved。
    fn load(&mut self, key: &str) -> Result<bool> {
        if self.document(key).is_some() {
            return Ok(true);
        }
        let path = Path::new(key);
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs")
            || !path.starts_with(self.repository)
        {
            return Ok(false);
        }
        let path = path.canonicalize()?;
        if !path.starts_with(self.repository) {
            return Ok(false);
        }
        let document = Document::parse(self.repository, path, self.project, self.options)?;
        self.categories
            .extend(document.declarations.iter().map(|declaration| {
                (
                    declaration.node.data().id.clone(),
                    declaration.node.category(),
                )
            }));
        self.external.insert(key.to_owned(), document);
        Ok(true)
    }

    /// 由是否在选中文件范围内决定 Node 或 External，二者使用同一声明身份。
    fn reference(&self, id: NodeId) -> ReferenceTarget {
        if self.selected.contains(&id) {
            ReferenceTarget::Node { id }
        } else {
            ReferenceTarget::External { symbol: id.0 }
        }
    }

    /// 字段类型只连向图内 Type；表达式中的类型和数据值不改名为普通引用。
    fn includes_reference(&self, semantic: &str, target: &ReferenceTarget) -> bool {
        if semantic == "code.type_use" {
            return matches!(target, ReferenceTarget::Node { id }
                if self.selected.contains(id) && self.categories.get(id) == Some(&CodeCategory::Type));
        }
        if semantic != "code.references" {
            return true;
        }
        let category = match target {
            ReferenceTarget::Node { id } => self.categories.get(id),
            ReferenceTarget::External { symbol } => self.categories.get(&NodeId(symbol.clone())),
            ReferenceTarget::Unresolved { .. } => None,
        };
        !matches!(category, Some(CodeCategory::Type | CodeCategory::Value))
    }

    /// 父模块信息来自 rust-analyzer，覆盖 #[path]，不根据文件名伪造模块层次。
    fn module_parents(&mut self, key: &str) -> Result<Vec<ReferenceTarget>> {
        let mut targets = Vec::new();
        if self.test_files.get(key) == Some(&true) {
            return Ok(targets);
        }
        for parent in self.parents(key)? {
            let location = match parent {
                None => {
                    if let Some(target) = self.crate_root(key)? {
                        targets.push(target);
                    }
                    continue;
                }
                Some(location) => location,
            };
            if location.key == key || !self.load(&location.key)? {
                continue;
            }
            let document = self
                .document(&location.key)
                .ok_or_else(|| failure("父模块文档不可用"))?;
            let offset = location.offset;
            if document.excludes(offset) || self.test_files.get(&location.key) == Some(&true) {
                continue;
            }
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

    /// 仅为选中的、由原生数据库确认的入口创建根容器；图外根保留同一身份的 External。
    fn crate_root(&mut self, key: &str) -> Result<Option<ReferenceTarget>> {
        if !self.load(key)? {
            return Ok(None);
        }
        let document = self
            .document(key)
            .ok_or_else(|| failure("crate 根文档不可用"))?;
        let Some(scope) = syntax::file_scope(self.repository, &document.path) else {
            return Ok(None);
        };
        let id = NodeId(format!("{scope}::crate"));
        if self.documents.iter().any(|document| document.key == key) {
            let name = document
                .path
                .strip_prefix(self.repository)
                .map_err(failure)?
                .to_string_lossy()
                .replace('\\', "/");
            let source = document.source.to_string();
            self.crate_roots.entry(id.clone()).or_insert_with(|| {
                CodeNode::Container(CodeNodeData {
                    id: id.clone(),
                    language: LanguageId("rust".into()),
                    native_kind: "crate".into(),
                    name: Some(name),
                    signature: None,
                    code: None,
                    source: Some(source),
                    contains: Vec::new(),
                    references: Vec::new(),
                    extensions: BTreeMap::new(),
                })
            });
            self.selected.insert(id.clone());
            self.categories.insert(id.clone(), CodeCategory::Container);
        }
        Ok(Some(self.reference(id)))
    }

    /// 在引用发生处查询定义；成功的空响应保留未知目标，失败响应向外传播。
    fn definition(
        &mut self,
        key: &str,
        name: &SyntaxNode,
        semantic: &str,
    ) -> Result<Vec<ReferenceTarget>> {
        let text = name.text().to_string();
        if text == "Self"
            && let Some(enclosing) = name.ancestors().find(|node| {
                matches!(
                    node.kind(),
                    SyntaxKind::IMPL
                        | SyntaxKind::TRAIT
                        | SyntaxKind::STRUCT
                        | SyntaxKind::ENUM
                        | SyntaxKind::UNION
                )
            })
        {
            if let Some(implementation) = ast::Impl::cast(enclosing.clone()) {
                if let Some(concrete) = implementation
                    .self_ty()
                    .and_then(|ty| type_name(ty.syntax()))
                    .filter(|concrete| concrete.text() != "Self")
                {
                    return self.definition(key, &concrete, semantic);
                }
            } else if let Some(declaration) = self.document(key).and_then(|document| {
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
        let cache_key = (key.to_owned(), u32::from(name.text_range().start()));
        if !self.definitions.contains_key(&cache_key) {
            let document = self
                .document(key)
                .ok_or_else(|| failure("引用来源文档不存在"))?;
            let locations = self
                .project
                .definitions(
                    self.project.file_id(&document.path)?,
                    u32::from(name.text_range().start()),
                )?
                .into_iter()
                .map(|target| self.navigation(target))
                .collect::<Result<Vec<_>>>()?;
            self.definitions.insert(cache_key.clone(), locations);
        }
        let locations = self.definitions[&cache_key].clone();
        if locations.is_empty() {
            return Ok(vec![unknown(&text)]);
        }
        let mut targets = Vec::new();
        for location in locations {
            targets.extend(self.target(&location.key, &location.offset, &text, semantic)?);
        }
        targets.sort();
        targets.dedup();
        Ok(targets)
    }

    /// 将定义位置映射成高层目标，不把局部变量或派生标注误当作生成方法。
    fn target(
        &mut self,
        key: &str,
        position: &TextSize,
        observed: &str,
        semantic: &str,
    ) -> Result<Vec<ReferenceTarget>> {
        let key = Path::new(key)
            .canonicalize()
            .ok()
            .and_then(|path| path.to_str().map(str::to_owned))
            .unwrap_or_else(|| key.to_owned());
        // 字段类型不需要图外身份信息；在读取或解析图外源码之前就停止。
        if semantic == "code.type_use" && !self.documents.iter().any(|document| document.key == key)
        {
            return Ok(Vec::new());
        }
        if !self.load(&key)? {
            return Ok(vec![unknown(observed)]);
        }
        let document = self
            .document(&key)
            .ok_or_else(|| failure("定义目标文档不存在"))?;
        let offset = *position;
        if document.excludes(offset) || self.test_files.get(&key) == Some(&true) {
            return Ok(Vec::new());
        }
        if let Some(declaration) = document.declared_at(offset) {
            let category = declaration.node.category();
            if semantic == "code.calls" && category != CodeCategory::Callable {
                if closure_initializer(&declaration.syntax) {
                    return Ok(Vec::new());
                }
                return Ok(vec![unknown(observed)]);
            }
            if semantic == "code.type_use" && category != CodeCategory::Type {
                return Ok(Vec::new());
            }
            return Ok(vec![self.reference(declaration.node.data().id.clone())]);
        }
        // 类型参数等位置不是 Type 声明，不能回退到外层对象或生成未知目标。
        if semantic == "code.type_use" {
            return Ok(Vec::new());
        }
        // 已确认的闭包没有独立图目标；不制造 Unresolved 节点，也不回指其外层对象。
        let token = document.tree.token_at_offset(offset).right_biased();
        if let Some(token) = token {
            let ancestors: Vec<_> = token.parent_ancestors().collect();
            if semantic == "code.calls" {
                if let Some(statement) = ancestors
                    .iter()
                    .find(|node| node.kind() == SyntaxKind::LET_STMT)
                {
                    // 只匹配 let 的绑定位置，闭包参数中的函数指针仍是未解析动态调用。
                    if ast::LetStmt::cast(statement.clone())
                        .and_then(|statement| statement.pat())
                        .is_some_and(|pattern| pattern.syntax().text_range().contains(offset))
                        && closure_initializer(statement)
                    {
                        return Ok(Vec::new());
                    }
                }
                if ancestors.iter().any(|node| {
                    node.kind() == SyntaxKind::CLOSURE_EXPR && node.text_range().start() == offset
                }) {
                    return Ok(Vec::new());
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
            let modules = self.module_parents(&key)?;
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

/// 识别绑定或值声明直接提供的闭包，不猜测函数指针参数和返回值的运行时目标。
fn closure_initializer(declaration: &SyntaxNode) -> bool {
    declaration
        .children()
        .find(|node| ast::Expr::can_cast(node.kind()))
        .is_some_and(|expression| is_closure_expression(&expression))
}

/// 括号和函数指针转换不改变匿名闭包的对象边界。
fn is_closure_expression(expression: &SyntaxNode) -> bool {
    match ast::Expr::cast(expression.clone()) {
        Some(ast::Expr::ClosureExpr(_)) => true,
        Some(ast::Expr::ParenExpr(parenthesized)) => parenthesized
            .expr()
            .is_some_and(|inner| is_closure_expression(inner.syntax())),
        Some(ast::Expr::CastExpr(cast)) => cast
            .expr()
            .is_some_and(|inner| is_closure_expression(inner.syntax())),
        _ => false,
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

/// 查询调用、导入、函数值候选和宏使用；赋值目标与非字段类型位置不查询。
fn reference_site(node: &SyntaxNode) -> Option<(&'static str, SyntaxNode)> {
    use SyntaxKind::*;
    match node.kind() {
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
                .is_none_or(|parent| parent.kind() != CALL_EXPR)
                && !is_assignment_target(node) =>
        {
            child(node, PATH)
                .and_then(|path| path_name(&path))
                .map(|name| ("code.references", name))
        }
        _ => None,
    }
}

/// 沿赋值位置的组成部分上溯，省略目标查询；索引和调用参数中的函数值仍独立解析。
fn is_assignment_target(node: &SyntaxNode) -> bool {
    use SyntaxKind::*;
    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        match parent.kind() {
            BIN_EXPR => {
                return ast::BinExpr::cast(parent).is_some_and(|expression| {
                    matches!(expression.op_kind(), Some(ast::BinaryOp::Assignment { .. }))
                        && expression
                            .lhs()
                            .is_some_and(|left| left.syntax() == &current)
                });
            }
            PAREN_EXPR
            | FIELD_EXPR
            | TUPLE_EXPR
            | ARRAY_EXPR
            | RECORD_EXPR_FIELD
            | RECORD_EXPR_FIELD_LIST
            | RECORD_EXPR => current = parent,
            PREFIX_EXPR
                if ast::PrefixExpr::cast(parent.clone()).is_some_and(|expression| {
                    expression.op_kind() == Some(ast::UnaryOp::Deref)
                }) =>
            {
                current = parent;
            }
            INDEX_EXPR
                if ast::IndexExpr::cast(parent.clone())
                    .and_then(|expression| expression.base())
                    .is_some_and(|base| base.syntax() == &current) =>
            {
                current = parent;
            }
            _ => return false,
        }
    }
    false
}
