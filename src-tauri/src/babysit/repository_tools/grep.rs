use super::{
    ensure_not_cancelled, is_contained, path_string, GrepRequest, GrepResponse, RepositoryToolError,
};
use crate::operation::CancellationSignal;
use regex::Regex;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Grep 单次调用最多返回的匹配行数量。
const MAX_RESULTS: usize = 500;
/// 单个 Grep 文件参与匹配的最大文本字节数。
const MAX_FILE_BYTES: usize = 1024 * 1024;
/// 单条 Grep 输出行的最大 UTF-8 字节数。
const MAX_LINE_BYTES: usize = 8 * 1024;

/// 在一个已验证的文件或目录目标下搜索文本内容。
pub(super) fn execute(
    workspace_root: &Path,
    request: &GrepRequest,
    cancellation: &CancellationSignal,
) -> Result<GrepResponse, RepositoryToolError> {
    ensure_not_cancelled(cancellation)?;
    let pattern = Regex::new(&request.pattern).map_err(|_| RepositoryToolError::InvalidPattern)?;
    let target = resolve_target(workspace_root, &request.path)?;
    let mut matches = Vec::new();
    let mut truncated = false;

    // 允许 workspace 内的 symlink 继续解析，但 filter_entry 先证明每个目录目标，
    // 因而指向外部的 symlink 不会把遍历带出 Review Workspace。
    let walker = WalkDir::new(&target)
        .follow_links(true)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            !cancellation.is_cancelled()
                && entry
                    .path()
                    .canonicalize()
                    .is_ok_and(|canonical| is_contained(workspace_root, &canonical))
        });

    for entry in walker {
        ensure_not_cancelled(cancellation)?;
        let entry = entry.map_err(|_| RepositoryToolError::Io)?;
        let Ok(canonical) = entry.path().canonicalize() else {
            continue;
        };
        if !is_contained(workspace_root, &canonical) {
            continue;
        }
        let metadata = canonical.metadata().map_err(|_| RepositoryToolError::Io)?;
        if !metadata.is_file() {
            continue;
        }
        let Some((text, file_truncated)) = read_text(&canonical, cancellation)? else {
            continue;
        };
        truncated |= file_truncated;
        let Some(relative_path) = workspace_relative(workspace_root, entry.path()) else {
            continue;
        };
        let file_matches = matching_lines(
            &pattern,
            &text,
            &relative_path,
            cancellation,
            &mut truncated,
        )?;
        matches.extend(file_matches);
        if matches.len() > MAX_RESULTS {
            truncated = true;
            break;
        }
    }

    ensure_not_cancelled(cancellation)?;
    matches.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.line_number.cmp(&right.line_number))
            .then(left.line.cmp(&right.line))
    });
    matches.dedup();
    truncated |= matches.len() > MAX_RESULTS;
    matches.truncate(MAX_RESULTS);
    Ok(GrepResponse { matches, truncated })
}

/// 把 Grep 的相对、绝对和 parent-component 目标解析为 workspace 内 canonical 路径。
fn resolve_target(
    workspace_root: &Path,
    requested_path: &str,
) -> Result<PathBuf, RepositoryToolError> {
    let requested = Path::new(requested_path);
    let candidate = if requested_path.is_empty() || requested == Path::new(".") {
        workspace_root.to_path_buf()
    } else if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        workspace_root.join(requested)
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|_| RepositoryToolError::TargetUnavailable)?;
    if !is_contained(workspace_root, &canonical) {
        return Err(RepositoryToolError::OutsideWorkspace);
    }
    Ok(canonical)
}

/// 将已验证的遍历项投影为 workspace-relative POSIX 路径。
fn workspace_relative(workspace_root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(workspace_root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(path_string(relative))
}

/// 逐块读取文本，避免一次不可取消的整文件系统调用。
fn read_text(
    path: &Path,
    cancellation: &CancellationSignal,
) -> Result<Option<(String, bool)>, RepositoryToolError> {
    let mut file = File::open(path).map_err(|_| RepositoryToolError::Io)?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut binary = false;
    let mut utf8_tail = Vec::new();
    let mut truncated = false;
    loop {
        ensure_not_cancelled(cancellation)?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| RepositoryToolError::Io)?;
        if count == 0 {
            break;
        }
        binary |= buffer[..count].contains(&0);
        if super::validate_utf8_chunk(&buffer[..count], &mut utf8_tail).is_err() {
            binary = true;
        }
        let remaining = MAX_FILE_BYTES.saturating_sub(bytes.len());
        bytes.extend_from_slice(&buffer[..count.min(remaining)]);
        truncated |= count > remaining;
    }
    ensure_not_cancelled(cancellation)?;
    if binary || !utf8_tail.is_empty() {
        return Ok(None);
    }
    while std::str::from_utf8(&bytes).is_err() {
        bytes.pop();
        truncated = true;
    }
    Ok(String::from_utf8(bytes).ok().map(|text| (text, truncated)))
}

/// 收集一个文本文件的匹配行，确保二进制文件永远不会产生部分结果。
fn matching_lines(
    pattern: &Regex,
    text: &str,
    path: &str,
    cancellation: &CancellationSignal,
    truncated: &mut bool,
) -> Result<Vec<super::GrepMatch>, RepositoryToolError> {
    let mut matches = Vec::new();
    for (index, line) in text.lines().enumerate() {
        ensure_not_cancelled(cancellation)?;
        if pattern.is_match(line) {
            let rendered = bounded_line(line);
            *truncated |= rendered.len() < line.len();
            matches.push(super::GrepMatch {
                path: path.to_owned(),
                line_number: index + 1,
                line: rendered,
            });
            if matches.len() > MAX_RESULTS {
                break;
            }
        }
    }
    Ok(matches)
}

/// 在 UTF-8 边界上截断单条匹配行，防止极长行占满模型上下文。
fn bounded_line(line: &str) -> String {
    if line.len() <= MAX_LINE_BYTES {
        return line.to_owned();
    }
    let mut end = MAX_LINE_BYTES;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    line[..end].to_owned()
}
