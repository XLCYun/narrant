//! TypeScript、TSX 与 JavaScript 共用的原生声明定位；补齐 provider 的语法覆盖空缺。

use std::collections::HashSet;

use super::extraction_adapter::{field, text, AdapterOutput, DeclarationSeed, ImportSeed};
use super::{ParsedSource, StructureCategory, StructureNativeName};

/// 先消费 provider 的实际条目，再按同一组原生规则补齐未被 provider 覆盖的节点。
pub(super) fn collect(parsed: &ParsedSource) -> AdapterOutput {
    let mut output = AdapterOutput::default();
    let mut seen = HashSet::new();
    for evidence in &parsed.outline {
        collect_declaration(parsed, evidence.syntax_node, &mut seen, &mut output);
    }
    for (node, syntax) in parsed.syntax.iter().enumerate() {
        if !syntax.named || syntax.extra || syntax.is_missing {
            continue;
        }
        collect_declaration(parsed, node, &mut seen, &mut output);
        match syntax.native_kind.as_str() {
            "import_statement" => collect_import(parsed, node, &mut output.imports),
            "export_statement" => collect_export(parsed, node, &mut output.imports),
            _ => {}
        }
    }
    output
        .declarations
        .sort_by_key(|seed| parsed.syntax[seed.node].range.bytes.start);
    output
}

/// provider 有时返回 export/declare 包装层，统一下探到实际声明以免重复计数。
fn collect_declaration(
    parsed: &ParsedSource,
    node: usize,
    seen: &mut HashSet<usize>,
    output: &mut AdapterOutput,
) {
    match parsed.syntax[node].native_kind.as_str() {
        "export_statement"
        | "ambient_declaration"
        | "lexical_declaration"
        | "variable_declaration" => {
            for child in &parsed.syntax[node].children {
                if parsed.syntax[*child].named {
                    collect_declaration(parsed, *child, seen, output);
                }
            }
            return;
        }
        _ => {}
    }
    if seen.contains(&node) {
        return;
    }
    if let Some(seed) = declaration(parsed, node) {
        seen.insert(node);
        output.declarations.push(seed);
    }
}

