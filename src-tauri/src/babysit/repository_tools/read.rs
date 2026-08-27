use super::{
    ensure_not_cancelled, path_string, ReadContinuation, ReadInput, ReadResponse,
    RepositoryToolError, READ_OUTPUT_LIMIT,
};
use crate::operation::CancellationSignal;
use std::fs::{self, File};
use std::io::Read as IoRead;
use std::path::{Path, PathBuf};

/// 在 canonical workspace 内执行一次 Read。
pub(super) fn execute(
    workspace_root: &Path,
    input: &ReadInput,
    cancellation: &CancellationSignal,
) -> Result<ReadResponse, RepositoryToolError> {
    ensure_not_cancelled(cancellation)?;
    let (target, relative_path) = resolve_target(workspace_root, &input.path, cancellation)?;
    ensure_not_cancelled(cancellation)?;
    let metadata = fs::metadata(&target).map_err(|_| RepositoryToolError::TargetUnavailable)?;
    if metadata.is_file() {
        read_file(&target, relative_path, input, cancellation)
    } else if metadata.is_dir() {
        read_directory(&target, relative_path, input, cancellation)
    } else {
        Err(RepositoryToolError::UnsupportedTarget)
    }
}

/// 将用户路径解析为 canonical target，并证明其位于 Workspace 内。
fn resolve_target(
    workspace_root: &Path,
    requested_path: &str,
    cancellation: &CancellationSignal,
) -> Result<(PathBuf, String), RepositoryToolError> {
    ensure_not_cancelled(cancellation)?;
    let requested = Path::new(requested_path);
    let candidate = if requested.is_absolute() {
        requested.to_owned()
    } else {
        workspace_root.join(requested)
    };
    let canonical =
        fs::canonicalize(candidate).map_err(|_| RepositoryToolError::TargetUnavailable)?;
    ensure_not_cancelled(cancellation)?;
    let relative = canonical
        .strip_prefix(workspace_root)
        .map_err(|_| RepositoryToolError::OutsideWorkspace)?;
    let identity = if relative.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        path_string(relative)
    };
    Ok((canonical, identity))
}

/// 读取普通文本文件，在扫描过程中维持 UTF-8 校验和 bounded 输出缓冲。
fn read_file(
    target: &Path,
    relative_path: String,
    input: &ReadInput,
    cancellation: &CancellationSignal,
) -> Result<ReadResponse, RepositoryToolError> {
    let mut file = File::open(target).map_err(|_| RepositoryToolError::TargetUnavailable)?;
    let output_limit = input
        .limit
        .unwrap_or(READ_OUTPUT_LIMIT)
        .min(READ_OUTPUT_LIMIT);
    let mut output = Vec::with_capacity(output_limit);
    let mut utf8_tail = Vec::new();
    let mut offset = 0usize;
    let mut buffer = [0u8; 8 * 1024];
    loop {
        ensure_not_cancelled(cancellation)?;
        let read = file
            .read(&mut buffer)
            .map_err(|_| RepositoryToolError::TargetUnavailable)?;
        if read == 0 {
            break;
        }
        let bytes = &buffer[..read];
        if bytes.contains(&0) {
            return Err(RepositoryToolError::NonTextContent);
        }
        validate_utf8_chunk(bytes, &mut utf8_tail)?;
        let chunk_end = offset.saturating_add(read);
        let requested_end = input.offset.saturating_add(output_limit);
        let capture_start = input.offset.max(offset);
        let capture_end = requested_end.min(chunk_end);
        if capture_start < capture_end {
            output.extend_from_slice(&bytes[capture_start - offset..capture_end - offset]);
        }
        offset = chunk_end;
    }
    ensure_not_cancelled(cancellation)?;
    if !utf8_tail.is_empty() {
        return Err(RepositoryToolError::NonTextContent);
    }
    let text = text_from_captured_bytes(output)?;
    let next_offset = input.offset.saturating_add(text.len());
    Ok(ReadResponse {
        path: relative_path,
        content: text,
        continuation: (next_offset < offset).then_some(ReadContinuation {
            offset: next_offset,
        }),
        truncated: next_offset < offset,
    })
}

/// 读取目录条目并按名称排序，再沿用 Read 的统一字节预算和 continuation 语义。
fn read_directory(
    target: &Path,
    relative_path: String,
    input: &ReadInput,
    cancellation: &CancellationSignal,
) -> Result<ReadResponse, RepositoryToolError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(target).map_err(|_| RepositoryToolError::TargetUnavailable)? {
        ensure_not_cancelled(cancellation)?;
        entries.push(
            entry
                .map_err(|_| RepositoryToolError::TargetUnavailable)?
                .file_name()
                .to_string_lossy()
                .into_owned(),
        );
    }
    ensure_not_cancelled(cancellation)?;
    entries.sort();
    let (content, next_offset, truncated) = slice_text(&entries.join("\n"), input)?;
    Ok(ReadResponse {
        path: relative_path,
        content,
        continuation: truncated.then_some(ReadContinuation {
            offset: next_offset,
        }),
        truncated,
    })
}

/// 将目录 listing 或文件截取结果限制在合法 UTF-8 字符边界内。
fn slice_text(text: &str, input: &ReadInput) -> Result<(String, usize, bool), RepositoryToolError> {
    let start = input.offset.min(text.len());
    if !text.is_char_boundary(start) {
        return Err(RepositoryToolError::InvalidInput);
    }
    let mut end = start
        .saturating_add(
            input
                .limit
                .unwrap_or(READ_OUTPUT_LIMIT)
                .min(READ_OUTPUT_LIMIT),
        )
        .min(text.len());
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    let content = text[start..end].to_owned();
    let next_offset = input.offset.saturating_add(content.len());
    Ok((content, next_offset, end < text.len()))
}

/// 将文件 bounded 字节缓冲解码为文本，并区分起点不合法和终点被预算截断。
fn text_from_captured_bytes(mut bytes: Vec<u8>) -> Result<String, RepositoryToolError> {
    let had_bytes = !bytes.is_empty();
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(error) => {
            let utf8_error = error.utf8_error();
            if utf8_error.error_len().is_some() {
                return Err(RepositoryToolError::InvalidInput);
            }
            let valid = utf8_error.valid_up_to();
            bytes = error.into_bytes();
            bytes.truncate(valid);
            if valid == 0 && had_bytes {
                return Err(RepositoryToolError::InvalidInput);
            }
            String::from_utf8(bytes).map_err(|_| RepositoryToolError::InvalidInput)
        }
    }
}

/// 增量校验文件字节，允许 chunk 边界落在 UTF-8 多字节序列中。
pub(super) fn validate_utf8_chunk(
    bytes: &[u8],
    pending: &mut Vec<u8>,
) -> Result<(), RepositoryToolError> {
    let previous = std::mem::take(pending);
    let mut combined = Vec::with_capacity(previous.len() + bytes.len());
    combined.extend_from_slice(&previous);
    combined.extend_from_slice(bytes);
    match std::str::from_utf8(&combined) {
        Ok(_) => Ok(()),
        Err(error) if error.error_len().is_none() => {
            pending.extend_from_slice(&combined[error.valid_up_to()..]);
            if pending.len() <= 3 {
                Ok(())
            } else {
                Err(RepositoryToolError::NonTextContent)
            }
        }
        Err(_) => Err(RepositoryToolError::NonTextContent),
    }
}
