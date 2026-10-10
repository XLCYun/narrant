use super::*;
use syntax::AstNode;

use crate::test_support as support;

/// 合法二进制资源拥有 VFS 身份，但公开文本、AST 与导航查询必须返回错误而非泄漏 panic。
#[test]
fn binary_resource_queries_return_errors_from_the_same_snapshot() {
    let root = support::fixture();
    let source =
        "pub fn target() {} pub fn run() { target(); let bytes = include_bytes!(\"bad.rs\"); }";
    std::fs::write(root.path().join("src/lib.rs"), source).unwrap();
    let resource_path = root.path().join("src/bad.rs");
    std::fs::write(&resource_path, [0xff, 0xfe]).unwrap();
    let before = support::snapshot(root.path());
    let project = RustProject::load(
        &root.path().join("Cargo.toml"),
        &LoadPolicy::repository_only(),
    )
    .unwrap();
    let resource = project.file_id(&resource_path).unwrap();
    assert_eq!(
        project.file_path(resource).unwrap(),
        resource_path.canonicalize().unwrap()
    );
    support::assert_unchanged(root.path(), &before);

    // 源码和资源的后续磁盘变化都不能补写到已冻结的数据库快照。
    std::fs::write(&resource_path, "pub fn changed() {}\n").unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
    let file = project.file_id(&root.path().join("src/lib.rs")).unwrap();
    assert_eq!(project.source(file).unwrap(), source);
    let definitions = project
        .definitions(file, source.find("target();").unwrap() as u32)
        .unwrap();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].file_id, file);
    assert_eq!(definitions[0].name.as_str(), "target");

    // 分别核对每种公共入口的错误契约；这不是完整图在错误后继续分析的调用范例。
    assert_resource_query_error(project.source(resource));
    assert_resource_query_error(project.parse(resource));
    assert_resource_query_error(project.definitions(resource, 0));
    assert_resource_query_error(project.outgoing_calls(resource, 0));
    assert_resource_query_error(project.parent_modules(resource));
}

/// 二进制资源查询保留上游实际故障诊断，不能变成取消、成功空列表或遗漏错误。
fn assert_resource_query_error<T>(result: Result<T>) {
    let error = result.err().expect("二进制资源没有可查询的原生 FileText");
    assert!(error.downcast_ref::<base_db::salsa::Cancelled>().is_none());
    assert!(
        format!("{error:#}").contains("Unable to fetch file text"),
        "{error:#}"
    );
}

/// Salsa 的三类取消在异常边界后仍可按类型识别，不被归为普通内部故障。
#[test]
fn native_transaction_preserves_typed_cancellation() {
    use base_db::salsa::Cancelled;
    for cancelled in [
        Cancelled::Local,
        Cancelled::PendingWrite,
        Cancelled::PropagatedPanic,
    ] {
        let kind = std::mem::discriminant(&cancelled);
        let error = native_transaction::<()>("取消契约", || {
            std::panic::resume_unwind(Box::new(cancelled))
        })
        .unwrap_err();
        let cancelled = error
            .downcast_ref::<Cancelled>()
            .expect("取消类型必须保留在错误链中");
        assert_eq!(std::mem::discriminant(cancelled), kind);
        assert!(format!("{error:#}").contains("原生取消"));
    }
    // IDE 的 Cancellable 正常返回取消错误时，也不能在外层事务中擦除其类型。
    let error = native_transaction::<()>("取消契约", || {
        Err(anyhow::Error::new(Cancelled::Local).context("原生查询已取消"))
    })
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<Cancelled>(),
        Some(Cancelled::Local)
    ));
}

