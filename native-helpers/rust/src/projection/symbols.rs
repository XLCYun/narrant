//! 从实际源码声明提供图外符号描述，保持符号 ID 的编码与显示名称相互独立。

use ::syntax::{AstNode, ast};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use super::model::{ExternalSymbol, SymbolScope};
use super::syntax::{Declaration, Document, file_scope};

/// 只描述本次确实引用的图外目标，不把依赖文件中的其他声明加入输出图。
pub(super) fn describe(
    repository: &Path,
    document: &Document,
    referenced: &BTreeSet<&str>,
) -> BTreeMap<String, ExternalSymbol> {
    let scope = if document.path.starts_with(repository) {
        SymbolScope::Repository
    } else {
        SymbolScope::Dependency
    };
    let mut descriptions = BTreeMap::new();
    for declaration in &document.declarations {
        let symbol = &declaration.node.data().id.0;
        if referenced.contains(symbol.as_str()) {
            descriptions.insert(
                symbol.clone(),
                ExternalSymbol {
                    symbol: symbol.clone(),
                    name: qualified_name(document, declaration),
                    scope,
                },
            );
        }
    }
    if let Some(file) = file_scope(repository, &document.path) {
        let symbol = format!("{file}::crate");
        if referenced.contains(symbol.as_str()) {
            let name = document
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            descriptions.insert(
                symbol.clone(),
                ExternalSymbol {
                    symbol,
                    name,
                    scope,
                },
            );
        }
    }
    descriptions
}

/// 方法保留所属类型或 trait 的名称；所有文本都来自语法树，不反向解析编码后的 ID。
fn qualified_name(document: &Document, declaration: &Declaration) -> String {
    let mut parts = Vec::new();
    let mut current = Some(declaration);
    while let Some(item) = current {
        let data = item.node.data();
        let name = if let Some(implementation) = ast::Impl::cast(item.syntax.clone()) {
            implementation
                .self_ty()
                .map(|ty| ty.syntax().text().to_string())
        } else {
            data.name.clone()
        };
        if let Some(name) = name {
            parts.push(name);
        } else if parts.is_empty() {
            parts.push(
                data.signature
                    .clone()
                    .unwrap_or_else(|| data.native_kind.clone()),
            );
        }
        current = item.parent.as_ref().and_then(|parent| {
            document
                .declarations
                .iter()
                .find(|candidate| &candidate.node.data().id == parent)
        });
    }
    parts.reverse();
    parts.join("::")
}
