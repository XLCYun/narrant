//! AST 收集实际引用位置，ty 只提供定义证据；坐标或协议错误不能降级为空关系。

use super::{
    Result, failure, is_test,
    project::{self, Project},
    syntax::{self, Document},
};
use crate::{DefinitionCandidate, PythonProject, SourceOrigin};
use anyhow::Context;
use narrant_native_common::model::{
    CodeCategory, CodeReference, ExternalSymbol, NodeId, ReferenceTarget, SymbolScope,
};
use ruff_python_ast::{
    self as ast, Expr, Stmt,
    visitor::{self, Visitor},
};
use ruff_text_size::Ranged;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// 一次源码出现及其内容 owner；同一引用不在父子对象上重复收集。
#[derive(Clone)]
struct Occurrence {
    owner: NodeId,
    semantic: &'static str,
    offset: usize,
    name: String,
    queryable: bool,
    root: Option<String>,
    direct: bool,
}

/// 只持有本项目文档和查询缓存，跨项目及两侧不存在可变语义状态共享。
struct Resolver<'a> {
    native: &'a PythonProject,
    repository: &'a Path,
    project: &'a Project,
    documents: BTreeMap<PathBuf, Document>,
    definitions: BTreeMap<(PathBuf, usize), Vec<DefinitionCandidate>>,
    externals: BTreeMap<String, ExternalSymbol>,
    include_tests: bool,
    prepared: BTreeSet<PathBuf>,
    conflicting_projects: BTreeSet<PathBuf>,
}

/// 每个选中文档按实际 AST 引用建立关系，其他文档只用于验证目标。
pub(super) fn analyze(
    native: &PythonProject,
    repository: &Path,
    project: &Project,
    documents: BTreeMap<PathBuf, Document>,
    include_tests: bool,
) -> Result<(Vec<Document>, BTreeMap<String, ExternalSymbol>)> {
    let selected = documents.keys().cloned().collect::<Vec<_>>();
    let mut resolver = Resolver {
        native,
        repository,
        project,
        documents,
        definitions: BTreeMap::new(),
        externals: BTreeMap::new(),
        include_tests,
        prepared: BTreeSet::new(),
        conflicting_projects: BTreeSet::new(),
    };
    for path in &selected {
        resolver.prepare(path)?;
    }
    for path in &selected {
        let document = &resolver.documents[path];
        let Some(root) = &document.root else {
            continue;
        };
        let mut collector = Occurrences {
            document,
            owner: NodeId(format!("{}:module_body", root.0)),
            values: vec![],
        };
        visitor::walk_body(&mut collector, &document.body);
        let occurrences = collector.values;
        for occurrence in occurrences {
            if let Some(target) = resolver.resolve(path, &occurrence)? {
                let node = resolver
                    .documents
                    .get_mut(path)
                    .unwrap()
                    .nodes
                    .iter_mut()
                    .find(|node| node.data().id == occurrence.owner)
                    .ok_or_else(|| failure("Python 引用缺少内容所属对象"))?;
                syntax::node_data_mut(node).references.push(CodeReference {
                    semantic: occurrence.semantic.into(),
                    target,
                });
            }
        }
        for node in &mut resolver.documents.get_mut(path).unwrap().nodes {
            let references = &mut syntax::node_data_mut(node).references;
            references.sort();
            references.dedup();
        }
    }
    Ok((
        selected
            .iter()
            .map(|path| resolver.documents.remove(path).unwrap())
            .collect(),
        resolver.externals,
    ))
}

