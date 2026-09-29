//! Ruff AST 负责对象身份与内容；投影从不改写交给语言服务的原文。

use super::{
    failure,
    known::{Known, Proofs},
    Result,
};
use crate::structure_review::{
    code_graph::{CodeNode, CodeNodeData, CodeReference, LanguageId, NodeId, ReferenceTarget},
    source_text::{LineBreakRules, SourceText},
};
use ruff_python_ast::{self as ast, Expr, PySourceType, PythonVersion, Stmt};
use ruff_python_codegen::{Generator, Indentation, Mode};
use ruff_python_parser::{parse_unchecked, ParseOptions};
use ruff_source_file::LineEnding;
use ruff_text_size::{Ranged, TextRange};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// 原文、AST 与声明定位属于同一份文档；语义证明只重建对象，不重新读取源码。
pub(super) struct Document {
    pub root: Option<NodeId>,
    pub nodes: Vec<CodeNode>,
    pub body: ast::Suite,
    pub source: SourceText,
    pub declarations: Vec<Declaration>,
    pub owners: BTreeMap<usize, NodeId>,
    pub bindings: super::bindings::Bindings,
    relative: String,
    stub: bool,
}

/// 精确声明选区用于核对 ty 返回的位置，不把函数体任意坐标当作函数定义。
#[derive(Clone)]
pub(super) struct Declaration {
    pub selection: TextRange,
    pub id: NodeId,
    pub eligible: bool,
}

impl Document {
    /// 不接受恢复 AST：普通解析错误和目标版本不支持的语法均为硬失败。
    pub fn parse(relative: &Path, source: &str, minor: u8) -> Result<Self> {
        let stub = relative.extension().is_some_and(|ext| ext == "pyi");
        let parsed = parse_unchecked(
            source,
            ParseOptions::from(if stub {
                PySourceType::Stub
            } else {
                PySourceType::Python
            })
            .with_target_version(PythonVersion { major: 3, minor }),
        );
        if !parsed.errors().is_empty() || !parsed.unsupported_syntax_errors().is_empty() {
            return Err(failure(format!(
                "{}：Python 语法错误：{:?} {:?}",
                relative.display(),
                parsed.errors(),
                parsed.unsupported_syntax_errors()
            )));
        }
        let ast::Mod::Module(module) = parsed.into_syntax() else {
            return Err(failure("Python 文件不是模块"));
        };
        let mut document = Self {
            root: None,
            nodes: vec![],
            bindings: super::bindings::Bindings::new(&module.body, source.len()),
            body: module.body,
            source: SourceText::new(
                source,
                LineBreakRules {
                    lone_cr: true,
                    unicode_separators: false,
                },
            ),
            declarations: vec![],
            owners: BTreeMap::new(),
            relative: relative.to_string_lossy().replace('\\', "/"),
            stub,
        };
        document.rebuild(&Proofs::new());
        Ok(document)
    }

    /// 标准库证明会改变少量对象角色；身份仍仅取仓库路径、词法 owner 与声明序号。
    pub fn rebuild(&mut self, proofs: &Proofs) {
        if self.body.is_empty() {
            return;
        }
        let file_id = NodeId(format!("python:repo:{}", encode(&self.relative)));
        let mut file = data(file_id.clone(), "file", &self.relative);
        file.source = Some(self.source.to_string());
        let mut builder = Builder {
            source: &self.source,
            proofs,
            stub: self.stub,
            nodes: vec![],
            ordinals: BTreeMap::new(),
            declarations: vec![],
            owners: BTreeMap::new(),
            groups: vec![],
        };
        let (projection, children) = builder.suite(&self.body, &file_id, false, true);
        file.contains = children;
        let mut execution = data(
            NodeId(format!("{}:module_body", file_id.0)),
            "module_body",
            "module_body",
        );
        execution.code = Some(suite(&projection));
        execution.source = Some(self.source.to_string());
        file.contains.push(execution.id.clone());
        builder.nodes.push(CodeNode::ExecutionRegion(execution));
        builder.nodes.push(CodeNode::Container(file));
        self.root = Some(file_id);
        self.bindings.set_groups(&builder.groups);
        self.nodes = builder.nodes;
        self.declarations = builder.declarations;
        self.owners = builder.owners;
    }
}