/// 只映射真实语法类别；不把匿名回调或普通局部变量包装成声明。
fn declaration(parsed: &ParsedSource, node: usize) -> Option<DeclarationSeed> {
    use StructureCategory as Category;
    use StructureNativeName as Native;

    let kind = parsed.syntax[node].native_kind.as_str();
    let (category, native_name, body_field) = match kind {
        "class_declaration" | "abstract_class_declaration" => {
            (Category::Class, Native::Class, Some("body"))
        }
        "class" if !is_bound_expression(parsed, node) => {
            (Category::Class, Native::Class, Some("body"))
        }
        "interface_declaration" => (Category::Interface, Native::Interface, Some("body")),
        "type_alias_declaration" => (Category::Type, Native::TypeAlias, None),
        "enum_declaration" => (Category::Enum, Native::Enum, Some("body")),
        "enum_assignment" => (Category::Member, Native::EnumVariant, Some("value")),
        "function_declaration" | "generator_function_declaration" | "function_signature" => {
            (Category::Function, Native::Function, Some("body"))
        }
        "function_expression" | "generator_function" | "arrow_function"
            if is_default_value(parsed, node)
                || (kind != "arrow_function"
                    && field(parsed, node, "name").is_some()
                    && !is_bound_expression(parsed, node)) =>
        {
            let native = if kind == "arrow_function" {
                Native::ArrowFunction
            } else {
                Native::Function
            };
            (Category::Function, native, Some("body"))
        }
        "variable_declarator" => return variable_declaration(parsed, node),
        "method_definition" => {
            let name = field(parsed, node, "name")?;
            let native = if direct_token(parsed, node, "get") {
                Native::Getter
            } else if direct_token(parsed, node, "set") {
                Native::Setter
            } else if text(parsed, name) == "constructor" && in_class_body(parsed, node) {
                Native::Constructor
            } else {
                Native::Method
            };
            (Category::Method, native, Some("body"))
        }
        "method_signature" | "abstract_method_signature" if is_named_type_member(parsed, node) => {
            let native = if direct_token(parsed, node, "get") {
                Native::Getter
            } else if direct_token(parsed, node, "set") {
                Native::Setter
            } else if field(parsed, node, "name")
                .is_some_and(|name| text(parsed, name) == "constructor")
                && in_class_body(parsed, node)
            {
                Native::Constructor
            } else {
                Native::Method
            };
            (Category::Method, native, None)
        }
        "public_field_definition" | "field_definition" => {
            (Category::Field, Native::Field, Some("value"))
        }
        "property_signature" if is_named_type_member(parsed, node) => {
            (Category::Field, Native::Property, None)
        }
        "required_parameter" | "optional_parameter" => return parameter_property(parsed, node),
        "call_signature" if is_named_type_member(parsed, node) => {
            return Some(unnamed_member(
                parsed,
                node,
                "(call)",
                Native::CallSignature,
            ));
        }
        "construct_signature" if is_named_type_member(parsed, node) => {
            return Some(unnamed_member(
                parsed,
                node,
                "(new)",
                Native::ConstructSignature,
            ));
        }
        "index_signature" if is_named_type_member(parsed, node) => {
            let name = field(parsed, node, "name")
                .map(|name| format!("[{}]", text(parsed, name)))
                .unwrap_or_else(|| "[index]".to_owned());
            return Some(unnamed_member(parsed, node, &name, Native::IndexSignature));
        }
        "internal_module" => (Category::Namespace, Native::Namespace, Some("body")),
        "module" => (Category::Module, Native::Module, Some("body")),
        _ if is_bare_enum_variant(parsed, node) => {
            return Some(DeclarationSeed {
                node,
                name_node: Some(node),
                name: text(parsed, node).to_owned(),
                category: Category::Member,
                native_name: Native::EnumVariant,
                bodies: Vec::new(),
                exported: false,
            });
        }
        _ => return None,
    };
    let name_node = field(parsed, node, "name").or_else(|| field(parsed, node, "property"));
    let name = match name_node {
        Some(name) => text(parsed, name).to_owned(),
        None if is_default_value(parsed, node) => "default".to_owned(),
        None => return None,
    };
    Some(DeclarationSeed {
        node,
        name_node,
        name,
        category,
        native_name,
        bodies: body_field
            .and_then(|name| field(parsed, node, name))
            .into_iter()
            .collect(),
        exported: is_exported(parsed, node),
    })
}

/// 构造参数只有带访问级别或 readonly 才是字段；其 class 归属由共同流程提升。
fn parameter_property(parsed: &ParsedSource, node: usize) -> Option<DeclarationSeed> {
    let parameters = parsed.syntax[node].parent?;
    let constructor = parsed.syntax[parameters].parent?;
    if parsed.syntax[parameters].native_kind != "formal_parameters"
        || !matches!(
            parsed.syntax[constructor].native_kind.as_str(),
            "method_definition" | "method_signature"
        )
        || !in_class_body(parsed, constructor)
        || !field(parsed, constructor, "name")
            .is_some_and(|name| text(parsed, name) == "constructor")
        || !(direct_token(parsed, node, "readonly")
            || named_child(parsed, node, "accessibility_modifier").is_some())
    {
        return None;
    }
    let name_node = field(parsed, node, "name").or_else(|| field(parsed, node, "pattern"))?;
    if parsed.syntax[name_node].native_kind != "identifier" {
        return None;
    }
    Some(DeclarationSeed {
        node,
        name_node: Some(name_node),
        name: text(parsed, name_node).to_owned(),
        category: StructureCategory::Field,
        native_name: StructureNativeName::Property,
        bodies: field(parsed, node, "value").into_iter().collect(),
        exported: false,
    })
}

