//! 真实 Rust 入口必须由 Narrant 回收其实际 Cargo 子进程，不假设旧 Host/worker 拓扑。
use super::super::*;
use std::{collections::BTreeMap, fs, process::Command, sync::Mutex, time::Instant};

/// 从系统进程表定位自有真实 Cargo 后代；命令只用于测试观察，不注入分析请求。
#[cfg(unix)]
fn cargo_descendant(root: i32) -> Option<i32> {
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
        (owned.contains(pid)
            && Path::new(command)
                .file_name()
                .is_some_and(|name| name == "cargo"))
        .then_some(*pid)
    })
}

/// 暂停已经观察到的真实 Cargo 再取消，确保返回屏障面对仍存活的实际分析工具。
#[cfg(unix)]
#[test]
#[ignore = "需要真实 Rust CodeGraph 包和现有项目工具链，Unix 进程观察在 macOS 验收"]
fn actual_cargo_descendant_is_reaped_before_analysis_returns() {
    let repository = tempfile::tempdir().unwrap();
    fs::write(
        repository.path().join("Cargo.toml"),
        "[workspace]\nresolver=\"2\"\nmembers=[\"crates/*\"]\n",
    )
    .unwrap();
    // 足量独立清单让真实 metadata 查询可被观察，不执行任何构建脚本或伪造工具。
    for index in 0..256 {
        let directory = repository.path().join(format!("crates/c{index}/src"));
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.parent().unwrap().join("Cargo.toml"),
            format!("[package]\nname=\"fixture_{index}\"\nversion=\"0.1.0\"\nedition=\"2021\"\n"),
        )
        .unwrap();
        fs::write(directory.join("lib.rs"), "pub fn value() -> u32 { 1 }\n").unwrap();
    }
    let observation = tempfile::tempdir().unwrap();
    let marker = observation.path().join("cli.pid");
    let root = super::candidate::root("rust");
    let observed = Arc::new(Mutex::new(None));
    let shared = observed.clone();
    let started = Instant::now();
    let analyzer = CodeGraphAnalyzer::from_pack("rust", &root)
        .unwrap()
        .with_test_launcher(
            crate::child_process::test_support::node_executable(),
            vec![
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("src/structure_review/codegraph_client/tests/observe-cli.mjs")
                    .into_os_string(),
                root.join("bin/codegraph-rust").into_os_string(),
            ],
            BTreeMap::from([(
                "CODEGRAPH_OBSERVED_PID".into(),
                marker.clone().into_os_string(),
            )]),
        )
        .with_cancellation(Arc::new(move || {
            if let Ok(value) = fs::read_to_string(&marker) {
                let root = value.parse::<i32>().unwrap();
                if let Some(cargo) = cargo_descendant(root) {
                    // SAFETY: Cargo 来自当前真实入口的活后代，不是按预设名称启动的假 worker。
                    if unsafe { libc::kill(cargo, libc::SIGSTOP) } == 0 {
                        *shared.lock().unwrap() = Some((root, cargo));
                        return true;
                    }
                }
            }
            assert!(
                started.elapsed() < Duration::from_secs(15),
                "未观察到真实 Cargo 后代"
            );
            false
        }));
    let result = analyzer.analyze(&AnalysisInput {
        repository_root: repository.path().into(),
        files: vec!["crates/c0/src/lib.rs".into()],
        options: Default::default(),
    });
    let (root, cargo) = observed
        .lock()
        .unwrap()
        .expect("必须观察到真实 Cargo，不能以成功空图替代");
    // SAFETY: 返回后第一项检查只查询本次保存的入口和 Cargo，不补等或执行兜底终止。
    let completion = [root, cargo].map(|pid| unsafe { libc::kill(pid, 0) });
    assert_eq!(completion, [-1, -1]);
    assert!(result.unwrap_err().is_cancelled());
    assert!(!repository.path().join("Cargo.lock").exists());
    assert!(!repository.path().join("target").exists());
    eprintln!("real Rust entry {root} and actual Cargo descendant {cargo} exited before return");
}