/// 多文件真实 Cargo 项目直接验证定义、共享父模块、宏/闭包出向调用与同源 AST。
#[test]
fn embedded_queries_preserve_multifile_semantics_and_workspace() {
    let root = support::fixture();
    let before = support::snapshot(root.path());
    let project = RustProject::load(
        &root.path().join("Cargo.toml"),
        &LoadPolicy::repository_only(),
    )
    .unwrap();
    let file = project.file_id(&root.path().join("src/lib.rs")).unwrap();
    let store = project.file_id(&root.path().join("src/store.rs")).unwrap();
    let source = project.source(file).unwrap();
    assert_eq!(
        project.parse(file).unwrap().syntax().text().to_string(),
        source.as_str()
    );
    let definition = project
        .definitions(file, source.find("helper();").unwrap() as u32)
        .unwrap();
    assert_eq!(definition.len(), 1, "{definition:?}");
    assert_eq!(definition[0].file_id, store);
    assert_eq!(definition[0].name.as_str(), "helper");
    for function in ["update", "run", "direct_closure"] {
        let calls = project
            .outgoing_calls(
                file,
                source.find(&format!("fn {function}")).unwrap() as u32 + 3,
            )
            .unwrap();
        assert!(
            calls
                .iter()
                .any(|call| call.target.file_id == store && call.target.name.as_str() == "helper"),
            "{function}: {calls:?}"
        );
    }
    assert!(
        project
            .outgoing_calls(file, source.find("fn macro_call").unwrap() as u32 + 3)
            .unwrap()
            .is_empty()
    );
    assert!(
        project
            .outgoing_calls(file, source.find("fn function_value").unwrap() as u32 + 3)
            .unwrap()
            .is_empty()
    );
    let parents = project.parent_modules(store).unwrap();
    let names: Vec<_> = parents
        .iter()
        .map(|parent| match parent {
            ModuleParent::Declaration(target) => target.name.as_str(),
            ModuleParent::CrateRoot => panic!("模块源不应成为 crate 根"),
        })
        .collect();
    assert_eq!(names, ["storage", "mirror"]);
    assert!(matches!(
        project.parent_modules(file).unwrap().as_slice(),
        [ModuleParent::CrateRoot]
    ));
    let smoke = project
        .file_id(&root.path().join("tests/smoke.rs"))
        .unwrap();
    assert!(matches!(
        project.parent_modules(smoke).unwrap().as_slice(),
        [ModuleParent::CrateRoot]
    ));
    assert!(!root.path().join("build-script-executed").exists());
    assert!(!root.path().join("proc-macro-executed").exists());
    support::assert_unchanged(root.path(), &before);
}

/// 空文件、自定义 crate 入口通过语义身份识别，不依赖 src/lib.rs 或清单导航位置。
#[test]
fn custom_empty_root_is_identified_without_manifest_navigation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("Cargo.toml"), "[package]\nname=\"custom_root\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"entry.rs\"\n").unwrap();
    std::fs::write(root.path().join("entry.rs"), "").unwrap();
    let project = RustProject::load(
        &root.path().join("Cargo.toml"),
        &LoadPolicy::repository_only(),
    )
    .unwrap();
    let entry = project.file_id(&root.path().join("entry.rs")).unwrap();
    assert!(matches!(
        project.parent_modules(entry).unwrap().as_slice(),
        [ModuleParent::CrateRoot]
    ));
    let raw = project
        .analysis
        .parent_module(FilePosition {
            file_id: entry,
            offset: 0.into(),
        })
        .unwrap();
    assert!(raw.iter().all(|target| target.file_id == entry));
}

/// 测试 cfg 由显式政策控制；启用后跨文件测试调用可以解析。
#[test]
fn test_cfg_controls_semantic_module_context() {
    let root = support::fixture();
    for test_cfg in [false, true] {
        let policy = LoadPolicy {
            test_cfg,
            ..LoadPolicy::repository_only()
        };
        let project = RustProject::load(&root.path().join("Cargo.toml"), &policy).unwrap();
        let file = project.file_id(&root.path().join("src/specs.rs")).unwrap();
        let parents = project.parent_modules(file).unwrap();
        assert_eq!(parents.len(), usize::from(test_cfg), "{parents:?}");
        let text = project.source(file).unwrap();
        let calls = project
            .outgoing_calls(file, text.find("fn checks").unwrap() as u32 + 3)
            .unwrap();
        assert_eq!(
            calls.iter().any(|call| call.target.name.as_str() == "run"),
            test_cfg
        );
    }
}

/// 已加载快照不跟随磁盘变化，越界和 Unicode 字符内偏移不能进入原生查询。
#[test]
fn queries_use_one_immutable_snapshot_and_validate_offsets() {
    let root = support::fixture();
    let project = RustProject::load(
        &root.path().join("Cargo.toml"),
        &LoadPolicy::repository_only(),
    )
    .unwrap();
    let file = project.file_id(&root.path().join("src/lib.rs")).unwrap();
    let old = project.source(file).unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
    assert_eq!(project.source(file).unwrap(), old);
    assert!(project.definitions(file, old.len() as u32 + 1).is_err());
    assert!(
        project
            .definitions(file, old.find("验").unwrap() as u32 + 1)
            .is_err()
    );
    assert!(project.parse(vfs::FileId::from_raw(0x7fff_ffff)).is_err());
}
