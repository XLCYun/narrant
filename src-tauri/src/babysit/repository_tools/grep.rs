use super::{
    ensure_not_cancelled, is_contained, path_string, GrepMatch, GrepRequest, GrepResponse,
    RepositoryToolError,
};

use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::WalkBuilder;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// Grep 单次调用最多返回的匹配行数量。
const MAX_RESULTS: usize = 500;
/// Grep 最终 JSON 响应的最大 UTF-8 字节数。
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
/// Grep 接受的正则表达式最大 UTF-8 字节数。
const MAX_PATTERN_BYTES: usize = 16 * 1024;
/// 编译后正则程序的近似最大字节数。
const REGEX_SIZE_LIMIT: usize = 2 * 1024 * 1024;
/// 正则 DFA 每线程缓存的最大字节数。
const REGEX_DFA_SIZE_LIMIT: usize = 2 * 1024 * 1024;
/// 正则语法树允许的最大嵌套层数。
const REGEX_NEST_LIMIT: u32 = 250;

/// 在一个已验证的文件或目录目标下搜索文本内容。
pub(super) fn execute(
    workspace_root: &Path,
    request: &GrepRequest,
    cancellation: &CancellationToken,
) -> Result<GrepResponse, RepositoryToolError> {
    ensure_not_cancelled(cancellation)?;
    let matcher = build_matcher(&request.pattern)?;
    let target = resolve_target(workspace_root, &request.path)?;
    let mut response = GrepResponse {
        matches: Vec::new(),
        truncated: false,
    };
    let mut builder = WalkBuilder::new(&target);
    let containment_root = workspace_root.to_path_buf();
    let traversal_cancellation = cancellation.clone();
    builder
        .follow_links(true)
        .hidden(false)
        .parents(false)
        .ignore(true)
        .git_ignore(true)
        .require_git(false)
        .git_global(false)
        .git_exclude(false)
        .filter_entry(move |entry| {
            !traversal_cancellation.is_cancelled()
                && entry
                    .path()
                    .canonicalize()
                    .is_ok_and(|canonical| is_contained(&containment_root, &canonical))
        })
        .sort_by_file_path(|left, right| left.cmp(right));
    // 当搜索起点位于子目录时，显式加入 workspace 根规则；更深层规则仍由 walker 发现。
    for ignore_file in [
        workspace_root.join(".gitignore"),
        workspace_root.join(".ignore"),
    ] {
        if ignore_file.is_file() {
            let _ = builder.add_ignore(ignore_file);
        }
    }
    for entry in builder.build() {
        ensure_not_cancelled(cancellation)?;
        let entry = entry.map_err(|_| RepositoryToolError::Io)?;
        let Ok(canonical) = entry.path().canonicalize() else {
            continue;
        };
        if !is_contained(workspace_root, &canonical) || !canonical.is_file() {
            continue;
        }
        let Some(relative_path) = workspace_relative(workspace_root, entry.path()) else {
            continue;
        };
        for candidate in search_file(&matcher, &canonical, &relative_path, cancellation)? {
            if response.matches.len() >= MAX_RESULTS
                || !push_within_budget(&mut response, candidate)
            {
                response.truncated = true;
                break;
            }
        }
        if response.truncated {
            break;
        }
    }
    ensure_not_cancelled(cancellation)?;
    Ok(response)
}

/// 使用 ripgrep 的正则引擎构建有资源上限的匹配器。
fn build_matcher(pattern: &str) -> Result<RegexMatcher, RepositoryToolError> {
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(RepositoryToolError::InvalidPattern);
    }
    RegexMatcherBuilder::new()
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
        .nest_limit(REGEX_NEST_LIMIT)
        .build(pattern)
        .map_err(|_| RepositoryToolError::InvalidPattern)
}

/// 把 Grep 目标解析为 workspace 内词法路径，并用 canonical identity 证明 containment。
fn resolve_target(
    workspace_root: &Path,
    requested_path: &str,
) -> Result<PathBuf, RepositoryToolError> {
    let requested = Path::new(requested_path);
    let candidate = if requested_path.is_empty() || requested == Path::new(".") {
        workspace_root.to_path_buf()
    } else if requested.is_absolute() {
        rebase_absolute_to_workspace(workspace_root, requested)
            .unwrap_or_else(|| requested.to_path_buf())
    } else {
        workspace_root.join(requested)
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|_| RepositoryToolError::TargetUnavailable)?;
    if !is_contained(workspace_root, &canonical) {
        return Err(RepositoryToolError::OutsideWorkspace);
    }
    let mut traversal_target = normalize_lexical(&candidate);
    if traversal_target.canonicalize().ok().as_ref() != Some(&canonical) {
        let relative = canonical
            .strip_prefix(workspace_root)
            .map_err(|_| RepositoryToolError::OutsideWorkspace)?;
        traversal_target = workspace_root.join(relative);
    }
    Ok(traversal_target)
}

/// 将可能使用平台 alias 拼写的绝对路径重挂到 canonical workspace，同时保留其内部逻辑后缀。
fn rebase_absolute_to_workspace(workspace_root: &Path, requested: &Path) -> Option<PathBuf> {
    for ancestor in requested.ancestors() {
        if ancestor
            .canonicalize()
            .is_ok_and(|canonical| canonical == workspace_root)
        {
            let suffix = requested.strip_prefix(ancestor).ok()?;
            return Some(workspace_root.join(suffix));
        }
    }
    None
}

