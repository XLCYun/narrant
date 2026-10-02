//! 只有固定标准库中的定义证据可改变 property、overload 和 TypeAlias 的对象映射。

use ruff_python_ast::{
    visitor::{self, Visitor},
    Expr, Stmt,
};
use ruff_text_size::Ranged;
use std::{collections::BTreeMap, path::Path};

/// 有明确声明语义的少量标准库对象，不按源码短名称猜测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Known {
    Property,
    StaticMethod,
    ClassMethod,
    Overload,
    TypeAlias,
    /// 属性写入实际导航到其他声明，不能据此建立本类实例字段。
    NotInstanceField,
}

/// 键是原文表达式字节位置，只在本次构建使用，不进入图身份或差异内容。
pub(super) type Proofs = BTreeMap<usize, Known>;

impl Known {
    /// 文件必须来自已校验的固定 stdlib；同名仓库对象不能匹配。
    pub fn at_stdlib(path: &Path, name: &str) -> Option<Self> {
        match (path.to_str()?, name) {
            ("builtins.pyi", "property") => Some(Self::Property),
            ("builtins.pyi", "staticmethod") => Some(Self::StaticMethod),
            ("builtins.pyi", "classmethod") => Some(Self::ClassMethod),
            ("typing.pyi", "overload") => Some(Self::Overload),
            ("typing.pyi", "TypeAlias") => Some(Self::TypeAlias),
            _ => None,
        }
    }
}

/// 仅查询会影响独立对象映射的位置，局部定义仍完整属于外层 Callable。
pub(super) fn queries(body: &[Stmt]) -> Vec<Expr> {
    /// 收集声明头，不把嵌套函数体变成第二套对象模型。
    struct Collector(Vec<Expr>);
    impl<'a> Visitor<'a> for Collector {
        /// 属性访问器链在提取阶段核对先前已证明的 getter 绑定。
        fn visit_stmt(&mut self, stmt: &'a Stmt) {
            match stmt {
                Stmt::FunctionDef(value) => {
                    self.0.extend(
                        value
                            .decorator_list
                            .iter()
                            .map(|decorator| decorator.expression.clone()),
                    );
                }
                Stmt::AnnAssign(value) => {
                    self.0.push(*value.annotation.clone());
                }
                _ => visitor::walk_stmt(self, stmt),
            }
        }
    }
    let mut collector = Collector(vec![]);
    visitor::walk_body(&mut collector, body);
    collector
        .0
        .retain(|expr| matches!(expr, Expr::Name(_) | Expr::Attribute(_)));
    collector.0.sort_by_key(Ranged::start);
    collector.0.dedup_by_key(|expression| expression.start());
    collector.0
}

/// 仅独立类参与继承归属验证，函数内的局部类仍属于外层 Callable。
pub(super) fn classes(body: &[Stmt]) -> Vec<ruff_python_ast::StmtClassDef> {
    /// 收集类声明时保留完整基类表达式，跳过所有函数体。
    struct Collector(Vec<ruff_python_ast::StmtClassDef>);
    impl<'a> Visitor<'a> for Collector {
        /// 不穿越函数边界，类体中的嵌套类仍继续收集。
        fn visit_stmt(&mut self, statement: &'a Stmt) {
            match statement {
                Stmt::FunctionDef(_) => {}
                Stmt::ClassDef(class) => {
                    self.0.push(class.clone());
                    visitor::walk_body(self, &class.body);
                }
                _ => visitor::walk_stmt(self, statement),
            }
        }
    }
    let mut collector = Collector(vec![]);
    visitor::walk_body(&mut collector, body);
    collector.0
}
