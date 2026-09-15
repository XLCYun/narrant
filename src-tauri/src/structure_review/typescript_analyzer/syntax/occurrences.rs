//! 收集语法意图与查询位置；此层绝不把名字相同当成已解析目标。
use super::{contains, text, Document, Occurrence, OccurrenceKind};
use crate::structure_review::typescript_analyzer::Result;
use oxc_ast::{ast::*, AstKind};
use oxc_ast_visit::{walk, Visit};
use oxc_codegen::{Codegen, CodegenOptions, CommentOptions};
use oxc_span::{GetSpan, Span};
use std::collections::BTreeMap;

/// 引用遍历器保留祖先上下文，并在结束时去掉被强语义覆盖的普通名字读取。
pub(super) struct Collector<'d, 'a> {
    document: &'d Document,
    stack: Vec<AstKind<'a>>,
    scoping: &'d oxc_semantic::Scoping,
    bindings: BTreeMap<(u32, u32), (Option<Span>, Vec<String>)>,
    canonical: BTreeMap<(u32, u32), String>,
    pending: Vec<(OccurrenceKind, Span, Span, bool, bool)>,
}
impl<'d, 'a> Collector<'d, 'a> {
    /// 只借用拥有数据的文档，不改变声明树。
    pub fn new(document: &'d Document, scoping: &'d oxc_semantic::Scoping) -> Self {
        Self {
            document,
            scoping,
            bindings: BTreeMap::new(),
            stack: vec![],
            pending: vec![],
            canonical: BTreeMap::new(),
        }
    }

    /// 记录候选位置；type_only 阻止类型位置被误作运行时调用。
    fn add(
        &mut self,
        kind: OccurrenceKind,
        span: Span,
        selection: Span,
        dynamic: bool,
        type_only: bool,
    ) {
        self.pending
            .push((kind, span, selection, dynamic, type_only));
    }

    /// 生成经过 UTF-16 校验且具有明确所有者的候选引用。
    pub fn finish(self) -> Result<Vec<Occurrence>> {
        let mut output = Vec::new();
        for (kind, span, selection, dynamic, type_only) in &self.pending {
            if *kind == OccurrenceKind::Reference
                && self.pending.iter().any(|(k, _, s, dynamic, _)| {
                    // 只有明确的同名选择才能替代读取；复合或动态候选不能吞掉内部证据。
                    *k != OccurrenceKind::Reference && !dynamic && *s == *selection
                })
            {
                continue;
            }
            let Some(owner) = self.document.owner(span.start) else {
                continue;
            };
            if output.iter().any(|o: &Occurrence| {
                o.owner == owner.node.data().id && o.kind == *kind && o.selection == *selection
            }) {
                continue;
            }
            output.push(Occurrence {
                owner: owner.node.data().id.clone(),
                kind: *kind,
                span: *span,
                selection: *selection,
                position: text::position(&self.document.source, selection.start)?,
                text: self.document.source[selection.start as usize..selection.end as usize]
                    .to_owned(),
                semantic_text: self
                    .canonical
                    .get(&(selection.start, selection.end))
                    .cloned()
                    .unwrap_or_else(|| {
                        self.document.source[selection.start as usize..selection.end as usize]
                            .to_owned()
                    }),
                root_binding: self
                    .bindings
                    .get(&(selection.start, selection.end))
                    .and_then(|b| b.0),
                members: self
                    .bindings
                    .get(&(selection.start, selection.end))
                    .map(|b| b.1.clone())
                    .unwrap_or_default(),
                dynamic: *dynamic,
                type_only: *type_only,
            });
        }
        Ok(output)
    }

    /// 保存规范名字，不改动发送给语言服务的原始位置或原文。
    fn canonical(&mut self, span: Span, text: String) {
        self.canonical.insert((span.start, span.end), text);
    }

    /// 静态成员访问的根由 Oxc 词法绑定确认，函数参数/局部同名声明不会误认成 import。
    fn expression_binding(&mut self, selection: Span, expression: &Expression<'a>) {
        if let Some((root, members)) = root_path(expression) {
            self.bindings.insert(
                (selection.start, selection.end),
                (super::aliases::binding(self.scoping, root), members),
            );
        }
    }

    /// 类型注解和 import/export 内的名字不可被推测为普通运行时函数读取。
    fn type_only(&self) -> bool {
        self.stack.iter().any(|node| {
            matches!(
                node,
                AstKind::TSTypeAnnotation(_)
                    | AstKind::TSTypeAliasDeclaration(_)
                    | AstKind::TSInterfaceDeclaration(_)
                    | AstKind::TSTypeParameterDeclaration(_)
                    | AstKind::TSTypeParameterInstantiation(_)
                    | AstKind::TSTypeQuery(_)
                    | AstKind::TSImportType(_)
                    | AstKind::TSFunctionType(_)
                    | AstKind::TSConstructorType(_)
            )
        })
    }

