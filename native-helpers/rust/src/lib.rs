//! 内嵌固定 rust-analyzer 的只读语义核心与统一协议图投影。

mod discovery;
mod loading;
pub mod projection;
pub mod protocol;
mod readonly_metadata;

use anyhow::{Context, Result, bail, ensure};
use ide::{
    Analysis, AnalysisHost, CallHierarchyConfig, CallItem, FilePosition, GotoDefinitionConfig,
    NavigationTarget, RaFixtureConfig,
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
};
use vfs::{AbsPathBuf, FileId, Vfs, VfsPath};

pub use loading::{DependencyScope, LoadPolicy, NetworkPolicy, ProjectToolchain};
pub use syntax;

/// 原生父模块结果把源码声明与 Cargo crate 根身份分开，清单不成为源码声明。
#[derive(Debug)]
pub enum ModuleParent {
    Declaration(NavigationTarget),
    CrateRoot,
}

/// 一次加载获得同源数据库、VFS 和固定分析快照，不暴露可变数据库入口。
/// 完整分析的调用方遇到任一查询错误须终止并丢弃项目，不能重试或继续拼接部分图。
pub struct RustProject {
    _host: AnalysisHost,
    analysis: Analysis,
    vfs: Vfs,
    cargo_parents: bool,
}

impl RustProject {
    /// 加载明确的 Cargo 清单；调用者须固定输入直到加载完成，禁止用户代码执行与隐式安装。
    pub fn load(manifest: &Path, policy: &LoadPolicy) -> Result<Self> {
        let (database, vfs) = loading::load(manifest, policy)?;
        let host = AnalysisHost::with_database(database);
        let analysis = host.analysis();
        Ok(Self {
            _host: host,
            analysis,
            vfs,
            cargo_parents: manifest
                .file_name()
                .is_some_and(|name| name == "Cargo.toml"),
        })
    }

    /// 多项目共用一次数据库装载，所有查询失败均由调用方终止整次请求。
    pub(crate) fn load_projects(
        manifests: &[PathBuf],
        policies: &[LoadPolicy],
        selected: &[PathBuf],
    ) -> Result<Self> {
        let (database, vfs) = loading::load_projects(manifests, policies, selected)?;
        let host = AnalysisHost::with_database(database);
        let analysis = host.analysis();
        Ok(Self {
            _host: host,
            analysis,
            vfs,
            cargo_parents: manifests
                .iter()
                .any(|path| path.file_name().is_some_and(|name| name == "Cargo.toml")),
        })
    }

    /// 将已加载文件映射为数据库身份；不存在或被排除的文件必须报告错误。
    pub fn file_id(&self, path: &Path) -> Result<FileId> {
        let path = path
            .canonicalize()
            .with_context(|| format!("无法定位源码 {}", path.display()))?;
        ensure!(path.to_str().is_some(), "源码路径必须为 UTF-8 路径");
        let path = AbsPathBuf::assert_utf8(path);
        self.vfs
            .file_id(&VfsPath::from(path))
            .filter(|(_, excluded)| *excluded == vfs::FileExcluded::No)
            .map(|(id, _)| id)
            .context("源码未完整加载到 VFS")
    }

    /// 将原生导航的文件身份还原为真实路径，不引入 URI 或协议位置转换。
    pub fn file_path(&self, file_id: FileId) -> Result<PathBuf> {
        ensure!(
            self.vfs.iter().any(|(id, _)| id == file_id),
            "未知 VFS 文件身份"
        );
        self.vfs
            .file_path(file_id)
            .as_path()
            .map(|path| PathBuf::from(path.as_str()))
            .context("导航目标没有真实源码路径")
    }

    /// 从语义快照读取同一 revision 的 AST，后续源码提取不能另用应用 parser。
    pub fn parse(&self, file_id: FileId) -> Result<syntax::ast::SourceFile> {
        native_transaction("Rust AST 查询失败", || {
            self.ensure_file(file_id)?;
            let db = self._host.raw_database();
            let mut first_errors = Vec::new();
            // 保留旧语法投影的版本兼容顺序，但每个候选仍由同一 DB 的原生 parse 查询提供。
            for edition in [
                syntax::Edition::Edition2024,
                syntax::Edition::Edition2021,
                syntax::Edition::Edition2018,
                syntax::Edition::Edition2015,
            ] {
                let file = base_db::EditionedFileId::new(db, file_id, edition);
                let parsed = file.parse(db);
                let errors = parsed.errors();
                if errors.is_empty() {
                    return Ok(parsed.tree());
                }
                if first_errors.is_empty() {
                    first_errors = errors;
                }
            }
            bail!("Rust 源文件无法解析：{first_errors:?}")
        })
    }

