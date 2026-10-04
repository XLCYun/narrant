//! 当前测试可执行文件充当受控父子夹具，共享所有者测试无需语言工具链。

use super::{publish, wait_file};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const FIXTURE: &str = concat!(module_path!(), "::process_fixture");

/// libtest 完整名不含 crate 前缀，因此应用与轻量 harness 使用相同夹具源码。
pub(crate) fn fixture_command(role: &str, directory: &Path) -> Command {
    let (_, fixture) = FIXTURE.split_once("::").expect("夹具模块包含 crate 前缀");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", fixture, "--ignored", "--nocapture"])
        .env("NARRANT_PROCESS_ROLE", role)
        .env("NARRANT_PROCESS_DIR", directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

/// 角色只协调创建与退出时序；实际回收全部由宿主的生产 ProcessTree/Job 执行。
#[test]
#[ignore = "仅由进程所有权测试作为子进程执行"]
fn process_fixture() {
    let Ok(role) = std::env::var("NARRANT_PROCESS_ROLE") else {
        return;
    };
    let directory = PathBuf::from(std::env::var_os("NARRANT_PROCESS_DIR").unwrap());
    if role == "short" {
        return;
    }
    if role == "leaf" {
        publish(&directory.join("leaf-ready"), "ready").unwrap();
    } else {
        if role == "gap-parent" {
            assert!(fixture_command("short", &directory)
                .status()
                .unwrap()
                .success());
        }
        let mut command = fixture_command("leaf", &directory);
        if std::env::var_os("NARRANT_PROCESS_INHERIT_PIPES").is_some() {
            command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        }
        let child = command.spawn().unwrap();
        publish(&directory.join("child-pid"), &child.id().to_string()).unwrap();
        wait_file(&directory.join("leaf-ready"));
        publish(&directory.join("ready"), "ready").unwrap();
        if role == "parent-exits" {
            wait_file(&directory.join("handles-ready"));
            return;
        }
        if role == "admission" {
            wait_file(&directory.join("attempt-spawn"));
            let outcome = match fixture_command("short", &directory).spawn() {
                Err(error) => format!("rejected:{:?}", error.raw_os_error()),
                Ok(mut unexpected) => {
                    let _ = unexpected.kill();
                    unexpected.wait().unwrap();
                    "accepted".into()
                }
            };
            publish(&directory.join("attempt-result"), &outcome).unwrap();
        }
    }
    // 仅在测试宿主意外消失时兜底，不作为任何退出断言的等待证据。
    std::thread::sleep(Duration::from_secs(60));
}