impl Resolver<'_> {
    /// 确定关系要求唯一合格声明；缺少第三方上下文仅令这一出现保持未解析。
    fn resolve(&mut self, path: &Path, occurrence: &Occurrence) -> Result<Option<ReferenceTarget>> {
        let mut candidates = BTreeMap::new();
        let mut conflict = false;
        let ambiguous = occurrence.root.as_ref().is_some_and(|name| {
            self.documents[path]
                .bindings
                .ambiguous(name, occurrence.offset, occurrence.direct)
        });
        if occurrence.queryable && !ambiguous {
            let mut locations = self.query(path, occurrence.offset)?;
            let mut aliases = BTreeSet::new();
            while let Some(location) = locations.pop() {
                let Some(target) = self.locate(&location)? else {
                    return Ok(None);
                };
                let actual = target.path;
                if self.conflicting_projects.contains(&actual) {
                    return Ok((occurrence.semantic != "code.references").then(|| {
                        ReferenceTarget::Unresolved {
                            name: occurrence.name.clone(),
                        }
                    }));
                }
                self.prepare(&actual)?;
                let scope = target.scope;
                let offset = target.offset;
                let document = &self.documents[&actual];
                if document
                    .bindings
                    .at(offset)
                    .is_some_and(|(_, unique)| !unique)
                {
                    return Ok((occurrence.semantic != "code.references").then(|| {
                        ReferenceTarget::Unresolved {
                            name: occurrence.name.clone(),
                        }
                    }));
                }
                let Some(declaration) = document.declarations.iter().find(|declaration| {
                    declaration
                        .selection
                        .contains(ruff_text_size::TextSize::try_from(offset).unwrap())
                }) else {
                    // ty 也可能同时返回赋值锚点；沿已证明唯一的简单别名查询 RHS，保留其所有反证。
                    let binding = document.bindings.at(offset).filter(|(_, unique)| *unique);
                    let alias = binding.and_then(|(binding, _)| match &binding.kind {
                        super::bindings::BindingKind::Assignment(Some(
                            value @ (Expr::Name(_) | Expr::Attribute(_)),
                        )) => Some(value),
                        _ => None,
                    });
                    if let Some(alias) = alias {
                        let offset = match alias {
                            Expr::Attribute(value) => value.attr.start().to_usize(),
                            _ => alias.start().to_usize(),
                        };
                        if super::bindings::root_name(alias).is_none_or(|name| {
                            document.bindings.ambiguous(
                                name,
                                offset,
                                occurrence.semantic == "code.calls"
                                    && matches!(alias, Expr::Name(_)),
                            )
                        }) || aliases.len() >= 64
                        {
                            conflict = true;
                        } else if aliases.insert((actual.clone(), offset)) {
                            let definitions = self.query(&actual, offset)?;
                            conflict |= definitions.is_empty();
                            locations.extend(definitions);
                        }
                        continue;
                    }
                    // 唯一 import 锚点可跳过，其余真实目标不能被过滤成另一个确定目标。
                    conflict |= !document
                        .bindings
                        .at(offset)
                        .is_some_and(|(binding, unique)| {
                            unique && matches!(binding.kind, super::bindings::BindingKind::Import)
                        });
                    continue;
                };
                let node = document
                    .nodes
                    .iter()
                    .find(|node| node.data().id == declaration.id)
                    .ok_or_else(|| failure("ty 目标声明没有对应对象"))?;
                let valid = declaration.eligible
                    && match occurrence.semantic {
                        "code.calls" => {
                            matches!(node.category(), CodeCategory::Callable | CodeCategory::Type)
                        }
                        "code.references" => node.category() == CodeCategory::Callable,
                        "code.extends" | "code.type_use" => node.category() == CodeCategory::Type,
                        _ => false,
                    };
                if !valid {
                    conflict = true;
                    continue;
                }
                let symbol = if scope == SymbolScope::Dependency {
                    declaration
                        .id
                        .0
                        .replacen("python:repo:", "python:stdlib:ty-0.0.84:", 1)
                } else {
                    declaration.id.0.clone()
                };
                candidates.insert(
                    symbol.clone(),
                    ExternalSymbol {
                        symbol: symbol.clone(),
                        name: node.data().name.clone().unwrap_or_default(),
                        scope,
                    },
                );
            }
        }
        if !conflict && candidates.len() == 1 {
            let (symbol, description) = candidates.pop_first().unwrap();
            self.externals.insert(symbol.clone(), description);
            Ok(Some(ReferenceTarget::External { symbol }))
        } else if occurrence.semantic == "code.references" {
            Ok(None)
        } else {
            Ok(Some(ReferenceTarget::Unresolved {
                name: occurrence.name.clone(),
            }))
        }
    }

    /// 装饰器分类使用经过路径和选区核对的标准库声明，仓库同名声明不提供证明。
    fn prepare(&mut self, path: &Path) -> Result<()> {
        if !self.prepared.insert(path.to_owned()) {
            return Ok(());
        }
        let expressions = super::known::queries(&self.documents[path].body);
        let mut proofs = super::known::Proofs::new();
        for expression in expressions {
            let offset = match &expression {
                Expr::Attribute(value) => value.attr.start().to_usize(),
                _ => expression.start().to_usize(),
            };
            if super::bindings::root_name(&expression)
                .is_some_and(|name| self.documents[path].bindings.ambiguous(name, offset, true))
            {
                continue;
            }
            let locations = self.query(path, offset)?;
            let mut known = None;
            let mut conflict = false;
            for location in locations {
                let Some(target) = self.locate(&location)? else {
                    conflict = true;
                    continue;
                };
                // 冲突项目文档只作目标语法校验，装饰器证明必须先拒绝该候选。
                if self.conflicting_projects.contains(&target.path) {
                    conflict = true;
                    continue;
                }
                let document = &self.documents[&target.path];
                if target.scope != SymbolScope::Dependency {
                    // import 自身锚点不代表目标，也不能覆盖其它合格定义。
                    if !document
                        .bindings
                        .at(target.offset)
                        .is_some_and(|(binding, unique)| {
                            unique && matches!(binding.kind, super::bindings::BindingKind::Import)
                        })
                    {
                        conflict = true;
                    }
                    continue;
                }
                let point = ruff_text_size::TextSize::try_from(target.offset).unwrap();
                let name = document
                    .declarations
                    .iter()
                    .find(|declaration| declaration.selection.contains(point))
                    .and_then(|declaration| {
                        document
                            .nodes
                            .iter()
                            .find(|node| node.data().id == declaration.id)
                    })
                    .and_then(|node| node.data().name.clone())
                    .or_else(|| {
                        document
                            .bindings
                            .at(target.offset)
                            .map(|(binding, _)| document.source[binding.selection].to_owned())
                    });
                let result = name
                    .as_deref()
                    .and_then(|name| super::known::Known::at_stdlib(&target.relative, name));
                if result.is_none() || known.is_some_and(|known| Some(known) != result) {
                    conflict = true;
                }
                known = known.or(result);
            }
            if !conflict && let Some(known) = known {
                proofs.insert(expression.start().to_usize(), known);
            }
        }
        self.documents.get_mut(path).unwrap().rebuild(&proofs);
        // ty 的赋值位置会优先返回写入本身；继承成员证据必须从基类声明取得。
        let mut blocked = BTreeSet::new();
        for class in super::known::classes(&self.documents[path].body) {
            let owner = &self.documents[path].owners[&class.start().to_usize()];
            let has_instance_writes = self.documents[path]
                .nodes
                .iter()
                .find(|node| &node.data().id == owner)
                .is_some_and(|class_node| {
                    self.documents[path].declarations.iter().any(|declaration| {
                        class_node.data().contains.contains(&declaration.id)
                            && declaration.instance_field
                    })
                });
            if !has_instance_writes {
                continue;
            }
            let owner = owner.clone();
            let inherited = self.inherited_members(path, &class, &mut BTreeSet::new())?;
            let node = self.documents[path]
                .nodes
                .iter()
                .find(|node| node.data().id == owner)
                .unwrap();
            for id in &node.data().contains {
                let member = self.documents[path]
                    .nodes
                    .iter()
                    .find(|node| &node.data().id == id)
                    .unwrap();
                if member.category() == CodeCategory::Value
                    && inherited.as_ref().is_none_or(|names| {
                        member
                            .data()
                            .name
                            .as_ref()
                            .is_some_and(|name| names.contains(name))
                    })
                {
                    blocked.insert(id.clone());
                }
            }
        }
        let mut changed = false;
        for declaration in &self.documents[path].declarations {
            if blocked.contains(&declaration.id) && declaration.instance_field {
                proofs.insert(
                    declaration.selection.start().to_usize(),
                    super::known::Known::NotInstanceField,
                );
                changed = true;
            }
        }
        if changed {
            self.documents.get_mut(path).unwrap().rebuild(&proofs);
        }
        Ok(())
    }

    /// 继承已有成员的写入无法证明是本类的新字段；未知基类也不能凭空确认归属。
    fn inherited_members(
        &mut self,
        path: &Path,
        class: &ast::StmtClassDef,
        visited: &mut BTreeSet<(PathBuf, usize)>,
    ) -> Result<Option<BTreeSet<String>>> {
        if !visited.insert((path.to_owned(), class.start().to_usize())) {
            return Ok(None);
        }
        let mut names = BTreeSet::new();
        if let Some(arguments) = &class.arguments {
            for base in &arguments.args {
                let base = if let Expr::Subscript(value) = base {
                    value.value.as_ref()
                } else {
                    base
                };
                let offset = match base {
                    Expr::Name(name) => name.start(),
                    Expr::Attribute(attribute) => attribute.attr.start(),
                    _ => return Ok(None),
                }
                .to_usize();
                if super::bindings::root_name(base).is_some_and(|name| {
                    self.documents[path].bindings.ambiguous(name, offset, false)
                }) {
                    return Ok(None);
                }
                let mut targets = BTreeMap::new();
                for location in self.query(path, offset)? {
                    let Some(target) = self.locate(&location)? else {
                        return Ok(None);
                    };
                    if self.conflicting_projects.contains(&target.path) {
                        return Ok(None);
                    }
                    self.prepare(&target.path)?;
                    let document = &self.documents[&target.path];
                    if document
                        .bindings
                        .at(target.offset)
                        .is_some_and(|(_, unique)| !unique)
                    {
                        return Ok(None);
                    }
                    let Some(declaration) = document.declarations.iter().find(|declaration| {
                        declaration
                            .selection
                            .contains(ruff_text_size::TextSize::try_from(target.offset).unwrap())
                    }) else {
                        if document
                            .bindings
                            .at(target.offset)
                            .is_some_and(|(binding, unique)| {
                                unique
                                    && matches!(binding.kind, super::bindings::BindingKind::Import)
                            })
                        {
                            continue;
                        }
                        return Ok(None);
                    };
                    let node = document
                        .nodes
                        .iter()
                        .find(|node| node.data().id == declaration.id)
                        .unwrap();
                    if node.data().native_kind != "class" || !declaration.eligible {
                        return Ok(None);
                    }
                    let base_class = super::known::classes(&document.body)
                        .into_iter()
                        .find(|class| class.name.range == declaration.selection)
                        .ok_or_else(|| failure("基类缺少原始声明"))?;
                    let members = node
                        .data()
                        .contains
                        .iter()
                        .filter_map(|id| document.nodes.iter().find(|node| &node.data().id == id))
                        .filter_map(|node| node.data().name.clone())
                        .collect::<BTreeSet<_>>();
                    targets.insert((target.path, declaration.id.clone()), (base_class, members));
                }
                if targets.len() != 1 {
                    return Ok(None);
                }
                let ((target_path, _), (base_class, members)) = targets.pop_first().unwrap();
                names.extend(members);
                let Some(inherited) = self.inherited_members(&target_path, &base_class, visited)?
                else {
                    return Ok(None);
                };
                names.extend(inherited);
            }
        }
        visited.remove(&(path.to_owned(), class.start().to_usize()));
        Ok(Some(names))
    }

    /// 直接使用原生字节范围；标准声明的 AST 仍由同一数据库读取。
    fn locate(&mut self, location: &DefinitionCandidate) -> Result<Option<TargetFile>> {
        let (relative, scope, actual) = match &location.origin {
            SourceOrigin::Repository(relative) => {
                let actual = self.repository.join(relative);
                let physical = project::checked_path(self.repository, &actual)?;
                if !self.include_tests
                    && (is_test(relative) || is_test(physical.strip_prefix(self.repository)?))
                {
                    return Ok(None);
                }
                (relative.clone(), SymbolScope::Repository, actual)
            }
            SourceOrigin::StandardDeclaration(path) => {
                let relative = PathBuf::from(
                    path.strip_prefix("stdlib/")
                        .ok_or_else(|| failure("原生目标不是固定标准库"))?,
                );
                (
                    relative,
                    SymbolScope::Dependency,
                    PathBuf::from(format!("/narrant/stdlib/{path}")),
                )
            }
        };
        if !self.documents.contains_key(&actual) {
            let mut target_minor = self.project.minor;
            if scope == SymbolScope::Repository {
                let target_project = Project::for_file(self.repository, &actual, &location.source)?;
                target_minor = target_project.minor;
                if target_project.root != self.project.root
                    && target_project.root != self.repository
                {
                    self.conflicting_projects.insert(actual.clone());
                }
            }
            // 未选中的上下文也按目标自身 minor 校验；跨项目文档不能提供关系证明。
            let body = self
                .native
                .candidate_ast(location, target_minor)
                .with_context(|| format!("{}：Python 目标语法提取失败", relative.display()))?
                .syntax()
                .body
                .clone();
            self.documents.insert(
                actual.clone(),
                Document::from_ast(&relative, &location.source, body)?,
            );
        }
        let offset = location.focus.start().to_usize();
        anyhow::ensure!(
            offset < location.source.len() && location.source.is_char_boundary(offset),
            "原生定义焦点越界"
        );
        Ok(Some(TargetFile {
            path: actual,
            relative,
            scope,
            offset,
        }))
    }

    /// 每个文档字节位置只查询一次，不将候选导航升级为确定运行时关系。
    fn query(&mut self, path: &Path, offset: usize) -> Result<Vec<DefinitionCandidate>> {
        let key = (path.to_owned(), offset);
        if let Some(result) = self.definitions.get(&key) {
            return Ok(result.clone());
        }
        let result = if let Ok(relative) = path.strip_prefix(self.repository) {
            self.native.definitions(relative, offset)?.candidates
        } else if let Ok(relative) = path.strip_prefix("/narrant/stdlib") {
            self.native
                .standard_definitions(
                    relative
                        .to_str()
                        .ok_or_else(|| failure("标准库路径不是 UTF-8"))?,
                    offset,
                )?
                .candidates
        } else {
            return Err(failure("查询文件不属于固定原生数据库"));
        };
        self.definitions.insert(key, result.clone());
        Ok(result)
    }
}

