//! Claude Code executable 候选的纯发现策略。

use std::path::PathBuf;

/// 保持 bare PATH 优先，并把用户选择与环境覆盖作为有序 fallback。
pub(super) fn candidate_paths(
    selected: Option<PathBuf>,
    environment: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from("claude")];
    paths.extend(selected);
    paths.extend(environment);
    let mut seen = std::collections::HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    paths
}
