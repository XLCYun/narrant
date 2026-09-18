use super::*;

/// 用明确的字节偏移和协议坐标同时核验两个转换方向。
fn assert_positions(text: &SourceText, expected: &[(usize, u32, u32)]) {
    for &(offset, line, character) in expected {
        let position = Position { line, character };
        assert_eq!(text.position(offset), Ok(position), "字节偏移 {offset}");
        assert_eq!(text.byte_offset(&position), Ok(offset), "坐标 {position:?}");
    }
}

/// 中文和补充平面字符使用 UTF-16 列号，CRLF 之后正确进入下一行。
#[test]
fn mixed_unicode_and_common_newlines_have_exact_positions() {
    for mode in [LineBreakMode::Rust, LineBreakMode::TypeScript] {
        let text = SourceText::new("中😀x\r\nz\n", mode);
        assert_positions(
            &text,
            &[
                (0, 0, 0),
                (3, 0, 1),
                (7, 0, 3),
                (8, 0, 4),
                (10, 1, 0),
                (11, 1, 1),
                (12, 2, 0),
            ],
        );
        assert_eq!(text.position(9), Err(SourceTextError::InsideLineBreak));
    }
}

/// Rust 仅识别 LF 与 CRLF，独立 CR、LS、PS 和 NEL 均计入列号。
#[test]
fn rust_keeps_other_separator_characters_in_line_content() {
    let text = SourceText::new("a\rb\u{2028}c\u{2029}d\u{85}e\n", LineBreakMode::Rust);
    assert_positions(
        &text,
        &[
            (0, 0, 0),
            (1, 0, 1),
            (2, 0, 2),
            (3, 0, 3),
            (6, 0, 4),
            (7, 0, 5),
            (10, 0, 6),
            (11, 0, 7),
            (13, 0, 8),
            (14, 0, 9),
            (15, 1, 0),
        ],
    );
}

/// TypeScript 识别独立 CR、LS 和 PS，但不会把 NEL 误当作换行。
#[test]
fn typescript_recognizes_its_additional_line_separators() {
    let text = SourceText::new("a\rb\u{2028}c\u{2029}d\u{85}e\n", LineBreakMode::TypeScript);
    assert_positions(
        &text,
        &[
            (0, 0, 0),
            (1, 0, 1),
            (2, 1, 0),
            (3, 1, 1),
            (6, 2, 0),
            (7, 2, 1),
            (10, 3, 0),
            (11, 3, 1),
            (13, 3, 2),
            (14, 3, 3),
            (15, 4, 0),
        ],
    );
}

/// 空文档和末尾换行始终保留可定位的空行，文件末尾也是有效边界。
#[test]
fn empty_text_and_trailing_newlines_preserve_the_final_line() {
    for mode in [LineBreakMode::Rust, LineBreakMode::TypeScript] {
        assert_positions(&SourceText::new("", mode), &[(0, 0, 0)]);
        assert_positions(&SourceText::new("x", mode), &[(0, 0, 0), (1, 0, 1)]);
        assert_positions(
            &SourceText::new("\n\r\n", mode),
            &[(0, 0, 0), (1, 1, 0), (3, 2, 0)],
        );
    }
    assert_positions(
        &SourceText::new("\r", LineBreakMode::Rust),
        &[(0, 0, 0), (1, 0, 1)],
    );
    assert_positions(
        &SourceText::new("\r", LineBreakMode::TypeScript),
        &[(0, 0, 0), (1, 1, 0)],
    );
}

/// 字节越界、多字节字符内部与 CRLF 内部分别返回对应错误。
#[test]
fn invalid_byte_offsets_report_precise_errors() {
    for mode in [LineBreakMode::Rust, LineBreakMode::TypeScript] {
        let text = SourceText::new("中😀\r\n", mode);
        for offset in [1, 2, 4, 5, 6] {
            assert_eq!(
                text.position(offset),
                Err(SourceTextError::InvalidUtf8Boundary),
                "字节偏移 {offset}",
            );
        }
        assert_eq!(text.position(8), Err(SourceTextError::InsideLineBreak));
        for offset in [10, u32::MAX as usize, usize::MAX] {
            assert_eq!(
                text.position(offset),
                Err(SourceTextError::OffsetOutOfBounds),
            );
        }
    }
    let text = SourceText::new("\u{2028}\u{2029}", LineBreakMode::TypeScript);
    for offset in [1, 2, 4, 5] {
        assert_eq!(
            text.position(offset),
            Err(SourceTextError::InvalidUtf8Boundary),
        );
    }
}

