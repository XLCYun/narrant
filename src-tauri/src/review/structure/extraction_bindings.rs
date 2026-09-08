//! 从真实绑定位置建立词法环境；成员名称、类型路径和表达式引用不冒充局部变量。

use std::collections::HashSet;

use super::extraction_adapter::{field, text};
use super::{
    ExtractedBinding, ExtractedBindingKind, ExtractedFile, ExtractedRange, ParsedSource,
    StructureCategory, StructureNativeName,
};

/// 返回导入之外的新绑定；调用方先保留导入绑定，再追加结果以统一去重。
pub(super) fn collect(
    parsed: &ParsedSource,
    file: &ExtractedFile,
    node_scopes: &[usize],
    scope_nodes: &[usize],
    node_owners: &[Option<usize>],
    range: impl Fn(usize) -> ExtractedRange,
) -> Vec<ExtractedBinding> {
    let mut declaration_nodes = vec![None; file.declarations.len()];
    for (node, owner) in node_owners.iter().enumerate() {
        if let Some(owner) = owner {
            if declaration_nodes[*owner].is_none() {
                declaration_nodes[*owner] = Some(node);
            }
        }
    }
    let seen = file
        .bindings
        .iter()
        .map(|binding| {
            (
                binding.range.bytes.start,
                binding.range.bytes.end,
                binding.scope,
            )
        })
        .collect();
    let mut collector = BindingCollector {
        parsed,
        file,
        node_scopes,
        scope_nodes,
        node_owners,
        declaration_nodes,
        range,
        seen,
        output: Vec::new(),
    };
    collector.declarations();
    collector.syntax_bindings();
    collector.output
}

/// 当前文件的索引只用于定位作用域，返回值不携带语法树句柄。
struct BindingCollector<'a, F> {
    parsed: &'a ParsedSource,
    file: &'a ExtractedFile,
    node_scopes: &'a [usize],
    scope_nodes: &'a [usize],
    node_owners: &'a [Option<usize>],
    declaration_nodes: Vec<Option<usize>>,
    range: F,
    seen: HashSet<(usize, usize, usize)>,
    output: Vec<ExtractedBinding>,
}

