//! 以同一份 UTF-8 源码校验 Oxc 字节偏移和 LSP UTF-16 位置。
use super::{failure, Result};
use crate::lsp::Position;

/// 一行的内容范围不包含换行符；下一行起点自然跳过完整 CRLF。
struct Line {
    start: usize,
    end: usize,
}

/// 对齐固定 TypeScript 的 computeLineStarts/isLineBreak，不把 U+0085 当作换行。
fn lines(source: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut characters = source.char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        if matches!(character, '\r' | '\n' | '\u{2028}' | '\u{2029}') {
            lines.push(Line { start, end: offset });
            start = offset + character.len_utf8();
            if character == '\r' && characters.peek().is_some_and(|(_, next)| *next == '\n') {
                characters.next();
                start += 1;
            }
        }
    }
    lines.push(Line {
        start,
        end: source.len(),
    });
    lines
}

/// 字节位置必须落在字符边界或行尾，不能处于多字节字符或 CRLF 中间。
pub(crate) fn position(source: &str, offset: u32) -> Result<Position> {
    let offset = offset as usize;
    if !source.is_char_boundary(offset) {
        return Err(failure("源码偏移不是有效的 UTF-8 边界"));
    }
    let lines = lines(source);
    let index = lines.partition_point(|line| line.start <= offset) - 1;
    let line = &lines[index];
    if offset > line.end {
        return Err(failure("源码偏移位于 CRLF 中间"));
    }
    Ok(Position {
        line: u32::try_from(index).map_err(failure)?,
        character: u32::try_from(source[line.start..offset].encode_utf16().count())
            .map_err(failure)?,
    })
}

/// 严格拒绝越行、越界和代理对中间的 LSP 坐标，所有换行类型共用行边界。
pub(crate) fn byte_offset(source: &str, position: &Position) -> Result<u32> {
    let lines = lines(source);
    let line = lines
        .get(position.line as usize)
        .ok_or_else(|| failure("LSP 行号越界"))?;
    let mut units = 0;
    for (offset, ch) in source[line.start..line.end].char_indices() {
        if units == position.character {
            return u32::try_from(line.start + offset).map_err(failure);
        }
        units += ch.len_utf16() as u32;
    }
    if units == position.character {
        return u32::try_from(line.end).map_err(failure);
    }
    Err(failure("LSP 位置不是有效的 UTF-16 边界"))
}