/// 只在函数外建立对象；函数内部的完整 AST 留在该 Callable 的内容中。
struct Builder<'a> {
    source: &'a str,
    proofs: &'a Proofs,
    stub: bool,
    nodes: Vec<CodeNode>,
    ordinals: BTreeMap<(NodeId, String, String), usize>,
    declarations: Vec<Declaration>,
    owners: BTreeMap<usize, NodeId>,
    groups: Vec<Vec<TextRange>>,
}

impl Builder<'_> {
    /// 返回有序比较套件与直接包含对象；控制结构保持原状，声明替换为不可与源码碰撞的锚点。
    fn suite(
        &mut self,
        body: &[Stmt],
        owner: &NodeId,
        class: bool,
        direct: bool,
    ) -> (Vec<Stmt>, Vec<NodeId>) {
        let mut projection = Vec::new();
        let mut children = Vec::new();
        let mut properties = BTreeSet::new();
        let mut index = 0;
        while index < body.len() {
            let statement = &body[index];
            match statement {
                Stmt::FunctionDef(function) => {
                    let role = self.role(function, &properties, direct);
                    if role == Some("getter") {
                        properties.insert(function.name.to_string());
                    } else if role.is_none() {
                        properties.remove(function.name.as_str());
                    }
                    let count = if direct {
                        self.overload_group(&body[index..])
                    } else {
                        0
                    };
                    let count = count.max(1);
                    let id =
                        self.function(&body[index..index + count], owner, class, role, count > 1);
                    children.push(id.clone());
                    for part in 0..count {
                        projection.push(anchor(&id, part));
                    }
                    index += count;
                    continue;
                }
                Stmt::ClassDef(value) => {
                    properties.remove(value.name.as_str());
                    let id = self.class(value, owner, class);
                    children.push(id.clone());
                    projection.push(anchor(&id, 0));
                }
                Stmt::TypeAlias(value) => {
                    if let Expr::Name(name) = value.name.as_ref() {
                        let id = self.alias(statement, name.id.as_str(), name.range, owner, class);
                        children.push(id.clone());
                        projection.push(anchor(&id, 0));
                    }
                }
                Stmt::AnnAssign(value)
                    if self.proofs.get(&value.annotation.start().to_usize())
                        == Some(&Known::TypeAlias)
                        && matches!(value.target.as_ref(), Expr::Name(_)) =>
                {
                    let Expr::Name(name) = value.target.as_ref() else {
                        unreachable!()
                    };
                    let id = self.alias(statement, name.id.as_str(), name.range, owner, class);
                    children.push(id.clone());
                    projection.push(anchor(&id, 0));
                }
                Stmt::AnnAssign(value)
                    if class && direct && matches!(value.target.as_ref(), Expr::Name(_)) =>
                {
                    let Expr::Name(name) = value.target.as_ref() else {
                        unreachable!()
                    };
                    properties.remove(name.id.as_str());
                    let (id, part) = self.field(
                        owner,
                        name.id.as_str(),
                        name.range,
                        Some(&expression(&value.annotation)),
                        value.value.as_deref().map(expression).as_deref(),
                        statement.range(),
                        true,
                    );
                    self.owners.insert(statement.start().to_usize(), id.clone());
                    children.push(id.clone());
                    projection.push(anchor(&id, part));
                }
                Stmt::Assign(value) if class && direct => {
                    let mut names = Vec::new();
                    for target in &value.targets {
                        field_names(target, &mut names);
                    }
                    let exclusive =
                        value.targets.len() == 1 && matches!(value.targets[0], Expr::Name(_));
                    for name in names {
                        properties.remove(name.id.as_str());
                        let (id, part) = self.field(
                            owner,
                            name.id.as_str(),
                            name.range,
                            None,
                            exclusive.then(|| expression(&value.value)).as_deref(),
                            statement.range(),
                            true,
                        );
                        children.push(id.clone());
                        if exclusive {
                            self.owners.insert(statement.start().to_usize(), id.clone());
                            projection.push(anchor(&id, part));
                        }
                    }
                    if !exclusive {
                        projection.push(statement.clone());
                    }
                }
                _ => {
                    // 控制分支可能改变 property 链绑定；不能跨分支猜测访问器身份。
                    let mut projected = statement.clone();
                    for block in blocks(&mut projected) {
                        properties.clear();
                        let (statements, nested) = self.suite(block, owner, class, false);
                        *block = statements.into_iter().collect();
                        children.extend(nested);
                    }
                    projection.push(projected);
                }
            }
            index += 1;
        }
        let mut seen = BTreeSet::new();
        children.retain(|id| seen.insert(id.clone()));
        (projection, children)
    }

    /// 同名普通声明按序号保留，字段则另按类与名称聚合。
    fn id(&mut self, owner: &NodeId, kind: &str, name: &str) -> NodeId {
        let ordinal = self
            .ordinals
            .entry((owner.clone(), kind.into(), name.into()))
            .or_default();
        let id = NodeId(format!("{}:{}:{}:{}", owner.0, kind, encode(name), ordinal));
        *ordinal += 1;
        id
    }

    /// getter 必须有 builtin 证明，后续访问器必须连接同一词法位置仍有效的 property。
    fn role(
        &self,
        function: &ast::StmtFunctionDef,
        properties: &BTreeSet<String>,
        direct: bool,
    ) -> Option<&'static str> {
        if function.decorator_list.len() != 1 {
            return None;
        }
        let expression = &function.decorator_list[0].expression;
        if self.proofs.get(&expression.start().to_usize()) == Some(&Known::Property) {
            return Some("getter");
        }
        if direct {
            if let Expr::Attribute(value) = expression {
                if matches!(value.value.as_ref(), Expr::Name(name) if name.id.as_str() == function.name.as_str() && properties.contains(name.id.as_str()))
                {
                    return match value.attr.as_str() {
                        "setter" => Some("setter"),
                        "deleter" => Some("deleter"),
                        _ => None,
                    };
                }
            }
        }
        None
    }

    /// 至少两个连续标准 overload，且普通 Python 文件需要对应实现；条件分支不调用此方法。
    fn overload_group(&self, body: &[Stmt]) -> usize {
        let Some(Stmt::FunctionDef(first)) = body.first() else {
            return 0;
        };
        let signatures = body.iter().take_while(|statement| matches!(statement, Stmt::FunctionDef(value) if value.name.as_str() == first.name.as_str() && value.is_async == first.is_async && self.is_overload(value) && declaration_body(&value.body))).count();
        if signatures < 2 {
            return 0;
        }
        let implementation = body.get(signatures).is_some_and(|statement| matches!(statement, Stmt::FunctionDef(value) if value.name.as_str() == first.name.as_str() && value.is_async == first.is_async && !self.is_overload(value) && self.known_decorators(value)));
        if implementation {
            signatures + 1
        } else if self.stub {
            signatures
        } else {
            0
        }
    }

    /// 单靠名为 overload 的装饰器不构成合法声明组。
    fn is_overload(&self, function: &ast::StmtFunctionDef) -> bool {
        self.known_decorators(function)
            && function.decorator_list.iter().any(|decorator| {
                self.proofs.get(&decorator.expression.start().to_usize()) == Some(&Known::Overload)
            })
    }

    /// 只把已证明不改变源码调用身份的声明装饰器视为合格目标。
    fn known_decorators(&self, function: &ast::StmtFunctionDef) -> bool {
        function.decorator_list.iter().all(|decorator| {
            matches!(
                self.proofs.get(&decorator.expression.start().to_usize()),
                Some(Known::Overload | Known::StaticMethod | Known::ClassMethod)
            )
        })
    }

    /// 一组源码片段共享对象，各片段仍在父套件中留下独立有序锚点。
    fn function(
        &mut self,
        statements: &[Stmt],
        owner: &NodeId,
        class: bool,
        role: Option<&str>,
        grouped: bool,
    ) -> NodeId {
        let Stmt::FunctionDef(first) = &statements[0] else {
            unreachable!()
        };
        let kind = role.unwrap_or(if class {
            "method"
        } else if first.is_async {
            "async_function"
        } else {
            "function"
        });
        let id = self.id(owner, kind, first.name.as_str());
        let mut node = data(id.clone(), kind, first.name.as_str());
        let mut signatures = Vec::new();
        let mut sources = Vec::new();
        let mut selections = Vec::new();
        for statement in statements {
            let Stmt::FunctionDef(value) = statement else {
                unreachable!()
            };
            let mut header = value.clone();
            header.body.clear();
            signatures.push(normalize(&Stmt::FunctionDef(header)));
            sources.push(self.source[statement.range()].to_owned());
            self.declarations.push(Declaration {
                selection: value.name.range,
                id: id.clone(),
                eligible: role.is_none() && self.known_decorators(value),
            });
            self.owners.insert(statement.start().to_usize(), id.clone());
            selections.push(value.name.range);
            if !self.is_overload(value) {
                node.code = if self.stub {
                    stub_content(&value.body)
                } else {
                    Some(suite(&value.body))
                };
            }
        }
        node.signature = Some(signatures.join("\n"));
        node.source = Some(sources.join("\n"));
        if class {
            member_of(&mut node, owner);
        }
        if grouped {
            self.groups.push(selections);
        }
        self.nodes.push(CodeNode::Callable(node));
        id
    }

    /// 类头属于 Type，类体只保留自身有序内容；独立成员由锚点隔离。
    fn class(&mut self, class: &ast::StmtClassDef, owner: &NodeId, member: bool) -> NodeId {
        let id = self.id(owner, "class", class.name.as_str());
        let mut node = data(id.clone(), "class", class.name.as_str());
        self.declarations.push(Declaration {
            selection: class.name.range,
            id: id.clone(),
            eligible: class.decorator_list.is_empty(),
        });
        self.owners.insert(class.start().to_usize(), id.clone());
        let mut header = class.clone();
        header.body.clear();
        node.signature = Some(normalize(&Stmt::ClassDef(header)));
        node.source = Some(self.source[class.range].to_owned());
        let (code, members) = self.suite(&class.body, &id, true, true);
        node.code = Some(suite(&code));
        node.contains = members;
        let receivers = class
            .body
            .iter()
            .filter_map(|statement| {
                let Stmt::FunctionDef(method) = statement else {
                    return None;
                };
                let object = self
                    .owners
                    .get(&method.start().to_usize())
                    .and_then(|id| self.nodes.iter().find(|node| &node.data().id == id))?;
                (method.decorator_list.is_empty()
                    || matches!(
                        object.data().native_kind.as_str(),
                        "getter" | "setter" | "deleter"
                    ))
                .then_some(method.start().to_usize())
            })
            .collect::<BTreeSet<_>>();
        let descriptors = self
            .nodes
            .iter()
            .filter(|object| {
                node.contains.contains(&object.data().id)
                    && matches!(
                        object.data().native_kind.as_str(),
                        "getter" | "setter" | "deleter"
                    )
            })
            .filter_map(|node| node.data().name.clone())
            .collect::<BTreeSet<_>>();
        for field in super::fields::instance_fields(class, &receivers) {
            if descriptors.contains(&field.name)
                || self.proofs.get(&field.selection.start().to_usize())
                    == Some(&Known::NotInstanceField)
            {
                continue;
            }
            let (field_id, _) = self.field(
                &id,
                &field.name,
                field.selection,
                field.annotation.as_deref(),
                None,
                field.source,
                false,
            );
            if !node.contains.contains(&field_id) {
                node.contains.push(field_id);
            }
        }
        if member {
            member_of(&mut node, owner);
        }
        self.nodes.push(CodeNode::Type(node));
        id
    }

    /// 类型别名必须有显式语法或 TypeAlias 证明，不根据大写名称与推断类型升级。
    fn alias(
        &mut self,
        statement: &Stmt,
        name: &str,
        selection: TextRange,
        owner: &NodeId,
        member: bool,
    ) -> NodeId {
        let id = self.id(owner, "type_alias", name);
        let mut node = data(id.clone(), "type_alias", name);
        node.signature = Some(normalize(statement));
        node.source = Some(self.source[statement.range()].to_owned());
        self.declarations.push(Declaration {
            selection,
            id: id.clone(),
            eligible: true,
        });
        self.owners.insert(statement.start().to_usize(), id.clone());
        if member {
            member_of(&mut node, owner);
        }
        self.nodes.push(CodeNode::Type(node));
        id
    }

    /// 字段按类与名字去重；方法 RHS 不传入此处，多个类初始化器按出现顺序保留。
    #[allow(clippy::too_many_arguments)]
    fn field(
        &mut self,
        owner: &NodeId,
        name: &str,
        selection: TextRange,
        annotation: Option<&str>,
        initializer: Option<&str>,
        source: TextRange,
        class: bool,
    ) -> (NodeId, usize) {
        let id = NodeId(format!("{}:field:{}:0", owner.0, encode(name)));
        let part = self
            .declarations
            .iter()
            .filter(|declaration| declaration.id == id)
            .count();
        self.declarations.push(Declaration {
            selection,
            id: id.clone(),
            eligible: true,
        });
        let index = if let Some(index) = self.nodes.iter().position(|node| node.data().id == id) {
            index
        } else {
            let mut node = data(id.clone(), "field", name);
            member_of(&mut node, owner);
            self.nodes.push(CodeNode::Value(node));
            self.nodes.len() - 1
        };
        let node = node_data_mut(&mut self.nodes[index]);
        let signature =
            annotation.map_or_else(|| name.to_owned(), |value| format!("{name}: {value}"));
        if node.signature.as_deref().is_none_or(|value| value == name) {
            node.signature = Some(signature);
        } else if annotation.is_some()
            && !node
                .signature
                .as_ref()
                .unwrap()
                .lines()
                .any(|line| line == signature)
        {
            append(&mut node.signature, &signature);
        }
        if let Some(initializer) = initializer {
            append(&mut node.code, initializer);
        }
        append(&mut node.source, &self.source[source]);
        node.extensions.insert(
            if class {
                "python.class_field"
            } else {
                "python.instance_field"
            }
            .into(),
            serde_json::json!(true),
        );
        (id, part)
    }
}

