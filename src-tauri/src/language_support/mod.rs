//! 应用共享的语言支持包：按需准备、安装事务与使用期间的删除保护。

mod model;
mod storage;

use fs2::FileExt;
use model::{valid_version, versions, Manifest, PackKey, PackSnapshot};
pub(crate) use model::{PackCollection, PackId, PackPhase};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use storage::{validate_manifest, Storage, MAX_ARCHIVE};
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

const ORIGIN: &str = "https://github.com/XLCYun/CodeGraph/releases/download";

/// GitHub 下载只跟随官方 HTTPS 资产域名；测试源仍可直连本地服务器。
fn trusted_download_redirect(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none()
        && matches!(
            url.host_str(),
            Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com")
        )
}

/// 包准备与使用租约共享一个状态；轮询不触发任何网络或目录扫描。
#[derive(Clone)]
pub(crate) struct LanguageSupportPacks {
    inner: Arc<Inner>,
}

/// 后台任务由应用持有，不依赖任何单个 Review 的取消信号。
struct Inner {
    storage: Storage,
    origin: String,
    client: reqwest::Client,
    /// 测试只让当前实例模拟一个未发布包，不改变生产目录或其他实例。
    #[cfg(test)]
    unpublished_pack: Option<PackId>,
    state: Mutex<State>,
    changed: watch::Sender<u64>,
    stop: CancellationToken,
    tasks: TaskTracker,
}

/// 用于快照及删除保护的少量内存状态，退出时无需持久化进度。
struct State {
    sequence: u64,
    entries: BTreeMap<PackKey, Entry>,
    requirements: BTreeMap<String, (Vec<PackId>, usize)>,
}

/// 使用计数只控制本进程，文件锁同时保护其他应用进程的使用者。
struct Entry {
    snapshot: PackSnapshot,
    users: usize,
}

/// 已安装目录只在租约持有期间供分析使用，避免设置页删除正在执行的工具。
pub(crate) struct PackLease {
    owner: LanguageSupportPacks,
    held: Vec<(PackKey, PathBuf, File)>,
    request: Option<String>,
}

impl PackLease {
    /// 返回本次分析已经固定并持有的包目录。
    pub fn path(&self, id: PackId) -> Option<&Path> {
        self.held
            .iter()
            .find(|(key, _, _)| key.id == id)
            .map(|(_, path, _)| path.as_path())
    }
}

impl Drop for PackLease {
    /// 文件锁和计数随实际 worker 一同释放，调用方取消等待不会提前删除工具。
    fn drop(&mut self) {
        let mut state = self.owner.inner.state.lock().unwrap();
        for (key, _, lock) in &self.held {
            if let Some(entry) = state.entries.get_mut(key) {
                entry.users -= 1;
                entry.snapshot.in_use = entry.users != 0;
            }
            // 显式解锁，避免其他线程 fork 时短暂继承描述符而延迟 close 的释放。
            if let Err(error) = FileExt::unlock(lock) {
                tracing::warn!(%error, "释放语言支持包使用锁失败");
            }
        }
        if let Some(request) = &self.request {
            if let Some((_, count)) = state.requirements.get_mut(request) {
                *count -= 1;
                if *count == 0 {
                    state.requirements.remove(request);
                }
            }
        }
        self.held.clear();
        self.owner.publish(&mut state);
    }
}

