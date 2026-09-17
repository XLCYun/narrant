//! typed AST 到声明元数据；只在此遍历中保留借用语法节点。
use super::DeclarationPart;
use crate::structure_review::code_graph::CodeCategory;
use oxc_ast::{ast::*, AstKind};
use oxc_ast_visit::{walk, Visit};
use oxc_span::{GetSpan, Span};
use std::collections::BTreeMap;

/// 脱离 arena 的中间声明；原文和规范代码各构造一次并校验配对。
pub(super) struct RawDeclaration {
    pub category: CodeCategory,
    pub kind: &'static str,
    pub name: Option<String>,
    pub parent: Option<usize>,
    pub parts: Vec<DeclarationPart>,
    pub prefix: String,
    pub callable_value: Option<Span>,
    pub ambient_function: bool,
    /// 被省略的数据包装仍提供词法身份，例如 handlers.save 中的 handlers。
    pub identity_path: Vec<String>,
}

/// 声明遍历栈同时追踪词法所有者与未建模的回调、语句包装。
pub(super) struct Collector<'a> {
    pub items: Vec<RawDeclaration>,
    stack: Vec<(AstKind<'a>, Option<usize>)>,
    root_statement: Option<(Option<usize>, Span)>,
    execution: BTreeMap<Option<usize>, BTreeMap<u32, Span>>,
}
impl<'a> Collector<'a> {
    /// 新建一次纯语法收集，不借用外部可变图状态。
    pub fn new() -> Self {
        Self {
            items: vec![],
            stack: vec![],
            root_statement: None,
            execution: BTreeMap::new(),
        }
    }

