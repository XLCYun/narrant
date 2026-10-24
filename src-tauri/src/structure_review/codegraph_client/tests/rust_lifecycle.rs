//! 真实 Rust 入口必须由 Narrant 回收其实际 Cargo 子进程，不假设旧 Host/worker 拓扑。
use super::super::*;
use std::{collections::BTreeMap, fs, process::Command, sync::Mutex, time::Instant};

/// 只接受真实入口的 metadata 后代，并确认它已经打开当前测试持有的 Cargo 缓存锁。
#[cfg(unix)]
fn cargo_descendant(root: i32, cache_lock: &Path) -> Option<i32> {
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,comm="])
        .output()
        .unwrap();
    assert!(output.status.success());
    let processes = String::from_utf8(output.stdout).unwrap();
    let rows = processes
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((
                fields.next()?.parse::<i32>().ok()?,
                fields.next()?.parse::<i32>().ok()?,
                fields.next()?,
            ))
        })
        .collect::<Vec<_>>();
    let mut owned = std::collections::BTreeSet::from([root]);
    loop {
        let size = owned.len();
        owned.extend(
            rows.iter()
                .filter_map(|(pid, parent, _)| owned.contains(parent).then_some(*pid))
                .collect::<Vec<_>>(),
        );
        if size == owned.len() {
            break;
        }
    }
    rows.iter().find_map(|(pid, _, command)| {
        if !owned.contains(pid)
            || Path::new(command)
                .file_name()
                .is_none_or(|name| name != "cargo")
        {
            return None;
        }
        // macOS 的非末列 comm 会截断路径，因此完整命令另查，不把版本探针当成 metadata。
        let arguments = Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "args="])
            .output()
            .unwrap();
        if !arguments.status.success()
            || String::from_utf8_lossy(&arguments.stdout)
                .split_whitespace()
                .nth(1)
                != Some("metadata")
        {
            return None;
        }
        // lsof 只读查询准确 PID 与隔离锁文件；不等待进程退出，也不向 PID 发送暂停信号。
        let opened = Command::new("lsof")
            .args(["-a", "-p", &pid.to_string(), "-Fn", "--"])
            .arg(cache_lock)
            .output()
            .expect("真实 Cargo 生命周期验收需要系统 lsof");
        let path = cache_lock.to_string_lossy();
        (opened.status.success()
            && String::from_utf8_lossy(&opened.stdout)
                .lines()
                .any(|line| line.strip_prefix('n') == Some(path.as_ref())))
        .then_some(*pid)
    })
}

/// 用隔离缓存锁稳定持有真实 Cargo，再取消；不依赖 metadata 耗时或扩充清单碰观察窗口。
#[cfg(unix)]
#[test]
#[ignore = "需要真实 Rust CodeGraph 包和现有项目工具链，Unix 进程观察在 macOS 验收"]
fn actual_cargo_descendant_is_reaped_before_analysis_returns() {
    let repository = tempfile::tempdir().unwrap();
    fs::write(
        repository.path().join("Cargo.toml"),
        "[workspace]\nresolver=\"2\"\nmembers=[\"crates/c0\"]\n",
    )
    .unwrap();
    let directory = repository.path().join("crates/c0/src");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.parent().unwrap().join("Cargo.toml"),
        "[package]\nname=\"fixture_0\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    let source = "pub fn value() -> u32 { 1 }\n";
    fs::write(directory.join("lib.rs"), source).unwrap();
    let observation = tempfile::tempdir().unwrap();
    let cargo_home = observation.path().join("cargo-home");
    fs::create_dir(&cargo_home).unwrap();
    fs::write(cargo_home.join("config.toml"), "[net]\noffline = true\n").unwrap();
    let cache_path = cargo_home.join(".package-cache");
    let cache_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&cache_path)
        .unwrap();
    fs2::FileExt::lock_exclusive(&cache_lock).unwrap();
    let cache_path = cache_path.canonicalize().unwrap();
    let marker = observation.path().join("cli.pid");
    let root = super::candidate::root("rust");
    let observed = Arc::new(Mutex::new(None));
    let shared = observed.clone();
    let started = Instant::now();
    let analyzer = CodeGraphAnalyzer::from_pack("rust", &root)
        .unwrap()
        // 完整依赖策略确实请求 Cargo 元数据；版本/目标规格探针不作为存活工具证据。
        .with_dependency_scope(RustDependencyScope::AllDependencies)
        .with_limits(AnalysisLimits {
            analysis_timeout: Duration::from_secs(15),
            ..Default::default()
        })
        .with_test_launcher(
            crate::child_process::test_support::node_executable(),
            vec![
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("src/structure_review/codegraph_client/tests/observe-cli.mjs")
                    .into_os_string(),
                root.join("bin/codegraph-rust").into_os_string(),
            ],
            BTreeMap::from([
                (
                    "CODEGRAPH_OBSERVED_PID".into(),
                    marker.clone().into_os_string(),
                ),
                ("CARGO_HOME".into(), cargo_home.into_os_string()),
            ]),
        )
        .with_cancellation(Arc::new(move || {
            let Some(root) = fs::read_to_string(&marker)
                .ok()
                .and_then(|value| value.parse::<i32>().ok())
            else {
                return false;
            };
            if let Some(cargo) = cargo_descendant(root, &cache_path) {
                // SAFETY: 仅观察真实、仍等待测试锁的自有 Cargo；不对裸 PID 发送暂停或终止信号。
                if unsafe { libc::kill(cargo, 0) } == 0 {
                    *shared.lock().unwrap() = Some((root, cargo));
                    return true;
                }
            }
            false
        }));
    let result = analyzer.analyze(&AnalysisInput {
        repository_root: repository.path().into(),
        files: vec!["crates/c0/src/lib.rs".into()],
        options: Default::default(),
    });
    let (root, cargo) = observed.lock().unwrap().unwrap_or_else(|| {
        panic!(
            "必须观察到真实 Cargo，不能以成功空图替代；elapsed={:?}; result={result:?}",
            started.elapsed()
        )
    });
    // SAFETY: 返回后第一项检查只查询本次保存的入口和 Cargo，不补等或执行兜底终止。
    let completion = [root, cargo].map(|pid| unsafe { libc::kill(pid, 0) });
    assert_eq!(completion, [-1, -1], "{result:?}");
    assert!(result.unwrap_err().is_cancelled());
    assert!(!repository.path().join("Cargo.lock").exists());
    assert!(!repository.path().join("target").exists());
    assert_eq!(
        fs::read_to_string(directory.join("lib.rs")).unwrap(),
        source
    );
    // 锁一直保留到立即退出快照及源码只读检查完成，不能靠解锁帮助 Cargo 正常退出。
    drop(cache_lock);
    eprintln!("real Rust entry {root} and actual Cargo metadata descendant {cargo} held on isolated cache lock exited before return");
}
