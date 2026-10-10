//! 内嵌固定 rust-analyzer 的只读语义核心；此阶段尚不提供协议或公共图适配。

mod loading;

use anyhow::{Context, Result, ensure};
use ide::{
    Analysis, AnalysisHost, CallHierarchyConfig, CallItem, FilePosition, GotoDefinitionConfig,
    NavigationTarget, RaFixtureConfig,
};
use std::path::{Path, PathBuf};
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
pub struct RustProject {
    _host: AnalysisHost,
    analysis: Analysis,
    vfs: Vfs,
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
        self.ensure_file(file_id)?;
        self.analysis.parse(file_id).context("Rust AST 查询已取消")
    }

    /// 返回数据库中实际参与语义分析的原文，范围使用 UTF-8 字节偏移。
    pub fn source(&self, file_id: FileId) -> Result<String> {
        self.ensure_file(file_id)?;
        self.analysis
            .file_text(file_id)
            .map(|text| text.to_string())
            .context("Rust 源码查询已取消")
    }

    /// 直接调用原生定义导航，保留所有目标及完整、焦点范围。
    pub fn definitions(&self, file_id: FileId, offset: u32) -> Result<Vec<NavigationTarget>> {
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
    }

    /// 直接读取函数的出向调用，保留宏和闭包的原生调用位置。
    pub fn outgoing_calls(&self, file_id: FileId, offset: u32) -> Result<Vec<CallItem>> {
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
    }

    /// 先用 crate 身份和根文件查询补齐 LSP handler 的 crate 根附加逻辑。
    pub fn parent_modules(&self, file_id: FileId) -> Result<Vec<ModuleParent>> {
        self.ensure_file(file_id)?;
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
        let source = self.source(file_id)?;
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
