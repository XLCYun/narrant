//! 固定支持包的原生 helper 装配；只提供工具与目标声明，不解释项目语法。

use super::{
    code_graph::AnalysisError,
    native_analyzer::{HelperCommand, HelperIdentity, NativeLanguageAnalyzer, Platform},
};
use crate::language_support::PackId;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

/// 支持包控制文件由安装清单逐字节校验；字段不能扩展成任意命令。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackControl {
    schema_version: u32,
    helper_version: String,
    platforms: Vec<BundledPlatform>,
    #[serde(default)]
    target_sdk: Option<TargetSdk>,
}

/// 本机 SDK 只允许固定平台与精确版本，路径从约定系统安装位置发现。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetSdk {
    kind: String,
    version: String,
    #[serde(default)]
    msvc_version: Option<String>,
}

/// 目标引用只允许支持包内相对目录，平台语义字段继续由公共 Host 严格校验。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BundledPlatform {
    id: String,
    kind: String,
    version: String,
    root: String,
    metadata: serde_json::Value,
}

/// 从已持有使用租约的安装目录建立两侧共享的固定分析配置。
pub(crate) fn from_pack(id: PackId, root: &Path) -> Result<NativeLanguageAnalyzer, AnalysisError> {
    let root = root.canonicalize().map_err(failure)?;
    let bytes = fs::read(root.join("control/native.json")).map_err(failure)?;
    // 安装标记绑定全部已验证的文件摘要，平台指纹不能仅由路径或版本组成。
    let installed = fs::read(root.join("installed.json")).map_err(failure)?;
    let control: PackControl = serde_json::from_slice(&bytes).map_err(failure)?;
    if control.schema_version != 1 || control.helper_version != "0.1.0" {
        return Err(failure("原生支持包控制文件版本不匹配"));
    }
    let (helper, projection, languages, profile) = match id {
        PackId::Java => ("java", "java-source-v1", vec!["java"], "java-static-v1"),
        PackId::Go => ("go", "go-source-v1", vec!["go"], "go-static-v1"),
        PackId::Csharp => (
            "csharp",
            "csharp-source-v1",
            vec!["csharp"],
            "csharp-static-v1",
        ),
        PackId::CCpp => (
            "clang",
            "clang-source-v1",
            vec!["c", "cpp"],
            "clang-static-v1",
        ),
        _ => return Err(failure("不是原生语言支持包")),
    };
    let executable = if id == PackId::Java {
        inside(
            &root,
            if cfg!(windows) {
                "runtime/jdk/bin/java.exe"
            } else {
                "runtime/jdk/bin/java"
            },
        )?
    } else if id == PackId::Csharp {
        inside(
            &root,
            if cfg!(windows) {
                "bin/dotnet.exe"
            } else {
                "bin/dotnet"
            },
        )?
    } else {
        inside(
            &root,
            &format!(
                "bin/narrant-{helper}{}",
                if cfg!(windows) { ".exe" } else { "" }
            ),
        )?
    };
    if !executable.is_file() {
        return Err(failure("支持包缺少 helper 执行入口"));
    }
    let args = if id == PackId::Java {
        let classes = inside(&root, "runtime/helper/classes")?;
        let libraries = inside(&root, "runtime/helper/lib")?.join("*");
        vec![
            "-cp".into(),
            std::env::join_paths([classes, libraries]).map_err(failure)?,
            "dev.narrant.java.Main".into(),
        ]
    } else if id == PackId::Csharp {
        vec![inside(&root, "runtime/helper/Narrant.CSharp.Helper.dll")?.into_os_string()]
    } else {
        vec![]
    };
    let mut platforms = Vec::new();
    for platform in control.platforms {
        let allowed = match id {
            PackId::Java => platform.kind == "java-jdk",
            PackId::Go => platform.kind == "go-sdk",
            PackId::Csharp => platform.kind == "dotnet-reference-assemblies",
            PackId::CCpp => {
                platform.kind == "clang-resource"
                    || platform.kind == "clang-target"
                    || platform.kind == "clang-system-headers"
            }
            _ => false,
        };
        if !allowed {
            return Err(failure("支持包的平台类型与语言不符"));
        }
        let platform_root = inside(&root, &platform.root)?;
        if !platform_root.is_dir() {
            return Err(failure("支持包目标声明目录不存在"));
        }
        platforms.push(Platform {
            id: platform.id,
            kind: platform.kind,
            version: platform.version,
            root: platform_root,
            fingerprint: format!("sha256:{:x}", Sha256::digest(&installed)),
            metadata: platform.metadata,
        });
    }
    if let Some(sdk) = control.target_sdk {
        if id != PackId::CCpp {
            return Err(failure("只有 Clang 包可声明系统目标 SDK"));
        }
        platforms.extend(discover_target_sdk(&sdk)?);
    }
    if platforms.is_empty() {
        return Err(failure(
            "target_unavailable: 支持包未提供经过验证的目标标准库",
        ));
    }
    let mut environment = BTreeMap::new();
    if cfg!(windows) {
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            environment.insert("SystemRoot".into(), system_root);
        }
    }
    if id == PackId::Csharp {
        environment.insert("DOTNET_ROOT".into(), root.join("bin").into_os_string());
    }
    environment.insert("DOTNET_MULTILEVEL_LOOKUP".into(), "0".into());
    environment.insert("DOTNET_EnableDiagnostics".into(), "0".into());
    Ok(NativeLanguageAnalyzer::new(
        HelperCommand {
            executable,
            args,
            environment,
            trusted_roots: vec![root],
        },
        HelperIdentity {
            helper_id: format!("narrant-{helper}"),
            helper_version: control.helper_version,
            projection_revision: projection.into(),
            languages: languages.into_iter().map(str::to_owned).collect(),
            profile: profile.into(),
        },
        platforms,
    ))
}

