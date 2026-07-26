use super::*;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 统计经由 GitHub Source 令牌提供者边界的调用次数。
#[derive(Default)]
struct TokenProviderSpy(AtomicUsize);

impl GitHubOpenedReview {
    /// 为令牌提供者边界测试构建不发起网络请求的最小 Review。
    pub(crate) fn empty_for_test() -> Self {
        Self {
            client: GitHubClient::new("https://api.github.com".to_owned(), None),
            access_token_provider: None,
            snapshot: GitHubSnapshot {
                owner: "test".to_owned(),
                repository: "test".to_owned(),
                title: "test".to_owned(),
                number: 1,
                author: "test".to_owned(),
                state: "open".to_owned(),
                draft: false,
                base_ref: "main".to_owned(),
                base_revision: "base".to_owned(),
                comparison_base: "base".to_owned(),
                head_ref: "head".to_owned(),
                head_revision: "head".to_owned(),
                description: String::new(),
                changed_files: 0,
                additions: 0,
                deletions: 0,
                file_changes: Vec::new(),
                github_url: "https://github.com/test/test/pull/1".to_owned(),
            },
        }
    }
}

#[async_trait]
impl GitHubAccessTokenProvider for TokenProviderSpy {
    async fn access_token(&self) -> Result<Option<SecretString>, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
}

#[tokio::test]
async fn each_file_change_read_requests_the_current_token_from_its_provider() {
    let provider = Arc::new(TokenProviderSpy::default());
    let mut review = tokio::task::spawn_blocking(GitHubOpenedReview::empty_for_test)
        .await
        .expect("test review worker completes");
    review.access_token_provider = Some(provider.clone());
    for _ in 0..2 {
        let error = review
            .read_file_change_with_access_token_provider("missing.rs".to_owned())
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "这个 File Change 不属于当前 Review Snapshot。"
        );
    }
    assert_eq!(provider.0.load(Ordering::SeqCst), 2);
}