    /// import 与 export 的别名只留在内部路由中，不生成图关系候选。
    fn alias_context(&self) -> bool {
        self.stack.iter().any(|node| {
            matches!(
                node,
                AstKind::ImportDeclaration(_)
                    | AstKind::TSImportEqualsDeclaration(_)
                    | AstKind::ExportSpecifier(_)
            )
        })
    }
}

/// 获取可静态查询的被调名字；计算成员和工厂返回值保持动态候选。
fn callee(expression: &Expression<'_>) -> (Span, bool, String) {
    match expression.get_inner_expression() {
        Expression::Identifier(i) => (i.span, false, i.name.to_string()),
        Expression::StaticMemberExpression(m) => (
            m.property.span,
            dynamic_receiver(&m.object),
            m.property.name.to_string(),
        ),
        Expression::PrivateFieldExpression(m) => (
            m.field.span,
            dynamic_receiver(&m.object),
            format!("#{}", m.field.name),
        ),
        Expression::ComputedMemberExpression(m) => {
            (m.expression.span(), true, expression_text(&m.expression))
        }
        other => (other.span(), true, expression_text(other)),
    }
}

/// 直接从 AST 打印复合表达式，保留字符串、模板和正则内部的语义空白。
fn expression_text(expression: &Expression<'_>) -> String {
    let mut codegen = Codegen::new().with_options(CodegenOptions {
        comments: CommentOptions::disabled(),
        ..Default::default()
    });
    codegen.print_expression(expression);
    codegen.into_source_text()
}

/// 仅从 AST 静态成员链提取词法根与成员；工厂/计算接收者没有可猜测的根。
fn root_path<'r, 'a>(
    expression: &'r Expression<'a>,
) -> Option<(&'r IdentifierReference<'a>, Vec<String>)> {
    match expression.get_inner_expression() {
        Expression::Identifier(id) => Some((id, vec![])),
        Expression::StaticMemberExpression(m) => {
            let (root, mut members) = root_path(&m.object)?;
            members.push(m.property.name.to_string());
            Some((root, members))
        }
        Expression::PrivateFieldExpression(m) => {
            let (root, mut members) = root_path(&m.object)?;
            members.push(format!("#{}", m.field.name));
            Some((root, members))
        }
        _ => None,
    }
}

/// JSX 成员链与普通静态访问使用同一词法根，不靠组件名字猜测导入。
fn jsx_root<'r, 'a>(
    member: &'r JSXMemberExpression<'a>,
) -> Option<(&'r IdentifierReference<'a>, Vec<String>)> {
    let (root, mut members) = match &member.object {
        JSXMemberExpressionObject::IdentifierReference(id) => (id.as_ref(), vec![]),
        JSXMemberExpressionObject::MemberExpression(m) => jsx_root(m)?,
        _ => return None,
    };
    members.push(member.property.name.to_string());
    Some((root, members))
}

/// 类型限定名查询末段声明，不能把 namespace 前缀当作字段目标类型。
fn type_selection(name: &TSTypeName<'_>) -> Span {
    match name {
        TSTypeName::QualifiedName(name) => name.right.span,
        _ => name.span(),
    }
}

/// 保守标记动态接收者；链式字段仍可由语言服务验证具体声明。
fn dynamic_receiver(expression: &Expression<'_>) -> bool {
    match expression.get_inner_expression() {
        Expression::Identifier(_) | Expression::ThisExpression(_) | Expression::Super(_) => false,
        Expression::StaticMemberExpression(m) => dynamic_receiver(&m.object),
        _ => true,
    }
}

