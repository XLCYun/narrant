//! 从已安装 Go SDK 读取标准库源码；不运行 go，也不触发工具链下载。

use super::{directories, failure, numeric_version, AnalysisError, Platform};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// 显式根、常规安装、golang.org/dl 目录和 PATH 都只作为静态安装线索。
pub(crate) fn discover() -> Result<Vec<Platform>, AnalysisError> {
    let mut roots: Vec<PathBuf> = ["NARRANT_GO_TARGET_SDK", "NARRANT_GO_SDK", "GOROOT"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    roots.extend(["/usr/local/go", "/usr/lib/go", "/opt/go"].map(PathBuf::from));
    for key in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(key).map(PathBuf::from) {
            roots.extend([base.join("Go"), base.join("Programs/Go")]);
        }
    }
    if let Some(home) =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
    {
        roots.extend(directories(&home.join("sdk")).into_iter().filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("go"))
        }));
    }
    for prefix in ["/opt/homebrew/opt", "/usr/local/opt"] {
        roots.extend(
            directories(Path::new(prefix))
                .into_iter()
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name == "go" || name.starts_with("go@"))
                })
                .map(|path| path.join("libexec")),
        );
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            if let Ok(executable) = directory
                .join(if cfg!(windows) { "go.exe" } else { "go" })
                .canonicalize()
            {
                if let Some(root) = executable.parent().and_then(Path::parent) {
                    roots.push(root.into());
                }
            }
        }
    }
    in_roots(&roots)
}

/// 各 major/minor 独立选最高稳定补丁，helper 再按项目声明和自身能力选用。
fn in_roots(roots: &[PathBuf]) -> Result<Vec<Platform>, AnalysisError> {
    let mut targets = BTreeMap::<(u32, u32), (Vec<u32>, Platform)>::new();
    for root in roots.iter().filter(|root| root.is_absolute()) {
        let Ok(root) = root.canonicalize() else {
            continue;
        };
        let Ok(contents) = fs::read_to_string(root.join("VERSION")) else {
            continue;
        };
        let Some(version) = contents
            .lines()
            .next()
            .and_then(|line| line.trim().strip_prefix("go"))
        else {
            continue;
        };
        let Some(parts) =
            numeric_version(version).filter(|parts| (2..=3).contains(&parts.len()) && parts[0] > 0)
        else {
            continue;
        };
        if !root.join("src/builtin/builtin.go").is_file()
            || !root.join("src/runtime/runtime.go").is_file()
        {
            continue;
        }
        let arch = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "amd64",
            other => other,
        };
        let os = if cfg!(target_os = "macos") {
            "darwin"
        } else {
            std::env::consts::OS
        };
        let mut metadata = serde_json::json!({"goos":os,"goarch":arch});
        if arch == "amd64" {
            metadata["goamd64"] = serde_json::json!("v1");
        }
        let platform = Platform {
            id: format!("go-sdk-{version}"),
            kind: "go-sdk".into(),
            version: version.into(),
            root,
            metadata,
        };
        let target = (parts[0], parts[1]);
        if targets
            .get(&target)
            .is_none_or(|(current, _)| parts > *current)
        {
            targets.insert(target, (parts, platform));
        }
    }
    if targets.is_empty() {
        return Err(failure(
            "target_unavailable: 未找到本机 Go SDK 标准库源码，请安装项目目标对应的 Go SDK",
        ));
    }
    Ok(targets
        .into_values()
        .map(|(_, platform)| platform)
        .collect())
}

#[cfg(test)]
#[path = "../tests/platforms/go.rs"]
mod tests;
