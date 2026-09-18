//! 共享不可变源码及行边界缓存，统一校验 UTF-8 字节偏移和 LSP UTF-16 坐标。

use std::{fmt, ops::Deref, sync::Arc};

use crate::lsp::Position;

/// 语言各自的换行规则；Rust 仅识别 LF 和 CRLF，TypeScript 还识别 CR、LS、PS。
#[derive(Clone, Copy, Debug)]
pub(crate) enum LineBreakMode {
    Rust,
    TypeScript,
}

/// 坐标必须指向源码字符边界，不能越过行尾或截断 UTF-16 代理对。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SourceTextError {
    OffsetOutOfBounds,
    InvalidUtf8Boundary,
    InsideLineBreak,
    LineOutOfBounds,
    InvalidUtf16Boundary,
    ColumnOutOfBounds,
    LineOverflow,
    ColumnOverflow,
}

impl fmt::Display for SourceTextError {
    /// 用中文说明坐标失效的具体原因，便于上层保留语言分析上下文。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::OffsetOutOfBounds => "源码字节偏移越界",
            Self::InvalidUtf8Boundary => "源码偏移不是有效的 UTF-8 边界",
            Self::InsideLineBreak => "源码偏移位于换行符内部",
            Self::LineOutOfBounds => "LSP 行号越界",
            Self::InvalidUtf16Boundary => "LSP 位置位于 UTF-16 代理对中间",
            Self::ColumnOutOfBounds => "LSP 列号超出当前行的内容范围",
            Self::LineOverflow => "源码行号超出 LSP 支持的范围",
            Self::ColumnOverflow => "源码 UTF-16 列号超出 LSP 支持的范围",
        })
    }
}

impl std::error::Error for SourceTextError {}

/// 一行的半开内容范围；换行符的起点是行尾，下一行从完整换行符之后开始。
#[derive(Debug)]
struct Line {
    start: usize,
    end: usize,
}

/// 源码和缓存一同共享，避免文档与语言会话复制文本或重复计算行边界。
#[derive(Debug)]
struct SourceTextInner {
    source: String,
    lines: Vec<Line>,
}

/// 已建立行边界缓存的不可变源码；克隆只增加共享引用计数。
#[derive(Clone, Debug)]
pub(crate) struct SourceText {
    inner: Arc<SourceTextInner>,
}

impl SourceText {
    /// 按语言换行规则扫描一次源码，同时保留末尾换行之后的空行。
    pub(super) fn new(source: impl Into<String>, mode: LineBreakMode) -> Self {
        let source = source.into();
        let mut lines = Vec::new();
        let mut start = 0;
        let mut characters = source.char_indices().peekable();
        while let Some((offset, character)) = characters.next() {
            let crlf =
                character == '\r' && characters.peek().is_some_and(|(_, next)| *next == '\n');
            let is_line_break = character == '\n'
                || crlf
                || matches!(mode, LineBreakMode::TypeScript)
                    && matches!(character, '\r' | '\u{2028}' | '\u{2029}');
            if is_line_break {
                lines.push(Line { start, end: offset });
                start = offset + character.len_utf8();
                if crlf {
                    characters.next();
                    start += 1;
                }
            }
        }
        lines.push(Line {
            start,
            end: source.len(),
        });
        Self {
            inner: Arc::new(SourceTextInner { source, lines }),
        }
    }

    /// 借用原始 UTF-8 源码，不复制文本或暴露可变访问。
    pub(super) fn as_str(&self) -> &str {
        &self.inner.source
    }

    /// 二分查找缓存行起点，仅扫描所在行前缀来计算 UTF-16 列号。
    pub(super) fn position(&self, offset: usize) -> Result<Position, SourceTextError> {
        let source = self.as_str();
        if offset > source.len() {
            return Err(SourceTextError::OffsetOutOfBounds);
        }
        if !source.is_char_boundary(offset) {
            return Err(SourceTextError::InvalidUtf8Boundary);
        }
        // 缓存始终含有从零开始的首行，因此合法偏移至少匹配一条行记录。
        let index = self
            .inner
            .lines
            .partition_point(|line| line.start <= offset)
            - 1;
        let line = &self.inner.lines[index];
        if offset > line.end {
            return Err(SourceTextError::InsideLineBreak);
        }
        lsp_position(index, source[line.start..offset].encode_utf16().count())
    }

    /// 在缓存行的内容范围内查找 UTF-16 列号，严格拒绝代理对中间和越行坐标。
    pub(super) fn byte_offset(&self, position: &Position) -> Result<usize, SourceTextError> {
        let index = usize::try_from(position.line).map_err(|_| SourceTextError::LineOutOfBounds)?;
        let line = self
            .inner
            .lines
            .get(index)
            .ok_or(SourceTextError::LineOutOfBounds)?;
        let mut units = 0_u32;
        for (offset, character) in self.as_str()[line.start..line.end].char_indices() {
            if units == position.character {
                return Ok(line.start + offset);
            }
            units = units
                .checked_add(character.len_utf16() as u32)
                .ok_or(SourceTextError::ColumnOverflow)?;
            if units > position.character {
                return Err(SourceTextError::InvalidUtf16Boundary);
            }
        }
        if units == position.character {
            Ok(line.end)
        } else {
            Err(SourceTextError::ColumnOutOfBounds)
        }
    }
}

impl Deref for SourceText {
    type Target = str;

    /// 允许现有解析器和源码切片直接借用文本，仍保持内容不可变。
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for SourceText {
    /// 保留源码本身的显示形式，供诊断信息和现有文档输出复用。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq for SourceText {
    /// 文本文档按源码内容比较，相等性不受缓存分配或换行模式影响。
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for SourceText {}

/// 校验 LSP 的有限行列范围，避免把超大源码位置静默截断为 u32。
fn lsp_position(line: usize, character: usize) -> Result<Position, SourceTextError> {
    Ok(Position {
        line: u32::try_from(line).map_err(|_| SourceTextError::LineOverflow)?,
        character: u32::try_from(character).map_err(|_| SourceTextError::ColumnOverflow)?,
    })
}

#[cfg(test)]
#[path = "tests/source_text.rs"]
mod tests;