/// 只发现明确版本的系统 SDK；缺失返回目标环境错误，不执行 xcrun 或项目命令。
fn discover_target_sdk(sdk: &TargetSdk) -> Result<Vec<Platform>, AnalysisError> {
    if sdk.kind == "windows-msvc" {
        return discover_windows_sdk(sdk);
    }
    if sdk.kind != "macos"
        || !cfg!(all(target_os = "macos", target_arch = "aarch64"))
        || !["15.5", "27.0"].contains(&sdk.version.as_str())
        || sdk.msvc_version.is_some()
    {
        return Err(failure(
            "target_unavailable: 当前宿主没有已验证的 Clang 目标 SDK profile",
        ));
    }
    let candidates = [
        format!("/Library/Developer/CommandLineTools/SDKs/MacOSX{}.sdk", sdk.version),
        format!("/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX{}.sdk", sdk.version),
        format!("/Applications/Xcode_16.4.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX{}.sdk", sdk.version),
    ];
    for candidate in candidates {
        let Ok(root) = Path::new(&candidate).canonicalize() else {
            continue;
        };
        let Ok(bytes) = fs::read(root.join("SDKSettings.json")) else {
            continue;
        };
        let settings: serde_json::Value = serde_json::from_slice(&bytes).map_err(failure)?;
        if settings["CanonicalName"] != format!("macosx{}", sdk.version) {
            continue;
        }
        for header in ["usr/include", "usr/include/c++/v1"] {
            inside(&root, header)?;
        }
        return Ok(vec![Platform {
            id: format!("macos-{}", sdk.version),
            kind: "clang-target".into(),
            version: sdk.version.clone(),
            root,
            fingerprint: format!("sha256:{:x}", Sha256::digest(&bytes)),
            metadata: serde_json::json!({"target_triple": format!("arm64-apple-macosx{}", sdk.version),
                "c_standard":"c17", "cpp_standard":"c++20", "system_include_directories":["usr/include/c++/v1", "usr/include"],
                "resource_platform_id":"clang-resource-21"}),
        }]);
    }
    Err(failure(format!(
        "target_unavailable: 未找到 macOS {} 目标 SDK，请安装匹配的 Xcode 或 Command Line Tools",
        sdk.version
    )))
}