    /// 创建对象并将同一词法所有者的函数重载合入第一个声明。
    fn push(
        &mut self,
        category: CodeCategory,
        kind: &'static str,
        name: Option<String>,
        spans: (Span, Span),
        body: Option<Span>,
        prefix: String,
    ) -> usize {
        let (span, selection) = spans;
        let parent = self.stack.last().and_then(|(_, owner)| *owner);
        let part = DeclarationPart {
            span,
            selection,
            body,
            parent_exclusion: span,
            source_span: span,
            names: if selection.is_empty() {
                vec![]
            } else {
                vec![selection]
            },
        };
        if category == CodeCategory::Callable
            && matches!(
                kind,
                "function" | "method" | "static_method" | "constructor"
            )
        {
            if let Some((index, existing)) = self
                .items
                .iter_mut()
                .enumerate()
                .rev()
                .find(|(_, d)| d.parent == parent && d.kind == kind && d.name == name)
            {
                if body.is_none() || existing.parts.iter().all(|p| p.body.is_none()) {
                    existing.parts.push(part);
                    return index;
                }
            }
        }
        let identity_path = self
            .stack
            .iter()
            .rev()
            .take_while(|(_, owner)| *owner == parent)
            .filter_map(|(node, _)| match node {
                AstKind::VariableDeclarator(v) => {
                    v.id.get_binding_identifier().map(|id| id.name.to_string())
                }
                AstKind::ObjectProperty(p) => p.key.static_name().map(|name| name.into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let index = self.items.len();
        self.items.push(RawDeclaration {
            category,
            kind,
            name,
            parent,
            parts: vec![part],
            prefix,
            callable_value: None,
            ambient_function: false,
            identity_path,
        });
        index
    }

    /// 只有顶层或 namespace 语句变成执行区域；函数体语句留在函数自身代码。
    fn top_statement(&self) -> bool {
        matches!(
            self.stack.last(),
            Some((AstKind::Program(_) | AstKind::TSModuleBlock(_), _))
        )
    }

    /// 文件和显式 namespace 各自收集执行代码，同一语句中的多个调用只记录一次。
    fn record_execution(&mut self, parent: Option<usize>, span: Span) {
        self.execution
            .entry(parent)
            .or_default()
            .insert(span.start, span);
    }

    /// 类与对象中的方法仍可建模，但任何函数/回调内部的 Callable 都归入外层对象。
    fn inside_function(&self) -> bool {
        self.stack.iter().any(|(node, _)| {
            matches!(
                node,
                AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
            )
        })
    }

    /// AST 静态属性名保持语义值，动态计算名使用固定匿名键和词法顺序，不用源码偏移。
    fn key(&self, key: &PropertyKey<'a>) -> String {
        match key {
            PropertyKey::PrivateIdentifier(i) => format!("#{}", i.name),
            _ => key
                .static_name()
                .map(|s| s.into_owned())
                .unwrap_or_else(|| "computed".into()),
        }
    }

    /// 动态计算键没有可精确匹配的声明名，保留空选择避免误匹配键内调用。
    fn key_selection(&self, key: &PropertyKey<'a>) -> Span {
        if key.static_name().is_some() || matches!(key, PropertyKey::PrivateIdentifier(_)) {
            key.span()
        } else {
            Span::new(key.span().start, key.span().start)
        }
    }
}
impl<'a> Visit<'a> for Collector<'a> {
    /// 每个文件或 namespace 只生成一个执行区域；不改变其中具名声明的词法归属。
    fn visit_program(&mut self, program: &Program<'a>) {
        walk::walk_program(self, program);
        for (parent, spans) in std::mem::take(&mut self.execution) {
            let parts = spans
                .into_values()
                .map(|span| DeclarationPart {
                    span,
                    selection: Span::new(span.start, span.start),
                    body: Some(span),
                    parent_exclusion: span,
                    source_span: span,
                    names: vec![],
                })
                .collect::<Vec<_>>();
            self.items.push(RawDeclaration {
                category: CodeCategory::ExecutionRegion,
                kind: "top_level",
                name: Some("顶层执行".into()),
                parent,
                parts,
                prefix: String::new(),
                callable_value: None,
                ambient_function: false,
                identity_path: vec![],
            });
        }
    }

    /// 普通语句与初始化代码共用作用域执行区域；函数体由 Callable 自身承载。
    fn visit_statement(&mut self, statement: &Statement<'a>) {
        let previous_statement = self.root_statement;
        if self.top_statement() {
            self.root_statement = Some((
                self.stack.last().and_then(|(_, owner)| *owner),
                statement.span(),
            ));
        }
        let execution = self.top_statement()
            && match statement {
                Statement::ExpressionStatement(_)
                | Statement::BlockStatement(_)
                | Statement::IfStatement(_)
                | Statement::ForStatement(_)
                | Statement::ForInStatement(_)
                | Statement::ForOfStatement(_)
                | Statement::WhileStatement(_)
                | Statement::DoWhileStatement(_)
                | Statement::SwitchStatement(_)
                | Statement::TryStatement(_)
                | Statement::ThrowStatement(_)
                | Statement::WithStatement(_)
                | Statement::LabeledStatement(_)
                | Statement::DebuggerStatement(_) => true,
                // 纯导出没有对象；导出表达式中的实际执行代码仍需要归属。
                Statement::ExportDefaultDeclaration(d) => {
                    d.declaration.as_expression().is_some_and(|e| {
                        !matches!(e.get_inner_expression(), Expression::Identifier(_))
                    })
                }
                Statement::TSExportAssignment(d) => !matches!(
                    d.expression.get_inner_expression(),
                    Expression::Identifier(_)
                ),
                _ => false,
            };
        if execution {
            self.record_execution(
                self.stack.last().and_then(|(_, owner)| *owner),
                statement.span(),
            );
        }
        walk::walk_statement(self, statement);
        self.root_statement = previous_statement;
    }

    /// 分类时解包具名函数值的透明表达式，源码与规范签名仍保留断言等语义。
    fn enter_node(&mut self, node: AstKind<'a>) {
        use CodeCategory::*;
        let mut owner = self.stack.last().and_then(|(_, owner)| *owner);
        let mut prefix = String::new();
        if let Some((parent, span)) = self.root_statement {
            if owner == parent
                && matches!(
                    node,
                    AstKind::CallExpression(_)
                        | AstKind::NewExpression(_)
                        | AstKind::TaggedTemplateExpression(_)
                        | AstKind::AssignmentExpression(_)
                        | AstKind::UpdateExpression(_)
                        | AstKind::JSXOpeningElement(_)
                )
            {
                self.record_execution(parent, span);
            }
        }
        if let AstKind::Directive(d) = node {
            if self.top_statement() {
                self.record_execution(owner, d.span);
            }
        }
        let inside_function = self.inside_function();
        let shape = match node {
            AstKind::Function(f)
                if !inside_function
                    && f.id.is_some()
                    && !owner.is_some_and(|id| self.items[id].callable_value == Some(f.span)) =>
            {
                Some((
                    Callable,
                    "function",
                    f.id.as_ref().map(|i| i.name.to_string()),
                    f.id.as_ref()
                        .map_or(Span::new(f.span.start, f.span.start), |i| i.span),
                    f.body.as_ref().map(|b| b.span),
                ))
            }
            AstKind::Class(c) if c.id.is_some() || c.r#type == ClassType::ClassDeclaration => {
                Some((
                    Type,
                    "class",
                    c.id.as_ref().map(|i| i.name.to_string()),
                    c.id.as_ref()
                        .map_or(Span::new(c.span.start, c.span.start), |i| i.span),
                    None,
                ))
            }
            AstKind::VariableDeclarator(v) if !inside_function => {
                let identifier = v.id.get_binding_identifier();
                let function = v.init.as_ref().and_then(function_value);
                identifier.zip(function).map(|(identifier, (kind, body))| {
                    prefix = self
                        .stack
                        .iter()
                        .rev()
                        .find_map(|(node, _)| match node {
                            AstKind::VariableDeclaration(d) => Some(format!("{} ", d.kind)),
                            _ => None,
                        })
                        .unwrap_or_default();
                    (
                        Callable,
                        kind,
                        Some(identifier.name.to_string()),
                        identifier.span,
                        body,
                    )
                })
            }
            AstKind::MethodDefinition(m)
                if !inside_function
                    && (m.key.static_name().is_some()
                        || matches!(m.key, PropertyKey::PrivateIdentifier(_))) =>
            {
                Some((
                    Callable,
                    match m.kind {
                        MethodDefinitionKind::Constructor => "constructor",
                        MethodDefinitionKind::Get => "getter",
                        MethodDefinitionKind::Set => "setter",
                        MethodDefinitionKind::Method if m.r#static => "static_method",
                        _ => "method",
                    },
                    Some(self.key(&m.key)),
                    self.key_selection(&m.key),
                    m.value.body.as_ref().map(|b| b.span),
                ))
            }
            AstKind::ObjectProperty(p) if !inside_function && p.key.static_name().is_some() => {
                function_value(&p.value).map(|(_, body)| {
                    (
                        Callable,
                        match p.kind {
                            PropertyKind::Get => "getter",
                            PropertyKind::Set => "setter",
                            _ => "object_method",
                        },
                        Some(self.key(&p.key)),
                        self.key_selection(&p.key),
                        body,
                    )
                })
            }
            AstKind::PropertyDefinition(p) => {
                let function = p.value.as_ref().and_then(function_value);
                if let Some((_, body)) = function {
                    (!inside_function
                        && (p.key.static_name().is_some()
                            || matches!(p.key, PropertyKey::PrivateIdentifier(_))))
                    .then(|| {
                        (
                            Callable,
                            if p.r#static {
                                "static_method"
                            } else {
                                "method"
                            },
                            Some(self.key(&p.key)),
                            self.key_selection(&p.key),
                            body,
                        )
                    })
                } else {
                    Some((
                        Value,
                        if p.r#static { "static_field" } else { "field" },
                        Some(self.key(&p.key)),
                        self.key_selection(&p.key),
                        p.value.as_ref().map(GetSpan::span),
                    ))
                }
            }
            AstKind::StaticBlock(b) => Some((
                ExecutionRegion,
                "static_block",
                None,
                Span::new(b.span.start, b.span.start),
                Some(b.span),
            )),
            AstKind::TSInterfaceDeclaration(d) => Some((
                Type,
                "interface",
                Some(d.id.name.to_string()),
                d.id.span,
                None,
            )),
            AstKind::TSTypeAliasDeclaration(d) => Some((
                Type,
                "type_alias",
                Some(d.id.name.to_string()),
                d.id.span,
                None,
            )),
            AstKind::TSEnumDeclaration(d) => {
                Some((Type, "enum", Some(d.id.name.to_string()), d.id.span, None))
            }
            AstKind::TSEnumMember(m) => Some((
                Value,
                "enum_member",
                Some(m.id.static_name().to_string()),
                m.id.span(),
                m.initializer.as_ref().map(GetSpan::span),
            )),
            AstKind::TSPropertySignature(p) => Some((
                Value,
                "property",
                Some(self.key(&p.key)),
                self.key_selection(&p.key),
                None,
            )),
            AstKind::TSMethodSignature(m) if !inside_function && m.key.static_name().is_some() => {
                Some((
                    Callable,
                    match m.kind {
                        TSMethodSignatureKind::Get => "getter",
                        TSMethodSignatureKind::Set => "setter",
                        _ => "method",
                    },
                    Some(self.key(&m.key)),
                    self.key_selection(&m.key),
                    None,
                ))
            }
            AstKind::TSIndexSignature(d) => Some((
                Value,
                "index_signature",
                None,
                Span::new(d.span.start, d.span.start),
                None,
            )),
            AstKind::TSNamespaceDeclaration(d) => Some((
                Container,
                "namespace",
                Some(d.id.name.to_string()),
                d.id.span,
                None,
            )),
            AstKind::TSExternalModuleDeclaration(d) => Some((
                Container,
                "module",
                Some(d.id.value.to_string()),
                d.id.span,
                None,
            )),
            AstKind::TSGlobalDeclaration(d) => Some((
                Container,
                "global",
                Some("global".into()),
                d.global_span,
                None,
            )),
            _ => None,
        };
        if let Some((category, kind, name, selection, body)) = shape {
            let id = self.push(category, kind, name, (node.span(), selection), body, prefix);
            if let AstKind::Function(f) = node {
                self.items[id].ambient_function |= f.id.is_some()
                    && (f.declare
                        || self.stack.iter().any(|(ancestor, _)| match ancestor {
                            AstKind::TSNamespaceDeclaration(d) => d.declare,
                            AstKind::TSExternalModuleDeclaration(d) => d.declare,
                            AstKind::TSGlobalDeclaration(d) => d.declare,
                            _ => false,
                        }));
            }
            let initializer = match node {
                AstKind::VariableDeclarator(v) => v.init.as_ref(),
                AstKind::ObjectProperty(p) if !p.method => Some(&p.value),
                AstKind::PropertyDefinition(p) => p.value.as_ref(),
                _ => None,
            }
            .map(Expression::get_inner_expression);
            self.items[id].callable_value = initializer
                .filter(|e| {
                    matches!(
                        e,
                        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
                    )
                })
                .map(GetSpan::span);
            if let AstKind::VariableDeclarator(v) = node {
                let part = self.items[id].parts.last_mut().unwrap();
                part.names =
                    v.id.get_binding_identifiers()
                        .iter()
                        .map(|i| i.span)
                        .collect();
                if let Some(AstKind::VariableDeclaration(d)) = self.stack.last().map(|(n, _)| n) {
                    part.parent_exclusion = d.span;
                    part.source_span = d.span;
                }
            }
            // 导出包装归入真实声明的签名和原文，不再生成独立图节点。
            if let Some((export_span, modifier)) = self
                .stack
                .iter()
                .rev()
                .take_while(|(n, _)| {
                    matches!(
                        n,
                        AstKind::VariableDeclaration(_)
                            | AstKind::ExportDeclaration(_)
                            | AstKind::ExportDefaultDeclaration(_)
                    )
                })
                .find_map(|(n, _)| match n {
                    AstKind::ExportDeclaration(e) => Some((e.span, "export ")),
                    AstKind::ExportDefaultDeclaration(e) => Some((e.span, "export default ")),
                    _ => None,
                })
            {
                let declaration = &mut self.items[id];
                let part = declaration.parts.last_mut().unwrap();
                part.source_span = export_span;
                part.parent_exclusion = export_span;
                if !declaration.prefix.starts_with(modifier) {
                    declaration.prefix.insert_str(0, modifier);
                }
            }
            owner = Some(id);
        }
        self.stack.push((node, owner));
    }

    /// 离开节点恢复词法所有者，嵌套命名声明不会污染后续兄弟归属。
    fn leave_node(&mut self, _: AstKind<'a>) {
        self.stack.pop();
    }
}

/// 只识别直接函数初始化器；工厂调用与条件表达式不能凭外形推断为函数。
fn function_value(expression: &Expression<'_>) -> Option<(&'static str, Option<Span>)> {
    match expression.get_inner_expression() {
        Expression::ArrowFunctionExpression(function) => {
            Some(("arrow", Some(function.body.span())))
        }
        Expression::FunctionExpression(function) => {
            Some(("function", function.body.as_ref().map(|body| body.span)))
        }
        _ => None,
    }
}
