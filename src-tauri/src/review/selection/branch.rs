use crate::core_git::{GitCommandError, GitOutput, GitProcessAdapter};
use crate::operation::CancellationSignal;
use crate::operation::OperationContext;
use crate::review::{build_comparison, ChangeComparison, ResolvedRevisions};
use std::path::Path;

/// Review 持久化的 canonical Branch Selection identity。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchSelection {
    pub base_ref: String,
    pub head_ref: String,
}

/// 一次 Branch Selection resolution 固定下来的不可变 Change Comparison。
pub struct ResolvedBranchSelection {
    pub comparison: ChangeComparison,
}

/// Repository 的 canonical 分支选项及 Git 明确报告的默认、当前分支。
#[derive(Debug, PartialEq, Eq)]
pub struct BranchRefs {
    pub refs: Vec<String>,
    pub default_ref: String,
    pub current_ref: String,
}

/// Branch Selection resolution 的稳定失败类别。
#[derive(Debug)]
pub enum BranchSelectionResolutionError {
    Git(GitCommandError),
}

impl BranchSelectionResolutionError {
    /// 返回失败是否由合作式取消产生。
    pub fn is_cancelled(&self) -> bool {
        match self {
            Self::Git(error) => error.is_cancelled(),
        }
    }
}

impl std::fmt::Display for BranchSelectionResolutionError {
    /// 输出供应用层展示的 resolution 诊断。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Git(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for BranchSelectionResolutionError {}

/// 将 selected-remote fetch、ref resolution 与比较计算隐藏在一个 seam 后。
#[derive(Clone, Copy, Default)]
pub struct BranchSelectionResolver;

impl BranchSelectionResolver {
    /// 将稳定 Branch Selection 解析为当前且已在本地验证的 Snapshot。
    pub async fn resolve(
        &self,
        operation: &OperationContext,
        repository_path: &Path,
        selection: &BranchSelection,
    ) -> Result<ResolvedBranchSelection, BranchSelectionResolutionError> {
        validate_branch_ref(&selection.base_ref).map_err(BranchSelectionResolutionError::Git)?;
        validate_branch_ref(&selection.head_ref).map_err(BranchSelectionResolutionError::Git)?;
        let adapter = GitProcessAdapter::new(repository_path);
        fetch_selected_remotes(&adapter, selection, operation.cancellation())
            .map_err(BranchSelectionResolutionError::Git)?;
        let base_revision = resolve_commit(&adapter, &selection.base_ref, operation.cancellation())
            .map_err(BranchSelectionResolutionError::Git)?;
        let head_revision = resolve_commit(&adapter, &selection.head_ref, operation.cancellation())
            .map_err(BranchSelectionResolutionError::Git)?;
        let comparison = build_comparison(
            operation,
            repository_path,
            ResolvedRevisions {
                base_ref: selection.base_ref.clone(),
                head_ref: selection.head_ref.clone(),
                base_revision,
                head_revision,
            },
        )
        .map_err(BranchSelectionResolutionError::Git)?;
        Ok(ResolvedBranchSelection { comparison })
    }
}

/// 通过一个 interface 枚举 Branch Selection 所需的 canonical refs。
#[derive(Clone, Copy, Default)]
pub struct BranchRefCatalog;

impl BranchRefCatalog {
    /// 返回分支选项及显式默认、当前选择。
    pub async fn list(
        &self,
        operation: &OperationContext,
        repository_path: &Path,
    ) -> Result<BranchRefs, GitCommandError> {
        let cancellation = operation.cancellation();
        let adapter = GitProcessAdapter::new(repository_path);
        let refs = list_canonical_refs(&adapter, cancellation)?;
        let current_ref = text_output(
            adapter
                .run(["symbolic-ref", "--quiet", "HEAD"], cancellation)
                .map_err(|_| GitCommandError::new("Repository 当前未检出本地分支"))?,
            "当前分支",
        )?;
        if !refs.contains(&current_ref) {
            return Err(GitCommandError::new(
                "Repository 当前分支不是 canonical 本地分支",
            ));
        }
        let default_ref = default_branch_ref(&adapter, &refs, &current_ref, cancellation)?;
        Ok(BranchRefs {
            refs,
            default_ref,
            current_ref,
        })
    }
}

fn validate_branch_ref(reference: &str) -> Result<(), GitCommandError> {
    if reference.starts_with("refs/heads/") || reference.starts_with("refs/remotes/") {
        Ok(())
    } else {
        Err(GitCommandError::new(
            "只允许 canonical local 或 remote-tracking branch ref",
        ))
    }
}

fn fetch_selected_remotes(
    adapter: &GitProcessAdapter,
    selection: &BranchSelection,
    cancellation: &CancellationSignal,
) -> Result<(), GitCommandError> {
    let mut remotes = [&selection.base_ref, &selection.head_ref]
        .into_iter()
        .filter_map(|reference| remote_name(reference))
        .collect::<Vec<_>>();
    remotes.sort_unstable();
    remotes.dedup();
    for remote in remotes {
        adapter.run_network(
            ["fetch", "--no-write-fetch-head", "--", remote],
            cancellation,
        )?;
    }
    Ok(())
}

fn remote_name(reference: &str) -> Option<&str> {
    reference
        .strip_prefix("refs/remotes/")?
        .split_once('/')
        .map(|(remote, _)| remote)
        .filter(|remote| !remote.is_empty())
}

fn resolve_commit(
    adapter: &GitProcessAdapter,
    reference: &str,
    cancellation: &CancellationSignal,
) -> Result<String, GitCommandError> {
    adapter.run(["check-ref-format", reference], cancellation)?;
    let referenced_object = text_output(
        adapter.run(["show-ref", "--verify", "--hash", reference], cancellation)?,
        "ref object",
    )?;
    validate_object_id(&referenced_object)?;
    let revision = format!("{referenced_object}^{{commit}}");
    let object_id = text_output(
        adapter.run(["rev-parse", "--verify", &revision], cancellation)?,
        "revision",
    )?;
    validate_object_id(&object_id)?;
    Ok(object_id)
}

fn list_canonical_refs(
    adapter: &GitProcessAdapter,
    cancellation: &CancellationSignal,
) -> Result<Vec<String>, GitCommandError> {
    let output = adapter.run(
        [
            "for-each-ref",
            "--format=%(refname)%00%(symref)%00",
            "refs/heads",
            "refs/remotes",
        ],
        cancellation,
    )?;
    let mut fields = output.stdout.split(|byte| *byte == 0);
    let mut refs = Vec::new();
    while let Some(reference) = fields.next() {
        let reference = trim_record_prefix(reference);
        if reference.is_empty() {
            break;
        }
        let Some(symref) = fields.next() else {
            return Err(GitCommandError::new("branch 枚举输出字段不完整"));
        };
        if symref.is_empty() {
            refs.push(
                String::from_utf8(reference)
                    .map_err(|_| GitCommandError::new("branch ref 不是有效的 UTF-8"))?,
            );
        }
    }
    refs.sort();
    refs.dedup();
    Ok(refs)
}

fn default_branch_ref(
    adapter: &GitProcessAdapter,
    refs: &[String],
    current_ref: &str,
    cancellation: &CancellationSignal,
) -> Result<String, GitCommandError> {
    let output = adapter.run(
        [
            "for-each-ref",
            "--format=%(refname)%00%(symref)%00",
            "refs/remotes",
        ],
        cancellation,
    )?;
    let mut fields = output.stdout.split(|byte| *byte == 0);
    let mut remote_defaults = Vec::new();
    while let Some(reference) = fields.next() {
        let reference = trim_record_prefix(reference);
        if reference.is_empty() {
            break;
        }
        let Some(symref) = fields.next() else {
            return Err(GitCommandError::new("remote default branch 输出字段不完整"));
        };
        if !symref.is_empty() {
            let source = String::from_utf8(reference)
                .map_err(|_| GitCommandError::new("remote HEAD ref 不是有效的 UTF-8"))?;
            let target = String::from_utf8(symref.to_vec())
                .map_err(|_| GitCommandError::new("remote default branch 不是有效的 UTF-8"))?;
            if refs.contains(&target) {
                remote_defaults.push((source, target));
            }
        }
    }
    remote_defaults
        .sort_by_key(|(source, _)| (!source.starts_with("refs/remotes/origin/"), source.clone()));
    Ok(remote_defaults
        .into_iter()
        .map(|(_, target)| target)
        .next()
        .or_else(|| {
            refs.iter()
                .find(|reference| *reference == "refs/heads/main")
                .cloned()
        })
        .or_else(|| {
            refs.iter()
                .find(|reference| *reference == "refs/heads/master")
                .cloned()
        })
        .unwrap_or_else(|| current_ref.to_owned()))
}

fn trim_record_prefix(record: &[u8]) -> Vec<u8> {
    record
        .iter()
        .copied()
        .skip_while(|byte| *byte == b'\n' || *byte == b'\r')
        .collect()
}

fn text_output(output: GitOutput, label: &str) -> Result<String, GitCommandError> {
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| GitCommandError::new(format!("{label} 输出不是 UTF-8")))?
        .trim();
    if text.is_empty() || text.lines().count() != 1 {
        return Err(GitCommandError::new(format!("{label} 输出格式无效")));
    }
    Ok(text.to_owned())
}

fn validate_object_id(object_id: &str) -> Result<(), GitCommandError> {
    if (40..=64).contains(&object_id.len())
        && object_id.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(GitCommandError::new("Core Git 返回了无效 object ID"))
    }
}

#[cfg(test)]
#[path = "branch_tests.rs"]
mod tests;
