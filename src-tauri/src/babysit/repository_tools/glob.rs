use super::{GlobRequest, GlobResponse, RepositoryToolError};
use crate::operation::CancellationSignal;
use std::path::Path;
use walkdir::WalkDir;

/// Glob 单次调用最多返回的路径数量。
const MAX_RESULTS: usize = 500;

/// 从 canonical workspace 根开始遍历并返回稳定排序的匹配路径。
pub(super) fn execute(
    workspace_root: &Path,
    request: &GlobRequest,
    cancellation: &CancellationSignal,
) -> Result<GlobResponse, RepositoryToolError> {
    ensure_not_cancelled(cancellation)?;
    let pattern = Pattern::parse(&request.pattern)?;
    let mut paths = Vec::new();

    for entry in WalkDir::new(workspace_root).follow_links(false) {
        ensure_not_cancelled(cancellation)?;
        let entry = entry.map_err(|_| RepositoryToolError::Io)?;
        let relative = entry
            .path()
            .strip_prefix(workspace_root)
            .map_err(|_| RepositoryToolError::Io)?;
        if relative.as_os_str().is_empty() || !pattern.matches(relative) {
            continue;
        }

        // 模式只作用于 workspace 内的遍历结果；命中项仍需单独 canonicalize，
        // 这样 workspace 内指向外部的 symlink 不会成为越界结果。
        let Ok(canonical) = entry.path().canonicalize() else {
            continue;
        };
        if !is_contained(workspace_root, &canonical) {
            continue;
        }
        paths.push(path_string(relative));
    }

    ensure_not_cancelled(cancellation)?;
    paths.sort_unstable();
    paths.dedup();
    let truncated = paths.len() > MAX_RESULTS;
    paths.truncate(MAX_RESULTS);
    Ok(GlobResponse { paths, truncated })
}

/// 将 Glob 结果限制为 canonical workspace 的后代路径（根目录本身除外）。
fn is_contained(workspace_root: &Path, candidate: &Path) -> bool {
    candidate.strip_prefix(workspace_root).is_ok()
}

/// 把平台路径投影为工具协议中的 workspace-relative POSIX 路径。
fn path_string(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

/// 返回取消状态，统一 Glob 的取消错误。
fn ensure_not_cancelled(cancellation: &CancellationSignal) -> Result<(), RepositoryToolError> {
    cancellation
        .is_cancelled()
        .then_some(RepositoryToolError::Cancelled)
        .map_or(Ok(()), Err)
}

/// 已解析的 Glob 模式；其根始终是 workspace 相对路径。
struct Pattern {
    segments: Vec<String>,
}

impl Pattern {
    /// 规范化分隔符并拒绝空模式，不将模式解释为主机绝对路径。
    fn parse(pattern: &str) -> Result<Self, RepositoryToolError> {
        let normalized = pattern.replace('\\', "/");
        let segments = normalized
            .split('/')
            .filter(|segment| !segment.is_empty() && *segment != ".")
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if segments.is_empty() {
            return Err(RepositoryToolError::InvalidPattern);
        }
        Ok(Self { segments })
    }

    /// 对 workspace-relative 候选路径执行 `*`、`?` 和完整段 `**` 匹配。
    fn matches(&self, path: &Path) -> bool {
        let candidate = path
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        matches_segments(&self.segments, &candidate)
    }
}

/// 用递归段匹配实现 `**` 可跨越零个或多个目录的 Glob 语义。
fn matches_segments(pattern: &[String], candidate: &[String]) -> bool {
    if pattern.is_empty() {
        return candidate.is_empty();
    }
    if pattern[0] == "**" {
        return matches_segments(&pattern[1..], candidate)
            || (!candidate.is_empty() && matches_segments(pattern, &candidate[1..]));
    }
    !candidate.is_empty()
        && segment_matches(&pattern[0], &candidate[0])
        && matches_segments(&pattern[1..], &candidate[1..])
}

/// 在一个路径段中匹配单字符问号和任意字符星号。
fn segment_matches(pattern: &str, candidate: &str) -> bool {
    let pattern = pattern.chars().collect::<Vec<_>>();
    let candidate = candidate.chars().collect::<Vec<_>>();
    let mut table = vec![vec![false; candidate.len() + 1]; pattern.len() + 1];
    table[0][0] = true;
    for (pattern_index, token) in pattern.iter().enumerate() {
        let row = pattern_index + 1;
        for candidate_index in 0..=candidate.len() {
            table[row][candidate_index] = match token {
                '*' => {
                    table[row - 1][candidate_index]
                        || (candidate_index > 0 && table[row][candidate_index - 1])
                }
                '?' => candidate_index > 0 && table[row - 1][candidate_index - 1],
                literal => {
                    candidate_index > 0
                        && *literal == candidate[candidate_index - 1]
                        && table[row - 1][candidate_index - 1]
                }
            };
        }
    }
    table[pattern.len()][candidate.len()]
}
