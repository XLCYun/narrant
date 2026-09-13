//! 按明确的测试属性和条件编译排除源码，可能被遮蔽的属性先通过 LSP 确认身份。

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use ra_ap_syntax::{
    ast::{self, HasAttrs, HasName},
    AstNode, Edition, SourceFile, SyntaxKind, SyntaxNode, TextRange, TextSize, WalkEvent,
};

use super::{
    lsp::RustLsp,
    syntax::{self, Document},
    Result,
};
use crate::lsp::Location;

/// 缓存属性定义身份；相同内置宏被多个函数引用时只读取和验证一次定义源码。
#[derive(Default)]
pub(super) struct TestAttributeResolver {
    definitions: BTreeMap<(String, u32, u32), bool>,
}

impl TestAttributeResolver {
    /// 只补查语法阶段因潜在遮蔽而保留的测试入口，普通 cfg 和已排除子树无需额外请求。
    pub fn resolve(
        &mut self,
        client: &mut RustLsp,
        repository: &Path,
        document: &mut Document,
    ) -> Result<()> {
        let mut confirmed = Vec::new();
        let mut walk = document.tree.preorder();
        while let Some(event) = walk.next() {
            let WalkEvent::Enter(node) = event else {
                continue;
            };
            if document.excludes_range(node.text_range()) {
                walk.skip_subtree();
                continue;
            }
            if node.kind() != SyntaxKind::FN || !test_only_with_attributes(&node, true) {
                continue;
            }
            let owner = ast::AnyHasAttrs::cast(node.clone()).expect("函数支持属性");
            let attributes: Vec<_> = ast::attrs_including_inner(&owner)
                .filter_map(|attr| attr.meta())
                .flat_map(|meta| {
                    meta.syntax()
                        .descendants()
                        .filter_map(ast::Meta::cast)
                        .collect::<Vec<_>>()
                })
                .filter(|meta| {
                    matches!(meta, ast::Meta::PathMeta(_))
                        && meta.simple_name().as_deref() == Some("test")
                })
                .collect();
            let mut builtin = !attributes.is_empty();
            for attribute in attributes {
                let position =
                    syntax::position(&document.source, attribute.syntax().text_range().start());
                let definitions = client.definitions(&document.uri, position)?;
                if definitions.is_empty() {
                    builtin = false;
                    break;
                }
                for location in definitions {
                    if !self.is_builtin_test(repository, &location)? {
                        builtin = false;
                        break;
                    }
                }
                if !builtin {
                    break;
                }
            }
            if builtin {
                confirmed.push(node.text_range());
                walk.skip_subtree();
            }
        }
        document.exclude_ranges(repository, confirmed)
    }

    /// 定义必须是 sysroot 中具备 rustc_builtin_macro 属性的 test 宏，自定义同名宏仍保留。
    fn is_builtin_test(&mut self, repository: &Path, location: &Location) -> Result<bool> {
        let key = (
            location.uri.clone(),
            location.position.line,
            location.position.character,
        );
        if let Some(builtin) = self.definitions.get(&key) {
            return Ok(*builtin);
        }
        let builtin = builtin_test_definition(repository, location)?;
        self.definitions.insert(key, builtin);
        Ok(builtin)
    }
}

/// 只检查 LSP 指向的宏声明；没有可读定义时保持未知，不根据 hover 文案或文件名猜测。
fn builtin_test_definition(repository: &Path, location: &Location) -> Result<bool> {
    let Some(path) = url::Url::parse(&location.uri)
        .ok()
        .and_then(|uri| uri.to_file_path().ok())
    else {
        return Ok(false);
    };
    if !syntax::file_scope(repository, &path)
        .is_some_and(|scope| scope.starts_with("rust:sysroot:"))
    {
        return Ok(false);
    }
    let Ok(source) = fs::read_to_string(path) else {
        return Ok(false);
    };
    let offset = syntax::byte_offset(&source, &location.position)?;
    let parsed = SourceFile::parse(&source, Edition::CURRENT);
    Ok(parsed
        .syntax_node()
        .descendants()
        .filter_map(ast::MacroDef::cast)
        .any(|definition| {
            definition.name().is_some_and(|name| {
                name.text() == "test" && name.syntax().text_range().contains(offset)
            }) && definition
                .attrs()
                .any(|attribute| attribute.simple_name().as_deref() == Some("rustc_builtin_macro"))
        }))
}

/// 收集互不重叠的测试范围；确认父范围后直接跳过其内部语法树。
pub(super) fn excluded_ranges(tree: &SyntaxNode) -> Vec<TextRange> {
    let shadowed_scopes = possible_test_import_scopes(tree);
    let mut ranges = Vec::new();
    let mut walk = tree.preorder();
    while let Some(event) = walk.next() {
        if let WalkEvent::Enter(node) = event {
            let builtin_test = node.kind() == SyntaxKind::FN
                && !node.ancestors().any(|scope| {
                    shadowed_scopes
                        .contains(&(scope.text_range().start(), scope.text_range().end()))
                });
            if test_only_with_attributes(&node, builtin_test) {
                ranges.push(node.text_range());
                walk.skip_subtree();
            }
        }
    }
    ranges
}