    /// 返回数据库中实际参与语义分析的原文，范围使用 UTF-8 字节偏移。
    pub fn source(&self, file_id: FileId) -> Result<String> {
        native_transaction("Rust 源码查询失败", || self.source_native(file_id))
    }

    /// 直接调用原生定义导航，保留所有目标及完整、焦点范围。
    pub fn definitions(&self, file_id: FileId, offset: u32) -> Result<Vec<NavigationTarget>> {
        native_transaction("Rust 定义查询失败", || {
            let position = self.position(file_id, offset)?;
            let config = GotoDefinitionConfig {
                ra_fixture: fixture_config(),
            };
            Ok(self
                .analysis
                .goto_definition(position, &config)
                .context("Rust 定义查询已取消")?
                .map(|result| result.info)
                .unwrap_or_default())
        })
    }

    /// 直接读取函数的出向调用，保留宏和闭包的原生调用位置。
    pub fn outgoing_calls(&self, file_id: FileId, offset: u32) -> Result<Vec<CallItem>> {
        native_transaction("Rust 调用查询失败", || {
            let position = self.position(file_id, offset)?;
            let config = CallHierarchyConfig {
                exclude_tests: false,
                ra_fixture: fixture_config(),
            };
            Ok(self
                .analysis
                .outgoing_calls(&config, position)
                .context("Rust 调用查询已取消")?
                .unwrap_or_default())
        })
    }

    /// 先用 crate 身份和根文件查询补齐 LSP handler 的 crate 根附加逻辑。
    pub fn parent_modules(&self, file_id: FileId) -> Result<Vec<ModuleParent>> {
        native_transaction("Rust 父模块查询失败", || {
            self.ensure_file(file_id)?;
            if !self.cargo_parents {
                return Ok(Vec::new());
            }
            for krate in self
                .analysis
                .crates_for(file_id)
                .context("Rust crate 查询已取消")?
            {
                if self
                    .analysis
                    .crate_root(krate)
                    .context("Rust crate 根查询已取消")?
                    == file_id
                {
                    return Ok(vec![ModuleParent::CrateRoot]);
                }
            }
            Ok(self
                .analysis
                .parent_module(FilePosition {
                    file_id,
                    offset: 0.into(),
                })
                .context("Rust 父模块查询已取消")?
                .into_iter()
                .map(ModuleParent::Declaration)
                .collect())
        })
    }

    /// 只供当前原生事务读取快照原文，避免位置校验另启可用性探测或嵌套异常边界。
    fn source_native(&self, file_id: FileId) -> Result<String> {
        self.ensure_file(file_id)?;
        self.analysis
            .file_text(file_id)
            .map(|text| text.to_string())
            .context("Rust 源码查询已取消")
    }

    /// 在进入原生数据库前校验身份，避免不可信查询触发内部断言。
    fn ensure_file(&self, file_id: FileId) -> Result<()> {
        ensure!(
            self.vfs.iter().any(|(id, _)| id == file_id),
            "未知 VFS 文件身份"
        );
        Ok(())
    }

    /// 位置必须落在同一快照的合法字符边界上。
    fn position(&self, file_id: FileId, offset: u32) -> Result<FilePosition> {
        let source = self.source_native(file_id)?;
        ensure!(
            source.is_char_boundary(offset as usize),
            "源码偏移超出范围或落在 UTF-8 字符内部"
        );
        Ok(FilePosition {
            file_id,
            offset: offset.into(),
        })
    }
}

/// 隔离一次真实原生事务的异常；保留取消类型与诊断，不探测文件、不重试或返回部分成功。
fn native_transaction<T>(context: &str, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(result) => result,
        Err(payload) => {
            let payload = match payload.downcast::<base_db::salsa::Cancelled>() {
                Ok(cancelled) => {
                    return Err(
                        anyhow::Error::new(*cancelled).context(format!("{context}（原生取消）"))
                    );
                }
                Err(payload) => payload,
            };
            let diagnostic = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("原生事务发生非字符串 panic");
            bail!("{context}（原生内部异常）：{diagnostic}");
        }
    }
}

/// 禁用上游编辑器测试夹具的特殊解释，所有语义仅来自本次真实项目。
fn fixture_config() -> RaFixtureConfig<'static> {
    RaFixtureConfig {
        disable_ra_fixture: true,
        ..RaFixtureConfig::default()
    }
}

#[cfg(test)]
#[path = "tests/support.rs"]
mod test_support;

#[cfg(test)]
#[path = "tests/core.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/projection.rs"]
mod projection_tests;