/// 相邻代理对及组合字符分别计数，代理对内部坐标不能落到相邻字符上。
#[test]
fn supplementary_and_combining_characters_have_exact_utf16_boundaries() {
    for mode in [LineBreakMode::Rust, LineBreakMode::TypeScript] {
        let text = SourceText::new("a😀𐐷中e\u{301}\n🐱", mode);
        assert_positions(
            &text,
            &[
                (0, 0, 0),
                (1, 0, 1),
                (5, 0, 3),
                (9, 0, 5),
                (12, 0, 6),
                (13, 0, 7),
                (15, 0, 8),
                (16, 1, 0),
                (20, 1, 2),
            ],
        );
        for (line, character) in [(0, 2), (0, 4), (1, 1)] {
            assert_eq!(
                text.byte_offset(&Position { line, character }),
                Err(SourceTextError::InvalidUtf16Boundary),
            );
        }
        for (line, character) in [(0, 9), (1, 3), (0, u32::MAX)] {
            assert_eq!(
                text.byte_offset(&Position { line, character }),
                Err(SourceTextError::ColumnOutOfBounds),
            );
        }
        for line in [2, u32::MAX] {
            assert_eq!(
                text.byte_offset(&Position { line, character: 0 }),
                Err(SourceTextError::LineOutOfBounds),
            );
        }
    }
}

/// 坐标不能把换行符计为列内容，也不能从空行继续跨到下一行。
#[test]
fn columns_cannot_cross_line_breaks_or_empty_lines() {
    for mode in [LineBreakMode::Rust, LineBreakMode::TypeScript] {
        let text = SourceText::new("a\r\n\nb", mode);
        for (line, character) in [(0, 2), (1, 1), (2, 2)] {
            assert_eq!(
                text.byte_offset(&Position { line, character }),
                Err(SourceTextError::ColumnOutOfBounds),
            );
        }
        let empty = SourceText::new("", mode);
        assert_eq!(
            empty.byte_offset(&Position {
                line: 0,
                character: 1
            }),
            Err(SourceTextError::ColumnOutOfBounds),
        );
        assert_eq!(
            empty.byte_offset(&Position {
                line: 1,
                character: 0
            }),
            Err(SourceTextError::LineOutOfBounds),
        );
    }
}

/// 使用独立列出的行起点与内容，穷举样本内所有字节偏移及行列坐标。
fn assert_all_boundaries(source: &str, mode: LineBreakMode, lines: &[(usize, &str)]) {
    let text = SourceText::new(source, mode);
    let mut expected = Vec::new();
    for (line, &(start, content)) in lines.iter().enumerate() {
        assert_eq!(&source[start..start + content.len()], content);
        for relative in content
            .char_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(content.len()))
        {
            expected.push((
                start + relative,
                line as u32,
                content[..relative].encode_utf16().count() as u32,
            ));
        }
    }
    assert_positions(&text, &expected);
    for offset in 0..=source.len() {
        let expected_position = expected.iter().find(|&&(byte, _, _)| byte == offset);
        let result = if let Some(&(_, line, character)) = expected_position {
            Ok(Position { line, character })
        } else if source.is_char_boundary(offset) {
            Err(SourceTextError::InsideLineBreak)
        } else {
            Err(SourceTextError::InvalidUtf8Boundary)
        };
        assert_eq!(
            text.position(offset),
            result,
            "源码 {source:?}，偏移 {offset}"
        );
    }
    for (line, &(_, content)) in lines.iter().enumerate() {
        let width = content.encode_utf16().count() as u32;
        for character in 0..=width + 2 {
            let position = Position {
                line: line as u32,
                character,
            };
            let expected_offset = expected
                .iter()
                .find(|&&(_, row, column)| row == position.line && column == character);
            let result = if let Some(&(offset, _, _)) = expected_offset {
                Ok(offset)
            } else if character < width {
                Err(SourceTextError::InvalidUtf16Boundary)
            } else {
                Err(SourceTextError::ColumnOutOfBounds)
            };
            assert_eq!(
                text.byte_offset(&position),
                result,
                "源码 {source:?}，坐标 {position:?}"
            );
        }
    }
    assert_eq!(
        text.byte_offset(&Position {
            line: lines.len() as u32,
            character: 0
        }),
        Err(SourceTextError::LineOutOfBounds),
    );
}