/// 变量绑定命名的函数保留完整参数签名；常量只有初始化表达式进入正文指纹。
fn variable_declaration(parsed: &ParsedSource, node: usize) -> Option<DeclarationSeed> {
    let name_node = field(parsed, node, "name")?;
    let value = field(parsed, node, "value");
    let expression = value.map(|value| unwrap_expression(parsed, value));
    let callable = expression.and_then(|value| {
        let native = match parsed.syntax[value].native_kind.as_str() {
            "arrow_function" => StructureNativeName::ArrowFunction,
            "function_expression" | "generator_function" => StructureNativeName::Function,
            "class" => StructureNativeName::Class,
            _ => return None,
        };
        Some((value, native))
    });
    let (category, native_name, bodies) = if let Some((expression, native)) = callable {
        if parsed.syntax[name_node].native_kind != "identifier" {
            return None;
        }
        let category = if native == StructureNativeName::Class {
            StructureCategory::Class
        } else {
            StructureCategory::Function
        };
        (
            category,
            native,
            field(parsed, expression, "body").into_iter().collect(),
        )
    } else if is_module_constant(parsed, node) {
        (
            StructureCategory::Constant,
            StructureNativeName::Constant,
            value.into_iter().collect(),
        )
    } else {
        return None;
    };
    Some(DeclarationSeed {
        node,
        name_node: Some(name_node),
        // 解构声明保留一个真实声明边界；各叶子绑定由共同作用域提取负责。
        name: text(parsed, name_node).to_owned(),
        category,
        native_name,
        bodies,
        exported: is_exported(parsed, node),
    })
}

/// 调用、构造与索引签名没有可复用的声明名称，使用稳定原生名称展示。
fn unnamed_member(
    parsed: &ParsedSource,
    node: usize,
    name: &str,
    native_name: StructureNativeName,
) -> DeclarationSeed {
    DeclarationSeed {
        node,
        name_node: None,
        name: name.to_owned(),
        category: StructureCategory::Member,
        native_name,
        bodies: Vec::new(),
        exported: is_exported(parsed, node),
    }
}

/// 只越过不执行代码的表达式包装；箭头的正文不会被误当作变量初始化包装。
fn unwrap_expression(parsed: &ParsedSource, mut node: usize) -> usize {
    loop {
        let kind = parsed.syntax[node].native_kind.as_str();
        if !matches!(
            kind,
            "parenthesized_expression"
                | "as_expression"
                | "satisfies_expression"
                | "type_assertion"
        ) {
            return node;
        }
        let next = if kind == "type_assertion" {
            parsed.syntax[node].children.iter().copied().find(|child| {
                parsed.syntax[*child].named && parsed.syntax[*child].native_kind != "type_arguments"
            })
        } else {
            parsed.syntax[node]
                .children
                .iter()
                .copied()
                .find(|child| parsed.syntax[*child].named)
        };
        match next {
            Some(next) => node = next,
            None => return node,
        }
    }
}

/// 绑定表达式由外层变量声明承载，避免同一个类或函数同时生成内外两个节点。
fn is_bound_expression(parsed: &ParsedSource, node: usize) -> bool {
    let mut current = node;
    while let Some(parent) = parsed.syntax[current].parent {
        match parsed.syntax[parent].native_kind.as_str() {
            "parenthesized_expression"
            | "as_expression"
            | "satisfies_expression"
            | "type_assertion" => current = parent,
            "variable_declarator" => return field(parsed, parent, "value") == Some(current),
            _ => return false,
        }
    }
    false
}

/// 顶层 const 包括 namespace/module 的直接内容，不包括函数或块内部普通变量。
fn is_module_constant(parsed: &ParsedSource, node: usize) -> bool {
    let Some(lexical) = parsed.syntax[node].parent else {
        return false;
    };
    if parsed.syntax[lexical].native_kind != "lexical_declaration"
        || !direct_token(parsed, lexical, "const")
    {
        return false;
    }
    let mut current = lexical;
    while let Some(parent) = parsed.syntax[current].parent {
        match parsed.syntax[parent].native_kind.as_str() {
            "export_statement" | "ambient_declaration" => current = parent,
            "program" => return true,
            "statement_block" => {
                return parsed.syntax[parent].parent.is_some_and(|owner| {
                    matches!(
                        parsed.syntax[owner].native_kind.as_str(),
                        "internal_module" | "module"
                    )
                });
            }
            _ => return false,
        }
    }
    false
}

