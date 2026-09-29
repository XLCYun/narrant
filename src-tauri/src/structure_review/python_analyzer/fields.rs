//! 实例属性必须来自未重绑定的实例方法参数，不根据 self 字符串猜测。

use super::syntax::expression;
use ruff_python_ast::{
    self as ast,
    visitor::{self, Visitor},
    Expr, Stmt,
};
use ruff_text_size::{Ranged, TextRange};
use std::collections::BTreeSet;

/// 字段只携带显式声明证据；赋值 RHS 始终保留在方法中。
pub(super) struct InstanceField {
    pub name: String,
    pub selection: TextRange,
    pub annotation: Option<String>,
    pub source: TextRange,
}

/// 只处理直接、无未知装饰器的实例方法，嵌套函数与局部类不提供字段证据。
pub(super) fn instance_fields(
    class: &ast::StmtClassDef,
    receivers: &BTreeSet<usize>,
) -> Vec<InstanceField> {
    let mut fields = Vec::new();
    for statement in &class.body {
        let Stmt::FunctionDef(method) = statement else {
            continue;
        };
        if !receivers.contains(&method.start().to_usize()) {
            continue;
        }
        let Some(parameter) = method
            .parameters
            .posonlyargs
            .iter()
            .chain(&method.parameters.args)
            .next()
        else {
            continue;
        };
        let receiver = parameter.parameter.name.as_str();
        let mut binding = Rebinding {
            receiver,
            found: false,
        };
        visitor::walk_body(&mut binding, &method.body);
        if binding.found {
            continue;
        }
        let mut writes = Writes {
            receiver,
            fields: &mut fields,
        };
        visitor::walk_body(&mut writes, &method.body);
    }
    fields
}

/// 任何显式重绑定都使整方法内的接收者证据不再可靠。
struct Rebinding<'a> {
    receiver: &'a str,
    found: bool,
}
impl<'a> Visitor<'a> for Rebinding<'_> {
    /// except 的别名也会覆盖并删除接收者绑定。
    fn visit_except_handler(&mut self, handler: &'a ast::ExceptHandler) {
        let ast::ExceptHandler::ExceptHandler(value) = handler;
        self.found |= value
            .name
            .as_ref()
            .is_some_and(|name| name.as_str() == self.receiver);
        visitor::walk_except_handler(self, handler);
    }

    /// Store/Del 包括普通、循环、解构和命名表达式赋值。
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Name(name) = expr {
            if name.id.as_str() == self.receiver && name.ctx != ast::ExprContext::Load {
                self.found = true;
            }
        }
        visitor::walk_expr(self, expr);
    }
    /// 同名声明及 global/nonlocal 也不能视为原来的参数绑定。
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        self.found |= match stmt {
            Stmt::FunctionDef(value) => value.name.as_str() == self.receiver,
            Stmt::ClassDef(value) => value.name.as_str() == self.receiver,
            Stmt::Global(value) => value
                .names
                .iter()
                .any(|name| name.as_str() == self.receiver),
            Stmt::Nonlocal(value) => value
                .names
                .iter()
                .any(|name| name.as_str() == self.receiver),
            _ => false,
        };
        visitor::walk_stmt(self, stmt);
    }
}

/// 只收集直接接收者的属性写入；不穿越新函数/类边界或跟踪别名。
struct Writes<'a> {
    receiver: &'a str,
    fields: &'a mut Vec<InstanceField>,
}
impl Writes<'_> {
    /// 解构目标中的每个显式属性均有自己的名称，但不复制初始化器。
    fn target(&mut self, expr: &Expr, annotation: Option<&Expr>, source: TextRange) {
        match expr {
            Expr::Attribute(attribute) if matches!(attribute.value.as_ref(), Expr::Name(name) if name.id.as_str() == self.receiver) =>
            {
                self.fields.push(InstanceField {
                    name: attribute.attr.to_string(),
                    selection: attribute.range,
                    annotation: annotation.map(expression),
                    source,
                });
            }
            Expr::Tuple(value) => {
                for target in &value.elts {
                    self.target(target, None, source);
                }
            }
            Expr::List(value) => {
                for target in &value.elts {
                    self.target(target, None, source);
                }
            }
            _ => {}
        }
    }
}
impl<'a> Visitor<'a> for Writes<'_> {
    /// 嵌套闭包和局部类的写入属于方法内容，不据此建立类字段。
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,
            Stmt::Assign(value) => {
                for target in &value.targets {
                    self.target(target, None, stmt.range());
                }
            }
            Stmt::AnnAssign(value) => {
                self.target(&value.target, Some(&value.annotation), stmt.range())
            }
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
    }
}