/// 在不解析 symlink 的前提下消除 `.` 与 `..`，保留请求使用的逻辑 alias。
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// 将已验证的遍历项投影为 workspace-relative POSIX 路径。
fn workspace_relative(workspace_root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(workspace_root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(path_string(relative))
}

/// 使用 ripgrep searcher 流式收集单个已验证文本文件的匹配行。
fn search_file(
    matcher: &RegexMatcher,
    path: &Path,
    relative_path: &str,
    cancellation: &CancellationToken,
) -> Result<Vec<GrepMatch>, RepositoryToolError> {
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .multi_line(false)
        .binary_detection(BinaryDetection::quit(0))
        .build();
    let file = File::open(path).map_err(|_| RepositoryToolError::Io)?;
    let mut reader = ValidatedReader {
        inner: file,
        cancellation,
        utf8_tail: Vec::new(),
        invalid_utf8: false,
    };
    let mut sink = MatchSink {
        path: relative_path,
        cancellation,
        matches: Vec::new(),
        binary: false,
    };
    searcher
        .search_reader(matcher, &mut reader, &mut sink)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::Interrupted {
                RepositoryToolError::Cancelled
            } else if error.kind() == io::ErrorKind::InvalidData {
                RepositoryToolError::NonTextContent
            } else {
                RepositoryToolError::Io
            }
        })
        .or_else(|error| {
            if error == RepositoryToolError::NonTextContent {
                Ok(())
            } else {
                Err(error)
            }
        })?;
    if sink.binary || reader.invalid_utf8 || !reader.utf8_tail.is_empty() {
        return Ok(Vec::new());
    }
    Ok(sink.matches)
}

/// 在 ripgrep 每次读取时检查取消并验证完整 UTF-8 字节流。
struct ValidatedReader<'a> {
    inner: File,
    cancellation: &'a CancellationToken,
    utf8_tail: Vec<u8>,
    invalid_utf8: bool,
}

impl Read for ValidatedReader<'_> {
    /// 读取下一块字节；非法 UTF-8 会使当前文件的暂存匹配整体回滚。
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancellation.is_cancelled() {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        let count = self.inner.read(buffer)?;
        if count == 0 {
            if !self.utf8_tail.is_empty() {
                self.invalid_utf8 = true;
                return Err(io::Error::from(io::ErrorKind::InvalidData));
            }
            return Ok(0);
        }
        if super::read::validate_utf8_chunk(&buffer[..count], &mut self.utf8_tail).is_err() {
            self.invalid_utf8 = true;
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        Ok(count)
    }
}

/// 把 ripgrep 匹配回调转换为稳定的 Repository Grep 结果。
struct MatchSink<'a> {
    path: &'a str,
    cancellation: &'a CancellationToken,
    matches: Vec<GrepMatch>,
    binary: bool,
}

impl Sink for MatchSink<'_> {
    type Error = io::Error;
    /// 接收一条完整匹配行，并在每次回调时检查取消状态。
    fn matched(
        &mut self,
        _searcher: &Searcher,
        matched: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        if self.cancellation.is_cancelled() {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        let line = std::str::from_utf8(matched.bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        let line = line.strip_suffix('\n').unwrap_or(line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if self.matches.len() <= MAX_RESULTS {
            self.matches.push(GrepMatch {
                path: self.path.to_owned(),
                line_number: matched.line_number().unwrap_or(1) as usize,
                line: line.to_owned(),
            });
        }
        // 即使结果预算已满也要读到文件末尾，才能保证后部二进制或非法 UTF-8 会回滚整文件。
        Ok(true)
    }

    /// ripgrep 发现 NUL 时标记整份文件为二进制并停止搜索。
    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        _binary_byte_offset: u64,
    ) -> Result<bool, Self::Error> {
        self.binary = true;
        Ok(false)
    }
}

/// 在最终 JSON 硬预算内加入匹配；必要时仅截断末条文本。
fn push_within_budget(response: &mut GrepResponse, candidate: GrepMatch) -> bool {
    response.matches.push(candidate);
    if serialized_len(response) <= MAX_RESPONSE_BYTES {
        return true;
    }
    let candidate = response.matches.pop().expect("刚加入的匹配必须存在");
    let mut boundaries: Vec<usize> = candidate
        .line
        .char_indices()
        .map(|(index, _)| index)
        .collect();
    boundaries.push(candidate.line.len());
    let (mut low, mut high) = (0, boundaries.len());
    while low + 1 < high {
        let middle = (low + high) / 2;
        let mut shortened = candidate.clone();
        shortened.line.truncate(boundaries[middle]);
        response.matches.push(shortened);
        if serialized_len(response) <= MAX_RESPONSE_BYTES {
            low = middle;
        } else {
            high = middle;
        }
        response.matches.pop();
    }
    if boundaries[low] > 0 {
        let mut shortened = candidate;
        shortened.line.truncate(boundaries[low]);
        response.matches.push(shortened);
    }
    false
}

/// 计算调用方最终收到的 GrepResponse JSON UTF-8 字节数。
fn serialized_len(response: &GrepResponse) -> usize {
    serde_json::to_vec(response).map_or(usize::MAX, |json| json.len())
}
