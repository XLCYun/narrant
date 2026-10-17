//! 用同源 AST 定位内嵌声明的词法限定名，不依赖临时 URI 或行号。
use ruff_python_ast::{
    Stmt,
    visitor::source_order::{SourceOrderVisitor, walk_stmt},
};
use ruff_text_size::TextRange;

/// 标准库符号身份由模块和词法声明名组成；不是关系或运行时目标裁决。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandardSymbolIdentity {
    pub module: String,
    pub qualified_name: String,
}

/// 只映射明确函数/类声明；其他导航仍保留来源与候选范围供后续适配。
pub(crate) fn standard_symbol(
    path: &str,
    statements: &[Stmt],
    focus: TextRange,
) -> Option<StandardSymbolIdentity> {
    let module_path = path.strip_prefix("stdlib/")?.strip_suffix(".pyi")?;
    let module = module_path
        .strip_suffix("/__init__")
        .unwrap_or(module_path)
        .replace('/', ".");
    let mut visitor = DeclarationFinder {
        focus,
        owners: Vec::new(),
        found: None,
    };
    for statement in statements {
        visitor.visit_stmt(statement);
    }
    visitor.found.map(|qualified_name| StandardSymbolIdentity {
        module,
        qualified_name,
    })
}

/// 遍历声明时维护词法归属，只在原生焦点精确匹配声明名时给出身份。
struct DeclarationFinder {
    focus: TextRange,
    owners: Vec<String>,
    found: Option<String>,
}

impl<'a> SourceOrderVisitor<'a> for DeclarationFinder {
    /// 控制结构继续递归，函数与类进入相应词法路径。
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        let name = match statement {
            Stmt::FunctionDef(value) => Some(&value.name),
            Stmt::ClassDef(value) => Some(&value.name),
            _ => None,
        };
        if let Some(name) = name {
            self.owners.push(name.as_str().to_owned());
            if name.range == self.focus {
                self.found = Some(self.owners.join("."));
            }
            walk_stmt(self, statement);
            self.owners.pop();
        } else {
            walk_stmt(self, statement);
        }
    }
}
