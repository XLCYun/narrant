//! 项目配置只在内存中归一化；发现范围止于本侧仓库，不继承用户环境。

use super::{Result, failure};
use pep440_rs::{Version, VersionSpecifiers};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

/// 相同根、搜索路径与版本才可共用会话；不同侧由分析调用隔离。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Project {
    pub root: PathBuf,
    pub source_roots: Vec<PathBuf>,
    pub minor: u8,
}

impl Project {
    /// 就近寻找仓库内项目；同目录 ty.toml 优先，但项目兼容范围仍取 pyproject。
    pub fn for_file(repository: &Path, file: &Path, source: &str) -> Result<Self> {
        let mut directory = file.parent().ok_or_else(|| failure("源码没有父目录"))?;
        let (root, configuration, requires) = loop {
            let ty = read_toml(repository, &directory.join("ty.toml"))?;
            let pyproject = read_toml(repository, &directory.join("pyproject.toml"))?;
            let project = pyproject.as_ref().and_then(|value| value.get("project"));
            let inline = pyproject
                .as_ref()
                .and_then(|value| value.get("tool"))
                .and_then(|value| value.get("ty"));
            if ty.is_some() || project.is_some() || inline.is_some() {
                let requires = project
                    .and_then(|value| value.get("requires-python"))
                    .map(string)
                    .transpose()?;
                break (
                    directory.to_owned(),
                    ty.or_else(|| inline.cloned())
                        .unwrap_or(toml::Value::Table(Default::default())),
                    requires,
                );
            }
            if directory == repository {
                break (
                    repository.to_owned(),
                    toml::Value::Table(Default::default()),
                    None,
                );
            }
            directory = directory
                .parent()
                .filter(|path| path.starts_with(repository))
                .ok_or_else(|| failure("项目配置发现越过仓库边界"))?;
        };
        let environment = configuration.get("environment");
        let explicit = environment
            .and_then(|value| value.get("python-version"))
            .map(string)
            .transpose()?;
        let requires = script_requires(source)?.or(requires);
        let minor = select_minor(explicit.as_deref(), requires.as_deref())?;
        let mut source_roots = Vec::new();
        if let Some(roots) = environment.and_then(|value| value.get("root")) {
            add_roots(repository, &root, roots, &mut source_roots)?;
        } else {
            if root.join("src").is_dir() {
                source_roots.push(checked_path(repository, &root.join("src"))?);
            }
            source_roots.push(root.clone());
        }
        if let Some(extra) = environment.and_then(|value| value.get("extra-paths")) {
            add_roots(repository, &root, extra, &mut source_roots)?;
        }
        if source_roots.is_empty() {
            source_roots.push(root.clone());
        }
        let mut seen = BTreeSet::new();
        source_roots.retain(|path| seen.insert(path.clone()));
        Ok(Self {
            root,
            source_roots,
            minor,
        })
    }
}

/// 只有仓库内真实源码目录进入搜索路径；用户环境和显式外部路径被剔除。
fn add_roots(
    repository: &Path,
    root: &Path,
    value: &toml::Value,
    output: &mut Vec<PathBuf>,
) -> Result<()> {
    let paths = value
        .as_array()
        .ok_or_else(|| failure("Python environment 的源码路径必须是字符串数组"))?;
    for value in paths {
        let value = string(value)?;
        let path = root.join(value);
        // 显式配置外部路径只影响不可解析的关系，仓库内符号链接逃逸则是硬失败。
        if !configured_inside(repository, &path) || environment_path(repository, &path) {
            continue;
        }
        if !path.exists() && fs::symlink_metadata(&path).is_err() {
            continue;
        }
        let path = checked_path(repository, &path)?;
        if !path.is_dir() {
            return Err(failure(format!(
                "Python 源码根不是目录：{}",
                path.display()
            )));
        }
        output.push(path);
    }
    Ok(())
}

/// 配置中的 Windows 普通绝对路径与 canonical 的扩展前缀等价；最终仍校验真实路径。
fn configured_inside(repository: &Path, path: &Path) -> bool {
    lexical_relative(repository, path).is_some()
}

/// URI 常省略 Windows 扩展前缀；恢复仓库路径表示时保留链接的词法文件名。
pub(super) fn lexical_relative(repository: &Path, path: &Path) -> Option<PathBuf> {
    let path = lexical_normalize(path);
    #[cfg(windows)]
    {
        let comparable = |path: &Path| {
            let value = path.to_string_lossy();
            if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
                format!(r"\\{unc}")
            } else {
                value.trim_start_matches(r"\\?\").to_owned()
            }
        };
        let root = comparable(repository);
        let candidate = comparable(&path);
        let lower_root = root.to_ascii_lowercase();
        let lower_candidate = candidate.to_ascii_lowercase();
        if lower_candidate == lower_root {
            Some(PathBuf::new())
        } else if lower_candidate.starts_with(&format!("{lower_root}\\")) {
            Some(PathBuf::from(&candidate[root.len() + 1..]))
        } else {
            None
        }
    }
    #[cfg(not(windows))]
    {
        path.strip_prefix(repository).ok().map(Path::to_owned)
    }
}

/// 移除配置路径中的点号，仅用于判断显式越界；实际可达目录仍须 canonicalize。
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(part.as_os_str()),
        }
    }
    normalized
}

/// Python 安装目录不能因为配置中列为源码根而进入上下文。
pub(super) fn environment_path(repository: &Path, path: &Path) -> bool {
    path.strip_prefix(repository)
        .unwrap_or(path)
        .components()
        .any(|part| {
            matches!(
                part.as_os_str().to_str(),
                Some("site-packages" | "dist-packages" | ".venv")
            )
        })
        || path
            .ancestors()
            .take_while(|parent| parent.starts_with(repository))
            .any(|parent| parent.join("pyvenv.cfg").is_file())
}