impl LanguageSupportPacks {
    /// 生产只使用应用固定分发源，调用方不能通过 IPC 注入 URL 或安装路径。
    pub fn new(root: PathBuf) -> Result<Self, String> {
        let target = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            "aarch64-apple-darwin"
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            "x86_64-pc-windows-msvc"
        } else {
            "unsupported"
        };
        Self::with_origin(root, ORIGIN.to_owned(), target.to_owned())
    }

    /// 应用集成测试复用真实下载和安装流程，生产调用方仍不能指定分发源。
    #[cfg(test)]
    pub(crate) fn with_test_origin(
        root: PathBuf,
        origin: String,
        target: String,
    ) -> Result<Self, String> {
        Self::with_origin(root, origin, target)
    }

    /// 装配存储与已验证版本，测试可用本地 HTTP 服务器经过同一下载路径。
    fn with_origin(root: PathBuf, origin: String, target: String) -> Result<Self, String> {
        // 可选能力的目录故障不能阻止 File Changes 和整个应用启动。
        let mut initialization_error = fs::create_dir_all(&root).err().map(display);
        let storage = Storage {
            root: root.canonicalize().unwrap_or(root),
            target,
        };
        if let Err(error) = storage.recover() {
            initialization_error = Some(error);
        }
        let expected = versions();
        let mut keys = match storage.keys("installed") {
            Ok(keys) => keys,
            Err(error) => {
                initialization_error = Some(error);
                Vec::new()
            }
        };
        keys.extend(expected.iter().map(|(id, version)| PackKey {
            id: *id,
            version: version.clone(),
        }));
        let entries = keys
            .into_iter()
            .map(|key| {
                let installed = storage.inspect(&key).ok();
                let error = if installed.is_none() {
                    initialization_error.clone()
                } else {
                    None
                };
                let entry = Entry {
                    snapshot: PackSnapshot {
                        id: key.id,
                        version: key.version.clone(),
                        target: storage.target.clone(),
                        required: expected.get(&key.id) == Some(&key.version),
                        phase: if installed.is_some() {
                            PackPhase::Ready
                        } else if initialization_error.is_some() {
                            PackPhase::Failed
                        } else {
                            PackPhase::NotInstalled
                        },
                        downloaded_bytes: 0,
                        total_bytes: None,
                        installed_bytes: installed
                            .map_or(0, |m| m.files.iter().map(|f| f.size).sum()),
                        in_use: false,
                        error,
                    },
                    users: 0,
                };
                (key, entry)
            })
            .collect();
        let (changed, _) = watch::channel(1);
        let client = reqwest::Client::builder();
        // 本地测试下载不经过系统代理；正式固定发布源保留用户代理策略。
        #[cfg(test)]
        let client = if origin != ORIGIN {
            client.no_proxy()
        } else {
            client
        };
        Ok(Self {
            inner: Arc::new(Inner {
                storage,
                origin,
                client: client
                    .redirect(reqwest::redirect::Policy::custom(|attempt| {
                        if attempt.previous().len() < 5 && trusted_download_redirect(attempt.url())
                        {
                            attempt.follow()
                        } else {
                            attempt.error("语言支持包重定向离开 GitHub 资产域名")
                        }
                    }))
                    .connect_timeout(Duration::from_secs(15))
                    .read_timeout(Duration::from_secs(30))
                    .timeout(Duration::from_secs(600))
                    .build()
                    .map_err(display)?,
                #[cfg(test)]
                unpublished_pack: None,
                state: Mutex::new(State {
                    sequence: 1,
                    entries,
                    requirements: BTreeMap::new(),
                }),
                changed,
                stop: CancellationToken::new(),
                tasks: TaskTracker::new(),
            }),
        })
    }

    /// 只复制内存状态，多个页面可以低成本共享这一查询。
    pub fn snapshot(&self) -> PackCollection {
        let state = self.inner.state.lock().unwrap();
        PackCollection {
            sequence: state.sequence,
            requirements: state
                .requirements
                .iter()
                .map(|(key, (ids, _))| (key.clone(), ids.clone()))
                .collect(),
            packs: state
                .entries
                .values()
                .map(|entry| entry.snapshot.clone())
                .collect(),
        }
    }

    /// 开始必要包的共享准备，并等待可用租约；取消只放弃本次等待。
    pub async fn ensure(
        &self,
        ids: &[PackId],
        cancelled: &CancellationToken,
        request: Option<String>,
    ) -> Result<PackLease, String> {
        if cancelled.is_cancelled() {
            return Err("结构分析已取消".into());
        }
        let mut updates = self.inner.changed.subscribe();
        let mut lease = PackLease {
            owner: self.clone(),
            held: vec![],
            request,
        };
        if let Some(request) = &lease.request {
            let mut state = self.inner.state.lock().unwrap();
            let entry = state
                .requirements
                .entry(request.clone())
                .or_insert_with(|| (ids.to_vec(), 0));
            entry.1 += 1;
            self.publish(&mut state);
        }
        let mut keys = vec![];
        for id in ids {
            let key = self.start(*id)?;
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        for key in keys {
            loop {
                if cancelled.is_cancelled() || self.inner.stop.is_cancelled() {
                    return Err("结构分析已取消".into());
                }
                {
                    let mut state = self.inner.state.lock().unwrap();
                    let entry = state.entries.get_mut(&key).ok_or("语言支持包不可用")?;
                    if entry.snapshot.phase == PackPhase::Failed {
                        return Err(entry
                            .snapshot
                            .error
                            .clone()
                            .unwrap_or("语言支持包安装失败".into()));
                    }
                    if entry.snapshot.phase == PackPhase::Ready {
                        let file = self.inner.storage.lock_file(&key)?;
                        let lock_result = FileExt::try_lock_shared(&file);
                        if let Err(error) = &lock_result {
                            if !lock_contended(error) {
                                return Err(display(error));
                            }
                        }
                        if lock_result.is_ok() {
                            self.inner.storage.inspect(&key)?;
                            entry.users += 1;
                            entry.snapshot.in_use = true;
                            lease.held.push((
                                key.clone(),
                                self.inner.storage.installed(&key),
                                file,
                            ));
                            self.publish(&mut state);
                            break;
                        }
                    }
                }
                tokio::select! {
                    _ = cancelled.cancelled() => return Err("结构分析已取消".into()),
                    _ = self.inner.stop.cancelled() => return Err("应用正在退出".into()),
                    _ = updates.changed() => {},
                    _ = tokio::time::sleep(Duration::from_millis(250)) => {},
                }
            }
        }
        Ok(lease)
    }

    /// 设置页的重试复用相同任务入口，返回时后台下载仍可继续。
    pub fn retry(&self, id: PackId) -> Result<PackCollection, String> {
        self.start(id)?;
        Ok(self.snapshot())
    }

    /// 只删除未使用且未准备中的已知版本，不能通过输入任意路径删除文件。
    pub fn remove(&self, id: PackId, version: &str) -> Result<PackCollection, String> {
        if !valid_version(version) {
            return Err("语言支持包版本无效".into());
        }
        let key = PackKey {
            id,
            version: version.into(),
        };
        let mut state = self.inner.state.lock().unwrap();
        let entry = state.entries.get_mut(&key).ok_or("找不到语言支持包版本")?;
        if entry.users > 0 || entry.snapshot.phase.active() {
            return Err("语言支持包正在使用，请稍后删除".into());
        }
        let lock = self.inner.storage.lock_file(&key)?;
        lock.try_lock_exclusive()
            .map_err(|_| "语言支持包被其他应用进程使用".to_owned())?;
        let path = self.inner.storage.installed(&key);
        if path.exists() {
            fs::remove_dir_all(path).map_err(display)?;
        }
        if entry.snapshot.required {
            entry.snapshot.phase = PackPhase::NotInstalled;
            entry.snapshot.installed_bytes = 0;
            entry.snapshot.error = None;
            entry.snapshot.downloaded_bytes = 0;
            entry.snapshot.total_bytes = None;
        } else {
            state.entries.remove(&key);
        }
        self.publish(&mut state);
        drop(state);
        Ok(self.snapshot())
    }

    /// 正常退出停止网络并等待解压任务收尾；强制退出残留由下次初始化回收。
    pub async fn shutdown(&self) {
        self.inner.stop.cancel();
        self.inner.tasks.close();
        self.inner.tasks.wait().await;
    }

    /// 锁内登记后才启动任务，多个 Review 的同时请求共用一次准备。
    fn start(&self, id: PackId) -> Result<PackKey, String> {
        let key = PackKey {
            id,
            version: versions().remove(&id).ok_or("未知语言支持包")?,
        };
        let mut state = self.inner.state.lock().unwrap();
        if self.inner.stop.is_cancelled() {
            return Err("应用正在退出".into());
        }
        let entry = state.entries.get_mut(&key).ok_or("语言支持包尚未初始化")?;
        if entry.snapshot.phase.active() {
            return Ok(key);
        }
        if entry.snapshot.phase == PackPhase::Ready && self.inner.storage.inspect(&key).is_ok() {
            return Ok(key);
        }
        entry.snapshot.phase = PackPhase::Downloading;
        entry.snapshot.downloaded_bytes = 0;
        entry.snapshot.total_bytes = None;
        entry.snapshot.error = None;
        self.publish(&mut state);
        let worker = self.clone();
        let task_key = key.clone();
        self.inner.tasks.spawn(async move {
            use futures_util::FutureExt;
            let result = std::panic::AssertUnwindSafe(worker.install(&task_key))
                .catch_unwind()
                .await
                .unwrap_or_else(|_| Err("语言支持包准备异常停止，请重试".into()));
            worker.update(&task_key, |snapshot| match result {
                Ok(size) => {
                    snapshot.phase = PackPhase::Ready;
                    snapshot.installed_bytes = size;
                    snapshot.error = None;
                }
                Err(error) => {
                    snapshot.phase = PackPhase::Failed;
                    snapshot.error = Some(error);
                }
            });
        });
        Ok(key)
    }

    /// 下载固定版本清单和归档，任何失败都在同盘 TempDir 中收尾。
    async fn install(&self, key: &PackKey) -> Result<u64, String> {
        let published = model::published(key.id);
        #[cfg(test)]
        let published = published && self.inner.unpublished_pack != Some(key.id);
        if self.inner.origin == ORIGIN && !published {
            return Err(format!(
                "{} CodeGraph 语言支持包尚未发布；需要经过验证的本地安装包。",
                key.id.slug()
            ));
        }
        if !["aarch64-apple-darwin", "x86_64-pc-windows-msvc"]
            .contains(&self.inner.storage.target.as_str())
        {
            return Err("当前平台尚未提供语言支持包".into());
        }
        let lock = self.inner.storage.lock_file(key)?;
        loop {
            if self.inner.stop.is_cancelled() {
                return Err("应用正在退出".into());
            }
            // 其他应用可能已完成安装并持有使用租约；复用不需要等待独占锁。
            match FileExt::try_lock_shared(&lock) {
                Ok(()) => {
                    if let Ok(manifest) = self.inner.storage.inspect(key) {
                        return Ok(manifest.files.iter().map(|file| file.size).sum());
                    }
                    FileExt::unlock(&lock).map_err(display)?;
                }
                Err(error) if lock_contended(&error) => {}
                Err(error) => return Err(display(error)),
            }
            match lock.try_lock_exclusive() {
                Ok(()) => break,
                Err(error) if lock_contended(&error) => {
                    tokio::time::sleep(Duration::from_millis(250)).await
                }
                Err(error) => return Err(display(error)),
            }
        }
        if let Ok(manifest) = self.inner.storage.inspect(key) {
            return Ok(manifest.files.iter().map(|f| f.size).sum());
        }
        let staging = self.inner.storage.staging(key);
        fs::create_dir_all(&staging).map_err(display)?;
        let temporary = tempfile::Builder::new()
            .prefix("download-")
            .tempdir_in(staging)
            .map_err(display)?;
        let result = self.install_in(key, temporary.path()).await;
        if let Err(error) = temporary.close() {
            tracing::warn!(%error, "语言支持包暂存清理失败，下次启动重试");
        }
        result
    }

    /// 读取有界清单、流式下载并验证摘要，随后让阻塞 worker 处理解压和提交。
    async fn install_in(&self, key: &PackKey, temporary: &Path) -> Result<u64, String> {
        let release = format!("{}/v{}", self.inner.origin, key.version);
        let asset = format!("{}-{}", key.id.slug(), self.inner.storage.target);
        let checksums = self
            .download_metadata(&format!("{release}/SHA256SUMS"), 64 * 1024)
            .await?;
        let manifest_name = format!("{asset}-manifest.json");
        let archive_name = format!("{asset}-pack.tar.gz");
        let bytes = self
            .download_metadata(&format!("{release}/{manifest_name}"), 2 * 1024 * 1024)
            .await?;
        let expected_manifest = checksum(&checksums, &manifest_name)?;
        let expected_archive = checksum(&checksums, &archive_name)?;
        if format!("{:x}", Sha256::digest(&bytes)) != expected_manifest {
            return Err("CodeGraph 发布清单校验失败".into());
        }
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(display)?;
        validate_manifest(&manifest, key, &self.inner.storage.target)?;
        if manifest.archive_sha256 != expected_archive {
            return Err("CodeGraph 发布摘要与归档清单不一致".into());
        }
        self.update(key, |snapshot| {
            snapshot.total_bytes = Some(manifest.archive_size)
        });
        let mut response = tokio::select! { _ = self.inner.stop.cancelled() => return Err("应用正在退出".into()), result = self.inner.client.get(format!("{release}/{archive_name}")).send() => result.map_err(display)?.error_for_status().map_err(display)? };
        let archive = temporary.join("download.part");
        let mut output = File::create(&archive).map_err(display)?;
        let mut hash = Sha256::new();
        let mut downloaded = 0u64;
        while let Some(chunk) = tokio::select! { _ = self.inner.stop.cancelled() => return Err("应用正在退出".into()), chunk = response.chunk() => chunk.map_err(display)? }
        {
            downloaded += chunk.len() as u64;
            if downloaded > manifest.archive_size || downloaded > MAX_ARCHIVE {
                return Err("语言支持包归档超过声明大小".into());
            }
            output.write_all(&chunk).map_err(display)?;
            hash.update(&chunk);
            self.update(key, |snapshot| snapshot.downloaded_bytes = downloaded);
        }
        drop(output);
        self.update(key, |snapshot| snapshot.phase = PackPhase::Verifying);
        if downloaded != manifest.archive_size
            || format!("{:x}", hash.finalize()) != manifest.archive_sha256
        {
            return Err("语言支持包归档校验失败，请重试".into());
        }
        self.update(key, |snapshot| snapshot.phase = PackPhase::Installing);
        let storage = self.inner.storage.clone();
        let key = key.clone();
        let temporary = temporary.to_owned();
        let stop = self.inner.stop.clone();
        tokio::task::spawn_blocking(move || {
            storage.unpack(&key, &manifest, &archive, &temporary, &stop)
        })
        .await
        .map_err(display)?
    }

    /// 固定发布元数据流式读取，避免 Content-Length 缺失时失去内存上限。
    async fn download_metadata(&self, url: &str, limit: usize) -> Result<Vec<u8>, String> {
        let mut response = tokio::select! { _ = self.inner.stop.cancelled() => return Err("应用正在退出".into()), result = self.inner.client.get(url).send() => result.map_err(display)?.error_for_status().map_err(display)? };
        let mut bytes = Vec::new();
        while let Some(chunk) = tokio::select! { _ = self.inner.stop.cancelled() => return Err("应用正在退出".into()), chunk = response.chunk() => chunk.map_err(display)? }
        {
            if bytes.len().saturating_add(chunk.len()) > limit {
                return Err("CodeGraph 发布元数据过大".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    /// 每次状态变化发布完整快照的递增序号，下载线程无需操作前端事件。
    fn update(&self, key: &PackKey, change: impl FnOnce(&mut PackSnapshot)) {
        let mut state = self.inner.state.lock().unwrap();
        if let Some(entry) = state.entries.get_mut(key) {
            change(&mut entry.snapshot);
            self.publish(&mut state);
        }
    }

    /// 唤醒包等待者；前端仍通过普通命令轮询同一状态。
    fn publish(&self, state: &mut State) {
        state.sequence += 1;
        self.inner.changed.send_replace(state.sequence);
    }
}

/// fs2 在 Windows 使用 ERROR_LOCK_VIOLATION，不一定映射成 WouldBlock。
fn lock_contended(error: &std::io::Error) -> bool {
    error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
        || error.kind() == std::io::ErrorKind::WouldBlock
}

/// 面向调用方保留可诊断的失败原因。
fn display(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
#[path = "tests/service.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/python.rs"]
mod python_tests;

#[cfg(test)]
#[path = "tests/native.rs"]
mod native_tests;

/// 发布摘要使用唯一文件名匹配，重复条目和非规范 SHA-256 均失败。
fn checksum(bytes: &[u8], name: &str) -> Result<String, String> {
    let content = std::str::from_utf8(bytes).map_err(display)?;
    let mut entries = BTreeMap::new();
    for line in content.lines() {
        let (hash, file) = line.split_once("  ").ok_or("CodeGraph 发布摘要格式错误")?;
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || entries.insert(file, hash).is_some()
        {
            return Err("CodeGraph 发布摘要无效或重复".into());
        }
    }
    entries
        .get(name)
        .map(|hash| (*hash).to_owned())
        .ok_or_else(|| format!("CodeGraph 发布摘要缺少 {name}"))
}

#[cfg(test)]
#[path = "tests/support.rs"]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "tests/released.rs"]
mod released_tests;
