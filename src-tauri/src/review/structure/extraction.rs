//! 把可复用 parser 的证据补足为声明、作用域和关系发生点，保持与 UI 无关。

use std::collections::HashMap;

use super::extraction_adapter::{field, text, AdapterOutput, DeclarationSeed};
use super::*;

/// 从单侧原始源码提取自有结构；不会读取文件系统或推测另一版本的缺失。
pub(crate) fn extract_source(path: &str, parsed: &ParsedSource) -> ExtractedFile {
    let adapter = if parsed.language == SourceLanguage::Rust {
        super::extraction_rust::collect(parsed)
    } else {
        super::extraction_typescript::collect(parsed)
    };
    Extractor::new(path, parsed, adapter).extract()
}

/// 候选匹配只消费精确叶子；真正变化判定仍比较包含 AST 形状的完整指纹。
pub(crate) fn fingerprint_tokens(fingerprint: &str) -> Vec<(String, String)> {
    let bytes = fingerprint.as_bytes();
    let mut cursor = 0;
    let mut tokens = Vec::new();
    while cursor < bytes.len() {
        if !bytes[cursor].is_ascii_digit() {
            cursor += 1;
            continue;
        }
        let Some((kind, next)) = read_fingerprint_part(fingerprint, cursor) else {
            break;
        };
        let Some((value, next)) = read_fingerprint_part(fingerprint, next) else {
            break;
        };
        tokens.push((kind.to_owned(), value.to_owned()));
        cursor = next;
    }
    tokens
}

/// 长度以 UTF-8 字节计数，字面量中的括号、数字和冒号不会影响分词。
fn read_fingerprint_part(fingerprint: &str, start: usize) -> Option<(&str, usize)> {
    let colon = fingerprint[start..].find(':')? + start;
    let length = fingerprint[start..colon].parse::<usize>().ok()?;
    let end = colon.checked_add(1)?.checked_add(length)?;
    Some((fingerprint.get(colon + 1..end)?, end))
}

/// 共同遍历统一处理字节坐标、归属和 token，语言 adapter 只提供真实语法位置。
struct Extractor<'a> {
    parsed: &'a ParsedSource,
    adapter: AdapterOutput,
    result: ExtractedFile,
    declarations: HashMap<usize, usize>,
    groups: HashMap<usize, usize>,
    scopes: Vec<usize>,
    scope_nodes: Vec<usize>,
    owners: Vec<Option<usize>>,
    group_owners: Vec<Option<usize>>,
    line_starts: Vec<usize>,
    attachments: HashMap<usize, usize>,
}

impl<'a> Extractor<'a> {
    /// 建立文件内索引；真实源码范围始终来自 parser，不重写恢复节点。
    fn new(path: &str, parsed: &'a ParsedSource, mut adapter: AdapterOutput) -> Self {
        adapter.declarations.sort_by_key(|item| item.node);
        let declarations = adapter
            .declarations
            .iter()
            .enumerate()
            .map(|(index, item)| (item.node, index))
            .collect();
        let groups = adapter
            .impls
            .iter()
            .enumerate()
            .map(|(index, item)| (item.node, index))
            .collect();
        let mut attachments: HashMap<usize, usize> = adapter
            .declarations
            .iter()
            .enumerate()
            .flat_map(|(index, seed)| {
                leading_nodes(parsed, seed.node)
                    .into_iter()
                    .map(move |node| (node, index))
            })
            .collect();
        for group in &adapter.impls {
            for attachment in leading_nodes(parsed, group.node) {
                attachments.insert(attachment, usize::MAX);
            }
        }
        let mut line_starts = vec![0];
        line_starts.extend(
            parsed
                .source
                .match_indices('\n')
                .map(|(index, _)| index + 1),
        );
        Self {
            parsed,
            adapter,
            result: ExtractedFile {
                path: path.to_owned(),
                declarations: Vec::new(),
                impl_groups: Vec::new(),
                scopes: Vec::new(),
                bindings: Vec::new(),
                imports: Vec::new(),
                occurrences: Vec::new(),
                mutations: Vec::new(),
                diagnostics: Vec::new(),
                complete: parsed.diagnostics.is_empty(),
                implicit_prelude: parsed.language != SourceLanguage::Rust
                    || !parsed.syntax.iter().any(|node| {
                        node.native_kind == "attribute"
                            && node
                                .children
                                .iter()
                                .find(|child| {
                                    parsed.syntax[**child].named && !parsed.syntax[**child].extra
                                })
                                .is_some_and(|child| {
                                    parsed.syntax[*child].native_kind == "identifier"
                                        && (matches!(
                                            text(parsed, *child),
                                            "no_implicit_prelude" | "r#no_implicit_prelude"
                                        ) || (text(parsed, *child) == "cfg_attr"
                                            && attribute_disables_prelude(parsed, &node.children)))
                                })
                    }),
            },
            declarations,
            groups,
            scopes: vec![0; parsed.syntax.len()],
            scope_nodes: Vec::new(),
            owners: vec![None; parsed.syntax.len()],
            group_owners: vec![None; parsed.syntax.len()],
            line_starts,
            attachments,
        }
    }

