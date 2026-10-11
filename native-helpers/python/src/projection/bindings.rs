//! 名称绑定只用于否定不可靠的导航证据，不自行推断运行时调用目标。

use ruff_python_ast::{
    self as ast, Expr, Stmt,
    visitor::{self, Visitor},
};
use ruff_text_size::{Ranged, TextRange, TextSize};
use std::collections::{BTreeMap, BTreeSet};

/// 赋值别名与 import 锚点区别处理；参数和声明不能冒充赋值转发。
#[derive(Clone)]
pub(super) enum BindingKind {
    Declaration,
    Import,
    Parameter,
    Assignment(Option<Expr>),
}

/// 源码绑定保留名称选区和控制条件，用于否定重复/有条件赋值的唯一性。
#[derive(Clone)]
pub(super) struct Binding {
    pub selection: TextRange,
    pub kind: BindingKind,
    pub conditional: bool,
}

/// 方法的词法闭包跳过类命名空间，类头和装饰器仍在外层求值。
struct Scope {
    range: TextRange,
    outer_expression: Option<TextRange>,
    parent: Option<usize>,
    class: bool,
    comprehension: bool,
    wildcard: bool,
    names: BTreeMap<String, Vec<Binding>>,
}

/// 每份文档建立一次词法索引；不暴露行号身份，也不成为另一套类型系统。
pub(super) struct Bindings {
    scopes: Vec<Scope>,
    nonlocal_writes: BTreeSet<String>,
    groups: BTreeMap<TextSize, usize>,
}
impl Bindings {
    /// 记录所有绑定和可能遮蔽；目标查找仍交给固定 ty。
    pub fn new(body: &[Stmt], len: usize) -> Self {
        let mut index = Self {
            scopes: vec![Scope {
                range: TextRange::new(TextSize::ZERO, TextSize::try_from(len).unwrap()),
                outer_expression: None,
                parent: None,
                class: false,
                comprehension: false,
                wildcard: false,
                names: BTreeMap::new(),
            }],
            nonlocal_writes: BTreeSet::new(),
            groups: BTreeMap::new(),
        };
        let mut collector = Collector {
            index: &mut index,
            scope: 0,
            conditional: false,
        };
        visitor::walk_body(&mut collector, body);
        index
    }

    /// 只有同一合法 overload 组可把多个源码声明视为一个名称绑定。
    pub fn set_groups(&mut self, groups: &[Vec<TextRange>]) {
        self.groups.clear();
        for (index, group) in groups.iter().enumerate() {
            for range in group {
                self.groups.insert(range.start(), index);
            }
        }
    }

    /// 当前套件的绑定会使旧证据失效；嵌套作用域的普通局部声明不影响外层。
    pub fn rebinds(&self, name: &str) -> bool {
        self.rebinds_except(name, None)
    }

    /// 当前函数声明可以接续旧 property；声明头中其它同名赋值仍会使旧证据失效。
    pub fn rebinds_except(&self, name: &str, declaration: Option<TextRange>) -> bool {
        self.scopes[0].wildcard
            || self.scopes[0].names.get(name).is_some_and(|bindings| {
                bindings
                    .iter()
                    .any(|binding| Some(binding.selection) != declaration)
            })
            || self.nonlocal_writes.contains(name)
    }
    /// 普通重复声明仍有歧义；不能只因同名而折叠。
    fn unique(&self, bindings: &[Binding]) -> bool {
        bindings.len() == 1
            || bindings
                .first()
                .and_then(|binding| self.groups.get(&binding.selection.start()))
                .is_some_and(|group| {
                    bindings.iter().all(|binding| {
                        !binding.conditional
                            && matches!(binding.kind, BindingKind::Declaration)
                            && self.groups.get(&binding.selection.start()) == Some(group)
                    })
                })
    }

    /// 查找位置可见的绑定；有星号导入或跨作用域写入时不能证明唯一。
    pub fn ambiguous(&self, name: &str, offset: usize, direct_call: bool) -> bool {
        self.ambiguous_inner(name, offset, direct_call, &mut BTreeSet::new())
    }

