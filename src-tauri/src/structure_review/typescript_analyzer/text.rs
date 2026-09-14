//! 以同一份 UTF-8 源码校验 Oxc 字节偏移和 LSP UTF-16 位置。
use super::{failure, Result};
use crate::lsp::Position;

/// 字节位置必须落在字符边界，且不能处于 CRLF 的中间。
pub(crate) fn position(source: &str, offset: u32) -> Result<Position> {
    let offset = offset as usize;
    let preceding = source
        .get(..offset)
        .ok_or_else(|| failure("源码偏移不是有效的 UTF-8 边界"))?;
    if offset > 0
        && source.as_bytes().get(offset) == Some(&b'\n')
        && source.as_bytes()[offset - 1] == b'\r'
    {
        return Err(failure("源码偏移位于 CRLF 中间"));
    }
    Ok(Position {
        line: preceding.bytes().filter(|b| *b == b'\n').count() as u32,
        character: preceding
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .encode_utf16()
            .count() as u32,
    })
}

/// 严格拒绝越行、越界和代理对中间的 LSP 坐标。
pub(crate) fn byte_offset(source: &str, position: &Position) -> Result<u32> {
    let mut starts = vec![0];
    starts.extend(source.match_indices('\n').map(|(i, _)| i + 1));
    let start = *starts
        .get(position.line as usize)
        .ok_or_else(|| failure("LSP 行号越界"))?;
    let end = starts
        .get(position.line as usize + 1)
        .map_or(source.len(), |end| end - 1);
    let line = source[start..end]
        .strip_suffix('\r')
        .unwrap_or(&source[start..end]);
    let mut units = 0;
    for (offset, ch) in line.char_indices() {
        if units == position.character {
            return u32::try_from(start + offset).map_err(failure);
        }
        units += ch.len_utf16() as u32;
    }
    if units == position.character {
        return u32::try_from(start + line.len()).map_err(failure);
    }
    Err(failure("LSP 位置不是有效的 UTF-16 边界"))
}