    /// 按语法树顺序建立词法环境，然后输出不包含 parser 句柄的结果。
    fn extract(mut self) -> ExtractedFile {
        self.collect_scopes();
        self.collect_declarations();
        self.collect_impls();
        self.collect_imports();
        self.collect_bindings();
        self.collect_mutations();
        self.collect_occurrences();
        for diagnostic in &self.parsed.diagnostics {
            self.result.diagnostics.push(StructureDiagnostic {
                reason: match diagnostic.kind {
                    SyntaxDiagnosticKind::Error => StructureDiagnosticReason::ParseError,
                    SyntaxDiagnosticKind::Missing => StructureDiagnosticReason::MissingSyntax,
                },
                message: match diagnostic.kind {
                    SyntaxDiagnosticKind::Error => "源码包含无法可靠解析的语法范围",
                    SyntaxDiagnosticKind::Missing => "语法恢复补出了缺失 token，原文未被修改",
                }
                .to_owned(),
                side: None,
                path: Some(self.result.path.clone()),
                range: Some(self.range(diagnostic.range.bytes.clone()).position),
                retryable: false,
            });
        }
        for (index, node) in self.parsed.syntax.iter().enumerate() {
            if node.native_kind == "union_item" {
                self.result.complete = false;
                self.result.diagnostics.push(StructureDiagnostic {
                    reason: StructureDiagnosticReason::UnreliableScope,
                    message: "当前原生词汇尚未覆盖 Rust union；其成员归属不作为可靠声明缺失的依据"
                        .to_owned(),
                    side: None,
                    path: Some(self.result.path.clone()),
                    range: Some(self.node_range(index).position),
                    retryable: false,
                });
            }
        }
        self.result
    }

    /// 以 Unicode 标量的 UTF-16 长度转换列，兼容中文、非 BMP 字符及 CRLF。
    fn range(&self, bytes: std::ops::Range<usize>) -> ExtractedRange {
        let position = |offset: usize| {
            let line = self
                .line_starts
                .partition_point(|start| *start <= offset)
                .saturating_sub(1);
            let column = self.parsed.source[self.line_starts[line]..offset]
                .encode_utf16()
                .count();
            (line as u32 + 1, column as u32 + 1)
        };
        let (start_line, start_column) = position(bytes.start);
        let (end_line, end_column) = position(bytes.end);
        ExtractedRange {
            bytes,
            position: StructureRange {
                start_line,
                start_column,
                end_line,
                end_column,
            },
        }
    }

    /// 将语法节点转换成原文件坐标。
    fn node_range(&self, node: usize) -> ExtractedRange {
        self.range(self.parsed.syntax[node].range.bytes.clone())
    }

    /// 文件根之外的损坏作用域传播不可靠状态，但保留根上其他完整声明。
    fn collect_scopes(&mut self) {
        for (index, node) in self.parsed.syntax.iter().enumerate() {
            let inherited_scope = node.parent.map(|parent| self.scopes[parent]);
            let owner = self
                .declarations
                .get(&index)
                .copied()
                .or_else(|| node.parent.and_then(|parent| self.owners[parent]));
            let group = self
                .groups
                .get(&index)
                .copied()
                .or_else(|| node.parent.and_then(|parent| self.group_owners[parent]));
            self.owners[index] = owner;
            self.group_owners[index] = group;
            if index == 0 || is_scope_kind(&node.native_kind) {
                let reliable = (index == 0 || !node.contains_error)
                    && node.native_kind != "union_item"
                    && inherited_scope.is_none_or(|scope| self.result.scopes[scope].reliable);
                self.scopes[index] = self.result.scopes.len();
                self.scope_nodes.push(index);
                self.result.scopes.push(ExtractedScope {
                    parent: inherited_scope,
                    declaration: owner,
                    range: self.node_range(index),
                    reliable,
                });
            } else {
                self.scopes[index] = inherited_scope.unwrap_or(0);
            }
        }
    }

