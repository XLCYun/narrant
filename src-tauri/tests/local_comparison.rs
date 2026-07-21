use git2::{build::CheckoutBuilder, Repository, Signature};
use narrant_lib::change_content::local::{compare_refs, list_refs};
use tempfile::TempDir;

fn commit_file(repository: &Repository, path: &str, contents: &str, message: &str) -> git2::Oid {
    let workdir = repository.workdir().expect("fixture has a worktree");
    std::fs::write(workdir.join(path), contents).expect("write fixture file");
    let mut index = repository.index().expect("open index");
    index.add_path(std::path::Path::new(path)).expect("stage file");
    index.write().expect("write index");
    let tree_id = index.write_tree().expect("write tree");
    let tree = repository.find_tree(tree_id).expect("find tree");
    let signature = Signature::now("Narrant test", "test@narrant.invalid").expect("signature");
    let parents = repository.head().ok().and_then(|head| head.peel_to_commit().ok()).into_iter().collect::<Vec<_>>();
    repository.commit(Some("HEAD"), &signature, &signature, message, &tree, &parents.iter().collect::<Vec<_>>()).expect("commit")
}

#[test]
fn compares_committed_base_and_head_refs_as_a_pinned_file_change() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    commit_file(&repository, "welcome.txt", "你好，旧内容\n", "base");
    repository.branch("base", &repository.head().unwrap().peel_to_commit().unwrap(), true).unwrap();
    repository.branch("head", &repository.head().unwrap().peel_to_commit().unwrap(), true).unwrap();
    repository.set_head("refs/heads/head").unwrap();
    repository.checkout_head(Some(CheckoutBuilder::new().force())).unwrap();
    let expected_head = commit_file(&repository, "welcome.txt", "你好，新内容\n", "head");

    let refs = list_refs(fixture.path()).expect("list refs");
    assert_eq!(refs, vec!["base", "head", "master"]);

    let snapshot = compare_refs(fixture.path(), "base", "head").expect("compare refs");

    assert_eq!(snapshot.base_ref, "base");
    assert_eq!(snapshot.head_ref, "head");
    assert_eq!(snapshot.comparison_base, snapshot.base_revision);
    assert_eq!(snapshot.head_revision, expected_head.to_string());
    assert_eq!(snapshot.file_changes.len(), 1);
    assert_eq!(snapshot.file_changes[0].path, "welcome.txt");
    assert_eq!(snapshot.file_changes[0].before.as_deref(), Some("你好，旧内容\n"));
    assert_eq!(snapshot.file_changes[0].after.as_deref(), Some("你好，新内容\n"));
    assert_eq!(snapshot.file_changes[0].status, "modified");
}
