//! 逐帧应答的本地测试服务器，各测试通过真实 stdio 跨越会话 Interface。

use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

/// 临时服务器先消费对应客户端消息，再发出该步响应，不预先伪造请求结果。
pub(crate) struct FakeServer {
    directory: tempfile::TempDir,
}

impl FakeServer {
    /// 每一步消费一帧，允许同时发出响应、反向请求和通知。
    pub fn new(steps: Vec<Vec<Value>>) -> Self {
        Self::with_workspace(|_| steps)
    }

    /// 响应中的源码 URI 使用临时仓库真实路径，支持跨文件语义和模块归属验证。
    pub fn with_workspace(steps: impl FnOnce(&Path) -> Vec<Vec<Value>>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let steps = steps(directory.path());
        Self::raw_in(
            directory,
            steps
                .into_iter()
                .map(|messages| {
                    messages
                        .into_iter()
                        .flat_map(|message| {
                            let body = message.to_string();
                            format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
                        })
                        .collect()
                })
                .collect(),
            true,
        )
    }

    /// 原始帧入口覆盖截断、无效头部和 EOF，其余测试使用结构化消息。
    pub fn raw(steps: Vec<Vec<u8>>, keep_alive: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        Self::raw_in(directory, steps, keep_alive)
    }

    /// 协议帧和可执行脚本保存在同一个测试目录。
    fn raw_in(directory: tempfile::TempDir, steps: Vec<Vec<u8>>, keep_alive: bool) -> Self {
        let mut script = String::from("#!/bin/sh\nset -eu\nprintf '%s' \"$$\" > pid\n");
        for (index, bytes) in steps.iter().enumerate() {
            fs::write(directory.path().join(format!("step-{index}")), bytes).unwrap();
            script.push_str(&format!(
                r#"
IFS= read -r header
IFS= read -r blank
length=$(printf '%s' "${{header#Content-Length: }}" | tr -d '\r')
body=$(dd bs=1 count="$length" 2>/dev/null)
printf '%s\n' "$body" >> requests
cat step-{index}
"#
            ));
        }
        if keep_alive {
            script.push_str("cat >/dev/null\n");
        }
        let executable = directory.path().join("server");
        fs::write(&executable, script).unwrap();
        fs::set_permissions(executable, fs::Permissions::from_mode(0o700)).unwrap();
        Self { directory }
    }

    /// 测试服务器文件及输出均收纳在自己的工作目录。
    pub fn workspace(&self) -> &Path {
        self.directory.path()
    }

    /// 向真实会话注入普通命令，不依赖语言服务器安装。
    pub fn command(&self) -> Command {
        Command::new(self.workspace().join("server"))
    }

    /// 读取对端记录的请求，验证协议顺序与参数。
    pub fn requests(&self) -> Vec<Value> {
        fs::read_to_string(self.workspace().join("requests"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    /// 检查退出时使用服务器自行报告的 PID，不读取会话私有字段。
    pub fn pid(&self) -> u32 {
        fs::read_to_string(self.workspace().join("pid"))
            .unwrap()
            .parse()
            .unwrap()
    }
}
