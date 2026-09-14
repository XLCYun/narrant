//! 用规范代码的真实 token 投影自身内容，空白和成员列表标点不依赖被删子项的位置。
use super::contains;
use oxc_ast::{
    ast::{ArrayExpressionElement, Program},
    AstKind,
};
use oxc_ast_visit::Visit;
use oxc_parser::{Kind, Token};
use oxc_span::{GetSpan, Span};
use std::collections::BTreeSet;

/// 逗号列表保留完整成员范围，避免把表达式内部逗号或字面量字符当作分隔符。
struct CommaList {
    span: Span,
    items: Vec<Span>,
    /// Oxc Elision 的范围就是对应空位的逗号，不能当作已删除子对象。
    elisions: BTreeSet<u32>,
}

/// 从 typed AST 收集可独立删除成员的列表，不改写原始语法树。
#[derive(Default)]
struct Lists(Vec<CommaList>);
impl<'a> Visit<'a> for Lists {
    /// 明确区分普通成员和数组空位，复用列表边界而不从连续逗号猜测稀疏元素。
    fn enter_node(&mut self, node: AstKind<'a>) {
        if let AstKind::ArrayExpression(array) = node {
            self.0.push(CommaList {
                span: array.span,
                items: array.elements.iter().map(GetSpan::span).collect(),
                elisions: array
                    .elements
                    .iter()
                    .filter_map(|element| match element {
                        ArrayExpressionElement::Elision(hole) => Some(hole.span.start),
                        _ => None,
                    })
                    .collect(),
            });
            return;
        }
        let (span, items) = match node {
            AstKind::ObjectExpression(object) => (
                object.span,
                object.properties.iter().map(GetSpan::span).collect(),
            ),
            AstKind::TSEnumBody(body) => {
                (body.span, body.members.iter().map(GetSpan::span).collect())
            }
            AstKind::FormalParameters(parameters) => (
                parameters.span,
                parameters
                    .items
                    .iter()
                    .map(GetSpan::span)
                    .chain(parameters.rest.iter().map(|rest| rest.span))
                    .collect(),
            ),
            AstKind::CallExpression(call) => (
                Span::new(
                    call.arguments
                        .first()
                        .map_or(call.span.end, |argument| argument.span().start),
                    call.span.end,
                ),
                call.arguments.iter().map(GetSpan::span).collect(),
            ),
            AstKind::NewExpression(call) => (
                Span::new(
                    call.arguments
                        .first()
                        .map_or(call.span.end, |argument| argument.span().start),
                    call.span.end,
                ),
                call.arguments.iter().map(GetSpan::span).collect(),
            ),
            _ => return,
        };
        self.0.push(CommaList {
            span,
            items,
            elisions: BTreeSet::new(),
        });
    }
}

/// 保留规范代码 token 内部文字，只统一 token 间排版；先前 Oxc 生成已显式保存 ASI。
pub(super) struct Content<'s> {
    source: &'s str,
    tokens: &'s [Token],
    lists: Vec<CommaList>,
}
impl<'s> Content<'s> {
    /// 规范 AST 和 token 必须来自同一次解析，与查询语言服务的原始位置完全分离。
    pub fn new(source: &'s str, tokens: &'s [Token], program: &Program<'_>) -> Self {
        let mut lists = Lists::default();
        lists.visit_program(program);
        Self {
            source,
            tokens,
            lists: lists.0,
        }
    }

    /// 删除子范围后重连存活成员，真实空位按 AST 保留；普通尾逗号不进入自身内容。
    fn omitted_commas(&self, span: Span, omitted: &[Span]) -> BTreeSet<u32> {
        let mut removed = BTreeSet::new();
        for list in self.lists.iter().filter(|list| contains(span, list.span)) {
            let separators: Vec<_> = self
                .tokens
                .iter()
                .filter(|token| {
                    token.kind() == Kind::Comma
                        && contains(list.span, token.span())
                        && !list.items.iter().any(|item| {
                            !list.elisions.contains(&item.start) && contains(*item, token.span())
                        })
                })
                .collect();
            removed.extend(separators.iter().map(|token| token.start()));
            let retained: Vec<_> = list
                .items
                .iter()
                .filter(|item| !omitted.iter().any(|range| contains(*range, **item)))
                .collect();
            for pair in retained.windows(2) {
                // 空位没有值 token；它本身的逗号可分隔下一个存活项。
                let left_end = if list.elisions.contains(&pair[0].start) {
                    pair[0].start
                } else {
                    pair[0].end
                };
                if let Some(comma) = separators
                    .iter()
                    .find(|token| left_end <= token.start() && token.end() <= pair[1].start)
                {
                    removed.remove(&comma.start());
                }
            }
            // 最后一个存活项若为空位，还需要它自己的尾逗号来表达该空位。
            if let Some(last) = retained
                .last()
                .filter(|last| list.elisions.contains(&last.start))
            {
                removed.remove(&last.start);
            }
        }
        removed
    }

    /// 所有节点走同一 token 路径，不按是否存在子项选择不同的字符串拼接规则。
    pub fn own_text(&self, span: Span, omitted: &[Span]) -> String {
        let omitted: Vec<_> = omitted
            .iter()
            .copied()
            .filter(|range| contains(span, *range))
            .collect();
        let commas = self.omitted_commas(span, &omitted);
        self.tokens
            .iter()
            .filter(|token| {
                !token.span().is_empty()
                    && contains(span, token.span())
                    && !omitted.iter().any(|range| contains(*range, token.span()))
                    && !commas.contains(&token.start())
            })
            .map(|token| &self.source[token.start() as usize..token.end() as usize])
            .collect::<Vec<_>>()
            .join(" ")
    }
}
