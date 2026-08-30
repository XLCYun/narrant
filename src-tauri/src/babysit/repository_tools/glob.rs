use super::{
    ensure_not_cancelled, is_contained, path_string, GlobRequest, GlobResponse, RepositoryToolError,
};
use crate::operation::CancellationSignal;
use globset::{GlobBuilder, GlobMatcher};
use std::collections::BinaryHeap;
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
    let mut paths = BinaryHeap::with_capacity(MAX_RESULTS + 1);
    let mut truncated = false;

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
        let relative = path_string(relative);
        if paths.len() < MAX_RESULTS + 1 {
            paths.push(relative);
        } else if paths.peek().is_some_and(|largest| relative < *largest) {
            paths.pop();
            paths.push(relative);
        }
        if paths.len() > MAX_RESULTS {
            truncated = true;
        }
    }

    ensure_not_cancelled(cancellation)?;
    let mut paths = paths.into_vec();
    paths.sort_unstable();
    paths.dedup();
    truncated |= paths.len() > MAX_RESULTS;
    paths.truncate(MAX_RESULTS);
    Ok(GlobResponse { paths, truncated })
}

/// 已解析的 Glob 模式；其根始终是 workspace 相对路径。
struct Pattern {
    matcher: GlobMatcher,
}

impl Pattern {
    /// 规范化分隔符并拒绝空模式，不将模式解释为主机绝对路径。
    fn parse(pattern: &str) -> Result<Self, RepositoryToolError> {
        let segments = pattern
            .replace('\\', "/")
            .split('/')
            .filter(|segment| !segment.is_empty() && *segment != ".")
            .map(compile_segment)
            .collect::<Vec<_>>();
        if segments.is_empty() {
            return Err(RepositoryToolError::InvalidPattern);
        }
        let normalized = segments.join("/");
        let matcher = GlobBuilder::new(&normalized)
            .literal_separator(true)
            .build()
            .map_err(|_| RepositoryToolError::InvalidPattern)?
            .compile_matcher();
        Ok(Self { matcher })
    }

    /// 对 workspace-relative 候选路径执行 `*`、`?` 和完整段 `**` 匹配。
    fn matches(&self, path: &Path) -> bool {
        let Some(candidate) = path
            .components()
            .map(|component| component.as_os_str().to_str())
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        self.matcher.is_match(candidate.join("/"))
    }
}

/// 仅保留公开契约中的 `*`、`?` 与完整段 `**`，其它 glob 元字符按字面量处理。
fn compile_segment(segment: &str) -> String {
    if segment == "**" {
        return segment.to_owned();
    }
    let mut compiled = String::new();
    let mut previous_star = false;
    for character in segment.chars() {
        match character {
            '*' if !previous_star => compiled.push('*'),
            '*' => {}
            '?' => compiled.push('?'),
            literal => compiled.push_str(&globset::escape(&literal.to_string())),
        }
        previous_star = character == '*';
    }
    compiled
}

/// 测试模式匹配边界，不需要在文件系统中创建不可表示的路径。
#[cfg(test)]
pub(super) fn matches_pattern_for_test(
    pattern: &str,
    path: &Path,
) -> Result<bool, RepositoryToolError> {
    Pattern::parse(pattern).map(|pattern| pattern.matches(path))
}