/// 经过文件边界和坐标校验的目标位置，尚不代表声明类别符合关系要求。
struct TargetFile {
    path: PathBuf,
    relative: PathBuf,
    scope: SymbolScope,
    offset: usize,
}

/// visitor 进入独立声明时切换 owner，进入局部函数/类时保持最近外层 Callable。
struct Occurrences<'a> {
    document: &'a Document,
    owner: NodeId,
    values: Vec<Occurrence>,
}
impl Occurrences<'_> {
    /// 属性查询定位属性名，避免把 receiver 的定义误认成方法。
    fn add(&mut self, expression: &Expr, semantic: &'static str) {
        let (offset, queryable) = match expression {
            Expr::Name(name) => (name.start().to_usize(), true),
            Expr::Attribute(attribute) => (attribute.attr.start().to_usize(), true),
            _ => (expression.start().to_usize(), false),
        };
        self.values.push(Occurrence {
            owner: self.owner.clone(),
            semantic,
            offset,
            name: syntax::expression(expression),
            queryable,
            root: super::bindings::root_name(expression).map(str::to_owned),
            direct: matches!(expression, Expr::Name(_)) && semantic == "code.calls",
        });
    }
    /// 继承与字段类型的参数化表达式只把实际类型名称作为目标，不查询整个运算。
    fn type_names(&mut self, expression: &Expr, semantic: &'static str) {
        match expression {
            Expr::Name(_) | Expr::Attribute(_) => self.add(expression, semantic),
            Expr::Subscript(value) => {
                self.type_names(&value.value, semantic);
                if semantic == "code.type_use" {
                    self.type_names(&value.slice, semantic);
                }
            }
            Expr::Tuple(value) => {
                for element in &value.elts {
                    self.type_names(element, semantic);
                }
            }
            Expr::BinOp(value) => {
                self.type_names(&value.left, semantic);
                self.type_names(&value.right, semantic);
            }
            _ => {}
        }
    }
}
impl<'a> Visitor<'a> for Occurrences<'a> {
    /// 默认值与装饰器属于声明；注解不伪装成函数值引用。
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        let previous = self.owner.clone();
        if let Some(owner) = self.document.owners.get(&stmt.start().to_usize()) {
            self.owner = owner.clone();
        }
        match stmt {
            Stmt::FunctionDef(value) => {
                for decorator in &value.decorator_list {
                    self.visit_expr(&decorator.expression);
                }
                for parameter in value.parameters.iter_non_variadic_params() {
                    if let Some(default) = &parameter.default {
                        self.visit_expr(default);
                    }
                }
                visitor::walk_body(self, &value.body);
            }
            Stmt::ClassDef(value) => {
                for decorator in &value.decorator_list {
                    self.visit_expr(&decorator.expression);
                }
                if let Some(arguments) = &value.arguments {
                    for base in &arguments.args {
                        self.type_names(base, "code.extends");
                    }
                    for keyword in &arguments.keywords {
                        self.visit_expr(&keyword.value);
                    }
                }
                visitor::walk_body(self, &value.body);
            }
            Stmt::AnnAssign(value) => {
                let expression_owner = self.owner.clone();
                if let Some(field) = self.document.declarations.iter().find(|declaration| {
                    declaration.selection == value.target.range()
                        && self.document.nodes.iter().any(|node| {
                            node.data().id == declaration.id
                                && node.category() == CodeCategory::Value
                        })
                }) {
                    self.owner = field.id.clone();
                    self.type_names(&value.annotation, "code.type_use");
                }
                self.owner = expression_owner;
                if let Some(expression) = &value.value {
                    self.visit_expr(expression);
                }
            }
            Stmt::Import(_) | Stmt::ImportFrom(_) => {}
            _ => visitor::walk_stmt(self, stmt),
        }
        self.owner = previous;
    }
    /// 每个 Call 独立记录；非调用位置仅在解析到 Callable 后才产生函数值引用。
    fn visit_expr(&mut self, expression: &'a Expr) {
        match expression {
            Expr::Call(value) => {
                self.add(&value.func, "code.calls");
                match value.func.as_ref() {
                    Expr::Attribute(attribute) => self.visit_expr(&attribute.value),
                    Expr::Name(_) => {}
                    other => self.visit_expr(other),
                }
                visitor::walk_arguments(self, &value.arguments);
            }
            Expr::Name(value) => {
                if value.ctx == ast::ExprContext::Load {
                    self.add(expression, "code.references");
                }
            }
            Expr::Attribute(value) => {
                if value.ctx == ast::ExprContext::Load {
                    self.add(expression, "code.references");
                }
                self.visit_expr(&value.value);
            }
            _ => visitor::walk_expr(self, expression),
        }
    }
}
