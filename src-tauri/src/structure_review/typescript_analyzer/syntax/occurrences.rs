//! 收集语法意图与查询位置；此层绝不把名字相同当成已解析目标。
use super::{contains, text, Document, Occurrence, OccurrenceKind};
use crate::structure_review::typescript_analyzer::Result;
use oxc_ast::{ast::*, AstKind};
use oxc_ast_visit::{walk, Visit};
use oxc_span::{GetSpan, Span};

/// 引用遍历器保留祖先上下文，并在结束时去掉被强语义覆盖的普通名字读取。
pub(super) struct Collector<'d, 'a> {
    document: &'d Document,
    stack: Vec<AstKind<'a>>,
    pending: Vec<(OccurrenceKind, Span, Span, bool, bool, bool)>,
}
impl<'d, 'a> Collector<'d, 'a> {
    /// 只借用拥有数据的文档，不改变声明树。
    pub fn new(document: &'d Document) -> Self {
        Self {
            document,
            stack: vec![],
            pending: vec![],
        }
    }

    /// 记录候选位置；type_only 和 import_alias 阻止后续误作运行时调用。
    fn add(
        &mut self,
        kind: OccurrenceKind,
        span: Span,
        selection: Span,
        dynamic: bool,
        type_only: bool,
        import_alias: bool,
    ) {
        self.pending
            .push((kind, span, selection, dynamic, type_only, import_alias));
    }

    /// 生成经过 UTF-16 校验且具有明确所有者的候选引用。
    pub fn finish(self) -> Result<Vec<Occurrence>> {
        let mut output = Vec::new();
        for (kind, span, selection, dynamic, type_only, import_alias) in &self.pending {
            if *kind == OccurrenceKind::Reference
                && self.pending.iter().any(|(k, _, s, _, _, _)| {
                    *k != OccurrenceKind::Reference && contains(*s, *selection)
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
                dynamic: *dynamic,
                type_only: *type_only,
                import_alias: *import_alias,
            });
        }
        Ok(output)
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

    /// import 与 export 的别名语法有单独候选，不再生成普通标识符读取。
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
fn callee(expression: &Expression<'_>) -> (Span, bool) {
    match expression.get_inner_expression() {
        Expression::Identifier(i) => (i.span, false),
        Expression::StaticMemberExpression(m) => (m.property.span, dynamic_receiver(&m.object)),
        Expression::PrivateFieldExpression(m) => (m.field.span, dynamic_receiver(&m.object)),
        Expression::ComputedMemberExpression(m) => (m.expression.span(), true),
        other => (other.span(), true),
    }
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
            SimpleAssignmentTarget::ComputedMemberExpression(m) => (m.expression.span(), true),
            _ => (target.span(), true),
        };
        self.add(
            OccurrenceKind::Write,
            target.span(),
            selection,
            dynamic,
            self.type_only(),
            false,
        );
        walk::walk_simple_assignment_target(self, target);
    }

    /// 从具体节点记录调用、写入、类型、模块与 JSX 候选，仍继续遍历内部表达式。
    fn enter_node(&mut self, node: AstKind<'a>) {
        use OccurrenceKind::*;
        let type_only = self.type_only();
        match node {
            AstKind::CallExpression(c) => { let (selection, dynamic) = callee(&c.callee); self.add(Call, c.span, selection, dynamic, type_only, false); }
            AstKind::NewExpression(c) => { let (selection, dynamic) = callee(&c.callee); self.add(Construct, c.span, selection, dynamic, type_only, false); }
            AstKind::AssignmentTargetPropertyIdentifier(p) => self.add(Write, p.binding.span, p.binding.span, false, type_only, false),
            AstKind::IdentifierReference(i) if !self.alias_context() => self.add(Reference, i.span, i.span, false, type_only, false),
            AstKind::PrivateFieldExpression(m) => self.add(Reference, m.span, m.field.span, dynamic_receiver(&m.object), type_only, false),
            AstKind::StaticMemberExpression(m) => self.add(Reference, m.span, m.property.span, dynamic_receiver(&m.object), type_only, false),
            AstKind::Class(c) => {
                if let Some(heritage) = &c.heritage { let (selection, dynamic) = callee(&heritage.expression); self.add(Extends, heritage.expression.span(), selection, dynamic, false, false); }
            }
            AstKind::TSClassImplements(i) => self.add(Implements, i.span, type_selection(&i.expression), false, true, false),
            AstKind::TSInterfaceHeritage(i) => self.add(Extends, i.span, type_selection(&i.type_name), false, true, false),
            AstKind::TSTypeReference(t) if self.stack.iter().any(|n| match n {
                AstKind::PropertyDefinition(p) => p.type_annotation.as_ref().is_some_and(|a| contains(a.span, t.span)),
                AstKind::AccessorProperty(p) => p.type_annotation.as_ref().is_some_and(|a| contains(a.span, t.span)),
                AstKind::TSPropertySignature(p) => p.type_annotation.as_ref().is_some_and(|a| contains(a.span, t.span)),
                AstKind::TSIndexSignature(p) => contains(p.type_annotation.span, t.span),
                AstKind::FormalParameter(p) if p.accessibility.is_some() || p.readonly || p.r#override => p.type_annotation.as_ref().is_some_and(|a| contains(a.span, t.span)),
                _ => false,
            }) => self.add(TypeUse, t.span, type_selection(&t.type_name), false, true, false),
            AstKind::ImportDeclaration(i) => self.add(Import, i.span, i.source.span, false, i.import_kind == ImportOrExportKind::Type, false),
            AstKind::ImportSpecifier(i) => self.add(Import, i.span, i.local.span, false, i.import_kind == ImportOrExportKind::Type || self.stack.iter().any(|n| matches!(n, AstKind::ImportDeclaration(d) if d.import_kind == ImportOrExportKind::Type)), true),
            AstKind::ImportDefaultSpecifier(i) => self.add(Import, i.span, i.local.span, false, self.stack.iter().any(|n| matches!(n, AstKind::ImportDeclaration(d) if d.import_kind == ImportOrExportKind::Type)), true),
            AstKind::ImportNamespaceSpecifier(i) => self.add(Import, i.span, i.local.span, false, self.stack.iter().any(|n| matches!(n, AstKind::ImportDeclaration(d) if d.import_kind == ImportOrExportKind::Type)), true),
            AstKind::TSImportEqualsDeclaration(i) => self.add(Import, i.span, i.id.span, false, i.import_kind == ImportOrExportKind::Type, true),
            AstKind::ExportFromDeclaration(e) => self.add(Reexport, e.span, e.source.span, false, e.export_kind == ImportOrExportKind::Type, false),
            AstKind::ExportAllDeclaration(e) => self.add(Reexport, e.span, e.source.span, false, e.export_kind == ImportOrExportKind::Type, false),
            AstKind::ExportSpecifier(e) => self.add(Reexport, e.span, e.local.span(), false, e.export_kind == ImportOrExportKind::Type || self.stack.iter().any(|n| matches!(n, AstKind::ExportNamedDeclaration(d) if d.export_kind == ImportOrExportKind::Type) || matches!(n, AstKind::ExportFromDeclaration(d) if d.export_kind == ImportOrExportKind::Type)), true),
            AstKind::JSXOpeningElement(e) => {
                let selection = match &e.name { JSXElementName::IdentifierReference(i) => Some(i.span), JSXElementName::MemberExpression(m) => Some(m.property.span), _ => None };
                if let Some(selection) = selection { self.add(JsxComponent, e.name.span(), selection, false, false, false); }
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
