use git2::{Delta, DiffFindOptions, DiffOptions, Repository};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedFileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub status: ChangeStatus,
    pub content_kind: ContentKind,
    pub before_size: Option<usize>,
    pub after_size: Option<usize>,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContentKind {
    Text,
    Binary,
    Submodule,
    Symlink,
}

fn resolved_content_kind(kinds: impl IntoIterator<Item = Option<ContentKind>>) -> ContentKind {
    kinds
        .into_iter()
        .flatten()
        .max_by_key(|kind| match kind {
            ContentKind::Submodule => 3,
            ContentKind::Symlink => 2,
            ContentKind::Binary => 1,
            ContentKind::Text => 0,
        })
        .unwrap_or(ContentKind::Text)
}

pub mod github {
    use super::{ChangeStatus, ContentKind, LoadedFileChange};
    use base64::Engine;
    use reqwest::blocking::Client;
    use serde::Deserialize;

    const INVALID_PULL_REQUEST_URL: &str =
        "请输入完整的 GitHub Pull Request URL，例如 https://github.com/owner/repo/pull/123";

    #[derive(Debug, PartialEq)]
    pub struct PullRequestLocation {
        pub owner: String,
        pub repository: String,
        pub number: u64,
    }

    #[derive(Debug, PartialEq)]
    pub struct ParsePullRequestLocationError;

