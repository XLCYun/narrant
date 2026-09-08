//! Rust 原生规则补齐 provider outline，保留 impl、字段与 use 的语法边界。

use std::collections::HashSet;

use super::extraction_adapter::{
    field, text, AdapterOutput, DeclarationSeed, ImplSeed, ImportSeed,
};
use super::{ExtractedModulePath, ParsedSource, StructureCategory, StructureNativeName};

/// 模块路径只来自实际 path 属性；条件路径和无法解码的值不能猜成默认文件名。
pub(super) fn module_path(parsed: &ParsedSource, leading: &[usize]) -> ExtractedModulePath {
    let mut explicit = None;
    for item in leading {
        if parsed.syntax[*item].native_kind != "attribute_item" {
            continue;
        }
        if parsed.syntax[*item].contains_error {
            return ExtractedModulePath::Unknown;
        }
        let Some(attribute) = parsed.syntax[*item]
            .children
            .iter()
            .copied()
            .find(|child| parsed.syntax[*child].native_kind == "attribute")
        else {
            return ExtractedModulePath::Unknown;
        };
        let Some(name) = parsed.syntax[attribute]
            .children
            .iter()
            .copied()
            .find(|child| parsed.syntax[*child].named && !parsed.syntax[*child].extra)
        else {
            return ExtractedModulePath::Unknown;
        };
        if parsed.syntax[name].native_kind != "identifier" {
            continue;
        }
        let attribute_name = text(parsed, name);
        match attribute_name.strip_prefix("r#").unwrap_or(attribute_name) {
            "cfg_attr" => {
                if field(parsed, attribute, "arguments")
                    .is_none_or(|arguments| contains_path_token(parsed, arguments))
                {
                    return ExtractedModulePath::Unknown;
                }
            }
            "path" => {
                let Some(value) = field(parsed, attribute, "value")
                    .and_then(|value| decode_path_literal(parsed, value))
                else {
                    return ExtractedModulePath::Unknown;
                };
                if explicit.as_ref().is_some_and(|previous| previous != &value) {
                    return ExtractedModulePath::Unknown;
                }
                explicit = Some(value);
            }
            _ => {}
        }
    }
    explicit.map_or(ExtractedModulePath::Default, ExtractedModulePath::Explicit)
}

/// cfg_attr 的 token tree 可能嵌套，字符串中的 path 文本不算路径属性。
fn contains_path_token(parsed: &ParsedSource, node: usize) -> bool {
    let syntax = &parsed.syntax[node];
    (syntax.native_kind == "identifier" && matches!(text(parsed, node), "path" | "r#path"))
        || syntax
            .children
            .iter()
            .any(|child| contains_path_token(parsed, *child))
}

/// Rust 字符串按原生转义解码；宏、字节字符串或未支持的转义保持未知。
fn decode_path_literal(parsed: &ParsedSource, node: usize) -> Option<String> {
    let value = text(parsed, node);
    let decoded = match parsed.syntax[node].native_kind.as_str() {
        "raw_string_literal" => {
            let raw = value.strip_prefix('r')?;
            let opening_quote = raw.find('"')?;
            let hashes = &raw[..opening_quote];
            if !hashes.chars().all(|character| character == '#') {
                return None;
            }
            let suffix = format!("\"{hashes}");
            let content = raw[opening_quote + 1..]
                .strip_suffix(suffix.as_str())?
                .replace("\r\n", "\n");
            if content.contains('\r') {
                return None;
            }
            content
        }
        "string_literal" => {
            let content = value.strip_prefix('"')?.strip_suffix('"')?;
            let content = content.replace("\r\n", "\n");
            if content.contains('\r') {
                return None;
            }
            let mut characters = content.chars();
            let mut decoded = String::new();
            while let Some(character) = characters.next() {
                if character != '\\' {
                    decoded.push(character);
                    continue;
                }
                decoded.push(match characters.next()? {
                    '\\' => '\\',
                    '"' => '"',
                    '\'' => '\'',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '0' => '\0',
                    'x' => {
                        let high = characters.next()?.to_digit(16)?;
                        let low = characters.next()?.to_digit(16)?;
                        let value = high * 16 + low;
                        if value > 0x7f {
                            return None;
                        }
                        char::from_u32(value)?
                    }
                    'u' => decode_unicode_escape(&mut characters)?,
                    _ => return None,
                });
            }
            decoded
        }
        _ => return None,
    };
    Some(decoded)
}

