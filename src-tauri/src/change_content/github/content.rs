use super::{ChangeStatus, GitHubClient, GitHubError, GitHubSnapshot, LoadedFileChange};
use crate::change_content::ContentKind;
use base64::Engine;
use serde::Deserialize;

impl GitHubClient {
    pub(super) fn read_file_change(
        &self,
        snapshot: &GitHubSnapshot,
        path: &str,
    ) -> Result<LoadedFileChange, GitHubError> {
        let listed = snapshot
            .file_changes
            .iter()
            .find(|change| change.path == path)
            .ok_or_else(|| {
                GitHubError("这个 File Change 不属于当前 Review Snapshot。".to_owned())
            })?;
        let before_path = listed.old_path.as_deref().unwrap_or(&listed.path);
        let before = if listed.status == ChangeStatus::Added {
            None
        } else {
            Some(self.read_content(snapshot, before_path, &snapshot.comparison_base)?)
        };
        let after = if listed.status == ChangeStatus::Deleted {
            None
        } else {
            Some(self.read_content(snapshot, &listed.path, &snapshot.head_revision)?)
        };
        let content_kind = super::super::resolved_content_kind([
            before.as_ref().map(|content| content.kind),
            after.as_ref().map(|content| content.kind),
        ]);
        Ok(LoadedFileChange {
            path: listed.path.clone(),
            old_path: listed.old_path.clone(),
            status: listed.status,
            content_kind,
            before_size: before.as_ref().and_then(|content| content.size),
            after_size: after.as_ref().and_then(|content| content.size),
            before: before.and_then(|content| content.text),
            after: after.and_then(|content| content.text),
        })
    }

    fn read_content(
        &self,
        snapshot: &GitHubSnapshot,
        path: &str,
        revision: &str,
    ) -> Result<EntryContent, GitHubError> {
        let commit_endpoint = format!(
            "{}/repos/{}/{}/git/commits/{}",
            self.api_base_url, snapshot.owner, snapshot.repository, revision
        );
        let response = self
            .get(commit_endpoint)
            .send()
            .map_err(|_| GitHubError("当前无法连接 GitHub，请检查网络后重试。".to_owned()))?;
        if !response.status().is_success() {
            return Err(self.map_content_error(response.status().as_u16()));
        }
        let commit: GitCommitResponse = response
            .json()
            .map_err(|_| GitHubError("GitHub 文件内容读取失败，请重试。".to_owned()))?;
        let components = path.split('/').collect::<Vec<_>>();
        let mut tree_sha = commit.tree.sha;
        let mut resolved_entry = None;
        for (index, component) in components.iter().enumerate() {
            let tree_endpoint = format!(
                "{}/repos/{}/{}/git/trees/{}",
                self.api_base_url, snapshot.owner, snapshot.repository, tree_sha
            );
            let tree_response = self
                .get(tree_endpoint)
                .send()
                .map_err(|_| GitHubError("当前无法连接 GitHub，请检查网络后重试。".to_owned()))?;
            if !tree_response.status().is_success() {
                return Err(self.map_content_error(tree_response.status().as_u16()));
            }
            let tree: TreeResponse = tree_response
                .json()
                .map_err(|_| GitHubError("GitHub 文件内容读取失败，请重试。".to_owned()))?;
            let entry = tree
                .tree
                .into_iter()
                .find(|entry| entry.path == *component)
                .ok_or_else(|| {
                    GitHubError(
                        "无法读取固定版本中的 File Change（404），请重新打开 Pull Request 后重试。"
                            .to_owned(),
                    )
                })?;
            if index + 1 == components.len() {
                resolved_entry = Some(entry);
                break;
            }
            if entry.kind != "tree" {
                return Err(GitHubError(
                    "GitHub 文件路径无效，请重新打开 Pull Request。".to_owned(),
                ));
            }
            tree_sha = entry.sha;
        }
        let entry =
            resolved_entry.ok_or_else(|| GitHubError("GitHub 文件路径无效。".to_owned()))?;
        if entry.mode == "160000" || entry.kind == "commit" {
            return Ok(EntryContent {
                kind: ContentKind::Submodule,
                size: entry.size,
                text: Some(entry.sha),
            });
        }
        let blob_endpoint = format!(
            "{}/repos/{}/{}/git/blobs/{}",
            self.api_base_url, snapshot.owner, snapshot.repository, entry.sha
        );
        let blob_response = self
            .get(blob_endpoint)
            .send()
            .map_err(|_| GitHubError("当前无法连接 GitHub，请检查网络后重试。".to_owned()))?;
        if !blob_response.status().is_success() {
            return Err(self.map_content_error(blob_response.status().as_u16()));
        }
        let blob: BlobResponse = blob_response
            .json()
            .map_err(|_| GitHubError("GitHub 文件内容读取失败，请重试。".to_owned()))?;
        let size = entry.size.or(Some(blob.size));
        let bytes = blob.decode()?;
        let kind = if entry.mode == "120000" {
            ContentKind::Symlink
        } else if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
            ContentKind::Binary
        } else {
            ContentKind::Text
        };
        Ok(EntryContent {
            kind,
            size,
            text: (kind != ContentKind::Binary)
                .then(|| String::from_utf8(bytes).ok())
                .flatten(),
        })
    }
}

/// Git tree endpoint 的最小响应模型。
#[derive(Deserialize)]
struct TreeResponse {
    tree: Vec<TreeEntryResponse>,
}

/// Git commit endpoint 中指向根 tree 的响应片段。
#[derive(Deserialize)]
struct GitCommitResponse {
    tree: TreeReferenceResponse,
}

/// 仅包含 tree SHA 的响应片段。
#[derive(Deserialize)]
struct TreeReferenceResponse {
    sha: String,
}

/// Git tree 中一个路径分量对应的 entry。
///
/// `mode` 与 `kind` 共同区分普通 blob、符号链接、子目录和 submodule。
#[derive(Deserialize)]
struct TreeEntryResponse {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
    size: Option<usize>,
}

/// Git blob endpoint 返回的 Base64 内容及原始字节数。
#[derive(Deserialize)]
struct BlobResponse {
    content: Option<String>,
    encoding: Option<String>,
    size: usize,
}

/// 从固定 revision 读取并分类后的单侧文件内容。
///
/// 二进制内容只保留类型和大小，不把字节转换为展示文本。
struct EntryContent {
    kind: ContentKind,
    size: Option<usize>,
    text: Option<String>,
}

impl BlobResponse {
    fn decode(self) -> Result<Vec<u8>, GitHubError> {
        match (self.content, self.encoding.as_deref()) {
            (Some(content), Some("base64")) => base64::engine::general_purpose::STANDARD
                .decode(content.replace(['\r', '\n'], ""))
                .map_err(|_| GitHubError("GitHub 文件内容读取失败，请重试。".to_owned())),
            _ => Err(GitHubError("GitHub 文件内容读取失败，请重试。".to_owned())),
        }
    }
}
