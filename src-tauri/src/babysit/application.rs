use super::*;

// Babysit 的请求 identity、缓存和 provider 编排集中在这一深模块中。
/// 缓存中不可变的 Review identity；不把 review 与 snapshot 合并成单一 ID。
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ReviewIdentity {
    pub review_id: String,
    pub snapshot_id: String,
}

/// 一次分析如何处理同 identity 的进程内缓存。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BabysitGenerationPolicy {
    ReuseCached,
    Regenerate,
}

/// 一次 file analysis 的调用参数与精确 Review Snapshot identity。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BabysitFileRequest {
    pub identity: ReviewIdentity,
    pub path: String,
    pub locale: String,
    /// 当前 Review Workspace 的绝对路径，不进入前端 identity 或缓存 key。
    pub workspace_path: String,
    pub detected_language: Option<String>,
    /// 决定复用精确缓存，或请求 provider 生成并替换一套新解释。
    pub generation_policy: BabysitGenerationPolicy,
}

/// UI 查询文件是否可进入 Babysit 的 provider-independent 结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BabysitEligibility {
    pub eligible: bool,
    pub reason: Option<BabysitEligibilityReason>,
}

/// 后端唯一维护的 Babysit 不可用原因；前端只负责本地化展示。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BabysitEligibilityReason {
    NonText,
    GeneratedOrVendored,
    LockMinifiedOrSnapshot,
    UnsupportedLanguage,
}

/// 仅在进程内存在的完整结果缓存 key。
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(super) struct CacheKey {
    pub(super) review_id: String,
    pub(super) snapshot_id: String,
    pub(super) path: String,
    pub(super) locale: String,
    pub(super) detected_language: Option<String>,
    pub(super) provider_id: String,
    pub(super) process_generation: String,
    pub(super) contract_version: String,
    pub(super) model: String,
    pub(super) reasoning_effort: String,
}

/// 负责 eligibility、provider orchestration、校验和 process-local cache 的深模块。
pub struct BabysitApplication {
    pub(super) model_catalogs: Mutex<
        VecDeque<(
            crate::application_settings::BabysitProvider,
            String,
            BabysitProviderModelCatalog,
        )>,
    >,
    next_model_catalog_revision: AtomicU64,
    pub(super) cache: Mutex<HashMap<CacheKey, BabysitFileExplanations>>,
    registry:
        HashMap<crate::application_settings::BabysitProvider, Arc<dyn BabysitProviderCapabilities>>,
    settings: Option<crate::application_settings::ApplicationSettingsApplication>,
}

/// 构造 Codex App Server 的 provider-independent setup failure。
pub(super) fn codex_setup_error(failure: BabysitProviderSetupFailure) -> BabysitError {
    BabysitError::provider_setup(CODEX_PROVIDER_ID, failure)
}