    /// 显示原生签名，比较使用保留 AST 形状的 token 编码。
    fn collect_declarations(&mut self) {
        for (index, seed) in self.adapter.declarations.iter().enumerate() {
            let node = &self.parsed.syntax[seed.node];
            let mut parent = node.parent.and_then(|parent| self.owners[parent]);
            if matches!(
                node.native_kind.as_str(),
                "required_parameter" | "optional_parameter"
            ) && seed.category == StructureCategory::Field
            {
                parent = super::extraction_adapter::ancestor(
                    self.parsed,
                    seed.node,
                    &["class_declaration", "abstract_class_declaration", "class"],
                )
                .and_then(|class| self.declarations.get(&class).copied());
            }
            let impl_group = self.group_owners[seed.node];
            let name_node = seed.name_node.unwrap_or(seed.node);
            let (documentation, documentation_range, attributes) = self.documentation(seed);
            let mut signature_tokens =
                self.fingerprint(seed.node, index, &seed.bodies, seed.name_node, false);
            let modifiers = self.wrapper_modifiers(seed.node);
            for modifier in &modifiers {
                push_token(
                    &mut signature_tokens,
                    "declaration-modifier",
                    text(self.parsed, *modifier),
                );
            }
            for attribute in attributes {
                signature_tokens.push_str(&self.fingerprint(attribute, index, &[], None, false));
            }
            let body_tokens = seed
                .bodies
                .iter()
                .map(|body| self.fingerprint(*body, index, &[], None, false))
                .collect::<Vec<_>>()
                .join("");
            let signature_end = seed
                .bodies
                .iter()
                .map(|body| self.parsed.syntax[*body].range.bytes.start)
                .min()
                .unwrap_or(node.range.bytes.end);
            let declaration_start = if node.parent.is_some_and(|parent| {
                self.parsed.syntax[parent].native_kind == "ordered_field_declaration_list"
            }) {
                leading_nodes(self.parsed, seed.node)
                    .first()
                    .map(|first| self.parsed.syntax[*first].range.bytes.start)
                    .unwrap_or(node.range.bytes.start)
            } else {
                node.range.bytes.start
            };
            let mut signature = self.parsed.source[declaration_start..signature_end]
                .trim()
                .to_owned();
            if !modifiers.is_empty() {
                signature = format!(
                    "{} {signature}",
                    modifiers
                        .iter()
                        .map(|modifier| text(self.parsed, *modifier))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
            let qualified_name = parent
                .map(|parent| {
                    format!(
                        "{}::{}",
                        self.result.declarations[parent].qualified_name, seed.name
                    )
                })
                .unwrap_or_else(|| seed.name.clone());
            let current_scope = self.scopes[seed.node];
            let mut scope = if is_scope_kind(&node.native_kind) {
                self.result.scopes[current_scope]
                    .parent
                    .unwrap_or(current_scope)
            } else {
                current_scope
            };
            if matches!(
                node.native_kind.as_str(),
                "required_parameter" | "optional_parameter"
            ) && seed.category == StructureCategory::Field
            {
                if let Some(parent) = parent {
                    scope = self.scopes[self.adapter.declarations[parent].node];
                }
            }
            self.result.declarations.push(ExtractedDeclaration {
                name: seed.name.clone(),
                qualified_name,
                native_kind: StructureNativeKind {
                    language: match self.parsed.language {
                        SourceLanguage::TypeScript | SourceLanguage::Tsx => {
                            StructureLanguage::TypeScript
                        }
                        SourceLanguage::JavaScript => StructureLanguage::JavaScript,
                        SourceLanguage::Rust => StructureLanguage::Rust,
                    },
                    category: seed.category,
                    name: seed.native_name,
                },
                parent,
                impl_group,
                scope,
                range: self.range(declaration_start..node.range.bytes.end),
                name_range: self.node_range(name_node),
                signature,
                signature_tokens,
                body_tokens,
                documentation,
                documentation_range,
                exported: seed.exported,
                module_path: if self.parsed.language == SourceLanguage::Rust
                    && seed.native_name == StructureNativeName::Module
                {
                    Some(super::extraction_rust::module_path(
                        self.parsed,
                        &leading_nodes(self.parsed, seed.node),
                    ))
                } else {
                    None
                },
                reliable: !node.contains_error && self.result.scopes[current_scope].reliable,
            });
        }
    }

    /// impl 只拥有自身头部；成员单独计数并等待仓库级类型绑定。
    fn collect_impls(&mut self) {
        for (index, seed) in self.adapter.impls.iter().enumerate() {
            let node = &self.parsed.syntax[seed.node];
            let signature_end = seed
                .body
                .map(|body| self.parsed.syntax[body].range.bytes.start)
                .unwrap_or(node.range.bytes.end);
            let leading = leading_nodes(self.parsed, seed.node);
            let comments: Vec<_> = leading
                .iter()
                .copied()
                .filter(|node| is_comment(&self.parsed.syntax[*node].native_kind))
                .collect();
            let mut documentation = String::new();
            let mut header_tokens = self.fingerprint(
                seed.node,
                usize::MAX,
                &seed.body.into_iter().collect::<Vec<_>>(),
                None,
                false,
            );
            for attachment in leading {
                if is_comment(&self.parsed.syntax[attachment].native_kind) {
                    push_token(&mut documentation, "comment", text(self.parsed, attachment));
                } else {
                    header_tokens.push_str(&self.fingerprint(
                        attachment,
                        usize::MAX,
                        &[],
                        None,
                        false,
                    ));
                }
            }
            documentation.push_str(&self.fingerprint(seed.node, usize::MAX, &[], None, true));
            let documentation_range = comments.first().map(|first| {
                self.range(
                    self.parsed.syntax[*first].range.bytes.start
                        ..self.parsed.syntax[*comments.last().unwrap()]
                            .range
                            .bytes
                            .end,
                )
            });
            self.result.impl_groups.push(ExtractedImplGroup {
                owner: seed.owner.clone(),
                trait_name: seed.trait_name.clone(),
                scope: self.scopes[seed.node],
                parent: node.parent.and_then(|parent| self.owners[parent]),
                range: self.node_range(seed.node),
                signature: self.parsed.source[node.range.bytes.start..signature_end]
                    .trim()
                    .to_owned(),
                header_tokens,
                documentation,
                documentation_range,
                members: self
                    .result
                    .declarations
                    .iter()
                    .enumerate()
                    .filter_map(|(member, declaration)| {
                        (declaration.impl_group == Some(index)
                            && declaration.parent
                                == node.parent.and_then(|parent| self.owners[parent]))
                        .then_some(member)
                    })
                    .collect(),
                reliable: !node.contains_error
                    && self.result.scopes[self.scopes[seed.node]].reliable,
            });
        }
    }

    /// 导入别名既是词法遮蔽证据，也是独立的模块关系发生点。
    fn collect_imports(&mut self) {
        for seed in &self.adapter.imports {
            let scope = self.scopes[seed.node];
            let range = self.node_range(seed.node);
            self.result.imports.push(ExtractedImport {
                module: seed.module.clone(),
                imported: seed.imported.clone(),
                local: seed.local.clone(),
                scope,
                range: range.clone(),
                reexport: seed.reexport,
                type_only: seed.type_only,
            });
            if seed.binding_node.is_some()
                && !seed.local.is_empty()
                && seed.local != "*"
                && seed.local != "_"
            {
                self.result.bindings.push(ExtractedBinding {
                    name: seed.local.clone(),
                    scope,
                    kind: ExtractedBindingKind::Import,
                    declaration: None,
                    range: seed
                        .binding_node
                        .map(|node| self.node_range(node))
                        .unwrap_or_else(|| range.clone()),
                    type_name: None,
                });
            }
            if !seed.module.is_empty() {
                self.result.occurrences.push(ExtractedOccurrence {
                    kind: StructureRelationshipKind::Imports,
                    written_target: seed.module.clone(),
                    declaration: self.owners[seed.node],
                    impl_group: self.group_owners[seed.node],
                    scope,
                    range,
                    callback_range: None,
                    dynamic_this: false,
                    reliable: !self.parsed.syntax[seed.node].contains_error
                        && self.result.scopes[scope].reliable,
                });
            }
        }
    }

    /// 每个具名声明和普通绑定都进入词法环境，局部变量不会变成图节点。
    fn collect_bindings(&mut self) {
        let bindings = super::extraction_bindings::collect(
            self.parsed,
            &self.result,
            &self.scopes,
            &self.scope_nodes,
            &self.owners,
            |node| self.node_range(node),
        );
        self.result.bindings.extend(bindings);
    }

    /// 记录实际写入的词法名称，右侧引用与对象键不会冒充被修改的绑定。
    fn collect_mutations(&mut self) {
        for (node, syntax) in self.parsed.syntax.iter().enumerate() {
            let target = match syntax.native_kind.as_str() {
                "assignment_expression"
                | "augmented_assignment_expression"
                | "compound_assignment_expr" => field(self.parsed, node, "left"),
                "update_expression" => field(self.parsed, node, "argument"),
                "for_in_statement"
                    if !syntax.children.iter().any(|child| {
                        matches!(
                            self.parsed.syntax[*child].native_kind.as_str(),
                            "const" | "let" | "var" | "using"
                        )
                    }) =>
                {
                    field(self.parsed, node, "left")
                }
                _ => None,
            };
            let Some(target) = target else {
                continue;
            };
            let mut names = Vec::new();
            mutation_identifiers(self.parsed, target, &mut names);
            let mut seen = std::collections::HashSet::new();
            for (name, member) in names {
                let name = text(self.parsed, name);
                if seen.insert((name, member.clone())) {
                    self.result.mutations.push(ExtractedMutation {
                        name: name.to_owned(),
                        member,
                        dynamic_this: self.dynamic_this(node),
                        scope: self.scopes[node],
                        range: self.node_range(node),
                    });
                }
            }
        }
    }

    /// export/default/declare 与变量修饰属于被包装的声明，不能在 provider 下探时丢失。
    fn wrapper_modifiers(&self, node: usize) -> Vec<usize> {
        let mut wrappers = Vec::new();
        let mut current = self.parsed.syntax[node].parent;
        while let Some(index) = current {
            if self.parsed.syntax[index].native_kind == "parenthesized_expression" {
                current = self.parsed.syntax[index].parent;
                continue;
            }
            if !matches!(
                self.parsed.syntax[index].native_kind.as_str(),
                "export_statement"
                    | "ambient_declaration"
                    | "lexical_declaration"
                    | "variable_declaration"
            ) {
                break;
            }
            wrappers.push(index);
            current = self.parsed.syntax[index].parent;
        }
        wrappers
            .into_iter()
            .rev()
            .flat_map(|wrapper| self.parsed.syntax[wrapper].children.iter().copied())
            .filter(|child| {
                !self.parsed.syntax[*child].named
                    && matches!(
                        text(self.parsed, *child),
                        "export"
                            | "default"
                            | "declare"
                            | "const"
                            | "let"
                            | "var"
                            | "using"
                            | "await"
                    )
            })
            .collect()
    }

    /// 保留真实调用和类型语法发生点，目标确定性由单侧仓库索引判断。
    fn collect_occurrences(&mut self) {
        for (index, node) in self.parsed.syntax.iter().enumerate() {
            let target = match node.native_kind.as_str() {
                "call_expression" => field(self.parsed, index, "function")
                    .map(|target| (StructureRelationshipKind::Calls, target)),
                "macro_invocation" => Some((StructureRelationshipKind::Calls, index)),
                "new_expression" => field(self.parsed, index, "constructor")
                    .map(|target| (StructureRelationshipKind::Calls, target)),
                "type_identifier" | "scoped_type_identifier" | "nested_type_identifier" => {
                    let parent_kind = node
                        .parent
                        .map(|parent| self.parsed.syntax[parent].native_kind.as_str())
                        .unwrap_or("");
                    if matches!(
                        parent_kind,
                        "scoped_type_identifier" | "nested_type_identifier"
                    ) || self
                        .adapter
                        .declarations
                        .iter()
                        .any(|seed| seed.name_node == Some(index))
                        || self
                            .result
                            .bindings
                            .iter()
                            .any(|binding| binding.range.bytes == node.range.bytes)
                        || self.is_impl_trait_target(index)
                    {
                        None
                    } else {
                        Some((self.type_relation(index), index))
                    }
                }
                "extends_clause" | "extends_type_clause" | "implements_clause" => None,
                _ => None,
            };
            if let Some((kind, target)) = target {
                self.push_occurrence(index, target, kind);
            }
            if node.native_kind == "call_expression"
                && field(self.parsed, index, "function")
                    .is_some_and(|function| self.parsed.syntax[function].native_kind == "import")
            {
                self.push_occurrence(index, index, StructureRelationshipKind::Imports);
            }
            if node.native_kind == "extends_clause"
                || (node.native_kind == "class_heritage"
                    && self.parsed.language == SourceLanguage::JavaScript)
            {
                for child in &node.children {
                    if (node.native_kind == "extends_clause"
                        && self.parsed.syntax[*child].field.as_deref() == Some("value"))
                        || (node.native_kind == "class_heritage"
                            && self.parsed.syntax[*child].named)
                    {
                        self.push_occurrence(*child, *child, StructureRelationshipKind::Extends);
                    }
                }
            }
        }
        for index in 0..self.adapter.impls.len() {
            let seed = &self.adapter.impls[index];
            if let Some(trait_name) = &seed.trait_name {
                let trait_node = field(self.parsed, seed.node, "trait").unwrap_or(seed.node);
                self.result.occurrences.push(ExtractedOccurrence {
                    kind: StructureRelationshipKind::Implements,
                    written_target: trait_name.clone(),
                    declaration: self.owners[seed.node],
                    impl_group: Some(index),
                    scope: self.scopes[seed.node],
                    range: self.node_range(trait_node),
                    callback_range: None,
                    dynamic_this: false,
                    reliable: !self.parsed.syntax[seed.node].contains_error
                        && self.result.scopes[self.scopes[seed.node]].reliable,
                });
            }
        }
    }

    /// 继承、实现中的类型不会错误归类为普通类型引用。
    fn type_relation(&self, node: usize) -> StructureRelationshipKind {
        let mut current = self.parsed.syntax[node].parent;
        while let Some(index) = current {
            match self.parsed.syntax[index].native_kind.as_str() {
                "extends_clause" | "extends_type_clause" => {
                    return StructureRelationshipKind::Extends
                }
                "implements_clause" => return StructureRelationshipKind::Implements,
                "trait_bounds"
                    if self.parsed.syntax[index].parent.is_some_and(|parent| {
                        self.parsed.syntax[parent].native_kind == "trait_item"
                    }) =>
                {
                    return StructureRelationshipKind::Extends
                }
                "type_annotation" | "type_arguments" | "parameters" | "formal_parameters"
                | "block" | "statement_block" => break,
                _ => {}
            }
            current = self.parsed.syntax[index].parent;
        }
        StructureRelationshipKind::TypeReference
    }

    /// impl 的主 trait 由实现块单独提供，泛型参数仍作为普通类型引用。
    fn is_impl_trait_target(&self, node: usize) -> bool {
        let mut current = Some(node);
        while let Some(index) = current {
            if self.parsed.syntax[index].native_kind == "type_arguments" {
                return false;
            }
            if self.parsed.syntax[index].field.as_deref() == Some("trait")
                && self.parsed.syntax[index]
                    .parent
                    .is_some_and(|parent| self.parsed.syntax[parent].native_kind == "impl_item")
            {
                return true;
            }
            if is_scope_kind(&self.parsed.syntax[index].native_kind) {
                break;
            }
            current = self.parsed.syntax[index].parent;
        }
        false
    }

    /// 从发生点向上找匿名执行边界，具名函数自身仍保持直接执行上下文。
    fn push_occurrence(&mut self, node: usize, target: usize, kind: StructureRelationshipKind) {
        let owner = self.owners[node];
        let mut current = self.parsed.syntax[node].parent;
        let mut callback = None;
        while let Some(index) = current {
            if self.declarations.contains_key(&index) {
                break;
            }
            if is_function_kind(&self.parsed.syntax[index].native_kind) {
                let mut parent = self.parsed.syntax[index].parent;
                while parent.is_some_and(|parent| {
                    self.parsed.syntax[parent].native_kind == "parenthesized_expression"
                }) {
                    parent = self.parsed.syntax[parent.unwrap()].parent;
                }
                let covered_named_value =
                    parent.is_some_and(|parent| self.declarations.contains_key(&parent));
                if !covered_named_value {
                    callback = Some(self.node_range(index));
                    break;
                }
            }
            current = self.parsed.syntax[index].parent;
        }
        self.result.occurrences.push(ExtractedOccurrence {
            kind,
            written_target: text(self.parsed, target).to_owned(),
            declaration: owner,
            impl_group: self.group_owners[node],
            scope: self.scopes[node],
            range: self.node_range(node),
            callback_range: callback,
            dynamic_this: self.dynamic_this(node),
            reliable: !self.parsed.syntax[node].contains_error
                && self.result.scopes[self.scopes[node]].reliable
                && super::extraction_adapter::ancestor(self.parsed, node, &["let_chain"]).is_none(),
        });
    }

    /// 箭头沿用词法 this；到达所属方法前遇到普通函数即不能绑定到外层类型。
    fn dynamic_this(&self, node: usize) -> bool {
        if self.parsed.language == SourceLanguage::Rust {
            return false;
        }
        let mut current = self.parsed.syntax[node].parent;
        while let Some(index) = current {
            match self.parsed.syntax[index].native_kind.as_str() {
                "function_declaration"
                | "generator_function_declaration"
                | "function_expression"
                | "generator_function" => return true,
                "method_definition"
                | "class_declaration"
                | "abstract_class_declaration"
                | "class" => return false,
                _ => current = self.parsed.syntax[index].parent,
            }
        }
        false
    }

    /// AST 形状保留 ASI 差异；字符串叶子的精确文本不会被空白归一化破坏。
    fn fingerprint(
        &self,
        node: usize,
        owner: usize,
        excluded: &[usize],
        name: Option<usize>,
        documentation: bool,
    ) -> String {
        let mut output = String::new();
        self.write_fingerprint(node, owner, excluded, name, documentation, &mut output);
        output
    }

    /// 长度前缀消除 token 边界碰撞，具名后代及其包装语句完全不归入父级。
    fn write_fingerprint(
        &self,
        node: usize,
        owner: usize,
        excluded: &[usize],
        name: Option<usize>,
        documentation: bool,
        output: &mut String,
    ) {
        if excluded.contains(&node)
            || name == Some(node)
            || self
                .declarations
                .get(&node)
                .is_some_and(|declaration| *declaration != owner)
            || self
                .attachments
                .get(&node)
                .is_some_and(|declaration| *declaration != owner)
        {
            return;
        }
        let syntax = &self.parsed.syntax[node];
        if is_comment(&syntax.native_kind) {
            if documentation {
                push_token(output, &syntax.native_kind, text(self.parsed, node));
            }
            return;
        }
        if documentation {
            for child in &syntax.children {
                self.write_fingerprint(*child, owner, excluded, name, true, output);
            }
            return;
        }
        if self.only_owned_descendants(node, owner) {
            return;
        }
        if syntax.children.is_empty() {
            if (syntax.native_kind == "," && self.ignorable_comma(node))
                || (syntax.native_kind == ";"
                    && !syntax.parent.is_some_and(|parent| {
                        matches!(
                            self.parsed.syntax[parent].native_kind.as_str(),
                            "for_statement" | "token_tree"
                        )
                    }))
            {
                return;
            }
            push_token(output, &syntax.native_kind, text(self.parsed, node));
        } else {
            output.push('(');
            output.push_str(&syntax.native_kind);
            for child in &syntax.children {
                self.write_fingerprint(*child, owner, excluded, name, false, output);
            }
            output.push(')');
        }
    }

    /// 列表分隔符通常是格式；稀疏数组空槽及宏 token-tree 的逗号保持原样。
    fn ignorable_comma(&self, node: usize) -> bool {
        let Some(parent) = self.parsed.syntax[node].parent else {
            return false;
        };
        let syntax = &self.parsed.syntax[parent];
        if syntax.native_kind == "token_tree" {
            return false;
        }
        if matches!(syntax.native_kind.as_str(), "array" | "array_pattern") {
            let siblings: Vec<_> = syntax
                .children
                .iter()
                .copied()
                .filter(|child| !self.parsed.syntax[*child].extra)
                .collect();
            let Some(position) = siblings.iter().position(|child| *child == node) else {
                return false;
            };
            return position > 0
                && position + 1 < siblings.len()
                && self.parsed.syntax[siblings[position + 1]].native_kind == "]"
                && self.parsed.syntax[siblings[position - 1]].native_kind != ",";
        }
        true
    }

    /// export/变量包装仅含具名后代时随子项整体排除，避免添加成员误改父级。
    fn only_owned_descendants(&self, node: usize, owner: usize) -> bool {
        if self
            .declarations
            .get(&node)
            .is_some_and(|declaration| *declaration != owner)
        {
            return true;
        }
        let syntax = &self.parsed.syntax[node];
        if !matches!(
            syntax.native_kind.as_str(),
            "lexical_declaration"
                | "variable_declaration"
                | "export_statement"
                | "ambient_declaration"
        ) {
            return false;
        }
        let named: Vec<_> = syntax
            .children
            .iter()
            .filter(|child| {
                self.parsed.syntax[**child].named
                    && !is_comment(&self.parsed.syntax[**child].native_kind)
            })
            .collect();
        !named.is_empty()
            && named
                .iter()
                .all(|child| self.only_owned_descendants(**child, owner))
    }

    /// 相邻注释和 owned 正文注释都参与文档比较；Rust 属性参与签名比较。
    fn documentation(
        &self,
        seed: &DeclarationSeed,
    ) -> (String, Option<ExtractedRange>, Vec<usize>) {
        let leading = leading_nodes(self.parsed, seed.node);
        let comments: Vec<_> = leading
            .iter()
            .copied()
            .filter(|node| is_comment(&self.parsed.syntax[*node].native_kind))
            .collect();
        let attributes = leading
            .into_iter()
            .filter(|node| !is_comment(&self.parsed.syntax[*node].native_kind))
            .collect();
        let owner = self.declarations[&seed.node];
        let mut documentation = String::new();
        for comment in &comments {
            push_token(&mut documentation, "comment", text(self.parsed, *comment));
        }
        documentation.push_str(&self.fingerprint(seed.node, owner, &[], None, true));
        let range = comments.first().map(|first| {
            self.range(
                self.parsed.syntax[*first].range.bytes.start
                    ..self.parsed.syntax[*comments.last().unwrap()]
                        .range
                        .bytes
                        .end,
            )
        });
        (documentation, range, attributes)
    }
}

/// 连续的前置注释和属性属于紧随的声明；不会重复归给父级正文。
fn leading_nodes(parsed: &ParsedSource, node: usize) -> Vec<usize> {
    let mut anchor = node;
    while parsed.syntax[anchor].parent.is_some_and(|parent| {
        matches!(
            parsed.syntax[parent].native_kind.as_str(),
            "export_statement"
                | "lexical_declaration"
                | "variable_declaration"
                | "ambient_declaration"
        )
    }) {
        anchor = parsed.syntax[anchor].parent.unwrap();
    }
    let mut attached = Vec::new();
    if let Some(parent) = parsed.syntax[anchor].parent {
        let siblings = &parsed.syntax[parent].children;
        let tuple_field = parsed.syntax[parent].native_kind == "ordered_field_declaration_list";
        if let Some(position) = siblings.iter().position(|sibling| *sibling == anchor) {
            let mut next_start = parsed.syntax[anchor].range.bytes.start;
            let mut comments_connected = true;
            for sibling in siblings[..position].iter().rev() {
                let syntax = &parsed.syntax[*sibling];
                let gap = &parsed.source[syntax.range.bytes.end..next_start];
                if !gap.trim().is_empty() {
                    break;
                }
                comments_connected &= gap.matches('\n').count() <= 1;
                if is_comment(&syntax.native_kind)
                    && (text(parsed, *sibling).starts_with("//!")
                        || text(parsed, *sibling).starts_with("/*!"))
                {
                    break;
                }
                if is_comment(&syntax.native_kind) {
                    let rust_outer_doc = parsed.language == SourceLanguage::Rust
                        && (text(parsed, *sibling).starts_with("///")
                            || text(parsed, *sibling).starts_with("/**"));
                    if comments_connected || rust_outer_doc {
                        attached.push(*sibling);
                    }
                } else if syntax.native_kind == "attribute_item"
                    || (tuple_field && syntax.native_kind == "visibility_modifier")
                {
                    attached.push(*sibling);
                } else {
                    break;
                }
                next_start = syntax.range.bytes.start;
            }
        }
    }
    attached.reverse();
    attached
}

/// 注释种类统一由真实 grammar 名称识别，内容仍逐字保留。
fn is_comment(kind: &str) -> bool {
    matches!(kind, "comment" | "line_comment" | "block_comment")
}

/// 这些语法节点建立词法环境；匿名函数拥有作用域但不自动成为声明。
fn is_scope_kind(kind: &str) -> bool {
    is_function_kind(kind)
        || matches!(
            kind,
            "statement_block"
                | "block"
                | "class_declaration"
                | "class"
                | "abstract_class_declaration"
                | "class_body"
                | "interface_declaration"
                | "interface_body"
                | "object_type"
                | "type_alias_declaration"
                | "enum_declaration"
                | "internal_module"
                | "module"
                | "mod_item"
                | "declaration_list"
                | "impl_item"
                | "trait_item"
                | "struct_item"
                | "enum_item"
                | "type_item"
                | "associated_type"
                | "union_item"
                | "catch_clause"
                | "for_statement"
                | "for_in_statement"
                | "for_expression"
                | "match_arm"
                | "function_signature"
                | "function_type"
                | "constructor_type"
                | "method_signature"
                | "abstract_method_signature"
                | "call_signature"
                | "construct_signature"
        )
}

/// 函数边界用于参数遮蔽与匿名回调执行来源。
fn is_function_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
            | "function_item"
            | "function_signature_item"
            | "closure_expression"
            | "async_block"
            | "gen_block"
    )
}