impl<F: Fn(usize) -> ExtractedRange> BindingCollector<'_, F> {
    /// class/trait 的成员只供限定成员查找，不进入普通标识符的词法名称空间。
    fn declarations(&mut self) {
        let file = self.file;
        for (index, declaration) in file.declarations.iter().enumerate() {
            if matches!(
                declaration.native_kind.category,
                StructureCategory::Method | StructureCategory::Field | StructureCategory::Member
            ) || matches!(
                declaration.native_kind.name,
                StructureNativeName::AssociatedFunction
                    | StructureNativeName::AssociatedConstant
                    | StructureNativeName::AssociatedType
                    | StructureNativeName::EnumVariant
            ) {
                continue;
            }
            let Some(node) = self.declaration_nodes[index] else {
                continue;
            };
            if field(self.parsed, node, "name").is_some_and(|name| {
                matches!(
                    self.parsed.syntax[name].native_kind.as_str(),
                    "object_pattern" | "array_pattern"
                )
            }) {
                // 一个解构声明可在图中出现，但词法环境只使用其实际叶子绑定。
                continue;
            }
            let scope = if is_var_binding(self.parsed, node) {
                self.var_scope(self.node_scopes[node])
            } else if matches!(
                self.parsed.syntax[node].native_kind.as_str(),
                "class" | "function_expression" | "generator_function"
            ) && !is_default_export(self.parsed, node)
            {
                // 具名类/函数表达式的内部名称不会泄漏到表达式外面。
                self.node_scopes[node]
            } else {
                declaration.scope
            };
            self.push(ExtractedBinding {
                name: declaration.name.clone(),
                scope,
                kind: ExtractedBindingKind::Declaration,
                declaration: Some(index),
                range: declaration.name_range.clone(),
                type_name: None,
            });
        }
    }

    /// 每种语法只读取明确的绑定字段，默认值、构造类型和守卫表达式不递归为绑定。
    fn syntax_bindings(&mut self) {
        let parsed = self.parsed;
        for (node, syntax) in parsed.syntax.iter().enumerate() {
            let scope = self.node_scopes[node];
            match syntax.native_kind.as_str() {
                "required_parameter" | "optional_parameter" | "parameter" => {
                    if let Some(pattern) = field(self.parsed, node, "pattern")
                        .or_else(|| field(self.parsed, node, "name"))
                    {
                        self.pattern(pattern, node, scope, ExtractedBindingKind::Parameter);
                    }
                }
                "self_parameter" => {
                    if let Some(pattern) = named_child(self.parsed, node, "self") {
                        self.pattern(pattern, node, scope, ExtractedBindingKind::Parameter);
                    }
                }
                "formal_parameters" | "closure_parameters" => {
                    for child in &syntax.children {
                        if self.parsed.syntax[*child].named
                            && !matches!(
                                self.parsed.syntax[*child].native_kind.as_str(),
                                "required_parameter" | "optional_parameter" | "parameter"
                            )
                        {
                            self.pattern(*child, *child, scope, ExtractedBindingKind::Parameter);
                        }
                    }
                }
                "arrow_function" => {
                    if let Some(parameter) = field(self.parsed, node, "parameter") {
                        self.pattern(parameter, parameter, scope, ExtractedBindingKind::Parameter);
                    }
                }
                "variable_declarator" => {
                    if let Some(pattern) = field(self.parsed, node, "name") {
                        let scope = if is_var_binding(self.parsed, node) {
                            self.var_scope(scope)
                        } else {
                            scope
                        };
                        self.pattern(pattern, node, scope, ExtractedBindingKind::Local);
                    }
                }
                "for_in_statement" => {
                    // 没有 const/let/var 的左侧是赋值目标，并不声明新的变量。
                    if has_direct_token(self.parsed, node, &["const", "let", "var", "using"]) {
                        if let Some(pattern) = field(self.parsed, node, "left") {
                            let scope = if has_direct_token(self.parsed, node, &["var"]) {
                                self.var_scope(scope)
                            } else {
                                scope
                            };
                            self.pattern(pattern, node, scope, ExtractedBindingKind::Local);
                        }
                    }
                }
                "let_declaration" => {
                    if let Some(pattern) = field(self.parsed, node, "pattern") {
                        self.pattern(pattern, node, scope, ExtractedBindingKind::Local);
                    }
                }
                "for_expression" => {
                    if let (Some(pattern), Some(body)) = (
                        field(self.parsed, node, "pattern"),
                        field(self.parsed, node, "body"),
                    ) {
                        // Rust 迭代表达式先求值，循环变量只在循环体里遮蔽同名目标。
                        self.pattern(
                            pattern,
                            node,
                            self.node_scopes[body],
                            ExtractedBindingKind::Local,
                        );
                    }
                }
                "let_condition" => {
                    if let (Some(pattern), Some(scope)) = (
                        field(self.parsed, node, "pattern"),
                        self.condition_binding_scope(node),
                    ) {
                        self.pattern(pattern, node, scope, ExtractedBindingKind::Local);
                    }
                }
                "match_arm" => {
                    if let Some(pattern) = field(self.parsed, node, "pattern") {
                        self.pattern(pattern, node, scope, ExtractedBindingKind::Local);
                    }
                }
                "catch_clause" => {
                    if let Some(pattern) = field(self.parsed, node, "parameter") {
                        self.pattern(pattern, node, scope, ExtractedBindingKind::Parameter);
                    }
                }
                "type_parameter" | "const_parameter" | "lifetime_parameter" => {
                    if let Some(name) = field(self.parsed, node, "name") {
                        self.named_binding(
                            name,
                            scope,
                            ExtractedBindingKind::Parameter,
                            None,
                            None,
                        );
                    }
                }
                "function_expression" | "generator_function" | "class" => {
                    self.expression_self_name(node);
                }
                _ => {}
            }
        }
    }

    /// 一个类型注解只赋给完整的单名绑定，解构出的字段不能冒用整个容器类型。
    fn pattern(
        &mut self,
        pattern: usize,
        typed_node: usize,
        scope: usize,
        kind: ExtractedBindingKind,
    ) {
        let mut identifiers = Vec::new();
        binding_identifiers(self.parsed, pattern, &mut identifiers);
        let type_name = if is_single_name_pattern(self.parsed, pattern) {
            field(self.parsed, typed_node, "type").map(|node| {
                text(self.parsed, node)
                    .trim_start_matches(':')
                    .trim()
                    .to_owned()
            })
        } else {
            None
        };
        for identifier in identifiers {
            self.named_binding(identifier, scope, kind, None, type_name.clone());
        }
    }

    /// 具名函数表达式的递归名只在自身可见，且不能错误指向外层所属函数。
    fn expression_self_name(&mut self, node: usize) {
        let Some(name) = field(self.parsed, node, "name") else {
            return;
        };
        let declaration = self.node_owners[node].filter(|owner| {
            self.declaration_nodes[*owner].is_some_and(|declaration| {
                declaration == node || expression_is_bound_to(self.parsed, node, declaration)
            })
        });
        self.named_binding(
            name,
            self.node_scopes[node],
            if declaration.is_some() {
                ExtractedBindingKind::Declaration
            } else {
                ExtractedBindingKind::Local
            },
            declaration,
            None,
        );
    }

    /// var 越过块、catch 和循环作用域，到最近函数、namespace 或文件范围。
    fn var_scope(&self, mut scope: usize) -> usize {
        loop {
            let node = self.scope_nodes[scope];
            if is_var_owner(&self.parsed.syntax[node].native_kind) {
                return scope;
            }
            match self.file.scopes[scope].parent {
                Some(parent) => scope = parent,
                None => return scope,
            }
        }
    }

    /// if-let/while-let 的绑定只进入成功分支，else、后续语句及被匹配值保持原作用域。
    fn condition_binding_scope(&self, node: usize) -> Option<usize> {
        let mut current = self.parsed.syntax[node].parent;
        while let Some(parent) = current {
            match self.parsed.syntax[parent].native_kind.as_str() {
                "if_expression" => {
                    return field(self.parsed, parent, "consequence")
                        .map(|body| self.node_scopes[body]);
                }
                "while_expression" => {
                    return field(self.parsed, parent, "body").map(|body| self.node_scopes[body]);
                }
                "match_arm" => return Some(self.node_scopes[parent]),
                "block" | "function_item" | "closure_expression" => return None,
                _ => current = self.parsed.syntax[parent].parent,
            }
        }
        None
    }

    /// 语法缺失名称不产生绑定；合法名称始终使用原始源码范围。
    fn named_binding(
        &mut self,
        node: usize,
        scope: usize,
        kind: ExtractedBindingKind,
        declaration: Option<usize>,
        type_name: Option<String>,
    ) {
        if self.parsed.syntax[node].is_missing || text(self.parsed, node).is_empty() {
            return;
        }
        self.push(ExtractedBinding {
            name: text(self.parsed, node).to_owned(),
            scope,
            kind,
            declaration,
            range: (self.range)(node),
            type_name,
        });
    }

    /// 同一名称位置在同一作用域只保留一次；内部递归名可在另一个作用域复用位置。
    fn push(&mut self, binding: ExtractedBinding) {
        let key = (
            binding.range.bytes.start,
            binding.range.bytes.end,
            binding.scope,
        );
        if self.seen.insert(key) {
            self.output.push(binding);
        }
    }
}