/// 每个片段只添加一次，原文不参与比较，但可完整展示多处声明。
fn append(target: &mut Option<String>, value: &str) {
    if let Some(target) = target {
        target.push('\n');
        target.push_str(value);
    } else {
        *target = Some(value.into());
    }
}

/// 直接类体的解构与多目标赋值仅提供字段声明，完整 RHS 仍留在类投影。
fn field_names<'a>(target: &'a Expr, output: &mut Vec<&'a ast::ExprName>) {
    match target {
        Expr::Name(name) => output.push(name),
        Expr::Tuple(value) => {
            for element in &value.elts {
                field_names(element, output);
            }
        }
        Expr::List(value) => {
            for element in &value.elts {
                field_names(element, output);
            }
        }
        Expr::Starred(value) => field_names(&value.value, output),
        _ => {}
    }
}

/// 声明式套件只包含文档与空实现标记，不把任意执行体纳入 overload 组。
fn declaration_body(body: &[Stmt]) -> bool {
    body.iter().all(|stmt| matches!(stmt, Stmt::Pass(_)) || matches!(stmt, Stmt::Expr(value) if matches!(value.value.as_ref(), Expr::EllipsisLiteral(_) | Expr::StringLiteral(_))))
}

/// .pyi 的省略实现不是运行时代码，有意义的 docstring 仍参与自身内容比较。
fn stub_content(body: &[Stmt]) -> Option<String> {
    let statements = body.iter().filter(|stmt| !matches!(stmt, Stmt::Pass(_)) && !matches!(stmt, Stmt::Expr(value) if matches!(value.value.as_ref(), Expr::EllipsisLiteral(_)))).cloned().collect::<Vec<_>>();
    (!statements.is_empty()).then(|| suite(&statements))
}

