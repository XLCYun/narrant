//! 固定侧选集、受信工具目录和显式平台元数据的边界校验。

use super::{failure, protocol, NativeLanguageAnalyzer, Platform, Result};
use crate::structure_review::code_graph::AnalysisInput;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// 规范化后只在本次请求内使用的输入，不进入节点身份。
pub(super) struct CheckedInput {
    pub repository: PathBuf,
    pub files: BTreeSet<String>,
    pub include_tests: bool,
    pub platforms: Vec<Platform>,
    pub executable: PathBuf,
}

impl CheckedInput {
    /// 序列化完整请求，绝对路径不能损坏无法通过 UTF-8 往返的文件名。
    pub fn request(&self, analyzer: &NativeLanguageAnalyzer, scratch: &Path) -> Result<Value> {
        Ok(json!({
            "protocol": protocol::PROTOCOL, "type": "analyze", "id": protocol::REQUEST_ID,
            "input": { "repository_root": utf8(&self.repository)?, "files": self.files,
                "include_tests": self.include_tests, "profile": analyzer.identity.profile },
            "environment": { "scratch_root": utf8(scratch)?, "platforms": self.platforms },
            "limits": { "analysis_timeout_ms": analyzer.limits.analysis_timeout.as_millis() as u64,
                "max_result_bytes": analyzer.limits.max_result_bytes,
                "max_diagnostic_bytes": analyzer.limits.max_diagnostic_bytes }
        }))
    }
}

/// 拒绝空、非 UTF-8 或含 NUL 的协议字符串。
fn utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or_else(|| failure("路径无法通过 UTF-8 协议无损往返"))
}

/// 按跨平台协议而非当前宿主语法拒绝盘符、反斜杠和非规范路径。
pub(super) fn relative(value: &str) -> Result<()> {
    if value.is_empty()
        || value.contains(['\0', '\\', ':'])
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(failure(format!("非法仓库相对路径：{value:?}")));
    }
    Ok(())
}

/// 校验相对文件的真实出处，源文件和配置文件共用同一边界。
pub(super) fn repository_file(repository: &Path, path: &str) -> Result<PathBuf> {
    relative(path)?;
    let resolved = repository.join(path).canonicalize().map_err(failure)?;
    if !resolved.starts_with(repository) || !resolved.is_file() {
        return Err(failure(format!("文件不在仓库内或不是普通文件：{path}")));
    }
    Ok(resolved)
}

/// 保证各类身份列表非空、无空值、无重复项。
pub(super) fn unique<'a>(
    values: impl IntoIterator<Item = &'a String>,
    label: &str,
) -> Result<BTreeSet<&'a str>> {
    let mut seen = BTreeSet::new();
    for value in values {
        if value.is_empty() || value.contains('\0') || !seen.insert(value.as_str()) {
            return Err(failure(format!("{label} 含空值或重复项")));
        }
    }
    Ok(seen)
}

/// 一次性验证全部输入，即使选集为空也不能绕过工具和路径边界。
pub(super) fn validate(
    analyzer: &NativeLanguageAnalyzer,
    input: &AnalysisInput,
) -> Result<CheckedInput> {
    if !input.repository_root.is_absolute() {
        return Err(failure("仓库根必须是绝对路径"));
    }
    let repository = input.repository_root.canonicalize().map_err(failure)?;
    utf8(&repository)?;
    if !repository.is_dir() {
        return Err(failure("仓库根不是目录"));
    }
    let mut files = BTreeSet::new();
    let mut resolved_files = BTreeSet::new();
    for file in &input.files {
        let path = utf8(file)?;
        #[cfg(windows)]
        let normalized = path.replace('\\', "/");
        #[cfg(not(windows))]
        let normalized = path.to_string();
        let resolved = repository_file(&repository, &normalized)?;
        if !files.insert(normalized) || !resolved_files.insert(resolved) {
            return Err(failure("选集包含重复路径或同一文件的别名"));
        }
    }
    let identity = &analyzer.identity;
    for value in [
        &identity.helper_id,
        &identity.helper_version,
        &identity.projection_revision,
        &identity.profile,
    ] {
        if value.is_empty() || value.contains('\0') {
            return Err(failure("helper 身份字段为空或无效"));
        }
    }
    if unique(&identity.languages, "helper 语言")?.is_empty() {
        return Err(failure("helper 语言集合为空"));
    }
    let limits = &analyzer.limits;
    for timeout in [
        limits.startup_timeout,
        limits.analysis_timeout,
        limits.cancellation_grace,
        limits.exit_timeout,
    ] {
        if timeout.as_millis() == 0
            || timeout.as_millis() > u64::MAX as u128
            || std::time::Instant::now().checked_add(timeout).is_none()
        {
            return Err(failure("helper 超时参数必须为可表示的正毫秒数"));
        }
    }
    if limits.max_result_bytes == 0
        || limits.max_result_bytes == usize::MAX
        || limits.max_diagnostic_bytes == 0
    {
        return Err(failure("helper 资源上限必须为有效正整数"));
    }
    if !analyzer.command.executable.is_absolute() {
        return Err(failure("helper 可执行文件必须是绝对路径"));
    }
    let executable = analyzer
        .command
        .executable
        .canonicalize()
        .map_err(failure)?;
    let mut approved = false;
    for root in &analyzer.command.trusted_roots {
        let root = external_root(root, &repository)?;
        approved |= executable.starts_with(root);
    }
    if !approved || !executable.is_file() {
        return Err(failure("helper 不在固定受信工具目录内"));
    }
    let mut platforms = analyzer.platforms.clone();
    let ids = unique(platforms.iter().map(|platform| &platform.id), "平台 ID")?;
    let kinds: BTreeMap<_, _> = platforms
        .iter()
        .map(|p| (p.id.as_str(), p.kind.as_str()))
        .collect();
    for platform in &platforms {
        for value in [&platform.version, &platform.fingerprint] {
            if value.is_empty() || value.contains('\0') {
                return Err(failure("平台版本或来源摘要为空"));
            }
        }
        validate_metadata(platform, &kinds)?;
    }
    drop(ids);
    for platform in &mut platforms {
        platform.root = external_root(&platform.root, &repository)?;
    }
    Ok(CheckedInput {
        repository,
        files,
        include_tests: input.options.include_tests,
        platforms,
        executable,
    })
}

