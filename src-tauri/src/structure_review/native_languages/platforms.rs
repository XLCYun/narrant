//! 只读发现本机 SDK 声明；运行 helper 的私有工具链与被分析项目目标相互独立。

mod go;
mod java;

pub(super) use go::discover as go;
pub(super) use java::discover as java;

use super::{failure, inside, AnalysisError, Platform};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// 读取已有安装目录，不运行 dotnet、MSBuild、xcrun 或项目中的任何命令。
pub(super) fn dotnet() -> Result<Vec<Platform>, AnalysisError> {
    let mut roots: Vec<PathBuf> = [
        "DOTNET_ROOT",
        "DOTNET_ROOT_ARM64",
        "DOTNET_ROOT_X64",
        "DOTNET_ROOT_X86",
        "DOTNET_ROOT(x86)",
    ]
    .into_iter()
    .filter_map(std::env::var_os)
    .map(PathBuf::from)
    .collect();
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        roots.push(PathBuf::from(home).join(".dotnet"));
    }
    if cfg!(windows) {
        for key in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = std::env::var_os(key) {
                roots.push(PathBuf::from(root).join("dotnet"));
            }
        }
    } else {
        roots.extend(
            [
                "/usr/local/share/dotnet",
                "/usr/local/share/dotnet/x64",
                "/opt/homebrew/share/dotnet",
                "/usr/share/dotnet",
                "/usr/lib/dotnet",
            ]
            .map(PathBuf::from),
        );
    }
    dotnet_in(&roots)
}

/// 每个现代 TFM 选择已安装的最高稳定补丁；同版本按发现顺序保留，整个 catalog 一次冻结。
fn dotnet_in(roots: &[PathBuf]) -> Result<Vec<Platform>, AnalysisError> {
    let mut targets: BTreeMap<String, (Vec<u32>, Platform)> = BTreeMap::new();
    for base in roots.iter().filter(|root| root.is_absolute()) {
        for package in directories(&base.join("packs/Microsoft.NETCore.App.Ref")) {
            let Some(name) = package.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(version) =
                numeric_version(name).filter(|v| v.len() == 3 && v[0] >= 5 && v[1] == 0)
            else {
                continue;
            };
            let tfm = format!("net{}.0", version[0]);
            let Ok(root) = package.join("ref").join(&tfm).canonicalize() else {
                continue;
            };
            if !root.join("System.Runtime.dll").is_file() || !root.join("mscorlib.dll").is_file() {
                continue;
            }
            let platform = Platform {
                id: format!("{tfm}-reference-assemblies"),
                kind: "dotnet-reference-assemblies".into(),
                version: name.into(),
                root,
                metadata: serde_json::json!({"tfm":tfm,"reference_pack":"Microsoft.NETCore.App.Ref","reference_pack_version":name}),
            };
            if targets
                .get(&tfm)
                .is_none_or(|(current, _)| version > *current)
            {
                targets.insert(tfm, (version, platform));
            }
        }
    }
    if targets.is_empty() {
        return Err(failure("target_unavailable: 未找到本机 .NET 标准引用程序集，请安装项目目标对应的 .NET SDK；仅安装运行时不足以分析"));
    }
    Ok(targets
        .into_values()
        .map(|(_, platform)| platform)
        .collect())
}

/// 优先采用显式 SDK 或当前开发工具的默认 SDK，其次检查常规安装位置；没有版本白名单。
pub(super) fn macos() -> Result<Vec<Platform>, AnalysisError> {
    for key in ["NARRANT_MACOS_SDK", "SDKROOT"] {
        if let Some(path) = std::env::var_os(key)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
        {
            return Ok(vec![macos_at(&path)?]);
        }
    }
    let mut developers: Vec<PathBuf> = std::env::var_os("DEVELOPER_DIR")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    developers.extend(
        [
            "/var/db/xcode_select_link",
            "/Library/Developer/CommandLineTools",
            "/Applications/Xcode.app/Contents/Developer",
        ]
        .map(PathBuf::from),
    );
    developers.extend(
        directories(Path::new("/Applications"))
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|v| v.to_str())
                    .is_some_and(|name| name.starts_with("Xcode") && name.ends_with(".app"))
            })
            .map(|path| path.join("Contents/Developer")),
    );
    for developer in developers {
        for directory in [
            developer.join("Platforms/MacOSX.platform/Developer/SDKs"),
            developer.join("SDKs"),
        ] {
            if let Ok(platform) = macos_at(&directory.join("MacOSX.sdk")) {
                return Ok(vec![platform]);
            }
            let mut installed: Vec<_> = directories(&directory)
                .iter()
                .filter_map(|path| macos_at(path).ok())
                .collect();
            installed.sort_by_key(|platform| numeric_version(&platform.version));
            if let Some(platform) = installed.pop() {
                return Ok(vec![platform]);
            }
        }
    }
    Err(failure(
        "target_unavailable: 未找到 macOS SDK 头文件，请安装 Xcode 或 Command Line Tools",
    ))
}

