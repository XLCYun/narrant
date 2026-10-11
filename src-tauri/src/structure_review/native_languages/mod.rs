//! 固定 helper 与本机目标声明的装配；不解释项目语法或执行项目构建。

mod platforms;
mod rust;
pub(crate) use rust::RustPackAnalyzer;

use super::{
    code_graph::AnalysisError,
    native_analyzer::{HelperCommand, HelperIdentity, NativeLanguageAnalyzer, Platform},
};
use crate::language_support::PackId;
use serde::Deserialize;
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

/// 支持包只声明目标环境种类，实际版本和目录从本机安装发现。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetSdk {
    kind: String,
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
    let control: PackControl = serde_json::from_slice(&bytes).map_err(failure)?;
    if control.schema_version != 1 || control.helper_version != "0.1.0" {
        return Err(failure("原生支持包控制文件版本不匹配"));
    }
    let (helper, projection, languages, profile) = match id {
        PackId::Rust => ("rust", "rust-source-v1", vec!["rust"], "rust-repository-v1"),
        PackId::Python => (
            "python",
            "python-source-v1",
            vec!["python"],
            "python-static-v1",
        ),
        PackId::TypeScriptJavascript => (
            "typescript",
            "typescript-source-v1",
            vec!["typescript", "javascript"],
            "typescript-static-v1",
        ),
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
    } else if id == PackId::TypeScriptJavascript {
        inside(
            &root,
            if cfg!(windows) {
                "bin/node.exe"
            } else {
                "bin/node"
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
    } else if id == PackId::TypeScriptJavascript {
        vec![
            "--disable-proto=throw".into(),
            inside(&root, "runtime/helper/main.mjs")?.into_os_string(),
        ]
    } else if id == PackId::Csharp {
        vec![inside(&root, "runtime/helper/Narrant.CSharp.Helper.dll")?.into_os_string()]
    } else {
        vec![]
    };
    let mut platforms = Vec::new();
    for platform in control.platforms {
        let allowed = match id {
            PackId::Python => platform.kind == "python-embedded-stdlib",
            PackId::TypeScriptJavascript => platform.kind == "typescript-declarations",
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
            metadata: platform.metadata,
        });
    }
    if let Some(sdk) = control.target_sdk {
        platforms.extend(match (id, sdk.kind.as_str()) {
            (PackId::Java, "java") => platforms::java(&root.join("runtime/jdk"))?,
            (PackId::Go, "go") => platforms::go()?,
            (PackId::Csharp, "dotnet") => platforms::dotnet()?,
            (PackId::CCpp, "macos") if cfg!(target_os = "macos") => platforms::macos()?,
            (PackId::CCpp, "windows-msvc") if cfg!(windows) => platforms::windows()?,
            _ => return Err(failure("支持包声明的本机目标环境与语言或宿主不匹配")),
        });
    }
    if platforms.is_empty() && id != PackId::Rust {
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
            trusted_roots: vec![if id == PackId::Python {
                inside(&root, "bin")?
            } else {
                root
            }],
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

#[cfg(test)]
#[path = "tests/typescript_graph.rs"]
mod typescript_graph;

/// TS/JS 文件测试开关与 helper 保持一致；全被排除的语言组不启动或准备工具。
pub(crate) fn typescript_needs_tools(
    path: &Path,
    options: super::code_graph::AnalysisOptions,
) -> bool {
    options.include_tests
        || !(path
            .components()
            .any(|part| part.as_os_str() == "__tests__")
            || path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem.ends_with(".test") || stem.ends_with(".spec")))
}

/// Python 文件级测试过滤与 helper 一致，全被排除的语言组不准备或启动支持包。
pub(crate) fn python_needs_tools(path: &Path, options: super::code_graph::AnalysisOptions) -> bool {
    options.include_tests
        || !(path.components().any(|part| part.as_os_str() == "tests")
            || path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| {
                    stem.starts_with("test_") || stem.ends_with("_test") || stem == "conftest"
                }))
}