/// Unicode 转义最多六个十六进制数字，拒绝代理项和越界码点。
fn decode_unicode_escape(characters: &mut std::str::Chars<'_>) -> Option<char> {
    if characters.next()? != '{' {
        return None;
    }
    let mut value = 0u32;
    let mut digits = 0usize;
    for character in characters.by_ref() {
        if character == '}' {
            return if digits == 0 {
                None
            } else {
                char::from_u32(value)
            };
        }
        if character == '_' && digits > 0 {
            continue;
        }
        if digits == 6 {
            return None;
        }
        value = value * 16 + character.to_digit(16)?;
        digits += 1;
    }
    None
}

/// 先消费现成 outline，再遍历原生节点补齐未覆盖的声明和导入。
pub(super) fn collect(parsed: &ParsedSource) -> AdapterOutput {
    let mut output = AdapterOutput::default();
    let mut visited = HashSet::new();
    for node in parsed
        .outline
        .iter()
        .map(|entry| entry.syntax_node)
        .chain(0..parsed.syntax.len())
    {
        if !visited.insert(node) {
            continue;
        }
        match parsed.syntax[node].native_kind.as_str() {
            "impl_item" => {
                if let Some(owner) = field(parsed, node, "type") {
                    output.impls.push(ImplSeed {
                        node,
                        owner: text(parsed, owner).to_owned(),
                        trait_name: field(parsed, node, "trait")
                            .map(|index| text(parsed, index).to_owned()),
                        body: field(parsed, node, "body"),
                    });
                }
            }
            "use_declaration" => {
                if let Some(argument) = field(parsed, node, "argument") {
                    collect_use(
                        parsed,
                        argument,
                        "",
                        is_visible(parsed, node),
                        &mut output.imports,
                    );
                }
            }
            "extern_crate_declaration" => collect_extern_crate(parsed, node, &mut output.imports),
            "ordered_field_declaration_list" => collect_tuple_fields(parsed, node, &mut output),
            _ => {
                if let Some(declaration) = declaration(parsed, node) {
                    output.declarations.push(declaration);
                }
            }
        }
    }
    output
        .declarations
        .sort_by_key(|seed| parsed.syntax[seed.node].range.bytes.start);
    output
        .impls
        .sort_by_key(|seed| parsed.syntax[seed.node].range.bytes.start);
    output
        .imports
        .sort_by_key(|seed| parsed.syntax[seed.node].range.bytes.start);
    output
}

/// 声明类别由实际语法节点决定，trait/impl 的成员只按直接容器判断。
fn declaration(parsed: &ParsedSource, node: usize) -> Option<DeclarationSeed> {
    use StructureCategory as Category;
    use StructureNativeName as Native;

    let member = member_owner(parsed, node).is_some();
    let (category, native_name, body_fields): (_, _, &[&str]) =
        match parsed.syntax[node].native_kind.as_str() {
            "struct_item" => (Category::Struct, Native::Struct, &["body"]),
            "trait_item" => (Category::Trait, Native::Trait, &["body"]),
            "enum_item" => (Category::Enum, Native::Enum, &["body"]),
            "enum_variant" => (Category::Member, Native::EnumVariant, &["body", "value"]),
            "mod_item" => (Category::Module, Native::Module, &["body"]),
            "type_item" if member => (Category::Type, Native::AssociatedType, &[]),
            "type_item" => (Category::Type, Native::TypeAlias, &[]),
            "associated_type" => (Category::Type, Native::AssociatedType, &[]),
            "const_item" if member => (Category::Constant, Native::AssociatedConstant, &["value"]),
            "const_item" => (Category::Constant, Native::Constant, &["value"]),
            "static_item" => (Category::Constant, Native::Static, &["value"]),
            "field_declaration" => (Category::Field, Native::Field, &[]),
            "function_item" | "function_signature_item" if member => {
                if has_self_parameter(parsed, node) {
                    (Category::Method, Native::Method, &["body"])
                } else {
                    (Category::Function, Native::AssociatedFunction, &["body"])
                }
            }
            "function_item" | "function_signature_item" => {
                (Category::Function, Native::Function, &["body"])
            }
            _ => return None,
        };
    let name_node = field(parsed, node, "name")?;
    // 缺失 token 不是一个可以命名的新声明，错误区域仍由共同覆盖诊断保留。
    if parsed.syntax[name_node].is_missing || text(parsed, name_node).is_empty() {
        return None;
    }
    Some(DeclarationSeed {
        node,
        name_node: Some(name_node),
        name: text(parsed, name_node).to_owned(),
        category,
        native_name,
        bodies: body_fields
            .iter()
            .filter_map(|name| field(parsed, node, name))
            .collect(),
        exported: is_visible(parsed, node),
    })
}