/// 对 test=false 的结果必须明确为 false；始终禁用的代码不因此被误判为测试代码。
pub(super) fn is_test_only(node: &SyntaxNode) -> bool {
    test_only_with_attributes(node, false)
}

/// 条件编译独立于属性宏；短名 test 只有没有潜在导入遮蔽时才作为内置测试入口。
fn test_only_with_attributes(node: &SyntaxNode, builtin_test: bool) -> bool {
    let Some(owner) = ast::AnyHasAttrs::cast(node.clone()) else {
        return false;
    };
    let metas: Vec<_> = ast::attrs_including_inner(&owner)
        .filter_map(|attr| attr.meta())
        .collect();
    let function = builtin_test && node.kind() == SyntaxKind::FN;
    let enabled = |test| all(metas.iter().map(|meta| retained(meta, test, function)));
    enabled(false) == Some(false) && enabled(true) != Some(false)
}

/// 一次扫描记录潜在属性导入的词法作用域，内部或相邻模块的导入不影响外部测试。
fn possible_test_import_scopes(tree: &SyntaxNode) -> BTreeSet<(TextSize, TextSize)> {
    tree.descendants()
        .filter_map(|node| {
            let imported = (node.kind() == SyntaxKind::USE
                && node
                    .descendants_with_tokens()
                    .filter_map(|element| element.into_token())
                    .any(|token| token.text() == "test" || token.text() == "*"))
                || (node.kind() == SyntaxKind::EXTERN_CRATE
                    && node
                        .children()
                        .filter_map(ast::Attr::cast)
                        .any(|attr| attr.simple_name().as_deref() == Some("macro_use")));
            if !imported {
                return None;
            }
            node.ancestors()
                .find(|scope| {
                    matches!(
                        scope.kind(),
                        SyntaxKind::SOURCE_FILE | SyntaxKind::ITEM_LIST | SyntaxKind::STMT_LIST
                    )
                })
                .map(|scope| (scope.text_range().start(), scope.text_range().end()))
        })
        .collect()
}

/// 属性只控制自身声明；cfg_attr 的条件不成立时不施加内部属性。
fn retained(meta: &ast::Meta, test: bool, function: bool) -> Option<bool> {
    match meta {
        ast::Meta::CfgMeta(cfg) => evaluate(cfg.cfg_predicate()?, test),
        ast::Meta::CfgAttrMeta(attr) => {
            let condition = evaluate(attr.cfg_predicate()?, test);
            let result = all(attr.metas().map(|meta| retained(&meta, test, function)));
            any([condition.map(|value| !value), result])
        }
        ast::Meta::PathMeta(_) if function && meta.simple_name().as_deref() == Some("test") => {
            Some(test)
        }
        _ => Some(true),
    }
}

/// 只固定 test，平台、feature 和自定义 cfg 均保持未知；不枚举构建配置。
fn evaluate(predicate: ast::CfgPredicate, test: bool) -> Option<bool> {
    match predicate {
        ast::CfgPredicate::CfgAtom(atom) => {
            if atom.eq_token().is_some() {
                None
            } else if atom
                .ident_token()
                .is_some_and(|token| token.text() == "test")
            {
                Some(test)
            } else if atom.true_token().is_some() {
                Some(true)
            } else if atom.false_token().is_some() {
                Some(false)
            } else {
                None
            }
        }
        ast::CfgPredicate::CfgComposite(composite) => {
            let keyword = composite.keyword()?;
            let mut predicates = composite.cfg_predicates();
            match keyword.text() {
                "all" => all(predicates.map(|predicate| evaluate(predicate, test))),
                "any" => any(predicates.map(|predicate| evaluate(predicate, test))),
                "not" => {
                    let result = evaluate(predicates.next()?, test);
                    if predicates.next().is_some() {
                        None
                    } else {
                        result.map(|value| !value)
                    }
                }
                _ => None,
            }
        }
    }
}

/// 三值合取：明确的 false 足以拒绝；未知不会被当作 false。
fn all(values: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    let mut result = Some(true);
    for value in values {
        match value {
            Some(false) => return Some(false),
            None => result = None,
            Some(true) => {}
        }
    }
    result
}

/// 三值析取由合取对偶得到，保留无法判定的条件。
fn any(values: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    all(values.into_iter().map(|value| value.map(|value| !value))).map(|value| !value)
}

#[cfg(test)]
#[path = "tests/test_code.rs"]
mod tests;