/// 按 pattern 的原生字段遍历，绝不下探默认值、类型路径、对象键或 match 守卫。
fn binding_identifiers(parsed: &ParsedSource, node: usize, output: &mut Vec<usize>) {
    let syntax = &parsed.syntax[node];
    match syntax.native_kind.as_str() {
        "identifier"
        | "shorthand_property_identifier_pattern"
        | "shorthand_field_identifier"
        | "self" => output.push(node),
        "pair_pattern" => {
            if let Some(value) = field(parsed, node, "value") {
                binding_identifiers(parsed, value, output);
            }
        }
        "assignment_pattern" | "object_assignment_pattern" => {
            if let Some(left) = field(parsed, node, "left") {
                binding_identifiers(parsed, left, output);
            }
        }
        "field_pattern" => {
            if let Some(pattern) = field(parsed, node, "pattern") {
                binding_identifiers(parsed, pattern, output);
            } else if let Some(name) = field(parsed, node, "name") {
                if parsed.syntax[name].native_kind == "shorthand_field_identifier" {
                    output.push(name);
                }
            }
        }
        "array_pattern"
        | "object_pattern"
        | "rest_pattern"
        | "tuple_pattern"
        | "slice_pattern"
        | "tuple_struct_pattern"
        | "struct_pattern"
        | "ref_pattern"
        | "reference_pattern"
        | "mut_pattern"
        | "captured_pattern"
        | "or_pattern"
        | "match_pattern" => {
            for child in &syntax.children {
                let child_syntax = &parsed.syntax[*child];
                if child_syntax.named
                    && !child_syntax.extra
                    && !matches!(child_syntax.field.as_deref(), Some("type" | "condition"))
                {
                    binding_identifiers(parsed, *child, output);
                }
            }
        }
        // 其余节点不是已确认的绑定模式；尤其不能递归 constructor、类型或表达式。
        _ => {}
    }
}