    /// 继续验证单一别名链；重复访问或循环不能成为唯一调用证据。
    fn ambiguous_inner(
        &self,
        name: &str,
        offset: usize,
        direct_call: bool,
        visited: &mut BTreeSet<(String, usize)>,
    ) -> bool {
        if self.nonlocal_writes.contains(name)
            || visited.len() >= 64
            || !visited.insert((name.into(), offset))
        {
            return true;
        }
        let point = TextSize::try_from(offset).unwrap();
        let mut scope = Some(
            self.scopes
                .iter()
                .enumerate()
                .rev()
                .find(|(_, scope)| {
                    scope.range.contains(point)
                        && !scope
                            .outer_expression
                            .is_some_and(|range| range.contains(point))
                })
                .map_or(0, |(index, _)| index),
        );
        while let Some(index) = scope {
            let current = &self.scopes[index];
            if let Some(bindings) = current.names.get(name) {
                if !self.unique(bindings) || bindings[0].conditional {
                    return true;
                }
                if let BindingKind::Assignment(value) = &bindings[0].kind {
                    return match value {
                        Some(Expr::Name(alias)) => self.ambiguous_inner(
                            alias.id.as_str(),
                            alias.start().to_usize(),
                            direct_call,
                            visited,
                        ),
                        Some(value @ Expr::Attribute(_)) => root_name(value).is_none_or(|name| {
                            self.ambiguous_inner(name, value.start().to_usize(), false, visited)
                        }),
                        Some(Expr::Call(call)) if !direct_call => root_name(&call.func)
                            .is_some_and(|name| {
                                self.ambiguous_inner(
                                    name,
                                    call.func.start().to_usize(),
                                    true,
                                    visited,
                                )
                            }),
                        _ => direct_call,
                    };
                }
                return false;
            }
            if current.wildcard {
                return true;
            }
            scope = current.parent;
        }
        false
    }

    /// definition 偶尔返回普通赋值锚点；这种位置必须验证整组绑定后才能继续。
    pub fn at(&self, offset: usize) -> Option<(&Binding, bool)> {
        let point = TextSize::try_from(offset).ok()?;
        self.scopes
            .iter()
            .flat_map(|scope| scope.names.values())
            .find_map(|bindings| {
                bindings
                    .iter()
                    .find(|binding| binding.selection.contains(point))
                    .map(|binding| (binding, self.unique(bindings) && !binding.conditional))
            })
    }
}