/// 所有有效位置都与显式行样本一致，并覆盖连续换行和末尾独立 CR。
#[test]
fn all_offsets_and_positions_match_independent_line_fixtures() {
    for mode in [LineBreakMode::Rust, LineBreakMode::TypeScript] {
        assert_all_boundaries("", mode, &[(0, "")]);
        assert_all_boundaries("中😀", mode, &[(0, "中😀")]);
        assert_all_boundaries("\r\n", mode, &[(0, ""), (2, "")]);
        assert_all_boundaries("\n\n", mode, &[(0, ""), (1, ""), (2, "")]);
        assert_all_boundaries("中😀x\r\nz\n", mode, &[(0, "中😀x"), (10, "z"), (12, "")]);
        assert_all_boundaries(
            "a😀𐐷中e\u{301}\n🐱",
            mode,
            &[(0, "a😀𐐷中e\u{301}"), (16, "🐱")],
        );
        assert_all_boundaries("\u{85}\u{b}\u{c}", mode, &[(0, "\u{85}\u{b}\u{c}")]);
    }
    assert_all_boundaries(
        "\r\n\n\r",
        LineBreakMode::Rust,
        &[(0, ""), (2, ""), (3, "\r")],
    );
    assert_all_boundaries(
        "\r\n\n\r",
        LineBreakMode::TypeScript,
        &[(0, ""), (2, ""), (3, ""), (4, "")],
    );
    assert_all_boundaries(
        "a\rb\u{2028}c\u{2029}d\u{85}e\n",
        LineBreakMode::Rust,
        &[(0, "a\rb\u{2028}c\u{2029}d\u{85}e"), (15, "")],
    );
    assert_all_boundaries(
        "a\rb\u{2028}c\u{2029}d\u{85}e\n",
        LineBreakMode::TypeScript,
        &[(0, "a"), (2, "b"), (6, "c"), (10, "d\u{85}e"), (15, "")],
    );
    assert_all_boundaries(
        "\r\r\n\u{2028}\u{2029}",
        LineBreakMode::Rust,
        &[(0, "\r"), (3, "\u{2028}\u{2029}")],
    );
    assert_all_boundaries(
        "\r\r\n\u{2028}\u{2029}",
        LineBreakMode::TypeScript,
        &[(0, ""), (1, ""), (3, ""), (6, ""), (9, "")],
    );
}

/// 克隆共享原始文本及行缓存，多次坐标查询不会改变共享关系。
#[test]
fn clone_reuses_the_source_and_cached_line_index() {
    let text = SourceText::new("中😀\r\nx", LineBreakMode::Rust);
    let cloned = text.clone();
    let cached_lines = text.inner.lines.as_ptr();
    assert!(Arc::ptr_eq(&text.inner, &cloned.inner));
    assert_eq!(cached_lines, cloned.inner.lines.as_ptr());
    assert_eq!(text.as_str(), "中😀\r\nx");
    assert_eq!(text.as_str().as_ptr(), cloned.as_str().as_ptr());
    for _ in 0..3 {
        assert_positions(
            &cloned,
            &[(0, 0, 0), (3, 0, 1), (7, 0, 3), (9, 1, 0), (10, 1, 1)],
        );
    }
    assert!(Arc::ptr_eq(&text.inner, &cloned.inner));
    assert_eq!(cached_lines, text.inner.lines.as_ptr());
    assert_eq!(cached_lines, cloned.inner.lines.as_ptr());
}

/// 展示与字符串借用保留原文，相等性只比较文本而不受换行模式或共享地址影响。
#[test]
fn display_and_equality_use_the_original_source() {
    let source = "中😀\r\n\u{2028}x\r";
    let rust = SourceText::new(source, LineBreakMode::Rust);
    let typescript = SourceText::new(source, LineBreakMode::TypeScript);
    let independent = SourceText::new(source, LineBreakMode::Rust);
    assert_eq!(rust.to_string(), source);
    assert_eq!(typescript.to_string(), source);
    assert_eq!(&*rust, source);
    assert_eq!(rust, typescript);
    assert_eq!(rust, independent);
    assert!(!Arc::ptr_eq(&rust.inner, &independent.inner));
    assert_ne!(
        rust,
        SourceText::new("中😀\n\u{2028}x\r", LineBreakMode::Rust)
    );
}

/// LSP 的最大行列值可以无损转换，避免把上边界误判为溢出。
#[test]
fn largest_lsp_values_remain_representable() {
    assert_eq!(
        lsp_position(u32::MAX as usize, u32::MAX as usize),
        Ok(Position {
            line: u32::MAX,
            character: u32::MAX
        }),
    );
}

/// 在无需分配超大文本的情况下验证行号和列号溢出均会明确失败。
#[cfg(target_pointer_width = "64")]
#[test]
fn lsp_conversion_rejects_overflow_without_allocating_large_text() {
    let overflow = u32::MAX as usize + 1;
    assert_eq!(
        lsp_position(overflow, 0),
        Err(SourceTextError::LineOverflow)
    );
    assert_eq!(
        lsp_position(0, overflow),
        Err(SourceTextError::ColumnOverflow)
    );
    assert_eq!(
        lsp_position(usize::MAX, 0),
        Err(SourceTextError::LineOverflow)
    );
    assert_eq!(
        lsp_position(0, usize::MAX),
        Err(SourceTextError::ColumnOverflow)
    );
}
