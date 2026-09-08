use super::*;
use crate::review::structure::{
    self, StructureAnalysisStatus, StructureEvent, StructurePeekContent, StructureResult,
    StructureSourceRequest,
};

/// 后台工作仅持有固定 Reader 与可克隆失效证据，不持有整个生命周期或其运行时锁。
struct StructureWorkValidity {
    review_id: String,
    snapshot_id: String,
    store: Arc<ReviewStore>,
    runtime_valid: Arc<AtomicBool>,
    shutting_down: Arc<AtomicBool>,
}

impl StructureWorkValidity {
    /// 同时检查打开实例、退出状态及 durable current，覆盖刷新激活先于 Reader 替换的间隙。
    fn is_current(&self) -> bool {
        !self.shutting_down.load(Ordering::Acquire)
            && self.runtime_valid.load(Ordering::Acquire)
            && self
                .store
                .current_snapshot_identity(&self.review_id)
                .is_ok_and(|identity| identity == self.snapshot_id)
    }
}

impl ReviewLifecycle {
    /// 在阻塞线程分析固定 Snapshot；进度和最终结果均受本次打开实例的失效令牌约束。
    pub async fn analyze_structure<F>(
        &self,
        operation: &OperationContext,
        review_id: &str,
        snapshot_id: &str,
        mut emit: F,
    ) -> Result<StructureResult, ReviewLifecycleError>
    where
        F: FnMut(StructureEvent) + Send + 'static,
    {
        let runtime = self.structure_runtime(operation, review_id, snapshot_id)?;
        let validity = self.structure_work_validity(review_id, &runtime);
        let reader = runtime.reader.clone();
        let worker_operation = operation.clone();
        // 可选分析不获取 operation read gate；关闭和刷新不必等待完整仓库索引。
        let result = tokio::task::spawn_blocking(move || {
            structure::analyze_structure(
                &reader,
                &worker_operation,
                &validity.review_id,
                &validity.snapshot_id,
                || validity.is_current(),
                |mut event| {
                    if !validity.is_current() {
                        return;
                    }
                    if worker_operation.cancellation().is_cancelled() {
                        match &mut event {
                            StructureEvent::Progress { .. } => return,
                            StructureEvent::Checkpoint { result } => {
                                result.status = StructureAnalysisStatus::Cancelled;
                                result.summary.complete = false;
                            }
                        }
                    }
                    emit(event);
                },
            )
        })
        .await
        .map_err(|error| ReviewLifecycleError::new(format!("结构分析任务未正常结束：{error}")))?;
        self.require_same_structure_runtime(review_id, &runtime)?;
        let mut result = result.map_err(ReviewLifecycleError::git)?;
        // 取消即使晚于引擎终态也保留可信证据，只撤销本次请求的“完整完成”标记。
        if operation.cancellation().is_cancelled() {
            result.status = StructureAnalysisStatus::Cancelled;
            result.summary.complete = false;
        }
        Ok(result)
    }

    /// 独立读取结构来源的完整真实文件对；未修改文件也无需创建 File Change。
    pub async fn read_structure_source(
        &self,
        operation: &OperationContext,
        review_id: &str,
        snapshot_id: &str,
        request: StructureSourceRequest,
    ) -> Result<StructurePeekContent, ReviewLifecycleError> {
        let runtime = self.structure_runtime(operation, review_id, snapshot_id)?;
        let validity = self.structure_work_validity(review_id, &runtime);
        let reader = runtime.reader.clone();
        let worker_operation = operation.clone();
        let result = tokio::task::spawn_blocking(move || {
            if !validity.is_current() {
                return Err(ReviewLifecycleError::StaleSnapshot);
            }
            structure::read_structure_source(
                &reader,
                &worker_operation,
                &validity.review_id,
                &validity.snapshot_id,
                &request,
            )
            .map_err(ReviewLifecycleError::git)
        })
        .await
        .map_err(|error| {
            ReviewLifecycleError::new(format!("结构源码读取任务未正常结束：{error}"))
        })?;
        self.require_same_structure_runtime(review_id, &runtime)?;
        if operation.cancellation().is_cancelled() {
            return Err(ReviewLifecycleError::Cancelled("操作已取消。".to_owned()));
        }
        result
    }

    /// 在开始工作前校验 durable current 和打开实例，立即释放内存锁后再进入 Git。
    fn structure_runtime(
        &self,
        operation: &OperationContext,
        review_id: &str,
        snapshot_id: &str,
    ) -> Result<OpenReviewRuntime, ReviewLifecycleError> {
        if operation.cancellation().is_cancelled() {
            return Err(ReviewLifecycleError::Cancelled("操作已取消。".to_owned()));
        }
        if self.is_shutting_down() {
            return Err(ReviewLifecycleError::ShuttingDown);
        }
        if self.snapshot_id(review_id)? != snapshot_id {
            return Err(ReviewLifecycleError::StaleSnapshot);
        }
        let runtime = self.open_review_runtime(review_id)?.ok_or_else(|| {
            ReviewLifecycleError::ReviewUnavailable("Review 已关闭或失效，请重新打开。".to_owned())
        })?;
        if runtime.snapshot_id != snapshot_id || !runtime.valid.load(Ordering::Acquire) {
            return Err(ReviewLifecycleError::StaleSnapshot);
        }
        Ok(runtime)
    }

    /// 最终接收结果时重新检查原始令牌身份，拒绝同 Snapshot 关闭后重开的旧任务。
    fn require_same_structure_runtime(
        &self,
        review_id: &str,
        expected: &OpenReviewRuntime,
    ) -> Result<(), ReviewLifecycleError> {
        let current = self.structure_runtime(
            &OperationContext::background(),
            review_id,
            &expected.snapshot_id,
        )?;
        if !expected.valid.load(Ordering::Acquire) || !Arc::ptr_eq(&current.valid, &expected.valid)
        {
            return Err(ReviewLifecycleError::StaleSnapshot);
        }
        Ok(())
    }

    /// 将工作线程所需的只读校验数据克隆为 owned 状态，不借用 async 调用方。
    fn structure_work_validity(
        &self,
        review_id: &str,
        runtime: &OpenReviewRuntime,
    ) -> StructureWorkValidity {
        StructureWorkValidity {
            review_id: review_id.to_owned(),
            snapshot_id: runtime.snapshot_id.clone(),
            store: self.store.clone(),
            runtime_valid: runtime.valid.clone(),
            shutting_down: self.shutting_down.clone(),
        }
    }
}