/// export 只属于直接包裹的声明，不把导出 class 的所有私有成员误标为导出。
fn is_exported(parsed: &ParsedSource, node: usize) -> bool {
    let mut current = node;
    while let Some(parent) = parsed.syntax[current].parent {
        match parsed.syntax[parent].native_kind.as_str() {
            "export_statement" => return true,
            "lexical_declaration" | "variable_declaration" | "ambient_declaration" => {
                current = parent
            }
            _ => return false,
        }
    }
    false
}

/// 默认导出的匿名函数仍有可供导入的稳定名称 default。
fn is_default_value(parsed: &ParsedSource, node: usize) -> bool {
    parsed.syntax[node].parent.is_some_and(|parent| {
        parsed.syntax[parent].native_kind == "export_statement"
            && direct_token(parsed, parent, "default")
            && (field(parsed, parent, "value") == Some(node)
                || field(parsed, parent, "declaration") == Some(node))
    })
}

/// 构造函数名称只有 class 的直接成员才具备构造语义。
fn in_class_body(parsed: &ParsedSource, node: usize) -> bool {
    parsed.syntax[node]
        .parent
        .is_some_and(|parent| parsed.syntax[parent].native_kind == "class_body")
}

/// 匿名参数对象类型不生成独立成员；具名 class/interface/type 的成员保留原生行。
fn is_named_type_member(parsed: &ParsedSource, node: usize) -> bool {
    let mut current = parsed.syntax[node].parent;
    while let Some(parent) = current {
        match parsed.syntax[parent].native_kind.as_str() {
            "class_declaration"
            | "abstract_class_declaration"
            | "class"
            | "interface_declaration"
            | "type_alias_declaration" => return true,
            "function_declaration"
            | "function_expression"
            | "generator_function_declaration"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
            | "method_signature"
            | "function_signature"
            | "program" => return false,
            _ => current = parsed.syntax[parent].parent,
        }
    }
    false
}

/// 没有初始化值的枚举项直接位于 enum_body 的 name 字段。
fn is_bare_enum_variant(parsed: &ParsedSource, node: usize) -> bool {
    parsed.syntax[node].field.as_deref() == Some("name")
        && parsed.syntax[node]
            .parent
            .is_some_and(|parent| parsed.syntax[parent].native_kind == "enum_body")
}

/// 修饰符通过直接语法 token 判断，字符串、注释和子表达式中的同名文本不参与。
fn direct_token(parsed: &ParsedSource, node: usize, token: &str) -> bool {
    parsed.syntax[node]
        .children
        .iter()
        .any(|child| !parsed.syntax[*child].named && parsed.syntax[*child].native_kind == token)
}

