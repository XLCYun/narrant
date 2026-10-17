//! Cargo 1.82..1.94 的上游自动锁副本与显式 --lockfile-path 冲突，使用公开 metadata API 补齐。

use anyhow::{Context, Result, ensure};
use cargo_metadata::{CargoOpt, MetadataCommand};
use project_model::{
    CargoConfig, CargoFeatures, CargoWorkspace, ProjectWorkspace, ProjectWorkspaceKind,
};
use toolchain::Tool;

/// 只替换完整 Cargo metadata；原生工具链、cfg、目标布局、sysroot 和禁用构建状态原样保留。
pub(crate) fn complete(
    mut workspace: ProjectWorkspace,
    config: &CargoConfig,
) -> Result<ProjectWorkspace> {
    let ProjectWorkspaceKind::Cargo { cargo, .. } = &workspace.kind else {
        anyhow::bail!("旧 Cargo 锁兼容仅适用于 Cargo workspace");
    };
    let manifest = cargo.manifest_path().clone();
    let cargo_env = cargo.env().clone();
    let native_command = workspace
        .sysroot
        .tool(Tool::Cargo, manifest.parent(), &config.extra_env);
    let mut metadata = MetadataCommand::new();
    metadata
        .cargo_path(native_command.get_program())
        .current_dir(manifest.parent())
        .manifest_path(&*manifest);
    for (key, value) in native_command.get_envs() {
        if let Some(value) = value {
            metadata.env(key, value);
        } else {
            metadata.env_remove(key);
        }
    }
    match &config.features {
        CargoFeatures::All => {
            metadata.features(CargoOpt::AllFeatures);
        }
        CargoFeatures::Selected {
            features,
            no_default_features,
        } => {
            if *no_default_features {
                metadata.features(CargoOpt::NoDefaultFeatures);
            }
            if !features.is_empty() {
                metadata.features(CargoOpt::SomeFeatures(features.clone()));
            }
        }
    }
    let mut arguments = config.metadata_extra_args.clone();
    let mut extras = config.extra_args.iter();
    while let Some(argument) = extras.next() {
        if argument == "-Z"
            && let Some(value) = extras.next()
        {
            arguments.extend([argument.clone(), value.clone()]);
        }
    }
    if let Some(path) = &config.config_path {
        arguments.extend(["--config".into(), path.to_string()]);
    }
    for target in effective_targets(&workspace, config)? {
        arguments.extend(["--filter-platform".into(), target]);
    }
    metadata.other_options(arguments);
    // 此处不调用上游会追加第二个锁路径的 FetchMetadata，也不接受 no-deps 降级结果。
    let loaded = metadata
        .exec()
        .context("旧 Cargo 完整依赖 metadata 失败（只使用独立锁文件）")?;
    ensure!(
        loaded.resolve.is_some(),
        "完整依赖 metadata 缺少 resolve 图"
    );
    let replacement = CargoWorkspace::new(loaded, manifest, cargo_env, false);
    if let ProjectWorkspaceKind::Cargo { cargo, error, .. } = &mut workspace.kind {
        *cargo = replacement;
        *error = None;
    }
    Ok(workspace)
}

/// 读取 Cargo 自己合并的配置；仅补取公开 ProjectWorkspace 未暴露的有效 target 列表。
fn effective_targets(workspace: &ProjectWorkspace, config: &CargoConfig) -> Result<Vec<String>> {
    if let Some(target) = &config.target {
        return Ok(vec![target.clone()]);
    }
    let ProjectWorkspaceKind::Cargo { cargo, .. } = &workspace.kind else {
        unreachable!()
    };
    let manifest = cargo.manifest_path();
    let mut command = workspace
        .sysroot
        .tool(Tool::Cargo, manifest.parent(), &config.extra_env);
    command
        .args([
            "-Z",
            "unstable-options",
            "config",
            "get",
            "--format",
            "toml",
            "--show-origin",
        ])
        .env("RUSTC_BOOTSTRAP", "1");
    if let Some(path) = &config.config_path {
        command.arg("--config").arg(path);
    }
    let output = command.output().context("无法查询旧 Cargo 配置")?;
    if output.status.success() {
        let text = String::from_utf8(output.stdout).context("Cargo 配置不是 UTF-8")?;
        let table = toml::de::DeTable::parse(&text).context("Cargo 配置输出无法解析")?;
        if let Some(build) = table.get_ref().get("build")
            && let toml::de::DeValue::Table(build) = build.get_ref()
            && let Some(target) = build.get("target")
        {
            let values = match target.get_ref() {
                toml::de::DeValue::Array(values) => values.iter().collect::<Vec<_>>(),
                _ => vec![target],
            };
            return values
                .into_iter()
                .map(|value| {
                    let target = value
                        .get_ref()
                        .as_str()
                        .context("Cargo build.target 必须为字符串或字符串数组")?;
                    // JSON target 相对其配置文件的上两级目录；origin 由真实 Cargo 输出，不重建配置合并。
                    let suffix = text[value.span().end..]
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .trim_start_matches(',')
                        .trim();
                    if target.ends_with(".json")
                        && let Some(origin) = suffix.strip_prefix('#').map(str::trim)
                    {
                        let origin = std::path::Path::new(origin);
                        if origin.is_absolute()
                            && let Some(root) = origin.parent().and_then(std::path::Path::parent)
                        {
                            return Ok(root
                                .join(target)
                                .to_str()
                                .context("目标路径必须为 UTF-8")?
                                .to_owned());
                        }
                    }
                    Ok(target.to_owned())
                })
                .collect();
        }
    }
    project_model::toolchain_info::target_tuple::get(
        project_model::toolchain_info::QueryConfig::Rustc(
            &workspace.sysroot,
            manifest.parent().as_ref(),
        ),
        None,
        &config.extra_env,
    )
}

#[cfg(test)]
#[path = "tests/readonly_metadata.rs"]
mod tests;