/// Windows SDK 与 MSVC 标准库保持两个独立信任根，不放宽到 Program Files。
fn discover_windows_sdk(sdk: &TargetSdk) -> Result<Vec<Platform>, AnalysisError> {
    let msvc = sdk
        .msvc_version
        .as_deref()
        .ok_or_else(|| failure("target_unavailable: 缺少精确 MSVC 标准库版本"))?;
    if !cfg!(all(target_os = "windows", target_arch = "x86_64"))
        || sdk.version != "10.0.26100.0"
        || msvc.split('.').count() != 3
        || msvc
            .split('.')
            .any(|part| part.is_empty() || !part.bytes().all(|ch| ch.is_ascii_digit()))
    {
        return Err(failure(
            "target_unavailable: Windows SDK/MSVC 目标版本不受支持",
        ));
    }
    let kits = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    let root = kits
        .join("Windows Kits/10/Include")
        .join(&sdk.version)
        .canonicalize()
        .map_err(failure)?;
    for header in ["ucrt", "shared", "um", "winrt", "cppwinrt"] {
        inside(&root, header)?;
    }
    let programs = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let mut standard = None;
    for base in [&programs, &kits] {
        for edition in ["Enterprise", "Professional", "Community", "BuildTools"] {
            let candidate = base
                .join("Microsoft Visual Studio/2022")
                .join(edition)
                .join("VC/Tools/MSVC")
                .join(msvc)
                .join("include");
            if let Ok(path) = candidate.canonicalize() {
                if path.join("vector").is_file() && path.join("yvals_core.h").is_file() {
                    standard = Some(path);
                    break;
                }
            }
        }
        if standard.is_some() {
            break;
        }
    }
    let standard = standard
        .ok_or_else(|| failure(format!("target_unavailable: 缺少 MSVC {msvc} 标准库头文件")))?;
    let windows_bytes = fs::read(root.join("um/Windows.h")).map_err(failure)?;
    let standard_bytes = fs::read(standard.join("yvals_core.h")).map_err(failure)?;
    Ok(vec![
        Platform {
            id: "msvc-standard-headers".into(),
            kind: "clang-system-headers".into(),
            version: msvc.into(),
            root: standard,
            fingerprint: format!("sha256:{:x}", Sha256::digest(&standard_bytes)),
            metadata: serde_json::json!({}),
        },
        Platform {
            id: format!("windows-sdk-{}", sdk.version),
            kind: "clang-target".into(),
            version: sdk.version.clone(),
            root,
            fingerprint: format!("sha256:{:x}", Sha256::digest(&windows_bytes)),
            metadata: serde_json::json!({"target_triple":"x86_64-pc-windows-msvc", "c_standard":"c17", "cpp_standard":"c++20",
                "system_include_directories":["ucrt", "shared", "um", "winrt", "cppwinrt"], "resource_platform_id":"clang-resource-21",
                "additional_system_platform_ids":["msvc-standard-headers"]}),
        },
    ])
}

/// 固定入口和目标目录必须留在支持包中，不接受链接逃逸或路径组件歧义。
fn inside(root: &Path, relative: &str) -> Result<PathBuf, AnalysisError> {
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.contains('\\')
        || relative.contains(':')
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(failure("支持包相对路径无效"));
    }
    let resolved = root.join(path).canonicalize().map_err(failure)?;
    if !resolved.starts_with(root) {
        return Err(failure("支持包入口越界"));
    }
    Ok(resolved)
}

/// 能力准备失败保留明确诊断，不能映射为空代码图。
fn failure(error: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::new(format!("原生语言支持包：{error}"))
}

#[cfg(test)]
#[path = "tests/factory.rs"]
mod tests;