/// 单名的 ref/mut/rest 包装仍沿用整体注解，元组与对象解构需要独立的字段类型证据。
fn is_single_name_pattern(parsed: &ParsedSource, node: usize) -> bool {
    match parsed.syntax[node].native_kind.as_str() {
        "identifier" | "self" | "shorthand_property_identifier_pattern" => true,
        "assignment_pattern" => {
            field(parsed, node, "left").is_some_and(|left| is_single_name_pattern(parsed, left))
        }
        "rest_pattern" | "ref_pattern" | "reference_pattern" | "mut_pattern" => parsed.syntax[node]
            .children
            .iter()
            .copied()
            .find(|child| {
                parsed.syntax[*child].named
                    && parsed.syntax[*child].native_kind != "mutable_specifier"
            })
            .is_some_and(|child| is_single_name_pattern(parsed, child)),
        _ => false,
    }
}

/// var 的声明 token 位于外层语句，不能只依据 variable_declarator 推断块级作用域。
fn is_var_binding(parsed: &ParsedSource, node: usize) -> bool {
    parsed.syntax[node].parent.is_some_and(|parent| {
        parsed.syntax[parent].native_kind == "variable_declaration"
            && has_direct_token(parsed, parent, &["var"])
    })
}

/// JS 函数拥有 var 环境，TS namespace/module 在运行时同样形成独立环境。
fn is_var_owner(kind: &str) -> bool {
    matches!(
        kind,
        "program"
            | "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
            | "internal_module"
            | "module"
    )
}

/// 默认导出的具名声明在模块内可引用，其余具名表达式名称仅对自身可见。
fn is_default_export(parsed: &ParsedSource, node: usize) -> bool {
    parsed.syntax[node].parent.is_some_and(|parent| {
        parsed.syntax[parent].native_kind == "export_statement"
            && has_direct_token(parsed, parent, &["default"])
    })
}

/// 只越过括号与类型断言判断是否由这个变量命名，不能把一般匿名回调归给外层函数。
fn expression_is_bound_to(parsed: &ParsedSource, expression: usize, declaration: usize) -> bool {
    let mut current = expression;
    while let Some(parent) = parsed.syntax[current].parent {
        if parent == declaration {
            return parsed.syntax[parent].native_kind == "variable_declarator"
                && field(parsed, parent, "value") == Some(current);
        }
        if !matches!(
            parsed.syntax[parent].native_kind.as_str(),
            "parenthesized_expression"
                | "as_expression"
                | "satisfies_expression"
                | "type_assertion"
        ) {
            return false;
        }
        current = parent;
    }
    false
}

/// 只认直接 token，避免把子表达式中的同名文本当成声明关键字。
fn has_direct_token(parsed: &ParsedSource, node: usize, kinds: &[&str]) -> bool {
    parsed.syntax[node].children.iter().any(|child| {
        !parsed.syntax[*child].named && kinds.contains(&parsed.syntax[*child].native_kind.as_str())
    })
}

/// 读取直接原生子节点，不跨越参数或函数作用域借用名称。
fn named_child(parsed: &ParsedSource, node: usize, kind: &str) -> Option<usize> {
    parsed.syntax[node]
        .children
        .iter()
        .copied()
        .find(|child| parsed.syntax[*child].named && parsed.syntax[*child].native_kind == kind)
}