/// ES 导入保留默认、命名、namespace 和 type-only 绑定；副作用导入也保留来源。
fn collect_import(parsed: &ParsedSource, node: usize, output: &mut Vec<ImportSeed>) {
    let require_clause = named_child(parsed, node, "import_require_clause");
    let source = field(parsed, node, "source")
        .or_else(|| require_clause.and_then(|clause| field(parsed, clause, "source")));
    let Some(source) = source else {
        return;
    };
    let module = literal_name(parsed, source);
    let type_only = direct_token(parsed, node, "type");
    if let Some(clause) = require_clause {
        if let Some(binding) = named_child(parsed, clause, "identifier") {
            output.push(import_seed(
                node,
                Some(binding),
                &module,
                "*",
                text(parsed, binding),
                false,
                type_only,
            ));
        }
        return;
    }
    let Some(clause) = named_child(parsed, node, "import_clause") else {
        output.push(import_seed(node, None, &module, "", "", false, type_only));
        return;
    };
    for child in &parsed.syntax[clause].children {
        match parsed.syntax[*child].native_kind.as_str() {
            "identifier" => output.push(import_seed(
                node,
                Some(*child),
                &module,
                "default",
                text(parsed, *child),
                false,
                type_only,
            )),
            "namespace_import" => {
                if let Some(binding) = named_child(parsed, *child, "identifier") {
                    output.push(import_seed(
                        node,
                        Some(binding),
                        &module,
                        "*",
                        text(parsed, binding),
                        false,
                        type_only,
                    ));
                }
            }
            "named_imports" => {
                for specifier in &parsed.syntax[*child].children {
                    if parsed.syntax[*specifier].native_kind == "import_specifier" {
                        collect_specifier(
                            parsed, node, *specifier, &module, false, type_only, output,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

/// module 为空明确表示本文件的导出映射；不伪造一个不存在的导入模块路径。
fn collect_export(parsed: &ParsedSource, node: usize, output: &mut Vec<ImportSeed>) {
    let source = field(parsed, node, "source");
    let module = source
        .map(|source| literal_name(parsed, source))
        .unwrap_or_default();
    let type_only = direct_token(parsed, node, "type");
    if let Some(clause) = named_child(parsed, node, "export_clause") {
        for specifier in &parsed.syntax[clause].children {
            if parsed.syntax[*specifier].native_kind == "export_specifier" {
                collect_specifier(parsed, node, *specifier, &module, true, type_only, output);
            }
        }
    } else if let Some(namespace) = named_child(parsed, node, "namespace_export") {
        if let Some(binding) = parsed.syntax[namespace]
            .children
            .iter()
            .copied()
            .find(|child| parsed.syntax[*child].named)
        {
            output.push(import_seed(
                node,
                None,
                &module,
                "*",
                &literal_name(parsed, binding),
                true,
                type_only,
            ));
        }
    } else if source.is_some() && direct_token(parsed, node, "*") {
        output.push(import_seed(node, None, &module, "*", "*", true, type_only));
    } else if direct_token(parsed, node, "default") {
        if let Some(value) =
            field(parsed, node, "value").or_else(|| field(parsed, node, "declaration"))
        {
            let binding = if parsed.syntax[value].native_kind == "identifier" {
                Some(value)
            } else {
                field(parsed, value, "name")
            };
            if let Some(binding) = binding {
                output.push(import_seed(
                    node,
                    None,
                    "",
                    text(parsed, binding),
                    "default",
                    true,
                    false,
                ));
            }
        }
    }
}

/// 命名导入与重导出的 as 两侧含义一致：imported 为源名称，local 为当前可见名称。
fn collect_specifier(
    parsed: &ParsedSource,
    statement: usize,
    specifier: usize,
    module: &str,
    reexport: bool,
    type_only: bool,
    output: &mut Vec<ImportSeed>,
) {
    let Some(name) = field(parsed, specifier, "name") else {
        return;
    };
    let alias = field(parsed, specifier, "alias").unwrap_or(name);
    output.push(import_seed(
        statement,
        (!reexport).then_some(alias),
        module,
        &literal_name(parsed, name),
        &literal_name(parsed, alias),
        reexport,
        type_only || direct_token(parsed, specifier, "type"),
    ));
}

/// 在语法定位处创建扁平导入证据，后续模块负责真实词法作用域和跨文件绑定。
fn import_seed(
    node: usize,
    binding_node: Option<usize>,
    module: &str,
    imported: &str,
    local: &str,
    reexport: bool,
    type_only: bool,
) -> ImportSeed {
    ImportSeed {
        node,
        binding_node,
        module: module.to_owned(),
        imported: imported.to_owned(),
        local: local.to_owned(),
        reexport,
        type_only,
    }
}

/// 只匹配直接子节点，避免把子作用域中的导入或名称借给外层语法。
fn named_child(parsed: &ParsedSource, node: usize, kind: &str) -> Option<usize> {
    parsed.syntax[node]
        .children
        .iter()
        .copied()
        .find(|child| parsed.syntax[*child].named && parsed.syntax[*child].native_kind == kind)
}

/// 普通字面量去掉引号；转义文本保留原样，让后续解析保守地拒绝未知路径。
fn literal_name(parsed: &ParsedSource, node: usize) -> String {
    let value = text(parsed, node);
    if parsed.syntax[node].native_kind == "string"
        && value.len() >= 2
        && matches!(value.as_bytes()[0], b'\'' | b'"')
        && value.as_bytes().first() == value.as_bytes().last()
    {
        value[1..value.len() - 1].to_owned()
    } else {
        value.to_owned()
    }
}