/// 包括配置文件的实际位置也要留在仓库内，不能只比较词法前缀。
pub(super) fn checked_path(repository: &Path, path: &Path) -> Result<PathBuf> {
    let actual = path
        .canonicalize()
        .map_err(|error| failure(format!("{}：{error}", path.display())))?;
    if !actual.starts_with(repository) {
        return Err(failure(format!("Python 路径逃逸仓库：{}", path.display())));
    }
    Ok(actual)
}

/// 不存在的配置可继续向上查找；存在但无效的配置不能忽略。
fn read_toml(repository: &Path, path: &Path) -> Result<Option<toml::Value>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failure(error)),
        Ok(_) => {
            checked_path(repository, path)?;
            let source = fs::read_to_string(path).map_err(failure)?;
            toml::from_str(&source)
                .map(Some)
                .map_err(|error| failure(format!("{} 配置无效：{error}", path.display())))
        }
    }
}

/// 类型错误必须定位为配置问题，不回退到宿主版本。
fn string(value: &toml::Value) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| failure("Python 配置项必须是字符串"))
}

/// PEP 723 的依赖内容仅作 TOML 数据读取，绝不安装或执行脚本。
fn script_requires(source: &str) -> Result<Option<String>> {
    let mut lines = physical_lines(source);
    let mut found = None;
    while let Some(line) = lines.next() {
        if line != "# /// script" {
            continue;
        }
        if found.is_some() {
            return Err(failure("重复的 PEP 723 script 块"));
        }
        let mut content = String::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if line == "# ///" {
                closed = true;
                break;
            }
            let line = line
                .strip_prefix("# ")
                .or_else(|| (line == "#").then_some(""))
                .ok_or_else(|| failure("PEP 723 script 块格式无效"))?;
            content.push_str(line);
            content.push('\n');
        }
        if !closed {
            return Err(failure("PEP 723 script 块未结束"));
        }
        let data: toml::Value = toml::from_str(&content).map_err(failure)?;
        found = Some(data.get("requires-python").map(string).transpose()?);
    }
    Ok(found.flatten())
}

/// 在范围边界两侧取有限 patch 候选，避免把 3.10 等同于 3.10.0 或枚举无限版本。
fn select_minor(explicit: Option<&str>, requires: Option<&str>) -> Result<u8> {
    let requires = requires.filter(|value| !value.trim().is_empty());
    let specifiers = requires
        .map(VersionSpecifiers::from_str)
        .transpose()
        .map_err(failure)?;
    let explicit = explicit
        .map(|value| {
            value
                .strip_prefix("3.")
                .and_then(|minor| minor.parse::<u8>().ok())
                .filter(|minor| (7..=14).contains(minor))
                .ok_or_else(|| failure(format!("不支持的 Python 目标 {value}；支持 3.7–3.14")))
        })
        .transpose()?;
    if specifiers.is_none() {
        return Ok(explicit.unwrap_or(14));
    }
    let specifiers = specifiers.unwrap();
    for minor in explicit.map_or_else(|| (7..=14).collect::<Vec<_>>(), |minor| vec![minor]) {
        let mut patches = BTreeSet::from([0]);
        for specifier in specifiers.iter() {
            let release = specifier.version().release();
            if release.first() == Some(&3) && release.get(1) == Some(&u64::from(minor)) {
                let patch = release.get(2).copied().unwrap_or(0);
                patches.insert(patch);
                patches.insert(patch.saturating_sub(1));
                patches.insert(patch.saturating_add(1));
            }
        }
        if patches
            .into_iter()
            .any(|patch| specifiers.contains(&Version::new([3, u64::from(minor), patch])))
        {
            return Ok(minor);
        }
    }
    Err(failure(format!(
        "Python 目标与 requires-python={requires:?} 无兼容版本（支持 3.7–3.14）"
    )))
}

/// UTF-8 原文连同合法 BOM 交给两层；声明其他编码时明确拒绝。
pub(super) fn read_source(path: &Path) -> Result<String> {
    let source = fs::read_to_string(path).map_err(|error| {
        failure(format!(
            "{}：仅支持 UTF-8 Python 源码：{error}",
            path.display()
        ))
    })?;
    for line in physical_lines(source.trim_start_matches('\u{feff}')).take(2) {
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
            let encoding = line[index + 7..]
                .trim_start_matches([' ', '\t'])
                .chars()
                .take_while(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
                })
                .collect::<String>();
            (!encoding.is_empty()).then_some(encoding)
        });
        if let Some(encoding) = cookie {
            let encoding = encoding.to_ascii_lowercase().replace('_', "-");
            if !matches!(encoding.as_str(), "utf-8" | "utf8") && !encoding.starts_with("utf-8-") {
                return Err(failure(format!(
                    "{}：不支持 Python 编码 {encoding}，仅支持 UTF-8",
                    path.display()
                )));
            }
            break;
        }
    }
    if source.len() > u32::MAX as usize {
        return Err(failure("Python 源码超过解析器大小限制"));
    }
    Ok(source)
}

/// 元数据遵循 Python 的 LF、CRLF 与单独 CR 行界；不改写实际源码和 原生数据库 坐标。
fn physical_lines(source: &str) -> impl Iterator<Item = &str> {
    source
        .split('\n')
        .flat_map(|line| line.strip_suffix('\r').unwrap_or(line).split('\r'))
}
