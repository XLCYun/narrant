//! 只保存本文件词法绑定与公开别名路线；跨文件的模块身份仍由语言服务确认。
use oxc_ast::{ast::*, AstKind};
use oxc_ast_visit::Visit;
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};

/// 一条导入或公开导出路线；binding 是 Oxc 的本文件声明位置，不是跨文件符号身份。
#[derive(Clone, Debug)]
pub(crate) struct Alias {
    pub import: bool,
    pub binding: Option<Span>,
    /// 别名语法的定义查询位置，与图节点及本地目标声明的位置无关。
    pub selection: Option<Span>,
    pub exported: Option<String>,
    pub name: String,
    pub source: Option<Span>,
    pub type_only: bool,
    pub scope: Span,
}

/// 固定语义构建只用于读取词法绑定，避免按同名标识符误伤局部 shadow。
pub(super) fn binding(scoping: &Scoping, reference: &IdentifierReference<'_>) -> Option<Span> {
    let resolved = scoping.get_reference(reference.reference_id.get()?);
    // 类型导入的非法值读取可能没有 resolved symbol，但引用的词法作用域仍可靠。
    resolved
        .symbol_id()
        .or_else(|| scoping.find_binding(resolved.scope_id(), reference.name))
        .map(|id| scoping.symbol_span(id))
}

/// 公共别名提取保留公开名字、原始源串位置和所属模块。
pub(super) struct Collector<'s, 'a> {
    scoping: &'s Scoping,
    stack: Vec<AstKind<'a>>,
    pub aliases: Vec<Alias>,
    root: Span,
}
impl<'s, 'a> Collector<'s, 'a> {
    /// 借用当前原始 AST 的词法数据，不创建第二套作用域解析器。
    pub fn new(scoping: &'s Scoping, root: Span) -> Self {
        Self {
            scoping,
            stack: vec![],
            aliases: vec![],
            root,
        }
    }

    /// Ambient module/namespace 中的导出只属于该模块，不能混入同文件其他片段。
    fn scope(&self) -> Span {
        self.stack
            .iter()
            .rev()
            .find_map(|node| match node {
                AstKind::TSExternalModuleDeclaration(d) => Some(d.span),
                AstKind::TSNamespaceDeclaration(d) => Some(d.span),
                _ => None,
            })
            .unwrap_or(self.root)
    }

    /// 直接导出的值声明优先于星号导出，防止邻接 type-only 星号掩盖真实运行时出口。
    fn value_export(&mut self, name: &str, span: Option<Span>, selection: Option<Span>) {
        self.aliases.push(Alias {
            import: false,
            binding: span,
            selection,
            exported: Some(name.into()),
            name: name.into(),
            source: None,
            type_only: false,
            scope: self.scope(),
        });
    }

    /// 导入本地名字由词法 symbol span 标识，公开导入名字保持独立。
    fn import(&mut self, local: &BindingIdentifier<'_>, name: String, type_only: bool) {
        let Some(parent) = self.stack.iter().rev().find_map(|node| match node {
            AstKind::ImportDeclaration(d) => Some(*d),
            _ => None,
        }) else {
            return;
        };
        self.aliases.push(Alias {
            import: true,
            binding: Some(local.span),
            selection: Some(local.span),
            exported: None,
            name,
            source: Some(parent.source.span),
            type_only: type_only || parent.import_kind == ImportOrExportKind::Type,
            scope: self.scope(),
        });
    }
}
impl<'a> Visit<'a> for Collector<'_, 'a> {
    /// 导入/导出语法提供路由，词法引用只用于区分同文件真实绑定。
    fn enter_node(&mut self, node: AstKind<'a>) {
        match node {
            AstKind::ImportSpecifier(i) => self.import(
                &i.local,
                i.imported.name().to_string(),
                i.import_kind == ImportOrExportKind::Type,
            ),
            AstKind::ImportDefaultSpecifier(i) => self.import(&i.local, "default".into(), false),
            AstKind::ImportNamespaceSpecifier(i) => self.import(&i.local, "*".into(), false),
            AstKind::TSImportEqualsDeclaration(i) => {
                let source = match &i.module_reference {
                    TSModuleReference::ExternalModuleReference(r) => Some(r.expression.span),
                    _ => None,
                };
                self.aliases.push(Alias {
                    import: true,
                    binding: Some(i.id.span),
                    selection: Some(i.id.span),
                    exported: None,
                    name: "*".into(),
                    source,
                    type_only: i.import_kind == ImportOrExportKind::Type,
                    scope: self.scope(),
                });
            }
            AstKind::ExportSpecifier(e) => {
                let source = self.stack.iter().rev().find_map(|node| match node {
                    AstKind::ExportFromDeclaration(d) => Some(d.source.span),
                    _ => None,
                });
                let type_only = e.export_kind == ImportOrExportKind::Type
                    || self.stack.iter().any(|node| match node {
                        AstKind::ExportFromDeclaration(d) => {
                            d.export_kind == ImportOrExportKind::Type
                        }
                        AstKind::ExportNamedDeclaration(d) => {
                            d.export_kind == ImportOrExportKind::Type
                        }
                        _ => false,
                    });
                let local = match &e.local {
                    ModuleExportName::IdentifierReference(id) => binding(self.scoping, id),
                    _ => None,
                };
                self.aliases.push(Alias {
                    import: false,
                    binding: local,
                    selection: Some(e.local.span()),
                    exported: Some(e.exported.name().to_string()),
                    name: e.local.name().to_string(),
                    source,
                    type_only,
                    scope: self.scope(),
                });
            }
            AstKind::ExportAllDeclaration(e) => self.aliases.push(Alias {
                import: false,
                binding: None,
                selection: None,
                exported: e.exported.as_ref().map(|n| n.name().to_string()),
                name: "*".into(),
                source: Some(e.source.span),
                type_only: e.export_kind == ImportOrExportKind::Type,
                scope: self.scope(),
            }),
            AstKind::ExportDefaultDeclaration(d) => {
                let local = match &d.declaration {
                    ExportDefaultDeclarationKind::Identifier(id) => binding(self.scoping, id),
                    _ => None,
                };
                let selection = match &d.declaration {
                    ExportDefaultDeclarationKind::Identifier(id) => Some(id.span),
                    _ => None,
                };
                self.value_export("default", local, selection);
            }
            AstKind::ExportDeclaration(e) => match &e.declaration {
                Declaration::FunctionDeclaration(d) => {
                    if let Some(id) = &d.id {
                        self.value_export(&id.name, Some(id.span), None);
                    }
                }
                Declaration::ClassDeclaration(d) => {
                    if let Some(id) = &d.id {
                        self.value_export(&id.name, Some(id.span), None);
                    }
                }
                Declaration::VariableDeclaration(d) => {
                    for variable in &d.declarations {
                        for id in variable.id.get_binding_identifiers() {
                            self.value_export(&id.name, Some(id.span), None);
                        }
                    }
                }
                Declaration::TSEnumDeclaration(d) => {
                    self.value_export(&d.id.name, Some(d.id.span), None)
                }
                Declaration::TSNamespaceDeclaration(d) => {
                    self.value_export(&d.id.name, Some(d.id.span), None)
                }
                _ => {}
            },
            _ => {}
        }
        self.stack.push(node);
    }

    /// 离开模块恢复公开别名的所属范围。
    fn leave_node(&mut self, _: AstKind<'a>) {
        self.stack.pop();
    }
}
