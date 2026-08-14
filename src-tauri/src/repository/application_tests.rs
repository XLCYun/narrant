use super::*;
use crate::catalog::RepositoryCatalog;
use crate::review::ChangeComparison;
use std::process::Command;
use std::sync::Arc;

#[test]
fn open_overview_stops_at_availability_and_trust_gates() {
    let fixture = Fixture::new();
    let application = fixture.application();

    assert!(matches!(
        application.open_overview(fixture.entry.id).unwrap(),
        RepositoryOpenResult::TrustRequired { .. }
    ));
    assert_eq!(
        application
            .require_trusted_path(fixture.entry.id)
            .unwrap_err(),
        RepositoryApplicationError::WorkspaceTrustRequired
    );

    fixture.remove_repository();
    assert!(matches!(
        application.open_overview(fixture.entry.id).unwrap(),
        RepositoryOpenResult::Unavailable { .. }
    ));
    assert!(matches!(
        application.require_available_path(fixture.entry.id),
        Err(RepositoryApplicationError::Unavailable(_))
    ));
}

#[test]
fn grant_trust_and_open_returns_complete_repository_entry_overview() {
    let fixture = Fixture::new();
    let application = fixture.application();

    let overview = application.grant_trust_and_open(fixture.entry.id).unwrap();

    assert_eq!(overview.repository.id, fixture.entry.id);
    assert_eq!(
        overview.repository.canonical_path,
        fixture.entry.canonical_path
    );
    assert!(overview.reviews.is_empty());
}

#[test]
fn load_catalog_and_reviews_share_the_repository_application_projection() {
    let fixture = Fixture::new();
    fixture.grant_trust();
    let snapshot = ChangeComparison {
        base_ref: "refs/heads/main".to_owned(),
        comparison_base: "base".to_owned(),
        base_revision: "base".to_owned(),
        head_ref: "refs/heads/topic".to_owned(),
        head_revision: "head".to_owned(),
        file_changes: Vec::new(),
    };
    let saved = fixture
        .catalog
        .save_branch_review(fixture.entry.id, &snapshot)
        .unwrap();
    fixture.catalog.activate_snapshot(&saved).unwrap();
    let application = fixture.application();

    let catalog = application.load_catalog().unwrap();
    let reviews = application.load_reviews(fixture.entry.id).unwrap();

    assert_eq!(catalog.repositories, vec![fixture.entry.clone()]);
    assert_eq!(
        catalog.recent_reviews[0].repository_entry_id,
        fixture.entry.id
    );
    assert_eq!(catalog.recent_reviews[0].review.review_id, saved.review_id);
    assert_eq!(reviews.reviews.len(), 1);
    assert_eq!(reviews.reviews[0].review_id, saved.review_id);
    assert_eq!(
        reviews.reviews[0].selection,
        ReviewSelectionSummary::Branch {
            base_ref: "refs/heads/main".to_owned(),
            head_ref: "refs/heads/topic".to_owned(),
        }
    );
    assert_eq!(
        reviews.reviews[0].current_snapshot,
        ReviewSnapshotSummary {
            base_revision: "base".to_owned(),
            comparison_base: "base".to_owned(),
            head_revision: "head".to_owned(),
        }
    );
}

/// 创建一个真实 working-tree 与 SQLite Catalog，覆盖 application seam 的持久化行为。
struct Fixture {
    _application_data: tempfile::TempDir,
    repository: tempfile::TempDir,
    catalog: Arc<RepositoryCatalog>,
    entry: RepositoryEntry,
}

impl Fixture {
    /// 初始化未授予 Workspace Trust 的 Repository Entry。
    fn new() -> Self {
        let application_data = tempfile::TempDir::new().unwrap();
        let repository = test_repository();
        let catalog = Arc::new(
            RepositoryCatalog::open(&application_data.path().join("catalog.sqlite3")).unwrap(),
        );
        let entry = catalog.register(repository.path()).unwrap();
        Self {
            _application_data: application_data,
            repository,
            catalog,
            entry,
        }
    }

    /// 创建与 production 相同的共享 Catalog application。
    fn application(&self) -> RepositoryEntryApplication {
        RepositoryEntryApplication::new(self.catalog.clone())
    }

    /// 授予测试 Repository 后续 projection 所需的 Trust。
    fn grant_trust(&self) {
        self.catalog.grant_workspace_trust(self.entry.id).unwrap();
    }

    /// 删除 working tree，模拟已保存但当前不可用的 Repository Entry。
    fn remove_repository(&self) {
        std::fs::remove_dir_all(self.repository.path()).unwrap();
    }
}

/// 创建一个最小可注册的 working-tree Git repository。
fn test_repository() -> tempfile::TempDir {
    let repository = tempfile::TempDir::new().unwrap();
    run_git(repository.path(), &["init", "-q"]);
    run_git(
        repository.path(),
        &["config", "user.email", "test@narrant.invalid"],
    );
    run_git(repository.path(), &["config", "user.name", "Narrant Test"]);
    std::fs::write(repository.path().join("README.md"), "initial\n").unwrap();
    run_git(repository.path(), &["add", "README.md"]);
    run_git(repository.path(), &["commit", "-q", "-m", "initial"]);
    repository
}

/// 运行测试专用 Git command，并将失败转成可定位 panic。
fn run_git(directory: &std::path::Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        arguments,
        String::from_utf8_lossy(&output.stderr)
    );
}