impl BabysitApplication {
    /// 从唯一 provider registry seam 构造应用；测试可省略持久化设置并默认选择 Codex。
    pub(super) fn from_registry(
        registry: HashMap<
            crate::application_settings::BabysitProvider,
            Arc<dyn BabysitProviderCapabilities>,
        >,
        settings: Option<crate::application_settings::ApplicationSettingsApplication>,
    ) -> Self {
        Self {
            model_catalogs: Mutex::new(VecDeque::new()),
            next_model_catalog_revision: AtomicU64::new(1),
            cache: Mutex::new(HashMap::new()),
            registry,
            settings,
        }
    }
    /// 构造由 Application Settings 选择的生产 provider registry；此过程不探测任何 CLI。
    pub fn with_installed_provider_registry_at(
        settings_path: impl Into<std::path::PathBuf>,
    ) -> Self {
        let settings_path = settings_path.into();
        let codex_provider = Arc::new(CodexAppServerProvider::new("codex"));
        let codex_runtime = Arc::new(codex::runtime::CodexRuntime::new(
            codex_provider.clone(),
            settings_path.clone(),
        ));
        let claude_provider = Arc::new(ClaudeCodeProvider::new("claude"));
        let claude_runtime = Arc::new(claude::runtime::ClaudeRuntime::new(
            claude_provider.clone(),
            settings_path.clone(),
        ));
        // Custom capability 仅检查设置完整度；构造阶段绝不读取 endpoint 或 keychain。
        let custom_bundle = Arc::new(custom::CustomCapabilityBundle::new());
        let registry: HashMap<_, Arc<dyn BabysitProviderCapabilities>> = [
            (
                crate::application_settings::BabysitProvider::Codex,
                Arc::new(CodexCapabilityBundle {
                    provider: codex_provider,
                    runtime: codex_runtime,
                }) as Arc<dyn BabysitProviderCapabilities>,
            ),
            (
                crate::application_settings::BabysitProvider::ClaudeCode,
                Arc::new(ClaudeCapabilityBundle {
                    provider: claude_provider,
                    runtime: claude_runtime,
                }) as Arc<dyn BabysitProviderCapabilities>,
            ),
            (
                crate::application_settings::BabysitProvider::Custom,
                custom_bundle as Arc<dyn BabysitProviderCapabilities>,
            ),
        ]
        .into_iter()
        .collect();
        Self::from_registry(
            registry,
            Some(crate::application_settings::ApplicationSettingsApplication::new(settings_path)),
        )
    }

    /// 仅执行当前设置所选 provider 的 readiness，不生成分析或触碰未选中的 bundle。
    pub async fn check_selected_provider_readiness(
        &self,
        workspace_path: &Path,
    ) -> Result<(), BabysitError> {
        let (_, prepared) = self.prepare_selected_analysis(workspace_path).await?;
        drop(prepared);
        Ok(())
    }

    /// 仅在 UI 明确请求修改时准备 runtime 并读取动态可见模型目录。
    pub(crate) async fn fetch_model_catalog(
        &self,
        provider: crate::application_settings::BabysitProvider,
    ) -> Result<BabysitModelCatalog, BabysitError> {
        let catalog = self
            .registry
            .get(&provider)
            .ok_or_else(|| {
                BabysitError::provider_setup(
                    provider.identity(),
                    BabysitProviderSetupFailure::NotFound,
                )
            })?
            .model_catalog()
            .await?;
        let revision = format!(
            "{}:catalog-{}",
            provider.identity(),
            self.next_model_catalog_revision
                .fetch_add(1, Ordering::AcqRel)
        );
        self.remember_model_catalog(provider, revision.clone(), catalog.clone())?;
        Ok(BabysitModelCatalog {
            provider,
            revision,
            models: catalog.models(),
        })
    }

    /// 记录设置页本次成功加载的模型目录，供显式保存使用同一份能力快照。
    pub(super) fn remember_model_catalog<C>(
        &self,
        provider: crate::application_settings::BabysitProvider,
        revision: String,
        catalog: C,
    ) -> Result<(), BabysitError>
    where
        C: Into<BabysitProviderModelCatalog>,
    {
        let mut catalogs = self
            .model_catalogs
            .lock()
            .map_err(|_| BabysitError::Provider {
                detail: "模型目录状态不可用。".to_owned(),
            })?;
        if catalogs.len() == 8 {
            catalogs.pop_front();
        }
        catalogs.push_back((provider, revision, catalog.into()));
        Ok(())
    }

    /// 使用设置页当前编辑会话的目录校验模型偏好，避免保存时再次请求 App Server。
    pub(crate) fn validate_model_preference(
        &self,
        provider: crate::application_settings::BabysitProvider,
        revision: &str,
        preference: &crate::application_settings::BabysitModelPreference,
    ) -> Result<(), BabysitError> {
        let catalogs = self
            .model_catalogs
            .lock()
            .map_err(|_| BabysitError::ModelPreferenceInvalid)?;
        let (_, _, catalog) = catalogs
            .iter()
            .find(|(current_provider, current_revision, _)| {
                *current_provider == provider && current_revision == revision
            })
            .ok_or(BabysitError::ModelPreferenceInvalid)?;
        catalog
            .resolve(preference)
            .map(|_| ())
            .map_err(|_| BabysitError::ModelPreferenceInvalid)
    }