/// 读取 SDK 自己声明的版本及标准头文件；不计算指纹、不限定发行版本。
fn macos_at(path: &Path) -> Result<Platform, AnalysisError> {
    let root = path.canonicalize().map_err(failure)?;
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("SDKSettings.json")).map_err(failure)?)
            .map_err(failure)?;
    let version = settings["Version"]
        .as_str()
        .filter(|value| numeric_version(value).is_some())
        .ok_or_else(|| failure("macOS SDK 缺少有效版本声明"))?;
    for header in ["usr/include/stdio.h", "usr/include/c++/v1/vector"] {
        inside(&root, header)?;
    }
    let target = settings["DefaultProperties"]["MACOSX_DEPLOYMENT_TARGET"]
        .as_str()
        .filter(|value| numeric_version(value).is_some())
        .unwrap_or(version);
    Ok(Platform {
        id: format!("macos-{version}"),
        kind: "clang-target".into(),
        version: version.into(),
        root,
        metadata: serde_json::json!({"target_triple":format!("arm64-apple-macosx{target}"),
            "system_include_directories":["usr/include/c++/v1","usr/include"],"resource_platform_id":"clang-resource-21"}),
    })
}

/// Windows SDK 和 MSVC 头文件分别发现，始终只授权各自具体 include 根。
pub(super) fn windows() -> Result<Vec<Platform>, AnalysisError> {
    let kits = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    let programs = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let sdk = if let Some(root) = std::env::var_os("NARRANT_WINDOWS_SDK").map(PathBuf::from) {
        root
    } else {
        newest(&kits.join("Windows Kits/10/Include"), "um/Windows.h")
            .ok_or_else(|| failure("target_unavailable: 未找到 Windows SDK 头文件"))?
    };
    let standard = if let Some(root) = std::env::var_os("NARRANT_MSVC_ROOT").map(PathBuf::from) {
        root.join("include")
    } else {
        let mut candidates = Vec::new();
        for base in [&programs, &kits] {
            for release in directories(&base.join("Microsoft Visual Studio")) {
                for edition in directories(&release) {
                    if let Some(tools) = newest(&edition.join("VC/Tools/MSVC"), "include/vector") {
                        candidates.push(tools);
                    }
                }
            }
        }
        candidates.sort_by_key(|path| {
            path.file_name()
                .and_then(|v| v.to_str())
                .and_then(numeric_version)
        });
        candidates
            .pop()
            .ok_or_else(|| failure("target_unavailable: 未找到 MSVC C++ 标准库头文件"))?
            .join("include")
    };
    windows_at(&sdk, &standard)
}

/// 标准目录结构提供版本与实际文件位置；不将某个 SDK 或 MSVC 补丁写入产品支持列表。
fn windows_at(sdk: &Path, standard: &Path) -> Result<Vec<Platform>, AnalysisError> {
    let root = sdk.canonicalize().map_err(failure)?;
    let standard = standard.canonicalize().map_err(failure)?;
    let version = root
        .file_name()
        .and_then(|v| v.to_str())
        .filter(|v| numeric_version(v).is_some())
        .ok_or_else(|| failure("Windows SDK 版本目录无效"))?
        .to_owned();
    let msvc = standard
        .parent()
        .and_then(Path::file_name)
        .and_then(|v| v.to_str())
        .filter(|v| numeric_version(v).is_some())
        .ok_or_else(|| failure("MSVC 版本目录无效"))?
        .to_owned();
    for header in ["ucrt", "shared", "um", "winrt", "cppwinrt", "um/Windows.h"] {
        inside(&root, header)?;
    }
    for header in ["vector", "yvals_core.h"] {
        inside(&standard, header)?;
    }
    Ok(vec![
        Platform {
            id: "msvc-standard-headers".into(),
            kind: "clang-system-headers".into(),
            version: msvc,
            root: standard,
            metadata: serde_json::json!({}),
        },
        Platform {
            id: format!("windows-sdk-{version}"),
            kind: "clang-target".into(),
            version,
            root,
            metadata: serde_json::json!({"target_triple":"x86_64-pc-windows-msvc",
                "system_include_directories":["ucrt","shared","um","winrt","cppwinrt"],"resource_platform_id":"clang-resource-21","additional_system_platform_ids":["msvc-standard-headers"]}),
        },
    ])
}

/// 只列举已存在的直接子目录，缺少某个常规安装位置不阻止其他位置被发现。
fn directories(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    paths.sort();
    paths
}

/// 按数字而非字典序比较版本，忽略 preview 等不稳定目录。
fn numeric_version(value: &str) -> Option<Vec<u32>> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() < 2
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|ch| ch.is_ascii_digit()))
    {
        return None;
    }
    parts.into_iter().map(|part| part.parse().ok()).collect()
}

/// 从安装目录中选择带有必要声明文件的最高稳定版本。
fn newest(root: &Path, marker: &str) -> Option<PathBuf> {
    directories(root)
        .into_iter()
        .filter(|path| path.join(marker).is_file())
        .filter_map(|path| Some((numeric_version(path.file_name()?.to_str()?)?, path)))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, path)| path)
}

#[cfg(test)]
#[path = "tests/platforms.rs"]
mod tests;
