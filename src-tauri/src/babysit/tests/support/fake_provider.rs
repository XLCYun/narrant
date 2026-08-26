use super::*;

/// Fake provider 的可预测错误脚本；用于契约和 repair loop 测试。
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FakeProviderMode {
    #[default]
    Complete,
    InvalidThenRepair,
}

/// 不依赖网络、登录或模型的 deterministic provider。
#[cfg(test)]
pub struct FakeBabysitProvider {
    mode: FakeProviderMode,
    invalid_rounds_remaining: AtomicU8,
    generate_calls: AtomicUsize,
    repair_calls: AtomicUsize,
    shutdown_calls: AtomicUsize,
    resolved_model: Mutex<BabysitResolvedModel>,
}

#[cfg(test)]
impl Default for FakeBabysitProvider {
    /// 创建默认返回完整解释的 deterministic fake provider。
    fn default() -> Self {
        Self::new(FakeProviderMode::Complete)
    }
}

#[cfg(test)]
impl FakeBabysitProvider {
    /// 构造一个指定输出脚本的 fake provider。
    pub fn new(mode: FakeProviderMode) -> Self {
        Self {
            mode,
            invalid_rounds_remaining: AtomicU8::new(
                if mode == FakeProviderMode::InvalidThenRepair {
                    1
                } else {
                    0
                },
            ),
            generate_calls: AtomicUsize::new(0),
            repair_calls: AtomicUsize::new(0),
            shutdown_calls: AtomicUsize::new(0),
            resolved_model: Mutex::new(BabysitResolvedModel {
                model: "test-model".to_owned(),
                reasoning_effort: "test-effort".to_owned(),
            }),
        }
    }

    /// 构造前 N 次输出无效、随后输出完整结果的 provider。
    pub fn with_invalid_rounds(rounds: u8) -> Self {
        Self {
            mode: FakeProviderMode::InvalidThenRepair,
            invalid_rounds_remaining: AtomicU8::new(rounds),
            generate_calls: AtomicUsize::new(0),
            repair_calls: AtomicUsize::new(0),
            shutdown_calls: AtomicUsize::new(0),
            resolved_model: Mutex::new(BabysitResolvedModel {
                model: "test-model".to_owned(),
                reasoning_effort: "test-effort".to_owned(),
            }),
        }
    }

    /// 返回 generate 次数，测试用来证明完整文件只触发一次 provider request。
    pub fn generate_calls(&self) -> usize {
        self.generate_calls.load(Ordering::Acquire)
    }

    /// 返回 repair 次数，测试用来证明 repair budget 的边界。
    pub fn repair_calls(&self) -> usize {
        self.repair_calls.load(Ordering::Acquire)
    }

    /// 返回 shutdown 次数，用于验证应用退出只释放一次 provider 资源。
    pub fn shutdown_calls(&self) -> usize {
        self.shutdown_calls.load(Ordering::Acquire)
    }

    /// 测试切换未来分析使用的解析模型身份，不影响已构造的 provider 请求。
    pub fn set_resolved_model(&self, model: &str, reasoning_effort: &str) {
        if let Ok(mut resolved) = self.resolved_model.lock() {
            resolved.model = model.to_owned();
            resolved.reasoning_effort = reasoning_effort.to_owned();
        }
    }

    /// 按请求 targets 构造稳定解释，故意不复述 source，避免 fake 测试输出污染日志。
    pub(super) fn complete_output(request: &BabysitProviderRequest) -> BabysitFileExplanations {
        let mut output = BabysitFileExplanations::default();
        for target in &request.targets {
            let text = if request.locale.to_ascii_lowercase().starts_with("en") {
                format!(
                    "Explains line {} in the {:?} file side.",
                    target.line_number, target.side
                )
            } else {
                format!(
                    "解释文件 {:?} 一侧的第 {} 行代码。",
                    target.side, target.line_number
                )
            };
            match target.side {
                FileSide::Before => {
                    output.before.insert(target.line_number.to_string(), text);
                }
                FileSide::After => {
                    output.after.insert(target.line_number.to_string(), text);
                }
            }
        }
        output
    }

    /// 按错误脚本删去全部 target 并加入垃圾 key，制造可被单轮局部 repair 修复的输出。
    fn scripted_output(&self, request: &BabysitProviderRequest) -> BabysitFileExplanations {
        let mut output = Self::complete_output(request);
        if self.invalid_rounds_remaining.load(Ordering::Acquire) > 0 {
            for target in &request.targets {
                match target.side {
                    FileSide::Before => {
                        output.before.remove(&target.line_number.to_string());
                    }
                    FileSide::After => {
                        output.after.remove(&target.line_number.to_string());
                    }
                }
            }
            output
                .after
                .insert("not-a-line".to_owned(), "garbage".to_owned());
            output
                .after
                .insert("999999".to_owned(), "garbage".to_owned());
        }
        output
    }
}

#[async_trait]
#[cfg(test)]
impl BabysitAgentProvider for FakeBabysitProvider {
    /// 返回 fake provider 的稳定缓存 identity。
    fn provider_id(&self) -> &str {
        "fake"
    }

    /// 返回 fake 进程 generation，测试中保持固定以便验证缓存行为。
    fn process_generation(&self) -> String {
        "fake-process-v1".to_owned()
    }

    async fn resolve_analysis_model(&self) -> Result<BabysitResolvedModel, BabysitError> {
        self.resolved_model
            .lock()
            .map(|resolved| resolved.clone())
            .map_err(|_| BabysitError::CacheUnavailable)
    }

    /// 一次生成完整 File Change 的所有解释，不拆分逐行调用。
    async fn generate(
        &self,
        request: BabysitProviderRequest,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        ensure_not_cancelled(cancellation)?;
        self.generate_calls.fetch_add(1, Ordering::AcqRel);
        let output = if self.mode == FakeProviderMode::InvalidThenRepair {
            self.scripted_output(&request)
        } else {
            Self::complete_output(&request)
        };
        if self.invalid_rounds_remaining.load(Ordering::Acquire) > 0 {
            self.invalid_rounds_remaining.fetch_sub(1, Ordering::AcqRel);
        }
        Ok(output)
    }

    /// 按脚本修复上一轮完整响应，模拟同一 provider thread 的 repair。
    async fn repair(
        &self,
        mut request: BabysitProviderRequest,
        _previous: BabysitFileExplanations,
        failures: Vec<ValidationFailure>,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        ensure_not_cancelled(cancellation)?;
        self.repair_calls.fetch_add(1, Ordering::AcqRel);
        let requested = failures
            .iter()
            .filter_map(|failure| match failure {
                ValidationFailure::MissingLine { side, line_number }
                | ValidationFailure::EmptyExplanation { side, line_number } => {
                    Some((*side, *line_number))
                }
                _ => None,
            })
            .collect::<HashSet<_>>();
        request
            .targets
            .retain(|target| requested.contains(&(target.side, target.line_number)));
        let output = self.scripted_output(&request);
        if self.invalid_rounds_remaining.load(Ordering::Acquire) > 0 {
            self.invalid_rounds_remaining.fetch_sub(1, Ordering::AcqRel);
        }
        Ok(output)
    }

    /// Fake provider 没有子进程，但记录 shutdown 以覆盖应用生命周期 seam。
    async fn shutdown(&self) {
        self.shutdown_calls.fetch_add(1, Ordering::AcqRel);
    }
}
