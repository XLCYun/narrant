//! 原生引擎只能读取已校验的内存快照；没有磁盘、环境或命令执行入口。
use ruff_db::system::{
    DirectoryEntry, InMemorySystem, Metadata, System, SystemPath, SystemPathBuf, SystemVirtualPath,
    WhichError, WhichResult, WritableSystem, walk_directory::WalkDirectoryBuilder,
};
use ruff_notebook::{Notebook, NotebookError};
use std::{any::Any, io};

/// 对上游内存文件系统收窄为只读能力，不暴露可写对象或继承环境。
#[derive(Clone, Debug)]
pub(crate) struct SnapshotSystem(pub(crate) InMemorySystem);

impl System for SnapshotSystem {
    /// 只查询冻结快照中的文件元数据。
    fn path_metadata(&self, path: &SystemPath) -> io::Result<Metadata> {
        self.0.path_metadata(path)
    }
    /// 快照中的词法路径各自独立，物理边界已在加载前校验。
    fn canonicalize_path(&self, path: &SystemPath) -> io::Result<SystemPathBuf> {
        self.0.canonicalize_path(path)
    }
    /// 同一物理文件的不同源码路径仍有独立的模块身份。
    fn is_same_file(&self, first: &SystemPath, second: &SystemPath) -> io::Result<bool> {
        self.0.is_same_file(first, second)
    }
    /// 读取冻结的 UTF-8 源码。
    fn read_to_string(&self, path: &SystemPath) -> io::Result<String> {
        self.0.read_to_string(path)
    }
    /// 本核心不加载 notebook，未知路径按上游错误规则返回。
    fn read_to_notebook(&self, path: &SystemPath) -> Result<Notebook, NotebookError> {
        self.0.read_to_notebook(path)
    }
    /// 不载入虚拟文档，查询仍只发生在内存系统。
    fn read_virtual_path_to_string(&self, path: &SystemVirtualPath) -> io::Result<String> {
        self.0.read_virtual_path_to_string(path)
    }
    /// 不载入虚拟 notebook。
    fn read_virtual_path_to_notebook(
        &self,
        path: &SystemVirtualPath,
    ) -> Result<Notebook, NotebookError> {
        self.0.read_virtual_path_to_notebook(path)
    }
    /// 项目目录来自显式输入。
    fn current_directory(&self) -> &SystemPath {
        self.0.current_directory()
    }
    /// 禁止查找用户配置。
    fn user_config_directory(&self) -> Option<SystemPathBuf> {
        None
    }
    /// 禁止缓存落盘。
    fn cache_dir(&self) -> Option<SystemPathBuf> {
        None
    }
    /// 禁止查找 Python、uv 或项目可执行程序。
    fn which(&self, _name: &str) -> WhichResult {
        Err(WhichError::CannotFindBinaryPath)
    }
    /// 目录枚举只包含冻结文件。
    fn read_directory<'a>(
        &'a self,
        path: &SystemPath,
    ) -> io::Result<Box<dyn Iterator<Item = io::Result<DirectoryEntry>> + 'a>> {
        self.0.read_directory(path)
    }
    /// 遍历只发生在内存快照。
    fn walk_directory(&self, path: &SystemPath) -> WalkDirectoryBuilder {
        self.0.walk_directory(path)
    }
    /// 不允许引擎写入。
    fn as_writable(&self) -> Option<&dyn WritableSystem> {
        None
    }
    /// 类型擦除保持只读封装，不能下转为可写内存系统。
    fn as_any(&self) -> &dyn Any {
        self
    }
    /// 不暴露内部可写对象。
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    /// 数据库克隆继续沿用同一只读能力。
    fn dyn_clone(&self) -> Box<dyn System> {
        Box::new(self.clone())
    }
}