/// 按 Python 词法作用域收集；控制结构中的定义不能当作无条件唯一绑定。
struct Collector<'a> {
    index: &'a mut Bindings,
    scope: usize,
    conditional: bool,
}
impl Collector<'_> {
    /// 同名绑定保留全部出现，不能以最后一次赋值覆盖反证。
    fn add(&mut self, name: &str, selection: TextRange, kind: BindingKind) {
        self.index.scopes[self.scope]
            .names
            .entry(name.into())
            .or_default()
            .push(Binding {
                selection,
                kind,
                conditional: self.conditional,
            });
    }
    /// 一个新作用域的函数体中重新开始控制条件；声明本身保留外层条件。
    fn body(&mut self, body: &[Stmt], class: bool, parameters: Option<&ast::Parameters>) {
        let Some(first) = body.first() else {
            return;
        };
        let previous = self.scope;
        let condition = self.conditional;
        let mut parent = Some(previous);
        if !class && self.index.scopes[previous].class {
            parent = self.index.scopes[previous].parent;
        }
        self.scope = self.index.scopes.len();
        self.index.scopes.push(Scope {
            range: TextRange::new(first.start(), body.last().unwrap().end()),
            outer_expression: None,
            parent,
            class,
            comprehension: false,
            wildcard: false,
            names: BTreeMap::new(),
        });
        self.conditional = false;
        if let Some(parameters) = parameters {
            for parameter in parameters.iter() {
                self.add(
                    parameter.name().as_str(),
                    parameter.name().range(),
                    BindingKind::Parameter,
                );
            }
        }
        visitor::walk_body(self, body);
        self.scope = previous;
        self.conditional = condition;
    }

    /// lambda 与推导式都有自己的绑定表；推导式首个 iterable 在外层求值。
    fn expression_scope(
        &mut self,
        range: TextRange,
        parameters: Option<&ast::Parameters>,
        outer_expression: Option<TextRange>,
        comprehension: bool,
    ) -> (usize, bool) {
        let previous = (self.scope, self.conditional);
        let parent = if self.index.scopes[self.scope].class {
            self.index.scopes[self.scope].parent
        } else {
            Some(self.scope)
        };
        self.scope = self.index.scopes.len();
        self.index.scopes.push(Scope {
            range,
            outer_expression,
            parent,
            class: false,
            comprehension,
            wildcard: false,
            names: BTreeMap::new(),
        });
        self.conditional = comprehension;
        if let Some(parameters) = parameters {
            for parameter in parameters.iter() {
                self.add(
                    parameter.name().as_str(),
                    parameter.name().range(),
                    BindingKind::Parameter,
                );
            }
        }
        previous
    }

    /// 各迭代目标属于推导式；其中 walrus 由 visit_expr 写入最近的非推导式作用域。
    fn comprehension(
        &mut self,
        range: TextRange,
        generators: &[ast::Comprehension],
        values: &[&Expr],
    ) {
        let Some(first) = generators.first() else {
            return;
        };
        self.visit_expr(&first.iter);
        let previous = self.expression_scope(range, None, Some(first.iter.range()), true);
        for (index, generator) in generators.iter().enumerate() {
            if index > 0 {
                self.visit_expr(&generator.iter);
            }
            self.visit_expr(&generator.target);
            for condition in &generator.ifs {
                self.visit_expr(condition);
            }
        }
        for value in values {
            self.visit_expr(value);
        }
        (self.scope, self.conditional) = previous;
    }
    /// 解构只记录绑定，不把一个共享 RHS 拆成多个假别名。
    fn target(&mut self, target: &Expr, value: Option<&Expr>) {
        match target {
            Expr::Name(name) => self.add(
                name.id.as_str(),
                name.range,
                BindingKind::Assignment(value.cloned()),
            ),
            Expr::Tuple(value) => {
                for element in &value.elts {
                    self.target(element, None);
                }
            }
            Expr::List(value) => {
                for element in &value.elts {
                    self.target(element, None);
                }
            }
            Expr::Starred(value) => self.target(&value.value, None),
            _ => {}
        }
    }
}
impl<'a> Visitor<'a> for Collector<'_> {
    /// 函数和类的新作用域不污染外层绑定数；默认值仍在外层求值。
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(value) => {
                self.add(
                    value.name.as_str(),
                    value.name.range,
                    BindingKind::Declaration,
                );
                for decorator in &value.decorator_list {
                    self.visit_expr(&decorator.expression);
                }
                for parameter in value.parameters.iter_non_variadic_params() {
                    if let Some(default) = &parameter.default {
                        self.visit_expr(default);
                    }
                }
                self.body(&value.body, false, Some(&value.parameters));
            }
            Stmt::ClassDef(value) => {
                self.add(
                    value.name.as_str(),
                    value.name.range,
                    BindingKind::Declaration,
                );
                for decorator in &value.decorator_list {
                    self.visit_expr(&decorator.expression);
                }
                if let Some(arguments) = &value.arguments {
                    visitor::walk_arguments(self, arguments);
                }
                self.body(&value.body, true, None);
            }
            Stmt::Assign(value) => {
                for target in &value.targets {
                    self.target(
                        target,
                        (value.targets.len() == 1).then_some(value.value.as_ref()),
                    );
                }
                self.visit_expr(&value.value);
            }
            Stmt::AnnAssign(value) => {
                self.target(&value.target, value.value.as_deref());
                if let Some(value) = &value.value {
                    self.visit_expr(value);
                }
            }
            Stmt::Import(value) => {
                for alias in &value.names {
                    let name = alias
                        .asname
                        .as_ref()
                        .map(|name| name.as_str())
                        .unwrap_or_else(|| alias.name.as_str().split('.').next().unwrap());
                    self.add(name, alias.range, BindingKind::Import);
                }
            }
            Stmt::ImportFrom(value) => {
                for alias in &value.names {
                    if alias.name.as_str() == "*" {
                        self.index.scopes[self.scope].wildcard = true;
                    } else {
                        let name = alias.asname.as_ref().unwrap_or(&alias.name);
                        self.add(name.as_str(), alias.range, BindingKind::Import);
                    }
                }
            }
            Stmt::Global(value) => self
                .index
                .nonlocal_writes
                .extend(value.names.iter().map(ToString::to_string)),
            Stmt::Nonlocal(value) => self
                .index
                .nonlocal_writes
                .extend(value.names.iter().map(ToString::to_string)),
            _ => {
                let previous = self.conditional;
                self.conditional |= matches!(
                    stmt,
                    Stmt::If(_)
                        | Stmt::For(_)
                        | Stmt::While(_)
                        | Stmt::Try(_)
                        | Stmt::With(_)
                        | Stmt::Match(_)
                );
                visitor::walk_stmt(self, stmt);
                self.conditional = previous;
            }
        }
    }
    /// 捕获 for、with、增量赋值及命名表达式中的 Store/Del。
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(value) => {
                if let Some(parameters) = &value.parameters {
                    for parameter in parameters.iter_non_variadic_params() {
                        if let Some(default) = &parameter.default {
                            self.visit_expr(default);
                        }
                    }
                }
                let previous = self.expression_scope(
                    value.body.range(),
                    value.parameters.as_deref(),
                    None,
                    false,
                );
                self.visit_expr(&value.body);
                (self.scope, self.conditional) = previous;
                return;
            }
            Expr::ListComp(value) => {
                self.comprehension(value.range, &value.generators, &[&value.elt]);
                return;
            }
            Expr::SetComp(value) => {
                self.comprehension(value.range, &value.generators, &[&value.elt]);
                return;
            }
            Expr::Generator(value) => {
                self.comprehension(value.range, &value.generators, &[&value.elt]);
                return;
            }
            Expr::DictComp(value) => {
                let values = value
                    .key
                    .as_deref()
                    .into_iter()
                    .chain(std::iter::once(value.value.as_ref()))
                    .collect::<Vec<_>>();
                self.comprehension(value.range, &value.generators, &values);
                return;
            }
            Expr::Named(value) => {
                let previous = self.scope;
                while self.index.scopes[self.scope].comprehension {
                    self.scope = self.index.scopes[self.scope].parent.unwrap_or(0);
                }
                self.target(&value.target, Some(&value.value));
                self.scope = previous;
                self.visit_expr(&value.value);
                return;
            }
            _ => {}
        }
        if let Expr::Name(name) = expr
            && name.ctx != ast::ExprContext::Load
        {
            self.add(name.id.as_str(), name.range, BindingKind::Assignment(None));
        }
        visitor::walk_expr(self, expr);
    }
    /// except 的绑定不是 Name 表达式，需要显式保留有条件声明。
    fn visit_except_handler(&mut self, handler: &'a ast::ExceptHandler) {
        let ast::ExceptHandler::ExceptHandler(value) = handler;
        if let Some(name) = &value.name {
            self.add(name.as_str(), name.range, BindingKind::Assignment(None));
        }
        visitor::walk_except_handler(self, handler);
    }

    /// match 捕获是名称绑定，而不是 Name 表达式；包含解构中的星号和剩余映射。
    fn visit_pattern(&mut self, pattern: &'a ast::Pattern) {
        let name = match pattern {
            ast::Pattern::MatchAs(value) => value.name.as_ref(),
            ast::Pattern::MatchStar(value) => value.name.as_ref(),
            ast::Pattern::MatchMapping(value) => value.rest.as_ref(),
            _ => None,
        };
        if let Some(name) = name {
            self.add(name.as_str(), name.range, BindingKind::Assignment(None));
        }
        visitor::walk_pattern(self, pattern);
    }
}

/// 获取链式属性的根绑定；动态下标或调用结果没有可直接证明的名称链。
pub(super) fn root_name(expression: &Expr) -> Option<&str> {
    match expression {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(value) => root_name(&value.value),
        _ => None,
    }
}