    /// 校验 provider 对应 executable 并返回规范路径，全程不写 Application Settings。
    pub(crate) async fn validate_provider_executable(
        &self,
        provider: crate::application_settings::BabysitProvider,
        path: &Path,
    ) -> Result<std::path::PathBuf, BabysitError> {
        self.registry
            .get(&provider)
            .ok_or_else(|| {
                BabysitError::provider_setup(
                    provider.identity(),
                    BabysitProviderSetupFailure::NotFound,
                )
            })?
            .validate_executable(path)
            .await
    }

    /// provider 或其偏好成功保存后丢弃目录 revision 与已完成缓存，不触碰运行中任务。
    pub(crate) fn invalidate_provider_preferences(&self) {
        if let Ok(mut catalogs) = self.model_catalogs.lock() {
            catalogs.clear();
        }
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// 用后端规则判断 eligibility；detected_language 允许无扩展名和特殊文件名表达已识别语言。
    pub fn eligibility(
        &self,
        path: &str,
        content: &FileChangeContent,
        detected_language: Option<&str>,
    ) -> BabysitEligibility {
        eligibility(path, content, detected_language)
    }

    /// 在启动 discovery/provider 前复用 Babysit 的唯一后端 eligibility gate。
    pub fn validate_file_eligibility(
        &self,
        path: &str,
        content: &FileChangeContent,
        detected_language: Option<&str>,
    ) -> Result<(), BabysitError> {
        ensure_eligible(path, content, detected_language)
    }

    /// 以一个完整 File Change 为单位生成并缓存 before/after Line Explanations。
    pub async fn explain_file<F>(
        &self,
        request: BabysitFileRequest,
        content: FileChangeContent,
        cancellation: &CancellationSignal,
        mut notify: F,
    ) -> Result<BabysitFileExplanations, BabysitError>
    where
        F: FnMut(BabysitProgress) -> Result<(), BabysitError>,
    {
        ensure_not_cancelled(cancellation)?;
        ensure_eligible(
            &request.path,
            &content,
            request.detected_language.as_deref(),
        )?;
        let (selected_identity, prepared) = self
            .prepare_selected_analysis(Path::new(&request.workspace_path))
            .await?;
        let selected_provider = prepared.provider;
        let resolved = prepared.resolved;
        let key = CacheKey {
            review_id: request.identity.review_id.clone(),
            snapshot_id: request.identity.snapshot_id.clone(),
            path: request.path.clone(),
            locale: request.locale.clone(),
            detected_language: request.detected_language.clone(),
            provider_id: selected_identity.identity().to_owned(),
            process_generation: prepared.process_generation,
            contract_version: ANALYSIS_CONTRACT_VERSION.to_owned(),
            model: resolved.model.clone(),
            reasoning_effort: resolved.reasoning_effort.clone(),
        };
        if request.generation_policy == BabysitGenerationPolicy::ReuseCached {
            if let Some(result) = self
                .cache
                .lock()
                .map_err(|_| BabysitError::CacheUnavailable)?
                .get(&key)
                .cloned()
            {
                notify(BabysitProgress::Completed)?;
                return Ok(result);
            }
        }

        notify(BabysitProgress::Generating)?;
        let mut provider_request = build_provider_request(&request, &content)?;
        provider_request.model = resolved.model;
        provider_request.reasoning_effort = resolved.reasoning_effort;
        let mut output = selected_provider
            .generate(provider_request.clone(), cancellation)
            .await?;
        let mut failures = normalize_and_collect_repairs(&provider_request, &mut output);
        let mut attempt = 0;
        while !failures.is_empty() {
            if attempt >= MAX_REPAIR_ATTEMPTS {
                selected_provider.abandon(&provider_request).await;
                return Err(BabysitError::RepairExhausted {
                    detail: failures
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("；"),
                });
            }
            attempt += 1;
            if let Err(error) = ensure_not_cancelled(cancellation) {
                selected_provider.abandon(&provider_request).await;
                return Err(error);
            }
            if let Err(error) = notify(BabysitProgress::Repairing { attempt }) {
                selected_provider.abandon(&provider_request).await;
                return Err(error);
            }
            let patch = selected_provider
                .repair(
                    provider_request.clone(),
                    output.clone(),
                    failures.clone(),
                    cancellation,
                )
                .await?;
            merge_repair_patch(&mut output, patch, &failures);
            failures = normalize_and_collect_repairs(&provider_request, &mut output);
        }
        validate_response(&provider_request, &output).map_err(|failure| {
            BabysitError::InvalidProviderOutput {
                detail: failure.to_string(),
            }
        })?;
        ensure_not_cancelled(cancellation)?;
        notify(BabysitProgress::Completed)?;
        self.cache
            .lock()
            .map_err(|_| BabysitError::CacheUnavailable)?
            .insert(key, output.clone());
        Ok(output)
    }

    /// 从同一 Application Settings 快照解析 provider、偏好与 readiness ownership。
    async fn prepare_selected_analysis(
        &self,
        workspace_path: &Path,
    ) -> Result<
        (
            crate::application_settings::BabysitProvider,
            PreparedBabysitAnalysis,
        ),
        BabysitError,
    > {
        let selected_settings = self
            .settings
            .as_ref()
            .map(|settings| settings.load())
            .transpose()
            .map_err(|_| BabysitError::Provider {
                detail: "Application Settings 不可用。".to_owned(),
            })?;
        let selected_identity = selected_settings
            .as_ref()
            .map(|settings| settings.babysit.provider)
            .unwrap_or(crate::application_settings::BabysitProvider::Codex);
        let selected_preference = selected_settings
            .as_ref()
            .map(|settings| settings.babysit.provider_preference(selected_identity));
        let selected_bundle = self
            .registry
            .get(&selected_identity)
            .cloned()
            .ok_or_else(|| {
                BabysitError::provider_setup(
                    selected_identity.identity(),
                    BabysitProviderSetupFailure::NotFound,
                )
            })?;
        let prepared = selected_bundle
            .prepare_analysis(workspace_path, selected_preference.as_ref())
            .await?;
        Ok((selected_identity, prepared))
    }

    /// 暴露缓存清理 seam；Review 关闭和应用退出时由后续生命周期 issue 调用。
    pub fn clear(&self) -> Result<(), BabysitError> {
        self.cache
            .lock()
            .map_err(|_| BabysitError::CacheUnavailable)?
            .clear();
        Ok(())
    }

    /// 删除一个旧 Snapshot 的全部解释，避免 refresh 后残留旧 revision 的结果。
    pub fn clear_snapshot(&self, review_id: &str, snapshot_id: &str) -> Result<(), BabysitError> {
        self.cache
            .lock()
            .map_err(|_| BabysitError::CacheUnavailable)?
            .retain(|key, _| !(key.review_id == review_id && key.snapshot_id == snapshot_id));
        Ok(())
    }

    /// 删除一个 Review 的全部解释；切换或关闭 Review 时不保留跨 Review 缓存。
    pub fn clear_review(&self, review_id: &str) -> Result<(), BabysitError> {
        self.cache
            .lock()
            .map_err(|_| BabysitError::CacheUnavailable)?
            .retain(|key, _| key.review_id != review_id);
        Ok(())
    }

    /// 应用退出时清空缓存并交给 provider 释放进程/线程资源。
    pub async fn shutdown(&self) -> Result<(), BabysitError> {
        let clear_result = self.clear();
        for bundle in self.registry.values() {
            bundle.shutdown().await;
        }
        clear_result
    }
}
