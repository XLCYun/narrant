#[test]
fn preserves_platform_supported_special_paths_in_nul_delimited_output() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    commit_file(&repository, "seed.txt", "seed\n", "base");
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("base", &base, true).unwrap();

    #[cfg(unix)]
    let special_paths = vec!["space name.txt", "line\nbreak.txt", "你好.txt"];
    #[cfg(windows)]
    let special_paths = vec!["space name.txt", "你好.txt"];
    for path in special_paths {
        commit_file(&repository, path, "special\n", "special path");
    }
    let head = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("head", &head, true).unwrap();

    let review = open_comparison(fixture.path());
    let mut paths = review
        .comparison
        .file_changes
        .iter()
        .map(|change| change.path.as_str())
        .collect::<Vec<_>>();
    paths.sort();

    #[cfg(unix)]
    assert_eq!(paths, vec!["line\nbreak.txt", "space name.txt", "你好.txt"]);
    #[cfg(windows)]
    assert_eq!(paths, vec!["space name.txt", "你好.txt"]);
    let readable_path = if cfg!(unix) {
        "line\nbreak.txt"
    } else {
        "你好.txt"
    };
    assert_eq!(
        review
            .read(readable_path)
            .unwrap()
            .after
            .as_ref()
            .and_then(FileContent::text),
        Some("special\n")
    );
}

#[test]
fn distinguishes_executable_mode_changes_in_snapshot_metadata() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    let blob = repository.blob(b"#!/bin/sh\n").unwrap();
    commit_file(&repository, "seed.txt", "seed\n", "seed");
    commit_tree_entry(&repository, "script.sh", blob, 0o100644, "non executable");
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("base", &base, true).unwrap();
    commit_tree_entry(&repository, "script.sh", blob, 0o100755, "executable");
    let head = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("head", &head, true).unwrap();

    let review = open_comparison(fixture.path());
    let change = &review.comparison.file_changes[0];

    assert_eq!(change.status, FileChangeStatus::Modified);
    assert_eq!(
        change.before.as_ref().map(|content| content.mode),
        Some(0o100644)
    );
    assert_eq!(
        change.after.as_ref().map(|content| content.mode),
        Some(0o100755)
    );
    assert_eq!(
        change.before.as_ref().map(|content| &content.object_id),
        change.after.as_ref().map(|content| &content.object_id)
    );
}

#[test]
fn recovers_after_a_failed_revision_resolution() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    commit_file(&repository, "README.md", "same\n", "initial");
    let commit = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("base", &commit, true).unwrap();
    repository.branch("head", &commit, true).unwrap();

    assert!(resolve_comparison(fixture.path(), "refs/heads/missing", "refs/heads/head").is_err());
    let recovered = open_comparison(fixture.path());

    assert!(recovered.comparison.file_changes.is_empty());
}

#[test]
fn compares_canonical_refs_in_a_real_shallow_clone() {
    let source_fixture = TempDir::new().expect("source directory");
    let source = Repository::init(source_fixture.path()).expect("initialize source");
    commit_file(&source, "README.md", "base\n", "base");
    let base = source.head().unwrap().peel_to_commit().unwrap();
    source.branch("base", &base, true).unwrap();
    commit_file(&source, "README.md", "head\n", "head");
    let head = source.head().unwrap().peel_to_commit().unwrap();
    source.branch("head", &head, true).unwrap();

    let clone_fixture = TempDir::new().expect("clone parent");
    let clone_path = clone_fixture.path().join("shallow");
    let source_url = format!("file://{}", source_fixture.path().display());
    let status = Command::new("git")
        .args(["clone", "--quiet", "--depth=1", "--branch=head"])
        .arg(source_url)
        .arg(&clone_path)
        .status()
        .expect("run shallow clone");
    assert!(status.success());

    let refs = branch_refs(&clone_path).expect("list shallow refs").refs;
    assert!(refs.contains(&"refs/heads/head".to_owned()));
    assert!(refs.contains(&"refs/remotes/origin/head".to_owned()));
    let review = resolve_comparison(&clone_path, "refs/heads/head", "refs/remotes/origin/head")
        .expect("compare shallow refs");

    assert_eq!(
        review.comparison.base_revision,
        review.comparison.head_revision
    );
    assert!(review.comparison.file_changes.is_empty());
}