/// token 使用长度编码，避免不同叶子序列拼接后碰巧相同。
fn push_token(output: &mut String, kind: &str, value: &str) {
    use std::fmt::Write;
    let _ = write!(output, "{}:{}{}:{}", kind.len(), kind, value.len(), value);
}

/// 条件属性的禁用分支不能确认为开启 prelude，字符串里的同名文本不算属性名。
fn attribute_disables_prelude(parsed: &ParsedSource, nodes: &[usize]) -> bool {
    nodes.iter().any(|node| {
        let syntax = &parsed.syntax[*node];
        (syntax.native_kind == "identifier"
            && matches!(
                text(parsed, *node),
                "no_implicit_prelude" | "r#no_implicit_prelude"
            ))
            || attribute_disables_prelude(parsed, &syntax.children)
    })
}

/// 解构按写入位置展开；成员写入保守标记根接收者，保留对象可能被改写的事实。
fn mutation_identifiers(
    parsed: &ParsedSource,
    node: usize,
    names: &mut Vec<(usize, Option<ExtractedMutationMember>)>,
) {
    let syntax = &parsed.syntax[node];
    match syntax.native_kind.as_str() {
        "identifier"
        | "shorthand_property_identifier_pattern"
        | "shorthand_field_identifier"
        | "this"
        | "self" => names.push((node, None)),
        "member_expression" | "subscript_expression" | "field_expression" => {
            if let Some(receiver) = mutation_receiver(parsed, node) {
                names.push(receiver);
            }
        }
        "pair_pattern" | "pair" => {
            if let Some(value) = field(parsed, node, "value") {
                mutation_identifiers(parsed, value, names);
            }
        }
        "assignment_pattern" | "object_assignment_pattern" => {
            if let Some(left) = field(parsed, node, "left") {
                mutation_identifiers(parsed, left, names);
            }
        }
        "field_pattern" => {
            if let Some(pattern) =
                field(parsed, node, "pattern").or_else(|| field(parsed, node, "name"))
            {
                mutation_identifiers(parsed, pattern, names);
            }
        }
        "object_pattern"
        | "array_pattern"
        | "tuple_pattern"
        | "tuple_expression"
        | "parenthesized_expression"
        | "rest_pattern" => {
            for child in &syntax.children {
                if parsed.syntax[*child].named {
                    mutation_identifiers(parsed, *child, names);
                }
            }
        }
        _ => {}
    }
}

