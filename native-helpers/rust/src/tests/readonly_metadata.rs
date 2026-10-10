use super::*;

/// 数组内每个自定义 target 都按 Cargo 标明的配置出处绝对化，不按 helper cwd 猜测。
#[test]
#[ignore = "需要 NARRANT_TEST_CARGO_194 指定实际1.94工具链"]
fn json_target_paths_use_real_cargo_origins() {
    let toolchain = std::env::var("NARRANT_TEST_CARGO_194").unwrap();
    let root = crate::test_support::fixture();
    let mut config = CargoConfig {
        no_deps: true,
        ..Default::default()
    };
    config
        .extra_env
        .insert("RUSTUP_TOOLCHAIN".into(), Some(toolchain));
    config
        .extra_env
        .insert("RUSTUP_AUTO_INSTALL".into(), Some("0".into()));
    config
        .extra_env
        .insert("CARGO_CACHE_RUSTC_INFO".into(), Some("0".into()));
    let manifest = project_model::ProjectManifest::from_manifest_file(
        vfs::AbsPathBuf::assert_utf8(root.path().join("Cargo.toml").canonicalize().unwrap()),
    )
    .unwrap();
    let workspace = ProjectWorkspace::load(manifest, &config, &|_| ()).unwrap();
    std::fs::create_dir(root.path().join(".cargo")).unwrap();
    std::fs::write(
        root.path().join(".cargo/config.toml"),
        "[build]\ntarget=[\"targets/custom.json\",\"x86_64-unknown-linux-gnu\"]\n",
    )
    .unwrap();
    let targets = effective_targets(&workspace, &config).unwrap();
    assert_eq!(
        targets,
        vec![
            root.path()
                .canonicalize()
                .unwrap()
                .join("targets/custom.json")
                .to_str()
                .unwrap()
                .to_owned(),
            "x86_64-unknown-linux-gnu".into()
        ]
    );
}
