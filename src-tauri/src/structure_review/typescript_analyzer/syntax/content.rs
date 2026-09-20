//! 表达式完整比较；声明容器只扣除 AST 成员列表中的独立对象，标点交给 Oxc。
use super::{contains, declarations::RawDeclaration};
use oxc_allocator::{Allocator, CloneIn};
use oxc_ast::{ast::*, builder::AstBuilder, AstKind};
use oxc_ast_visit::Visit;
use oxc_codegen::{Codegen, CodegenOptions, CommentOptions, Context, Gen};
use oxc_parser::Token;
use oxc_span::{GetSpan, SourceType, Span};
use std::collections::{BTreeMap, BTreeSet};

/// 规范源码供完整表达式取词；容器签名由裁剪后的 AST 生成，不影响原始源码与位置。
pub(super) struct Content<'s> {
    source: &'s str,
    tokens: &'s [Token],
    signatures: BTreeMap<Span, String>,
}
impl<'s> Content<'s> {
    /// 规范 AST、token 与声明来自同一次解析，独立对象的范围只用于匹配成员。
    pub fn new(
        source: &'s str,
        tokens: &'s [Token],
        program: &Program<'_>,
        declarations: &[RawDeclaration],
    ) -> Self {
        let mut projection = Projection {
            source,
            source_type: program.source_type,
            members: Members(
                declarations
                    .iter()
                    .flat_map(|d| d.parts.iter().map(|p| p.span))
                    .collect(),
            ),
            signatures: BTreeMap::new(),
        };
        projection.visit_program(program);
        Self {
            source,
            tokens,
            signatures: projection.signatures,
        }
    }

    /// 重载按原顺序拼接；函数体、字段初始化器与执行区域完整保留内部表达式。
    pub fn declaration(&self, declaration: &RawDeclaration) -> (String, Option<String>) {
        let mut signatures = Vec::new();
        let mut bodies = Vec::new();
        for part in &declaration.parts {
            let signature = self
                .signatures
                .get(&part.span)
                .cloned()
                .unwrap_or_else(|| self.text(part.span, part.body));
            signatures.push(format!("{}{signature}", declaration.prefix));
            if let Some(body) = part.body {
                bodies.push(self.text(body, None));
            }
        }
        (
            signatures.join("\n"),
            (!bodies.is_empty()).then(|| bodies.join("\n")),
        )
    }

