//! 固定侧输入先做边界校验，再冻结到独立的只读 System；不发现环境或执行项目。
use crate::{ProjectInput, PythonProject, system::SnapshotSystem};
use anyhow::{Context, Result, ensure};
use ruff_db::system::{InMemorySystem, MemoryFileSystem, SystemPathBuf, WritableSystem};
use ruff_ranged_value::ValueSource;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};
use ty_project::{ProjectDatabase, ProjectMetadata, UseUv, metadata::options::Options};

pub(crate) const SNAPSHOT_ROOT: &str = "/narrant/repository";
const PREFIX: &str = "/narrant/environment";

/// 源码路径只允许干净的仓库相对路径；空路径表示项目/源码根。
pub(crate) fn relative(path: &Path) -> Result<()> {
    ensure!(
        path.components()
            .all(|part| matches!(part, Component::Normal(_))),
        "Python 路径必须是规范仓库相对路径：{}",
        path.display()
    );
    ensure!(path.to_str().is_some(), "Python 路径不是 UTF-8");
    Ok(())
}

/// 用固定内存根承载词法相对路径，避免临时 checkout 与平台绝对路径污染身份。
pub(crate) fn system_path(path: &Path) -> Result<SystemPathBuf> {
    relative(path)?;
    let mut output = SystemPathBuf::from(SNAPSHOT_ROOT);
    for component in path.components() {
        output.push(component.as_os_str().to_str().unwrap());
    }
    Ok(output)
}

/// 读取前解析链接并校验真实安装环境；快照与导航仍使用原来的词法相对路径。
fn check_path(repository: &Path, path: &Path) -> Result<()> {
    let actual = path
        .canonicalize()
        .with_context(|| format!("Python 路径无法读取：{}", path.display()))?;
    ensure!(
        actual.starts_with(repository),
        "Python 路径逃逸仓库：{}",
        path.display()
    );
    check_environment(repository, &actual)
}

/// 安装环境不能作为仓库源码进入语义上下文。
fn check_environment(repository: &Path, path: &Path) -> Result<()> {
    ensure!(
        !path
            .strip_prefix(repository)?
            .components()
            .any(|part| matches!(
                part.as_os_str().to_str(),
                Some("site-packages" | "dist-packages" | ".venv")
            ))
            && !path
                .ancestors()
                .take_while(|ancestor| ancestor.starts_with(repository))
                .any(|ancestor| ancestor.join("pyvenv.cfg").is_file()),
        "Python 可达源码包含安装环境：{}",
        path.display()
    );
    Ok(())
}

/// 保留 UTF-8 BOM 和原始换行；首个有效编码 cookie 不得偷偷切换解释方式。
fn read_source(path: &Path) -> Result<String> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("Python 源码不是可读 UTF-8：{}", path.display()))?;
    ensure!(
        source.len() <= u32::MAX as usize,
        "Python 源码超过解析器大小限制"
    );
    for line in source
        .trim_start_matches('\u{feff}')
        .split('\n')
        .flat_map(|line| line.strip_suffix('\r').unwrap_or(line).split('\r'))
        .take(2)
    {
        let line = line.trim_start_matches([' ', '\t', '\x0c']);
        if !line.starts_with('#') && !line.is_empty() {
            break;
        }
        let cookie = line.match_indices("coding").find_map(|(index, _)| {
            if !line
                .as_bytes()
                .get(index + 6)
                .is_some_and(|byte| matches!(byte, b':' | b'='))
            {
                return None;
            }
            let value = line[index + 7..]
                .trim_start_matches([' ', '\t'])
                .chars()
                .take_while(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
                })
                .collect::<String>();
            (!value.is_empty()).then_some(value)
        });
        if let Some(cookie) = cookie {
            let encoding = cookie.to_ascii_lowercase().replace('_', "-");
            ensure!(
                matches!(encoding.as_str(), "utf-8" | "utf8") || encoding.starts_with("utf-8-"),
                "Python 不支持编码 {encoding}：{}",
                path.display()
            );
            break;
        }
    }
    Ok(source)
}

/// 单个项目只读取明确源码根及项目根，配置发现留给应用层既有政策。
pub(crate) fn load(input: ProjectInput) -> Result<PythonProject> {
    ensure!(
        (7..=14).contains(&input.python_minor),
        "Python 目标必须为 3.7–3.14"
    );
    ensure!(!input.source_roots.is_empty(), "Python 源码根不能为空");
    let repository = input.repository.canonicalize()?;
    ensure!(repository.is_dir(), "Python 仓库根不是目录");
    let project = system_path(&input.project_root)?;
    let memory = InMemorySystem::from_memory_fs(MemoryFileSystem::with_current_directory(&project));
    let mut files = BTreeSet::new();
    for root in input
        .source_roots
        .iter()
        .chain(std::iter::once(&input.project_root))
    {
        relative(root)?;
        let root = repository.join(root);
        check_path(&repository, &root)?;
        check_environment(&repository, &root)?;
        ensure!(root.is_dir(), "Python 源码根不是目录");
        for entry in walkdir::WalkDir::new(&root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|entry| !matches!(entry.file_name().to_str(), Some(".git" | ".venv")))
        {
            let entry = entry?;
            check_path(&repository, entry.path())?;
            check_environment(&repository, entry.path())?;
            let relative = entry.path().strip_prefix(&repository)?.to_owned();
            if entry.file_type().is_dir() {
                memory.create_directory_all(&system_path(&relative)?)?;
            }
            if entry.file_type().is_file()
                && matches!(
                    entry.path().extension().and_then(|value| value.to_str()),
                    Some("py" | "pyi")
                )
                && files.insert(relative.clone())
            {
                memory
                    .fs()
                    .write_file_all(system_path(&relative)?, read_source(entry.path())?)?;
            }
        }
    }
    // 显式空 prefix 阻止 PythonEnvironment 进入解释器或 .venv 发现分支。
    memory.create_directory_all(&SystemPathBuf::from(format!(
        "{PREFIX}/lib/python3.{}/site-packages",
        input.python_minor
    )))?;
    memory.create_directory_all(&SystemPathBuf::from(format!("{PREFIX}/Lib/site-packages")))?;
    let mut environment = toml::Table::new();
    environment.insert(
        "root".into(),
        toml::Value::Array(
            input
                .source_roots
                .iter()
                .map(|root| system_path(root).map(|path| toml::Value::String(path.to_string())))
                .collect::<Result<Vec<_>>>()?,
        ),
    );
    environment.insert("python".into(), toml::Value::String(PREFIX.into()));
    environment.insert(
        "python-version".into(),
        toml::Value::String(format!("3.{}", input.python_minor)),
    );
    environment.insert("python-platform".into(), toml::Value::String("all".into()));
    environment.insert("extra-paths".into(), toml::Value::Array(Vec::new()));
    let mut options = toml::Table::new();
    options.insert("environment".into(), toml::Value::Table(environment));
    let options = Options::from_toml_str(&toml::to_string(&options)?, ValueSource::Cli)?;
    let mut metadata = ProjectMetadata::new("narrant", project).with_use_uv(UseUv::Off);
    metadata.set_override_options(options);
    let db = ProjectDatabase::fallible(metadata, SnapshotSystem(memory))?;
    Ok(PythonProject { db, files })
}
