//! typed AST 到声明元数据；只在此遍历中保留借用语法节点。
use super::DeclarationPart;
use crate::structure_review::code_graph::CodeCategory;
use oxc_ast::{ast::*, AstKind};
use oxc_ast_visit::{walk, Visit};
use oxc_span::{GetSpan, Span};

/// 脱离 arena 的中间声明；原文和规范代码各构造一次并校验配对。
pub(super) struct RawDeclaration {
    pub category: CodeCategory,
    pub kind: &'static str,
    pub name: Option<String>,
    pub parent: Option<usize>,
    pub parts: Vec<DeclarationPart>,
    pub prefix: String,
}

/// 声明遍历栈同时追踪词法所有者与未建模的回调、语句包装。
pub(super) struct Collector<'a> {
    pub items: Vec<RawDeclaration>,
    stack: Vec<(AstKind<'a>, Option<usize>)>,
}
impl<'a> Collector<'a> {
    /// 新建一次纯语法收集，不借用外部可变图状态。
    pub fn new() -> Self {
        Self {
            items: vec![],
            stack: vec![],
        }
    }

    /// 创建对象并将同一词法所有者的函数重载合入第一个声明。
    fn push(
        &mut self,
        category: CodeCategory,
        kind: &'static str,
        name: Option<String>,
        span: Span,
        selection: Span,
        body: Option<Span>,
        prefix: String,
    ) -> usize {
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
        let index = self.items.len();
        self.items.push(RawDeclaration {
            category,
            kind,
            name,
            parent,
            parts: vec![part],
            prefix,
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
    /// 普通顶层执行语句独立建模；具名声明继续走统一 typed AST 入口。
    fn visit_statement(&mut self, statement: &Statement<'a>) {
        let execution = self.top_statement()
            && matches!(
                statement,
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
                    | Statement::DebuggerStatement(_)
            );
        if execution {
            let span = statement.span();
            let id = self.push(
                CodeCategory::ExecutionRegion,
                "statement",
                None,
                span,
                Span::new(span.start, span.start),
                Some(span),
                String::new(),
            );
            let old = self.stack.last().unwrap().1;
            self.stack.last_mut().unwrap().1 = Some(id);
            walk::walk_statement(self, statement);
            self.stack.last_mut().unwrap().1 = old;
        } else {
            walk::walk_statement(self, statement);
        }
    }

    /// 分类时解包 const 箭头的透明表达式，源码与规范签名仍保留断言等语义。
    fn enter_node(&mut self, node: AstKind<'a>) {
        use CodeCategory::*;
        let mut owner = self.stack.last().and_then(|(_, owner)| *owner);
        let mut prefix = String::new();
        let shape = match node {
            AstKind::Function(f)
                if f.id.is_some()
                    || matches!(
                        f.r#type,
                        FunctionType::FunctionDeclaration | FunctionType::TSDeclareFunction
                    ) =>
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
            AstKind::Directive(d) if self.top_statement() => Some((
                ExecutionRegion,
                "directive",
                None,
                Span::new(d.span.start, d.span.start),
                Some(d.span),
            )),
            AstKind::FormalParameter(p)
                if p.accessibility.is_some() || p.readonly || p.r#override =>
            {
                Some((
                    Value,
                    "parameter_property",
                    p.pattern
                        .get_binding_identifier()
                        .map(|i| i.name.to_string()),
                    p.pattern.span(),
                    p.initializer.as_ref().map(|i| i.span()),
                ))
            }
            AstKind::VariableDeclarator(v) => {
                let kind = self.stack.iter().rev().find_map(|(n, _)| {
                    if let AstKind::VariableDeclaration(d) = n {
                        Some(d.kind)
                    } else {
                        None
                    }
                });
                prefix = kind.map_or(String::new(), |k| format!("{k} "));
                let arrow = if kind == Some(VariableDeclarationKind::Const)
                    && matches!(v.id, BindingPattern::BindingIdentifier(_))
                {
                    v.init.as_ref().and_then(|e| {
                        if let Expression::ArrowFunctionExpression(a) = e.get_inner_expression() {
                            Some(a)
                        } else {
                            None
                        }
                    })
                } else {
                    None
                };
                let name = Some(
                    v.id.get_binding_identifiers()
                        .iter()
                        .map(|i| i.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                Some((
                    if arrow.is_some() { Callable } else { Value },
                    if arrow.is_some() { "arrow" } else { "variable" },
                    name,
                    v.id.span(),
                    if let Some(a) = arrow {
                        Some(a.body.span())
                    } else {
                        v.init.as_ref().map(GetSpan::span)
                    },
                ))
            }
            AstKind::MethodDefinition(m) => Some((
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
            )),
            AstKind::ObjectProperty(p) => {
                let function = match &p.value {
                    Expression::FunctionExpression(f)
                        if p.method || p.kind != PropertyKind::Init =>
                    {
                        Some(f)
                    }
                    _ => None,
                };
                Some((
                    if function.is_some() { Callable } else { Value },
                    match p.kind {
                        PropertyKind::Get => "getter",
                        PropertyKind::Set => "setter",
                        _ if function.is_some() => "object_method",
                        _ => "object_property",
                    },
                    Some(self.key(&p.key)),
                    self.key_selection(&p.key),
                    function.map_or(Some(p.value.span()), |f| f.body.as_ref().map(|b| b.span)),
                ))
            }
            AstKind::PropertyDefinition(p) => Some((
                Value,
                if p.r#static { "static_field" } else { "field" },
                Some(self.key(&p.key)),
                self.key_selection(&p.key),
                p.value.as_ref().map(GetSpan::span),
            )),
            AstKind::AccessorProperty(p) => Some((
                Value,
                "accessor_property",
                Some(self.key(&p.key)),
                self.key_selection(&p.key),
                p.value.as_ref().map(GetSpan::span),
            )),
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
            AstKind::TSMethodSignature(m) => Some((
                Callable,
                match m.kind {
                    TSMethodSignatureKind::Get => "getter",
                    TSMethodSignatureKind::Set => "setter",
                    _ => "method",
                },
                Some(self.key(&m.key)),
                self.key_selection(&m.key),
                None,
            )),
            AstKind::TSCallSignatureDeclaration(d) => Some((
                Callable,
                "call_signature",
                None,
                Span::new(d.span.start, d.span.start),
                None,
            )),
            AstKind::TSConstructSignatureDeclaration(d) => Some((
                Callable,
                "construct_signature",
                None,
                Span::new(d.span.start, d.span.start),
                None,
            )),
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
            AstKind::ImportDeclaration(d) => Some((
                Extension,
                "import",
                Some(d.source.value.to_string()),
                d.source.span,
                None,
            )),
            AstKind::TSImportEqualsDeclaration(d) => Some((
                Extension,
                "import_equals",
                Some(d.id.name.to_string()),
                d.id.span,
                None,
            )),
            AstKind::ExportFromDeclaration(d) => Some((
                Extension,
                "reexport",
                Some(d.source.value.to_string()),
                d.source.span,
                None,
            )),
            AstKind::ExportAllDeclaration(d) => Some((
                Extension,
                "export_all",
                Some(d.source.value.to_string()),
                d.source.span,
                None,
            )),
            AstKind::ExportNamedDeclaration(d) => Some((
                Extension,
                "export_named",
                None,
                Span::new(d.span.start, d.span.start),
                None,
            )),
            AstKind::ExportDeclaration(d) => Some((
                Extension,
                "export",
                None,
                Span::new(d.span.start, d.span.start),
                None,
            )),
            AstKind::ExportDefaultDeclaration(d) => Some((
                Extension,
                "export_default",
                Some("default".into()),
                Span::new(d.span.start, d.span.start),
                None,
            )),
            AstKind::TSExportAssignment(d) => Some((
                Extension,
                "export_assignment",
                None,
                Span::new(d.span.start, d.span.start),
                Some(d.expression.span()),
            )),
            AstKind::TSNamespaceExportDeclaration(d) => Some((
                Extension,
                "namespace_export",
                Some(d.id.name.to_string()),
                d.id.span,
                None,
            )),
            _ => None,
        };
        if let Some((category, kind, name, selection, body)) = shape {
            let id = self.push(category, kind, name, node.span(), selection, body, prefix);
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
            // export 是独立 Extension，但声明展示保留完整导出包装和前导 JSDoc。
            if let Some(export_span) = self
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
                    AstKind::ExportDeclaration(e) => Some(e.span),
                    AstKind::ExportDefaultDeclaration(e) => Some(e.span),
                    _ => None,
                })
            {
                self.items[id].parts.last_mut().unwrap().source_span = export_span;
            }
            if !matches!(
                node,
                AstKind::ExportDeclaration(_) | AstKind::ExportDefaultDeclaration(_)
            ) {
                owner = Some(id);
            }
        }
        self.stack.push((node, owner));
    }

    /// 离开节点恢复词法所有者，嵌套命名声明不会污染后续兄弟归属。
    fn leave_node(&mut self, _: AstKind<'a>) {
        self.stack.pop();
    }
}
