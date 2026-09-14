use super::*;
use std::{
    collections::BTreeSet,
    os::unix::fs::PermissionsExt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Instant,
};

/// Shell 路径逐项单引号编码，夹具从不依赖可注入的命令替换。
fn quote(path: &std::path::Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

/// 捕捉指定 TLS 进程组的所有进程，避免把其他并行夹具的服务计入结果。
fn group_pids(pid: i32) -> BTreeSet<i32> {
    let output = std::process::Command::new("ps")
        .args(["-axo", "pid=,pgid="])
        .output()
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let child = fields.next()?.parse::<i32>().ok()?;
            let group = fields.next()?.parse::<i32>().ok()?;
            (group == pid).then_some(child)
        })
        .collect()
}

/// 使用真实固定服务，观测所有正常/取消/故障/快照变化出口均先回收进程再释放视图。
#[test]
#[ignore = "需要固定 TypeScript 工具；Unix 进程组验收"]
fn public_adapter_reclaims_view_and_children_on_every_exit() {
    for mode in ["normal", "cancel", "failure", "timeout", "changed", "panic"] {
        let repository = tempfile::tempdir().unwrap();
        let source = repository.path().join("main.ts");
        fs::write(
            &source,
            "export function target() {} export function caller() { target(); }",
        )
        .unwrap();
        let tools = tools::TypeScriptToolchain::default();
        let log = repository.path().join("lifecycle.log");
        let launcher = repository.path().join("node-wrapper");
        let failure = match mode {
            "failure" => "exit 19",
            "timeout" => "sleep 10",
            _ => ":",
        };
        fs::write(&launcher, format!("#!/bin/sh\ncase \"$1\" in */typescript-language-server/lib/cli.mjs) printf '%s\\n%s\\n' \"$$\" \"$PWD\" > {}; {};; esac\nexec {} \"$@\"\n", quote(&log), failure, quote(&tools.node))).unwrap();
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let pids = Arc::new(Mutex::new(BTreeSet::new()));
        let observed = pids.clone();
        let finished = done.clone();
        let log_copy = log.clone();
        let observer = std::thread::spawn(move || {
            let started = Instant::now();
            while !finished.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(30) {
                if let Ok(text) = fs::read_to_string(&log_copy) {
                    if let Some(pid) = text.lines().next().and_then(|s| s.parse().ok()) {
                        observed.lock().unwrap().extend(group_pids(pid));
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let changed = AtomicBool::new(false);
        let observed = pids.clone();
        let callback: CancellationCheck = Arc::new(move || {
            if observed.lock().unwrap().len() >= 2 {
                if mode == "cancel" {
                    return true;
                }
                if mode == "panic" {
                    panic!("主动触发适配器展开清理");
                }
                if mode == "changed" && !changed.swap(true, Ordering::SeqCst) {
                    fs::write(&source, "export function changed() {}").unwrap();
                }
            }
            false
        });
        let analyzer = TypeScriptLanguageAnalyzer::new(launcher, tools.runtime_root)
            .with_cancellation(callback)
            .with_timeout(if mode == "timeout" {
                Duration::from_millis(500)
            } else {
                Duration::from_secs(30)
            });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            analyzer.analyze(&AnalysisInput {
                repository_root: repository.path().into(),
                files: vec!["main.ts".into()],
                options: Default::default(),
            })
        }));
        done.store(true, Ordering::SeqCst);
        observer.join().unwrap();
        let text = fs::read_to_string(&log).unwrap();
        let mut lines = text.lines();
        let pid: i32 = lines.next().unwrap().parse().unwrap();
        let view = PathBuf::from(lines.next().unwrap());
        assert!(!view.exists(), "{mode} 残留视图 {view:?}");
        pids.lock().unwrap().insert(pid);
        for pid in pids.lock().unwrap().iter() {
            assert_eq!(unsafe { libc::kill(*pid, 0) }, -1, "{mode} 残留 PID {pid}");
        }
        match mode {
            "normal" => {
                result.unwrap().unwrap();
                assert!(pids.lock().unwrap().len() >= 2);
            }
            "cancel" => assert!(result.unwrap().unwrap_err().is_cancelled()),
            "panic" => assert!(result.is_err()),
            "changed" => assert!(result
                .unwrap()
                .unwrap_err()
                .message
                .contains("源文件已改变")),
            _ => {
                assert!(result.unwrap().is_err());
            }
        }
    }
}