/// 最近的声明列表必须直接属于 trait/impl，避免把方法内的局部函数误当成员。
fn member_owner(parsed: &ParsedSource, node: usize) -> Option<usize> {
    let container = parsed.syntax[node].parent?;
    if parsed.syntax[container].native_kind != "declaration_list" {
        return None;
    }
    let owner = parsed.syntax[container].parent?;
    matches!(
        parsed.syntax[owner].native_kind.as_str(),
        "impl_item" | "trait_item"
    )
    .then_some(owner)
}

/// 显式 self 和带类型的 self: Box<Self> 都属于实例方法。
fn has_self_parameter(parsed: &ParsedSource, node: usize) -> bool {
    field(parsed, node, "parameters").is_some_and(|parameters| {
        parsed.syntax[parameters].children.iter().any(|parameter| {
            parsed.syntax[*parameter].native_kind == "self_parameter"
                || field(parsed, *parameter, "pattern")
                    .is_some_and(|pattern| parsed.syntax[pattern].native_kind == "self")
        })
    })
}

/// pub 及 pub(crate)/pub(super) 均保留可见性事实，精确修饰仍在原始签名内。
fn is_visible(parsed: &ParsedSource, node: usize) -> bool {
    parsed.syntax[node]
        .children
        .iter()
        .any(|child| parsed.syntax[*child].native_kind == "visibility_modifier")
}

/// tuple 字段没有名称 token，以 Rust 的真实数字访问顺序命名，并定位到类型节点。
fn collect_tuple_fields(parsed: &ParsedSource, node: usize, output: &mut AdapterOutput) {
    let mut ordinal = 0usize;
    let mut visible = false;
    for child in &parsed.syntax[node].children {
        let syntax = &parsed.syntax[*child];
        if syntax.native_kind == "visibility_modifier" {
            visible = true;
        }
        if syntax.field.as_deref() != Some("type") {
            continue;
        }
        if !syntax.is_missing {
            output.declarations.push(DeclarationSeed {
                node: *child,
                name_node: None,
                name: ordinal.to_string(),
                category: StructureCategory::Field,
                native_name: StructureNativeName::Field,
                bodies: Vec::new(),
                exported: visible,
            });
        }
        ordinal += 1;
        visible = false;
    }
}