/// 工具与目标目录必须是仓库外的明确绝对目录，禁止以共同祖先扩大授权。
fn external_root(root: &Path, repository: &Path) -> Result<PathBuf> {
    if !root.is_absolute() {
        return Err(failure("受信工具或平台根必须是绝对路径"));
    }
    let root = root.canonicalize().map_err(failure)?;
    utf8(&root)?;
    if !root.is_dir() || root.starts_with(repository) || repository.starts_with(&root) {
        return Err(failure("受信工具或平台根不能与分析仓库重叠"));
    }
    Ok(root)
}

/// 按 kind 固定 metadata 字段，不接受任意编译参数、插件或命令扩展。
fn validate_metadata(platform: &Platform, kinds: &BTreeMap<&str, &str>) -> Result<()> {
    let object = platform
        .metadata
        .as_object()
        .ok_or_else(|| failure("平台 metadata 必须是对象"))?;
    let (required, optional): (&[&str], &[&str]) = match platform.kind.as_str() {
        "go-sdk" => (&["goos", "goarch"], &["goamd64"]),
        "java-jdk" => (&["release"], &[]),
        "dotnet-reference-assemblies" => {
            (&["tfm", "reference_pack", "reference_pack_version"], &[])
        }
        "clang-target" => (
            &[
                "target_triple",
                "c_standard",
                "cpp_standard",
                "system_include_directories",
                "resource_platform_id",
            ],
            &["additional_system_platform_ids"],
        ),
        "clang-resource" | "clang-system-headers" => (&[], &[]),
        _ => return Err(failure(format!("不支持的平台 kind：{}", platform.kind))),
    };
    if required.iter().any(|key| !object.contains_key(*key))
        || object
            .keys()
            .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(failure("平台 metadata 字段与 kind 不匹配"));
    }
    for (key, value) in object {
        if key == "system_include_directories" {
            let paths = value
                .as_array()
                .ok_or_else(|| failure("系统头目录必须是数组"))?;
            let root = platform.root.canonicalize().map_err(failure)?;
            let mut seen = BTreeSet::new();
            for path in paths {
                let path = path
                    .as_str()
                    .ok_or_else(|| failure("系统头目录必须是字符串"))?;
                relative(path)?;
                let resolved = root.join(path).canonicalize().map_err(failure)?;
                if !seen.insert(path) || !resolved.starts_with(&root) || !resolved.is_dir() {
                    return Err(failure("系统头目录重复、越界或不存在"));
                }
            }
        } else if key == "additional_system_platform_ids" {
            let entries = value
                .as_array()
                .ok_or_else(|| failure("附加系统平台必须是 ID 数组"))?;
            let mut seen = BTreeSet::new();
            for entry in entries {
                let id = entry
                    .as_str()
                    .ok_or_else(|| failure("附加系统平台 ID 必须是字符串"))?;
                if id.is_empty()
                    || id.contains('\0')
                    || id == platform.id
                    || !seen.insert(id)
                    || kinds.get(id) != Some(&"clang-system-headers")
                {
                    return Err(failure(
                        "附加系统平台 ID 重复、未知、自引用或不是批准的系统头目录",
                    ));
                }
            }
        } else if value
            .as_str()
            .is_none_or(|value| value.is_empty() || value.contains('\0'))
        {
            return Err(failure("平台 metadata 值必须为非空字符串"));
        }
    }
    if platform.kind == "go-sdk"
        && object.contains_key("goamd64")
        && (object["goarch"] != "amd64"
            || !["v1", "v2", "v3", "v4"].contains(&object["goamd64"].as_str().unwrap_or_default()))
    {
        return Err(failure("goamd64 只允许 amd64 的 v1–v4"));
    }
    if platform.kind == "clang-target" {
        if object["c_standard"] != "c17" || object["cpp_standard"] != "c++20" {
            return Err(failure("Clang 平台标准不在首轮明确 profile 内"));
        }
        if kinds.get(object["resource_platform_id"].as_str().unwrap_or_default())
            != Some(&"clang-resource")
        {
            return Err(failure("Clang 平台缺少已批准的 resource 平台"));
        }
    }
    Ok(())
}