    /// 只分开对象自己的签名与实现，不删内部对象；保留 token 内的字面量和数组空位。
    fn text(&self, span: Span, body: Option<Span>) -> String {
        let start = self
            .tokens
            .partition_point(|token| token.end() <= span.start);
        self.tokens[start..]
            .iter()
            .take_while(|token| token.start() < span.end)
            .filter(|token| {
                !token.span().is_empty()
                    && contains(span, token.span())
                    && !body.is_some_and(|body| contains(body, token.span()))
            })
            .map(|token| &self.source[token.start() as usize..token.end() as usize])
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// 仅匹配成员本身的范围；成员初始化器、参数及数组元素不参与裁剪。
struct Members(BTreeSet<Span>);
impl Members {
    /// 没有独立图节点的成员仍属于容器自身内容。
    fn keep(&self, member: &impl GetSpan) -> bool {
        !self.0.contains(&member.span())
    }

    /// namespace 的语句列表扣除独立声明及执行区域，保留其余未建模代码。
    fn module(&self, body: &mut TSModuleBlock<'_>) {
        body.directives.retain(|directive| self.keep(directive));
        body.body.retain_mut(|statement| {
            if !self.keep(statement) {
                return false;
            }
            match statement {
                Statement::ExportDeclaration(export) => self.declaration(&mut export.declaration),
                Statement::ExportDefaultDeclaration(export) => self.keep(&export.declaration),
                _ => statement
                    .as_declaration_mut()
                    .is_none_or(|declaration| self.declaration(declaration)),
            }
        });
    }

    /// 同一 const 语句可以混合 Callable 与普通值，只删除已经独立建模的绑定。
    fn declaration(&self, declaration: &mut Declaration<'_>) -> bool {
        if let Declaration::VariableDeclaration(variable) = declaration {
            variable.declarations.retain(|binding| self.keep(binding));
            !variable.declarations.is_empty()
        } else {
            self.keep(declaration)
        }
    }

    /// 类型别名仅在自身对象类型及联合、交叉、括号包装中排除独立成员。
    fn alias(&self, annotation: &mut TSType<'_>) {
        match annotation {
            TSType::TSTypeLiteral(body) => body.members.retain(|member| self.keep(member)),
            TSType::TSUnionType(union) => {
                for annotation in &mut union.types {
                    self.alias(annotation);
                }
            }
            TSType::TSIntersectionType(intersection) => {
                for annotation in &mut intersection.types {
                    self.alias(annotation);
                }
            }
            TSType::TSParenthesizedType(parenthesized) => {
                self.alias(&mut parenthesized.type_annotation);
            }
            _ => {}
        }
    }
}

/// 每次只复制一个声明以生成自身签名；表达式和未建模成员保持完整语法树。
struct Projection<'s> {
    source: &'s str,
    source_type: SourceType,
    members: Members,
    signatures: BTreeMap<Span, String>,
}
impl Projection<'_> {
    /// Oxc 负责列表分隔符与格式，沿用文件的 JSX 上下文及字面量源码。
    fn print(&self, node: &impl Gen) -> String {
        let mut codegen = Codegen::new()
            .with_source_text(self.source)
            .with_source_type(self.source_type)
            .with_options(CodegenOptions {
                comments: CommentOptions::disabled(),
                ..CodegenOptions::default()
            });
        node.print(&mut codegen, Context::default());
        codegen.into_source_text().trim().to_owned()
    }
}
impl<'a> Visit<'a> for Projection<'_> {
    /// 只裁剪声明主体的成员列表，不遍历或改写初始化表达式、参数及函数体。
    fn enter_node(&mut self, node: AstKind<'a>) {
        if self.members.keep(&node) {
            return;
        }
        let arena = Allocator::default();
        let signature = match node {
            AstKind::Class(class) => {
                let mut class = class.clone_in(&arena);
                class.body.body.retain(|member| self.members.keep(member));
                self.print(&class)
            }
            AstKind::TSInterfaceDeclaration(interface) => {
                let mut interface = interface.clone_in(&arena);
                interface
                    .body
                    .body
                    .retain(|member| self.members.keep(member));
                self.print(&interface)
            }
            AstKind::TSTypeAliasDeclaration(alias) => {
                let mut alias = alias.clone_in(&arena);
                self.members.alias(&mut alias.type_annotation);
                self.print(&alias)
            }
            AstKind::TSEnumDeclaration(enumeration) => {
                let mut enumeration = enumeration.clone_in(&arena);
                enumeration
                    .body
                    .members
                    .retain(|member| self.members.keep(member));
                self.print(&enumeration)
            }
            AstKind::TSNamespaceDeclaration(namespace) => {
                let mut namespace = namespace.clone_in(&arena);
                match &mut namespace.body {
                    TSNamespaceDeclarationBody::TSModuleBlock(body) => self.members.module(body),
                    TSNamespaceDeclarationBody::TSNamespaceDeclaration(child)
                        if !self.members.keep(child.as_ref()) =>
                    {
                        // namespace A.B 的 B 是独立容器，A 的自身声明等价于空主体。
                        namespace.body = TSNamespaceDeclarationBody::new_ts_module_block(
                            child.span,
                            [],
                            [],
                            &AstBuilder::new(&arena),
                        );
                    }
                    _ => {}
                }
                self.print(&namespace)
            }
            AstKind::TSExternalModuleDeclaration(module) => {
                let mut module = module.clone_in(&arena);
                if let Some(body) = &mut module.body {
                    self.members.module(body);
                }
                self.print(&module)
            }
            AstKind::TSGlobalDeclaration(global) => {
                let mut global = global.clone_in(&arena);
                self.members.module(&mut global.body);
                self.print(&global)
            }
            _ => return,
        };
        self.signatures.insert(node.span(), signature);
    }
}