#[test]
fn selected_remote_fetch_observes_force_push_without_credentials_injection() {
    let remote_fixture = TempDir::new().expect("remote directory");
    let remote = Repository::init_bare(remote_fixture.path()).expect("initialize bare remote");
    let source_fixture = TempDir::new().expect("source directory");
    let source = Repository::init(source_fixture.path()).expect("initialize source");
    commit_file(&source, "README.md", "first\n", "first");
    source
        .remote("origin", remote_fixture.path().to_str().unwrap())
        .unwrap();
    assert!(Command::new("git")
        .args(["push", "origin", "HEAD:refs/heads/topic"])
        .current_dir(source_fixture.path())
        .status()
        .unwrap()
        .success());
    let clone_fixture = TempDir::new().expect("clone parent");
    let clone_path = clone_fixture.path().join("clone");
    assert!(Command::new("git")
        .args(["clone", "--quiet"])
        .arg(remote_fixture.path())
        .arg(&clone_path)
        .status()
        .unwrap()
        .success());
    let old_head = remote.refname_to_id("refs/heads/topic").unwrap();

    std::fs::write(source_fixture.path().join("README.md"), "forced\n").unwrap();
    assert!(Command::new("git")
        .args(["add", "README.md"])
        .current_dir(source_fixture.path())
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args([
            "-c",
            "user.name=Narrant Test",
            "-c",
            "user.email=test@narrant.invalid",
            "commit",
            "--amend",
            "--quiet",
            "-m",
            "forced",
        ])
        .current_dir(source_fixture.path())
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["push", "--force", "origin", "HEAD:refs/heads/topic"])
        .current_dir(source_fixture.path())
        .status()
        .unwrap()
        .success());
    let forced_head = source.head().unwrap().target().unwrap();

    resolve_comparison(
        &clone_path,
        "refs/remotes/origin/topic",
        "refs/remotes/origin/topic",
    )
    .expect("fetch selected remote");

    let clone = Repository::open(&clone_path).unwrap();
    assert_eq!(
        clone.refname_to_id("refs/remotes/origin/topic").unwrap(),
        forced_head
    );
    assert_ne!(forced_head, old_head);
}

#[test]
fn selected_remote_fetch_reports_transport_failure_without_moving_tracking_ref() {
    let fixture = TempDir::new().expect("repository directory");
    let repository = Repository::init(fixture.path()).expect("initialize repository");
    let head = commit_file(&repository, "README.md", "stable\n", "stable");
    repository
        .reference("refs/remotes/origin/topic", head, true, "fixture")
        .unwrap();
    repository
        .remote("origin", "/definitely/missing/narrant-remote")
        .unwrap();

    let error = resolve_comparison(
        fixture.path(),
        "refs/remotes/origin/topic",
        "refs/remotes/origin/topic",
    )
    .err()
    .expect("missing remote must fail");

    assert!(error.to_string().contains("Core Git 失败"));
    assert_eq!(
        repository
            .refname_to_id("refs/remotes/origin/topic")
            .unwrap(),
        head
    );
}

#[test]
fn selected_remote_fetch_reports_http_authentication_failure_without_moving_ref() {
    let server = MockServer::start();
    let unauthorized = server.mock(|when, then| {
        when.method(GET);
        then.status(401)
            .header("WWW-Authenticate", "Basic realm=\"Narrant test\"");
    });
    let fixture = TempDir::new().expect("repository directory");
    let repository = Repository::init(fixture.path()).expect("initialize repository");
    let head = commit_file(&repository, "README.md", "stable\n", "stable");
    repository
        .reference("refs/remotes/origin/topic", head, true, "fixture")
        .unwrap();
    repository
        .remote("origin", &server.url("/private.git"))
        .unwrap();

    let error = resolve_comparison(
        fixture.path(),
        "refs/remotes/origin/topic",
        "refs/remotes/origin/topic",
    )
    .err()
    .expect("authentication failure must fail");

    assert!(error.to_string().contains("Core Git 失败"));
    assert_eq!(
        repository
            .refname_to_id("refs/remotes/origin/topic")
            .unwrap(),
        head
    );
    assert!(unauthorized.hits() > 0);
}

#[test]
fn comparison_without_a_common_ancestor_fails_without_mutating_refs() {
    let fixture = TempDir::new().expect("repository directory");
    let repository = Repository::init(fixture.path()).expect("initialize repository");
    let base = commit_file(&repository, "base.txt", "base\n", "base");
    repository
        .branch("base", &repository.find_commit(base).unwrap(), true)
        .unwrap();
    assert!(Command::new("git")
        .args(["checkout", "--orphan", "head"])
        .current_dir(fixture.path())
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["rm", "-rf", "--quiet", "."])
        .current_dir(fixture.path())
        .status()
        .unwrap()
        .success());
    commit_file(&repository, "head.txt", "head\n", "head");
    let head = repository.head().unwrap().target().unwrap();

    let error = match resolve_comparison(fixture.path(), "refs/heads/base", "refs/heads/head") {
        Ok(_) => panic!("unrelated histories must not produce a comparison"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("Core Git"));
    assert_eq!(repository.refname_to_id("refs/heads/base").unwrap(), base);
    assert_eq!(repository.refname_to_id("refs/heads/head").unwrap(), head);
}