/// 成员关系与 contains 分别保留语义方向与声明归属。
fn member_of(node: &mut CodeNodeData, owner: &NodeId) {
    node.references.push(CodeReference {
        semantic: "code.member_of".into(),
        target: ReferenceTarget::Node(owner.clone()),
    });
}

/// 固定 AST 模式保留字符串和 docstring，忽略普通注释与排版。
pub(super) fn normalize(statement: &Stmt) -> String {
    Generator::new(&Indentation::default(), LineEnding::Lf)
        .with_mode(Mode::AstUnparse)
        .stmt(statement)
}

/// 表达式和语句共享同一规范化规则。
pub(super) fn expression(value: &Expr) -> String {
    Generator::new(&Indentation::default(), LineEnding::Lf)
        .with_mode(Mode::AstUnparse)
        .expr(value)
}

/// 套件以顺序串接，不能用无序 contains 表达执行先后。
fn suite(body: &[Stmt]) -> String {
    body.iter().map(normalize).collect::<Vec<_>>().join("\n")
}

/// 编码分隔符，避免路径、名称和词法 owner 发生身份碰撞。
fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// 规范字段不包含行号、绝对路径或工具生成的符号编号。
fn data(id: NodeId, kind: &str, name: &str) -> CodeNodeData {
    CodeNodeData {
        id,
        language: LanguageId("python".into()),
        native_kind: kind.into(),
        name: Some(name.into()),
        signature: None,
        code: None,
        source: None,
        contains: vec![],
        references: vec![],
        extensions: BTreeMap::new(),
    }
}