/// 保留根接收者后的第一跳；this.state.x 写入 state，不能误认写入 this.x。
fn mutation_receiver(
    parsed: &ParsedSource,
    node: usize,
) -> Option<(usize, Option<ExtractedMutationMember>)> {
    let syntax = &parsed.syntax[node];
    match syntax.native_kind.as_str() {
        "identifier" | "this" | "self" => Some((node, None)),
        "parenthesized_expression" | "non_null_expression" => {
            let inner = syntax
                .children
                .iter()
                .copied()
                .find(|child| parsed.syntax[*child].named && !parsed.syntax[*child].extra)?;
            mutation_receiver(parsed, inner)
        }
        "member_expression" | "subscript_expression" | "field_expression" => {
            let object = field(parsed, node, "object").or_else(|| field(parsed, node, "value"))?;
            let (root, member) = mutation_receiver(parsed, object)?;
            if member.is_some() {
                return Some((root, member));
            }
            let member = if syntax.native_kind == "subscript_expression" {
                ExtractedMutationMember::Computed
            } else {
                let property =
                    field(parsed, node, "property").or_else(|| field(parsed, node, "field"))?;
                ExtractedMutationMember::Named(text(parsed, property).to_owned())
            };
            Some((root, Some(member)))
        }
        _ => None,
    }
}
