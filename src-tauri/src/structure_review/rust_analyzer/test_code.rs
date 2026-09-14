//! 按写作 #[test] 的属性和明确要求 test 的条件编译排除源码，不查询属性宏身份。

use ra_ap_syntax::{ast, AstNode, SyntaxNode, TextRange, WalkEvent};

/// 收集互不重叠的测试范围；确认父范围后直接跳过其内部语法树。
pub(super) fn excluded_ranges(tree: &SyntaxNode) -> Vec<TextRange> {
    let mut ranges = Vec::new();
    let mut walk = tree.preorder();
    while let Some(event) = walk.next() {
        if let WalkEvent::Enter(node) = event {
            if is_test_only(&node) {
                ranges.push(node.text_range());
                walk.skip_subtree();
            }
        }
    }
    ranges
}

/// 直接命中 #[test] 即排除；其他声明通过条件编译判断是否仅在测试配置下存在。
pub(super) fn is_test_only(node: &SyntaxNode) -> bool {
    let Some(owner) = ast::AnyHasAttrs::cast(node.clone()) else {
        return false;
    };
    let metas: Vec<_> = ast::attrs_including_inner(&owner)
        .filter_map(|attr| attr.meta())
        .collect();
    if metas.iter().any(|meta| {
        matches!(meta, ast::Meta::PathMeta(_)) && meta.simple_name().as_deref() == Some("test")
    }) {
        return true;
    }
    let enabled = |test| all(metas.iter().map(|meta| retained(meta, test)));
    enabled(false) == Some(false) && enabled(true) != Some(false)
}

/// 属性只控制自身声明；cfg_attr 的条件不成立时不施加内部属性。
fn retained(meta: &ast::Meta, test: bool) -> Option<bool> {
    match meta {
        ast::Meta::CfgMeta(cfg) => evaluate(cfg.cfg_predicate()?, test),
        ast::Meta::CfgAttrMeta(attr) => {
            let condition = evaluate(attr.cfg_predicate()?, test);
            let result = all(attr.metas().map(|meta| retained(&meta, test)));
            any([condition.map(|value| !value), result])
        }
        ast::Meta::PathMeta(_) if meta.simple_name().as_deref() == Some("test") => Some(test),
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