/// 仅遍历控制结构套件，不进入由专属对象处理的函数与类。
fn blocks(statement: &mut Stmt) -> Vec<&mut ast::Suite> {
    match statement {
        Stmt::If(value) => {
            let mut bodies = vec![&mut value.body];
            bodies.extend(
                value
                    .elif_else_clauses
                    .iter_mut()
                    .map(|clause| &mut clause.body),
            );
            bodies
        }
        Stmt::For(value) => vec![&mut value.body, &mut value.orelse],
        Stmt::While(value) => vec![&mut value.body, &mut value.orelse],
        Stmt::With(value) => vec![&mut value.body],
        Stmt::Match(value) => value.cases.iter_mut().map(|case| &mut case.body).collect(),
        Stmt::Try(value) => {
            let mut bodies = vec![&mut value.body, &mut value.orelse, &mut value.finalbody];
            bodies.extend(value.handlers.iter_mut().map(|handler| {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                &mut handler.body
            }));
            bodies
        }
        _ => vec![],
    }
}

/// 锚点不是合法 Python 标识符，只存在于比较表示，从不交给 LSP 或写回源码。
fn anchor(id: &NodeId, part: usize) -> Stmt {
    Stmt::Expr(ast::StmtExpr {
        node_index: ast::AtomicNodeIndex::NONE,
        range: Default::default(),
        value: Box::new(Expr::Name(ast::ExprName {
            node_index: ast::AtomicNodeIndex::NONE,
            range: Default::default(),
            id: format!("<declare {}#{part}>", id.0).into(),
            ctx: ast::ExprContext::Load,
        })),
    })
}

/// 内部构建阶段修改通用字段，公共图消费者仍只获得只读对象。
pub(super) fn node_data_mut(node: &mut CodeNode) -> &mut CodeNodeData {
    match node {
        CodeNode::Container(data)
        | CodeNode::Type(data)
        | CodeNode::Callable(data)
        | CodeNode::Value(data)
        | CodeNode::ExecutionRegion(data)
        | CodeNode::Extension(data) => data,
    }
}
