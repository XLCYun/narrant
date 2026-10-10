//! 只读发现 Java 目标声明；每个 major 保留最高稳定补丁，语言能力由 JDT 判断。

use super::{directories, failure, AnalysisError, Platform};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// 汇总显式路径、常见安装目录与私有运行 JDK；不执行 java_home、java 或项目命令。
pub(crate) fn discover(bundled: &Path) -> Result<Vec<Platform>, AnalysisError> {
    let mut roots: Vec<PathBuf> = ["NARRANT_JAVA_TARGET_JDK", "JAVA_HOME", "JDK_HOME"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    let mut containers = vec![
        PathBuf::from("/Library/Java/JavaVirtualMachines"),
        PathBuf::from("/usr/lib/jvm"),
    ];
    if let Some(home) =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
    {
        containers.extend([
            home.join("Library/Java/JavaVirtualMachines"),
            home.join(".sdkman/candidates/java"),
            home.join(".jdks"),
        ]);
    }
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(programs) = std::env::var_os(key).map(PathBuf::from) {
            containers.extend(
                [
                    "Java",
                    "Eclipse Adoptium",
                    "Microsoft",
                    "Amazon Corretto",
                    "BellSoft",
                    "Zulu",
                ]
                .map(|name| programs.join(name)),
            );
        }
    }
    for container in containers {
        roots.extend(directories(&container));
    }
    for prefix in ["/opt/homebrew/opt", "/usr/local/opt"] {
        for package in directories(Path::new(prefix)) {
            if package
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "openjdk" || name.starts_with("openjdk@"))
            {
                roots.push(package.join("libexec/openjdk.jdk"));
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let executable = directory.join(if cfg!(windows) { "java.exe" } else { "java" });
            if let Ok(executable) = executable.canonicalize() {
                if let Some(root) = executable.parent().and_then(Path::parent) {
                    roots.push(root.into());
                }
            }
        }
    }
    roots.push(bundled.into());
    in_roots(&roots)
}

/// 目录只提供候选，release 与标准库布局必须共同证明它是可用目标。
fn in_roots(roots: &[PathBuf]) -> Result<Vec<Platform>, AnalysisError> {
    let mut targets = BTreeMap::<u32, (Vec<u32>, Platform)>::new();
    for candidate in roots.iter().filter(|root| root.is_absolute()) {
        for root in [candidate.clone(), candidate.join("Contents/Home")] {
            let Ok(root) = root.canonicalize() else {
                continue;
            };
            let Ok(release) = fs::read_to_string(root.join("release")) else {
                continue;
            };
            let Some(version) = release.lines().find_map(|line| {
                line.strip_prefix("JAVA_VERSION=\"")
                    .and_then(|value| value.strip_suffix('"'))
            }) else {
                continue;
            };
            let Some(parts) = stable_version(version) else {
                continue;
            };
            let major = parts[0];
            let valid = if major < 9 {
                root.join("jre/lib/rt.jar").is_file()
            } else {
                root.join("lib/jrt-fs.jar").is_file() && root.join("lib/modules").is_file()
            };
            if !valid {
                continue;
            }
            let platform = Platform {
                id: format!("java{major}"),
                kind: "java-jdk".into(),
                version: version.into(),
                root,
                metadata: serde_json::json!({"release": major.to_string()}),
            };
            if targets
                .get(&major)
                .is_none_or(|(current, _)| parts > *current)
            {
                targets.insert(major, (parts, platform));
            }
        }
    }
    if targets.is_empty() {
        return Err(failure(
            "target_unavailable: 未找到本机 Java 标准库声明，请准备项目目标对应的 JDK",
        ));
    }
    Ok(targets
        .into_values()
        .map(|(_, platform)| platform)
        .collect())
}

/// 同时理解 JDK 8 的 1.8.0_update 与现代数字版本；EA、preview 不参与稳定目标选择。
fn stable_version(value: &str) -> Option<Vec<u32>> {
    let value = value.strip_suffix("-LTS").unwrap_or(value);
    let (base, build) = value
        .split_once('+')
        .map_or((value, None), |(base, build)| (base, Some(build)));
    let mut numbers = base
        .split(['.', '_'])
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                None
            } else {
                part.parse::<u32>().ok()
            }
        })
        .collect::<Option<Vec<_>>>()?;
    if numbers.first() == Some(&1) {
        numbers.remove(0);
    }
    if numbers.first().copied().unwrap_or(0) == 0 || numbers.len() > 4 {
        return None;
    }
    numbers.resize(4, 0);
    if let Some(build) = build {
        numbers.push(build.parse().ok()?);
    } else {
        numbers.push(0);
    }
    Some(numbers)
}

#[cfg(test)]
#[path = "../tests/platforms/java.rs"]
mod tests;
