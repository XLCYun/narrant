use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 应用声明的独立语言能力包，TS 与 JS 共用一套工具。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PackId {
    Rust,
    #[serde(rename = "typescript-javascript")]
    TypeScriptJavascript,
    Python,
    Java,
    Go,
    Csharp,
    #[serde(rename = "c-cpp")]
    CCpp,
}

impl PackId {
    /// 只有已接通生产工厂的语言才能进入真实 Native 支持包验收。
    pub const NATIVE_MIGRATED: &'static [Self] = &[
        Self::Rust,
        Self::Python,
        Self::TypeScriptJavascript,
        Self::Java,
        Self::Go,
        Self::Csharp,
        Self::CCpp,
    ];

    /// 稳定标识同时用于清单和应用拥有的相对目录。
    pub fn slug(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScriptJavascript => "typescript-javascript",
            Self::Python => "python",
            Self::Java => "java",
            Self::Go => "go",
            Self::Csharp => "csharp",
            Self::CCpp => "c-cpp",
        }
    }

    /// 当前包的必需入口，归档必须提供这些普通文件。
    pub fn required(self, target: &str) -> Vec<&'static str> {
        let windows = target == "x86_64-pc-windows-msvc";
        match self {
            Self::Rust => vec![
                "control/native.json",
                "licenses/Rust-dependencies.json",
                if windows {
                    "bin/narrant-rust.exe"
                } else {
                    "bin/narrant-rust"
                },
            ],
            Self::Java => vec![
                "control/native.json",
                if windows {
                    "runtime/jdk/bin/java.exe"
                } else {
                    "runtime/jdk/bin/java"
                },
                "runtime/helper/classes/dev/narrant/java/Main.class",
            ],
            Self::Go => vec![
                "control/native.json",
                if windows {
                    "bin/narrant-go.exe"
                } else {
                    "bin/narrant-go"
                },
            ],
            Self::Csharp => vec![
                "control/native.json",
                if windows {
                    "bin/dotnet.exe"
                } else {
                    "bin/dotnet"
                },
                "runtime/helper/Narrant.CSharp.Helper.dll",
                "runtime/helper/Narrant.CSharp.Helper.deps.json",
                "runtime/helper/Narrant.CSharp.Helper.runtimeconfig.json",
                "runtime/helper/Microsoft.CodeAnalysis.dll",
                "runtime/helper/Microsoft.CodeAnalysis.CSharp.dll",
            ],
            Self::CCpp => vec![
                "control/native.json",
                if windows {
                    "bin/narrant-clang.exe"
                } else {
                    "bin/narrant-clang"
                },
            ],
            Self::Python => vec![
                if windows {
                    "bin/narrant-python.exe"
                } else {
                    "bin/narrant-python"
                },
                "control/native.json",
                "licenses/Python-dependencies.json",
                "licenses/ruff-LICENSE",
                "licenses/typeshed-LICENSE",
            ],
            Self::TypeScriptJavascript => vec![
                if windows { "bin/node.exe" } else { "bin/node" },
                "control/native.json",
                "runtime/helper/main.mjs",
                "runtime/helper/analyze.mjs",
                "runtime/helper/projection.mjs",
                "runtime/helper/config.mjs",
                "runtime/helper/workspace.mjs",
                "runtime/helper/syntax.mjs",
                "runtime/typescript/lib/lib.d.ts",
                "runtime/typescript/lib/typescript.js",
                "runtime/@types/node/index.d.ts",
                "runtime/undici-types/index.d.ts",
            ],
        }
    }
}

/// 一份不可变的已验证版本，不使用远端 latest 选择工具。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct PackKey {
    pub id: PackId,
    pub version: String,
}

/// 可由轮询稳定读取的准备阶段；终态保留到后续明确操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PackPhase {
    NotInstalled,
    Downloading,
    Verifying,
    Installing,
    Ready,
    Failed,
}

impl PackPhase {
    /// 活动阶段禁止删除，并使前端继续查询。
    pub fn active(self) -> bool {
        matches!(self, Self::Downloading | Self::Verifying | Self::Installing)
    }
}

/// 逐包状态不暴露可执行路径，界面只消费用户需要的进度和管理信息。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PackSnapshot {
    pub id: PackId,
    pub version: String,
    pub target: String,
    pub required: bool,
    pub phase: PackPhase,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub installed_bytes: u64,
    pub in_use: bool,
    pub error: Option<String>,
}

/// 全应用共享的只读快照，序号阻止迟到查询回退界面。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PackCollection {
    pub sequence: u64,
    pub packs: Vec<PackSnapshot>,
    pub requirements: BTreeMap<String, Vec<PackId>>,
}

/// 固定发布清单包含包身份、归档摘要和解压后文件摘要。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Manifest {
    pub schema_version: u32,
    pub id: PackId,
    pub version: String,
    pub target: String,
    pub archive_size: u64,
    pub archive_sha256: String,
    pub files: Vec<ManifestFile>,
}

/// 文件权限按清单恢复，归档不能请求符号链接、硬链接或特殊文件。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ManifestFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub executable: bool,
}

/// 从与打包脚本共用的版本表提取应用所要求的精确包版本。
pub(super) fn versions() -> BTreeMap<PackId, String> {
    /// 编译时读取共用的包版本目录。
    #[derive(Deserialize)]
    struct Catalog {
        packs: BTreeMap<PackId, Definition>,
    }
    /// Rust 运行时仅消费精确版本，构建工具读取其余上游信息。
    #[derive(Deserialize)]
    struct Definition {
        version: String,
    }
    let catalog: Catalog = serde_json::from_str(include_str!(
        "../../../resources/language-support-packs.json"
    ))
    .expect("编译时语言支持包目录有效");
    catalog
        .packs
        .into_iter()
        .map(|(id, definition)| (id, definition.version))
        .collect()
}

/// 目录中的历史版本仅接受简单且无路径分隔符的正式版本号。
pub(super) fn valid_version(version: &str) -> bool {
    semver::Version::parse(version).is_ok_and(|value| {
        value.pre.is_empty() && value.build.is_empty() && value.to_string() == version
    })
}

/// 未发布的开发包只使用本机已验证安装，不能向生产分发源探测虚构版本。
pub(super) fn published(id: PackId) -> bool {
    let catalog: serde_json::Value = serde_json::from_str(include_str!(
        "../../../resources/language-support-packs.json"
    ))
    .expect("编译时语言支持包目录有效");
    catalog["packs"][id.slug()]["published"]
        .as_bool()
        .unwrap_or(true)
}