/// 递归展开 use 的语法树，保持组内 self、别名和通配符各自的源码位置。
fn collect_use(
    parsed: &ParsedSource,
    node: usize,
    prefix: &str,
    reexport: bool,
    output: &mut Vec<ImportSeed>,
) {
    match parsed.syntax[node].native_kind.as_str() {
        "use_list" => {
            for child in &parsed.syntax[node].children {
                if parsed.syntax[*child].named && !parsed.syntax[*child].extra {
                    collect_use(parsed, *child, prefix, reexport, output);
                }
            }
        }
        "scoped_use_list" => {
            if let Some(list) = field(parsed, node, "list") {
                let path = field(parsed, node, "path")
                    .map(|path| use_path(parsed, path))
                    .unwrap_or_else(|| "::".to_owned());
                collect_use(parsed, list, &join_path(prefix, &path), reexport, output);
            }
        }
        "use_as_clause" => {
            if let (Some(path), Some(alias)) =
                (field(parsed, node, "path"), field(parsed, node, "alias"))
            {
                push_import(parsed, node, path, prefix, Some(alias), reexport, output);
            }
        }
        "use_wildcard" => {
            let path = parsed.syntax[node]
                .children
                .iter()
                .copied()
                .find(|child| parsed.syntax[*child].named && !parsed.syntax[*child].extra);
            let module = path.map_or_else(
                || {
                    if parsed.syntax[node]
                        .children
                        .iter()
                        .any(|child| parsed.syntax[*child].native_kind == "::")
                    {
                        "::".to_owned()
                    } else {
                        prefix.to_owned()
                    }
                },
                |path| join_path(prefix, &use_path(parsed, path)),
            );
            output.push(ImportSeed {
                node,
                binding_node: None,
                module,
                imported: "*".to_owned(),
                local: "*".to_owned(),
                reexport,
                type_only: false,
            });
        }
        "identifier" | "scoped_identifier" | "self" | "super" | "crate" => {
            push_import(parsed, node, node, prefix, None, reexport, output);
        }
        _ => {}
    }
}

/// 路径按字段重新连接，因此 :: 附近的注释和空白不会进入绑定名称。
fn use_path(parsed: &ParsedSource, node: usize) -> String {
    if parsed.syntax[node].native_kind == "scoped_identifier" {
        let Some(name) = field(parsed, node, "name") else {
            return String::new();
        };
        let prefix = field(parsed, node, "path")
            .map(|path| use_path(parsed, path))
            .unwrap_or_default();
        format!("{prefix}::{}", text(parsed, name))
    } else {
        text(parsed, node).to_owned()
    }
}

/// 绝对路径覆盖外层前缀；仅连接语法路径，不推测 crate 的物理文件位置。
fn join_path(prefix: &str, path: &str) -> String {
    if prefix.is_empty() || path.starts_with("::") {
        path.to_owned()
    } else if prefix == "::" {
        format!("::{path}")
    } else {
        format!("{prefix}::{path}")
    }
}

/// 普通导入记录所属模块和末段，self 导入仍明确指向当前模块本身。
fn push_import(
    parsed: &ParsedSource,
    node: usize,
    path_node: usize,
    prefix: &str,
    alias: Option<usize>,
    reexport: bool,
    output: &mut Vec<ImportSeed>,
) {
    let path = join_path(prefix, &use_path(parsed, path_node));
    let (module, imported) = path.rsplit_once("::").unwrap_or(("", &path));
    let local = alias.map_or_else(
        || {
            if imported == "self" && !module.is_empty() {
                module.rsplit("::").next().unwrap_or("self").to_owned()
            } else {
                imported.to_owned()
            }
        },
        |alias| text(parsed, alias).to_owned(),
    );
    if imported.is_empty() || local.is_empty() {
        return;
    }
    let binding = alias
        .or_else(|| field(parsed, path_node, "name"))
        .unwrap_or(path_node);
    output.push(ImportSeed {
        node,
        binding_node: (local != "_").then_some(binding),
        module: if module.is_empty() && path.starts_with("::") {
            "::".to_owned()
        } else {
            module.to_owned()
        },
        imported: imported.to_owned(),
        local,
        reexport,
        type_only: false,
    });
}

/// 旧版 extern crate 仍建立真实名称绑定，重命名与匿名 trait 导入语义分开保留。
fn collect_extern_crate(parsed: &ParsedSource, node: usize, output: &mut Vec<ImportSeed>) {
    let Some(name) = field(parsed, node, "name") else {
        return;
    };
    let alias = field(parsed, node, "alias").unwrap_or(name);
    let local = text(parsed, alias).to_owned();
    output.push(ImportSeed {
        node,
        binding_node: (local != "_").then_some(alias),
        module: text(parsed, name).to_owned(),
        imported: "self".to_owned(),
        local,
        reexport: is_visible(parsed, node),
        type_only: false,
    });
}