    impl std::fmt::Display for ParsePullRequestLocationError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(INVALID_PULL_REQUEST_URL)
        }
    }

    impl std::error::Error for ParsePullRequestLocationError {}

    impl PullRequestLocation {
        pub fn parse(value: &str) -> Result<Self, ParsePullRequestLocationError> {
            let path = value
                .strip_prefix("https://github.com/")
                .ok_or(ParsePullRequestLocationError)?;
            let parts = path.split('/').collect::<Vec<_>>();
            let [owner, repository, "pull", number] = parts.as_slice() else {
                return Err(ParsePullRequestLocationError);
            };
            if owner.is_empty() || repository.is_empty() {
                return Err(ParsePullRequestLocationError);
            }
            let number = number.parse().map_err(|_| ParsePullRequestLocationError)?;
            if number == 0 {
                return Err(ParsePullRequestLocationError);
            }
            Ok(Self {
                owner: (*owner).to_owned(),
                repository: (*repository).to_owned(),
                number,
            })
        }
    }

    #[derive(Clone, Debug)]
    pub struct GitHubSnapshot {
        pub owner: String,
        pub repository: String,
        pub title: String,
        pub number: u64,
        pub author: String,
        pub state: String,
        pub draft: bool,
        pub base_ref: String,
        pub base_revision: String,
        pub comparison_base: String,
        pub head_ref: String,
        pub head_revision: String,
        pub description: String,
        pub changed_files: usize,
        pub additions: usize,
        pub deletions: usize,
        pub file_changes: Vec<GitHubFileChange>,
        pub github_url: String,
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct GitHubFileChange {
        pub path: String,
        pub old_path: Option<String>,
        pub status: ChangeStatus,
    }

    #[derive(Debug)]
    pub struct GitHubError(String);

    impl std::fmt::Display for GitHubError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(&self.0)
        }
    }

    impl std::error::Error for GitHubError {}

    impl From<ParsePullRequestLocationError> for GitHubError {
        fn from(error: ParsePullRequestLocationError) -> Self {
            Self(error.to_string())
        }
    }

    #[derive(Debug)]
    pub struct GitHubOpenedReview {
        api_base_url: String,
        client: Client,
        snapshot: GitHubSnapshot,
    }

    impl GitHubOpenedReview {
        pub fn open(api_base_url: String, url: &str) -> Result<Self, GitHubError> {
            let api_base_url = api_base_url.trim_end_matches('/').to_owned();
            let client = Client::builder()
                .user_agent("Narrant/0.1")
                .build()
                .expect("build GitHub HTTP client");
            let snapshot = load_pull_request(&api_base_url, &client, url)?;
            Ok(Self {
                api_base_url,
                client,
                snapshot,
            })
        }

        pub fn snapshot(&self) -> &GitHubSnapshot {
            &self.snapshot
        }

        pub fn read_file_change(&self, path: &str) -> Result<LoadedFileChange, GitHubError> {
            self.read_file_change_from_snapshot(&self.snapshot, path)
        }
    }

    fn load_pull_request(
        api_base_url: &str,
        client: &Client,
        url: &str,
    ) -> Result<GitHubSnapshot, GitHubError> {
        let location = PullRequestLocation::parse(url)?;
        let endpoint = format!(
            "{}/repos/{}/{}/pulls/{}",
            api_base_url, location.owner, location.repository, location.number
        );
        let response = client
            .get(endpoint)
            .send()
            .map_err(|_| GitHubError("当前无法连接 GitHub，请检查网络后重试。".to_owned()))?;
        if !response.status().is_success() {
            return Err(map_response_error(response.status().as_u16()));
        }
        let metadata: PullRequestResponse = response
            .json()
            .map_err(|_| GitHubError("GitHub 返回了无法识别的数据，请稍后重试。".to_owned()))?;
        let github_url = format!(
            "https://github.com/{}/{}/pull/{}",
            location.owner, location.repository, location.number
        );
        if metadata.changed_files > 3000 {
            return Err(GitHubError(format!(
                    "此 Pull Request 有 {} 个 File Change，超过 GitHub API 的 3000 个上限，无法保证审核完整。请在 GitHub 查看：{}",
                    metadata.changed_files, github_url
                )));
        }

        let comparison_endpoint = format!(
            "{}/repos/{}/{}/compare/{}...{}",
            api_base_url, location.owner, location.repository, metadata.base.sha, metadata.head.sha
        );
        let comparison_response = client
            .get(comparison_endpoint)
            .send()
            .map_err(|_| GitHubError("当前无法连接 GitHub，请检查网络后重试。".to_owned()))?;
        if !comparison_response.status().is_success() {
            return Err(map_response_error(comparison_response.status().as_u16()));
        }
        let comparison: ComparisonResponse = comparison_response
            .json()
            .map_err(|_| GitHubError("GitHub 返回了无法识别的数据，请稍后重试。".to_owned()))?;

        let mut file_changes = Vec::with_capacity(metadata.changed_files);
        let mut page = 1;
        while file_changes.len() < metadata.changed_files {
            let endpoint = format!(
                "{}/repos/{}/{}/pulls/{}/files",
                api_base_url, location.owner, location.repository, location.number
            );
            let response = client
                .get(endpoint)
                .query(&[("per_page", 100), ("page", page)])
                .send()
                .map_err(|_| GitHubError("当前无法连接 GitHub，请检查网络后重试。".to_owned()))?;
            if !response.status().is_success() {
                return Err(map_response_error(response.status().as_u16()));
            }
            let page_changes: Vec<FileResponse> = response
                .json()
                .map_err(|_| GitHubError("GitHub 返回了无法识别的数据，请稍后重试。".to_owned()))?;
            if page_changes.is_empty() {
                break;
            }
            file_changes.extend(page_changes.into_iter().map(file_change));
            page += 1;
        }
        if file_changes.len() != metadata.changed_files {
            return Err(GitHubError(
                "GitHub 返回的 File Change 列表不完整，请稍后重试或在 GitHub 查看。".to_owned(),
            ));
        }

        Ok(GitHubSnapshot {
            owner: location.owner,
            repository: location.repository,
            title: metadata.title,
            number: metadata.number,
            author: metadata.user.login,
            state: if metadata.merged_at.is_some() {
                "merged".to_owned()
            } else {
                metadata.state
            },
            draft: metadata.draft,
            base_ref: metadata.base.reference,
            base_revision: metadata.base.sha.clone(),
            comparison_base: comparison.merge_base_commit.sha,
            head_ref: metadata.head.reference,
            head_revision: metadata.head.sha,
            description: metadata.body.unwrap_or_default(),
            changed_files: metadata.changed_files,
            additions: metadata.additions,
            deletions: metadata.deletions,
            file_changes,
            github_url,
        })
    }

    impl GitHubOpenedReview {
        fn read_file_change_from_snapshot(
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
            let content_kind = super::resolved_content_kind([
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
            let response =
                self.client.get(commit_endpoint).send().map_err(|_| {
                    GitHubError("当前无法连接 GitHub，请检查网络后重试。".to_owned())
                })?;
            if response.status().as_u16() == 404 {
                return Err(GitHubError(
                    "无法读取固定版本中的 File Change（404），请重新打开 Pull Request 后重试。"
                        .to_owned(),
                ));
            }
            if !response.status().is_success() {
                return Err(map_response_error(response.status().as_u16()));
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
                let tree_response = self.client.get(tree_endpoint).send().map_err(|_| {
                    GitHubError("当前无法连接 GitHub，请检查网络后重试。".to_owned())
                })?;
                if !tree_response.status().is_success() {
                    return Err(map_response_error(tree_response.status().as_u16()));
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
            let blob_response =
                self.client.get(blob_endpoint).send().map_err(|_| {
                    GitHubError("当前无法连接 GitHub，请检查网络后重试。".to_owned())
                })?;
            if !blob_response.status().is_success() {
                return Err(map_response_error(blob_response.status().as_u16()));
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

    #[derive(Deserialize)]
    struct PullRequestResponse {
        title: String,
        number: u64,
        user: UserResponse,
        state: String,
        draft: bool,
        merged_at: Option<String>,
        base: RevisionResponse,
        head: RevisionResponse,
        body: Option<String>,
        changed_files: usize,
        additions: usize,
        deletions: usize,
    }

    #[derive(Deserialize)]
    struct UserResponse {
        login: String,
    }

    #[derive(Deserialize)]
    struct RevisionResponse {
        #[serde(rename = "ref")]
        reference: String,
        sha: String,
    }

    #[derive(Deserialize)]
    struct ComparisonResponse {
        merge_base_commit: CommitResponse,
    }

    #[derive(Deserialize)]
    struct CommitResponse {
        sha: String,
    }

    #[derive(Deserialize)]
    struct FileResponse {
        filename: String,
        previous_filename: Option<String>,
        status: String,
    }

    #[derive(Deserialize)]
    struct TreeResponse {
        tree: Vec<TreeEntryResponse>,
    }

    #[derive(Deserialize)]
    struct GitCommitResponse {
        tree: TreeReferenceResponse,
    }

    #[derive(Deserialize)]
    struct TreeReferenceResponse {
        sha: String,
    }

    #[derive(Deserialize)]
    struct TreeEntryResponse {
        path: String,
        mode: String,
        #[serde(rename = "type")]
        kind: String,
        sha: String,
        size: Option<usize>,
    }

    #[derive(Deserialize)]
    struct BlobResponse {
        content: Option<String>,
        encoding: Option<String>,
        size: usize,
    }

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

    fn file_change(response: FileResponse) -> GitHubFileChange {
        GitHubFileChange {
            path: response.filename,
            old_path: response.previous_filename,
            status: match response.status.as_str() {
                "added" => ChangeStatus::Added,
                "removed" => ChangeStatus::Deleted,
                "renamed" => ChangeStatus::Renamed,
                _ => ChangeStatus::Modified,
            },
        }
    }

    fn map_response_error(status: u16) -> GitHubError {
        match status {
            404 => GitHubError(
                "找不到这个公开 Pull Request。请检查 URL，私有仓库需要登录后访问。".to_owned(),
            ),
            403 | 429 => {
                GitHubError("GitHub 匿名访问次数已用尽，请稍后重试或登录后继续。".to_owned())
            }
            _ => GitHubError("GitHub 服务暂时不可用，请稍后重试。".to_owned()),
        }
    }
}

pub mod local {
    use super::*;

    pub struct LocalOpenedReview {
        repository: Repository,
        snapshot: LocalSnapshot,
    }

    impl LocalOpenedReview {
        pub fn open(
            repository_path: &Path,
            base_ref: &str,
            head_ref: &str,
        ) -> Result<Self, git2::Error> {
            let repository = Repository::open(repository_path)?;
            let snapshot = compare_refs(&repository, base_ref, head_ref)?;
            Ok(Self {
                repository,
                snapshot,
            })
        }

        pub fn snapshot(&self) -> &LocalSnapshot {
            &self.snapshot
        }

        pub fn read_file_change(&self, path: &str) -> Result<LoadedFileChange, git2::Error> {
            let stored = self
                .snapshot
                .file_changes
                .iter()
                .find(|change| change.path == path)
                .ok_or_else(|| {
                    git2::Error::from_str("File Change is not part of the Review Snapshot")
                })?;
            let before = read_entry(&self.repository, stored.before_entry.as_ref())?;
            let after = read_entry(&self.repository, stored.after_entry.as_ref())?;
            Ok(LoadedFileChange {
                path: stored.path.clone(),
                old_path: stored.old_path.clone(),
                status: stored.status,
                content_kind: super::resolved_content_kind([
                    before.as_ref().map(|content| content.kind),
                    after.as_ref().map(|content| content.kind),
                ]),
                before_size: before.as_ref().and_then(|entry| entry.size),
                after_size: after.as_ref().and_then(|entry| entry.size),
                before: before.and_then(|entry| entry.text),
                after: after.and_then(|entry| entry.text),
            })
        }
    }

    pub struct LocalSnapshot {
        pub base_ref: String,
        pub head_ref: String,
        pub base_revision: git2::Oid,
        pub comparison_base: git2::Oid,
        pub head_revision: git2::Oid,
        pub file_changes: Vec<LocalFileChange>,
    }

    pub struct LocalFileChange {
        pub path: String,
        pub old_path: Option<String>,
        pub status: ChangeStatus,
        before_entry: Option<LocalEntry>,
        after_entry: Option<LocalEntry>,
    }

    struct LocalEntry {
        object_id: git2::Oid,
        kind: LocalEntryKind,
    }

    enum LocalEntryKind {
        RegularBlob,
        Symlink,
        Submodule,
    }

    pub fn list_refs(repository_path: &Path) -> Result<Vec<String>, git2::Error> {
        let repository = Repository::open(repository_path)?;
        let mut refs = repository
            .branches(None)?
            .map(|branch| {
                let (branch, _) = branch?;
                branch
                    .name()?
                    .map(str::to_owned)
                    .ok_or_else(|| git2::Error::from_str("branch name is not UTF-8"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        refs.sort();
        Ok(refs)
    }

    fn compare_refs(
        repository: &Repository,
        base_ref: &str,
        head_ref: &str,
    ) -> Result<LocalSnapshot, git2::Error> {
        let base_commit = repository.revparse_single(base_ref)?.peel_to_commit()?;
        let head_commit = repository.revparse_single(head_ref)?.peel_to_commit()?;
        let comparison_base_id = repository.merge_base(base_commit.id(), head_commit.id())?;
        let comparison_base = repository.find_commit(comparison_base_id)?;
        let comparison_base_tree = comparison_base.tree()?;
        let head_tree = head_commit.tree()?;
        let mut options = DiffOptions::new();
        let mut diff = repository.diff_tree_to_tree(
            Some(&comparison_base_tree),
            Some(&head_tree),
            Some(&mut options),
        )?;
        let mut find_options = DiffFindOptions::new();
        diff.find_similar(Some(&mut find_options))?;
        let mut file_changes = Vec::new();

        for delta in diff.deltas() {
            file_changes.push(local_file_change(&comparison_base_tree, &head_tree, delta)?);
        }

        Ok(LocalSnapshot {
            base_ref: base_ref.to_owned(),
            head_ref: head_ref.to_owned(),
            base_revision: base_commit.id(),
            comparison_base: comparison_base_id,
            head_revision: head_commit.id(),
            file_changes,
        })
    }

    fn local_file_change(
        comparison_base_tree: &git2::Tree<'_>,
        head_tree: &git2::Tree<'_>,
        delta: git2::DiffDelta<'_>,
    ) -> Result<LocalFileChange, git2::Error> {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .ok_or_else(|| git2::Error::from_str("file change has no path"))?;
        let before_entry = entry_at(comparison_base_tree, delta.old_file().path())?;
        let after_entry = entry_at(head_tree, delta.new_file().path())?;
        Ok(LocalFileChange {
            path: path.to_string_lossy().into_owned(),
            old_path: (delta.status() == Delta::Renamed)
                .then(|| {
                    delta
                        .old_file()
                        .path()
                        .map(|path| path.to_string_lossy().into_owned())
                })
                .flatten(),
            status: status(delta.status()),
            before_entry,
            after_entry,
        })
    }

    fn entry_at(
        tree: &git2::Tree<'_>,
        path: Option<&Path>,
    ) -> Result<Option<LocalEntry>, git2::Error> {
        let Some(path) = path else { return Ok(None) };
        let entry = match tree.get_path(path) {
            Ok(entry) => entry,
            Err(_) => return Ok(None),
        };
        if entry.kind() == Some(git2::ObjectType::Commit) {
            return Ok(Some(LocalEntry {
                object_id: entry.id(),
                kind: LocalEntryKind::Submodule,
            }));
        }
        if entry.kind() != Some(git2::ObjectType::Blob) {
            return Ok(None);
        }
        let kind = if entry.filemode() == 0o120000 {
            LocalEntryKind::Symlink
        } else {
            LocalEntryKind::RegularBlob
        };
        Ok(Some(LocalEntry {
            object_id: entry.id(),
            kind,
        }))
    }

    fn read_entry(
        repository: &Repository,
        entry: Option<&LocalEntry>,
    ) -> Result<Option<EntryContent>, git2::Error> {
        let Some(entry) = entry else { return Ok(None) };
        if matches!(entry.kind, LocalEntryKind::Submodule) {
            return Ok(Some(EntryContent {
                kind: ContentKind::Submodule,
                size: None,
                text: Some(entry.object_id.to_string()),
            }));
        }
        let blob = repository.find_blob(entry.object_id)?;
        let kind = match entry.kind {
            LocalEntryKind::Symlink => ContentKind::Symlink,
            LocalEntryKind::RegularBlob if blob.is_binary() => ContentKind::Binary,
            LocalEntryKind::RegularBlob => ContentKind::Text,
            LocalEntryKind::Submodule => unreachable!(),
        };
        Ok(Some(EntryContent {
            kind,
            size: Some(blob.size()),
            text: (kind != ContentKind::Binary)
                .then(|| std::str::from_utf8(blob.content()).ok().map(str::to_owned))
                .flatten(),
        }))
    }

    struct EntryContent {
        kind: ContentKind,
        size: Option<usize>,
        text: Option<String>,
    }

    /// git2 的底层差异类型转换成自己的 ChangeStatus 枚举
    fn status(delta: Delta) -> ChangeStatus {
        match delta {
            Delta::Added => ChangeStatus::Added,
            Delta::Deleted => ChangeStatus::Deleted,
            Delta::Renamed => ChangeStatus::Renamed,
            _ => ChangeStatus::Modified,
        }
    }
}