impl<'a> Visit<'a> for Collector<'_, 'a> {
    /// 每个具体左值分别记录，解构写入不会误标默认值或计算键的读取。
    fn visit_simple_assignment_target(&mut self, target: &SimpleAssignmentTarget<'a>) {
        let (selection, dynamic) = match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(i) => (i.span, false),
            SimpleAssignmentTarget::StaticMemberExpression(m) => {
                (m.property.span, dynamic_receiver(&m.object))
            }
            SimpleAssignmentTarget::PrivateFieldExpression(m) => {
                (m.field.span, dynamic_receiver(&m.object))
            }
            SimpleAssignmentTarget::ComputedMemberExpression(m) => {
                self.canonical(m.expression.span(), expression_text(&m.expression));
                (m.expression.span(), true)
            }
            _ => (target.span(), true),
        };
        self.add(
            OccurrenceKind::Write,
            target.span(),
            selection,
            dynamic,
            self.type_only(),
        );
        walk::walk_simple_assignment_target(self, target);
    }

    /// 从具体节点记录调用、写入、类型与 JSX 候选，仍继续遍历内部表达式。
    fn enter_node(&mut self, node: AstKind<'a>) {
        use OccurrenceKind::*;
        let type_only = self.type_only();
        match node {
            AstKind::CallExpression(c) => {
                let (selection, dynamic, name) = callee(&c.callee);
                self.expression_binding(selection, &c.callee);
                self.canonical(selection, name);
                self.add(Call, c.span, selection, dynamic, type_only);
            }
            AstKind::TaggedTemplateExpression(t) => {
                let (selection, dynamic, name) = callee(&t.tag);
                self.expression_binding(selection, &t.tag);
                self.canonical(selection, name);
                self.add(Call, t.span, selection, dynamic, type_only);
            }
            AstKind::NewExpression(c) => {
                let (selection, dynamic, name) = callee(&c.callee);
                self.expression_binding(selection, &c.callee);
                self.canonical(selection, name);
                self.add(Construct, c.span, selection, dynamic, type_only);
            }
            AstKind::AssignmentTargetPropertyIdentifier(p) => {
                self.add(Write, p.binding.span, p.binding.span, false, type_only)
            }
            AstKind::IdentifierReference(i) if !self.alias_context() => {
                self.bindings.insert(
                    (i.span.start, i.span.end),
                    (super::aliases::binding(self.scoping, i), vec![]),
                );
                self.add(Reference, i.span, i.span, false, type_only);
            }
            AstKind::PrivateFieldExpression(m) => self.add(
                Reference,
                m.span,
                m.field.span,
                dynamic_receiver(&m.object),
                type_only,
            ),
            AstKind::StaticMemberExpression(m) => {
                if let Some((root, mut members)) = root_path(&m.object) {
                    members.push(m.property.name.to_string());
                    self.bindings.insert(
                        (m.property.span.start, m.property.span.end),
                        (super::aliases::binding(self.scoping, root), members),
                    );
                }
                self.add(
                    Reference,
                    m.span,
                    m.property.span,
                    dynamic_receiver(&m.object),
                    type_only,
                );
            }
            AstKind::Class(c) => {
                if let Some(heritage) = &c.heritage {
                    let (selection, dynamic, name) = callee(&heritage.expression);
                    self.canonical(selection, name);
                    self.add(
                        Extends,
                        heritage.expression.span(),
                        selection,
                        dynamic,
                        false,
                    );
                }
            }
            AstKind::TSClassImplements(i) => self.add(
                Implements,
                i.span,
                type_selection(&i.expression),
                false,
                true,
            ),
            AstKind::TSInterfaceHeritage(i) => {
                self.add(Extends, i.span, type_selection(&i.type_name), false, true)
            }
            AstKind::TSTypeReference(t)
                if self.stack.iter().any(|n| match n {
                    AstKind::PropertyDefinition(p) => p
                        .type_annotation
                        .as_ref()
                        .is_some_and(|a| contains(a.span, t.span)),
                    AstKind::AccessorProperty(p) => p
                        .type_annotation
                        .as_ref()
                        .is_some_and(|a| contains(a.span, t.span)),
                    AstKind::TSPropertySignature(p) => p
                        .type_annotation
                        .as_ref()
                        .is_some_and(|a| contains(a.span, t.span)),
                    AstKind::TSIndexSignature(p) => contains(p.type_annotation.span, t.span),
                    AstKind::FormalParameter(p)
                        if p.accessibility.is_some() || p.readonly || p.r#override =>
                    {
                        p.type_annotation
                            .as_ref()
                            .is_some_and(|a| contains(a.span, t.span))
                    }
                    _ => false,
                }) =>
            {
                self.add(TypeUse, t.span, type_selection(&t.type_name), false, true)
            }
            AstKind::JSXOpeningElement(e) => {
                let selection = match &e.name {
                    JSXElementName::IdentifierReference(i) => Some(i.span),
                    JSXElementName::MemberExpression(m) => Some(m.property.span),
                    _ => None,
                };
                if let Some(selection) = selection {
                    let binding = match &e.name {
                        JSXElementName::IdentifierReference(id) => Some((id.as_ref(), vec![])),
                        JSXElementName::MemberExpression(m) => jsx_root(m),
                        _ => None,
                    };
                    if let Some((root, members)) = binding {
                        self.bindings.insert(
                            (selection.start, selection.end),
                            (super::aliases::binding(self.scoping, root), members),
                        );
                    }
                    self.add(JsxComponent, e.name.span(), selection, false, false);
                }
            }
            _ => {}
        }
        self.stack.push(node);
    }

    /// 回到父级语义上下文，避免类型注解影响后续运行时表达式。
    fn leave_node(&mut self, _: AstKind<'a>) {
        self.stack.pop();
    }
}
