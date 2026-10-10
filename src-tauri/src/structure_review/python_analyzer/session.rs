//! 每项目独占 ty 会话及控制目录；进程先回收，随后才释放缓存与空环境。

use super::{
    failure, lsp_failure,
    project::{self, Project},
    Result,
};
use crate::lsp::{CancellationCheck, LspClient, LspOptions};
use serde_json::json;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

/// 字段顺序保证异常返回或 panic 时也先销毁进程守卫，再删除控制目录。
pub(super) struct Session {
    client: Option<LspClient>,
    control: tempfile::TempDir,
    pub cache: PathBuf,
}

impl Session {
    /// 固定空配置阻止配置数组合并；存在的空 prefix 阻止 ty 回退到用户解释器。
    pub fn start(
        executable: &Path,
        repository: &Path,
        project: &Project,
        timeout: Duration,
        cancelled: CancellationCheck,
    ) -> Result<Self> {
        preflight(repository, project, &cancelled)?;
        let control = tempfile::tempdir().map_err(failure)?;
        if control
            .path()
            .canonicalize()
            .map_err(failure)?
            .starts_with(repository)
        {
            return Err(failure("Python 工具控制目录必须位于仓库外"));
        }
        let configuration = control.path().join("config");
        let cache = control.path().join("cache");
        let prefix = control.path().join("python");
        let bin = control.path().join("bin");
        for directory in [
            &configuration,
            &cache,
            &bin,
            &prefix.join(format!("lib/python3.{}/site-packages", project.minor)),
            &prefix.join("Lib/site-packages"),
        ] {
            fs::create_dir_all(directory).map_err(failure)?;
        }
        let empty = control.path().join("empty-ty.toml");
        fs::write(&empty, "").map_err(failure)?;
        // 开发工具可独立提供二进制；正式支持包的固定控制入口还须验证为空。
        let bundled = executable
            .parent()
            .and_then(Path::parent)
            .map(|root| root.join("control/ty.toml"));
        let empty = if let Some(bundled) = bundled.filter(|path| path.exists()) {
            if !fs::read(&bundled).map_err(failure)?.is_empty() {
                return Err(failure("Python 支持包控制配置不是固定空文件"));
            }
            bundled
        } else {
            empty
        };
        let mut command = Command::new(executable);
        command.arg("server").env_clear();
        // Windows 系统库加载仍需要系统目录；其余继承变量全部清除。
        for name in ["SystemRoot", "WINDIR", "SystemDrive"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .env("PATH", &bin)
            .env("XDG_CONFIG_HOME", &configuration)
            .env("XDG_CACHE_HOME", &cache)
            .env("APPDATA", &configuration)
            .env("LOCALAPPDATA", &cache)
            .env("USERPROFILE", control.path())
            .env("HOME", control.path())
            .env("TMPDIR", control.path())
            .env("TMP", control.path())
            .env("TEMP", control.path());
        let options = LspOptions {
            capabilities: json!({"general":{"positionEncodings":["utf-16"]},"workspace":{"configuration":false,"workspaceFolders":true},"textDocument":{"definition":{"linkSupport":true}}}),
            initialization_options: json!({"untrustedWorkspace":true,"experimental":{"useUv":"off"},"diagnosticMode":"off","configurationFile":empty,
                "configuration":{"environment":{"root":project.source_roots,"extra-paths":[],"python":prefix,"python-version":format!("3.{}", project.minor),"python-platform":"all"}}}),
            request_timeout: timeout,
            cancelled,
        };
        let client = LspClient::start(command, &project.root, options).map_err(lsp_failure)?;
        if !client
            .capabilities()
            .get("definitionProvider")
            .is_some_and(|value| value == &json!(true) || value.is_object())
        {
            return Err(failure("ty 未提供定义查询能力"));
        }
        if client
            .capabilities()
            .get("positionEncoding")
            .is_some_and(|value| value != "utf-16")
        {
            return Err(failure("ty 未协商 UTF-16 坐标"));
        }
        Ok(Self {
            client: Some(client),
            control,
            cache,
        })
    }

    /// 对上层暴露协议接口，但保留控制目录的所有权与退出顺序。
    pub fn client(&mut self) -> &mut LspClient {
        self.client.as_mut().expect("关闭后的 Python 会话不可使用")
    }

    /// 正常退出也验证进程与临时目录的清理结果，不吞掉生命周期失败。
    pub fn shutdown(mut self) -> Result<()> {
        self.client
            .take()
            .unwrap()
            .shutdown()
            .map_err(lsp_failure)?;
        self.control
            .close()
            .map_err(|error| failure(format!("Python 控制目录清理失败：{error}")))
    }
}

/// 在启动服务前校验可达源码路径；过滤响应 URI 不能替代这一步。
fn preflight(repository: &Path, project: &Project, cancelled: &CancellationCheck) -> Result<()> {
    let mut checked = BTreeSet::new();
    for root in project
        .source_roots
        .iter()
        .chain(std::iter::once(&project.root))
    {
        for entry in walkdir::WalkDir::new(root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|entry| entry.file_name() != ".git" && entry.file_name() != ".venv")
        {
            if cancelled() {
                return Err(
                    crate::structure_review::code_graph::AnalysisError::cancelled(
                        "Python 边界检查已取消",
                    ),
                );
            }
            let entry = entry.map_err(failure)?;
            if checked.insert(entry.path().to_owned()) {
                project::checked_path(repository, entry.path())?;
            }
            if project::environment_path(repository, entry.path()) {
                return Err(failure(format!(
                    "可达源码包含 Python 安装环境，无法建立源码边界：{}",
                    entry.path().display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(all(test, windows))]
#[path = "tests/windows_session.rs"]
mod windows_tests;
