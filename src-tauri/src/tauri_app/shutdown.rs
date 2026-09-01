use crate::{babysit, operation, review, telemetry};
#[cfg(test)]
use std::future::Future;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::Manager;
use tokio::sync::Notify;

const APPLICATION_STOP_DEADLINE: Duration = Duration::from_secs(15);
const EXIT_IDLE: u8 = 0;
const EXIT_STOPPING: u8 = 1;
const EXIT_FINAL: u8 = 2;
const UPDATE_RESTART_INTENT: u8 = 1;

/// 调用共享停止边界的业务意图；更新实现仍由调用方持有。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApplicationStopIntent {
    OrdinaryExit,
    #[allow(
        dead_code,
        reason = "Task 3 的 Application Update adapter 将消费此意图"
    )]
    UpdateRestart,
}

/// 一次共享停止流程的可复用最终结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApplicationStopOutcome {
    Completed,
    Failed,
    TimedOut,
}

/// 合并普通退出与显式更新重启的进程级安装意图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PostStopIntent {
    OrdinaryExit,
    UpdateRestart,
}

/// 确保 stop/install/final action 在进程内至多执行一次的协调器。
#[derive(Default)]
pub(crate) struct PostStopCoordinator {
    intent: AtomicU8,
    started: std::sync::atomic::AtomicBool,
}

impl PostStopCoordinator {
    /// 合并意图并启动唯一协调任务；更新重启优先于普通退出。
    pub(crate) fn request<R: tauri::Runtime>(
        self: &Arc<Self>,
        app: tauri::AppHandle<R>,
        intent: PostStopIntent,
    ) {
        if !self.begin(intent) {
            return;
        }
        let coordinator = self.clone();
        tauri::async_runtime::spawn(async move {
            coordinator.run(app).await;
        });
    }

    /// 原子合并 intent 并返回调用方是否拥有唯一执行权。
    fn begin(&self, intent: PostStopIntent) -> bool {
        if intent == PostStopIntent::UpdateRestart {
            self.intent.store(UPDATE_RESTART_INTENT, Ordering::Release);
        }
        self.started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// 返回全部竞态合并后的最终意图。
    fn merged_intent(&self) -> PostStopIntent {
        if self.intent.load(Ordering::Acquire) == UPDATE_RESTART_INTENT {
            PostStopIntent::UpdateRestart
        } else {
            PostStopIntent::OrdinaryExit
        }
    }

    /// 等待共享 stop outcome，Completed 才安装，并无条件执行一次最终动作。
    async fn run<R: tauri::Runtime>(&self, app: tauri::AppHandle<R>) {
        let outcome = app
            .state::<ApplicationShutdown>()
            .stop_once(&app, ApplicationStopIntent::OrdinaryExit)
            .await;
        let restart = self.merged_intent() == PostStopIntent::UpdateRestart;
        let install_intent = if restart {
            crate::application_update::InstallIntent::UpdateRestart
        } else {
            crate::application_update::InstallIntent::NormalExit
        };
        let mut exit_code = 0;
        let installed: Result<Option<crate::application_update::InstallExecutionOutcome>, String> =
            if outcome.allows_update_installation() {
                match app.try_state::<Arc<crate::application_update::ApplicationUpdateRuntime>>() {
                    Some(runtime) => runtime.try_install_after_completed_stop(&app, install_intent),
                    None => Ok(None),
                }
            } else {
                Ok(None)
            };
        let installed = match installed {
            Ok(Some(installed)) => Some(installed),
            Ok(None) => None,
            Err(error) => {
                exit_code = 1;
                tracing::warn!(target: "narrant::application_update", %error, "停止后更新安装失败，保留暂存包并退出");
                None
            }
        };
        if outcome != ApplicationStopOutcome::Completed {
            tracing::warn!(target: "narrant::shutdown", ?outcome, "应用清理未完整完成，跳过安装并继续退出");
        }
        app.state::<ApplicationShutdown>().mark_final_exit();
        if restart
            && matches!(
                installed,
                Some(crate::application_update::InstallExecutionOutcome::Installed { .. })
            )
        {
            app.restart();
        }
        app.exit(exit_code);
    }
}

impl ApplicationStopOutcome {
    /// 只有完整成功的停止结果才允许 updater 替换程序文件。
    #[allow(dead_code, reason = "Task 3 的 installer gate 将消费此结果")]
    pub(crate) fn allows_update_installation(self) -> bool {
        matches!(self, Self::Completed)
    }
}

/// 只在共享停止完整成功后执行安装 closure；超时或失败不会触碰暂存包。
#[cfg(test)]
pub(crate) fn install_after_successful_stop<T>(
    outcome: ApplicationStopOutcome,
    install: impl FnOnce() -> T,
) -> Option<T> {
    outcome.allows_update_installation().then(install)
}

/// 用户退出事件进入共享停止边界时，事件 adapter 应采取的动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExitRequestDecision {
    PreventAndStart,
    Prevent,
    AllowFinalExit,
}

/// Tauri-managed 的幂等应用停止边界与最终退出协调器。
pub(crate) struct ApplicationShutdown {
    execution: OnceLock<Arc<ApplicationStopExecution>>,
    deadline: Duration,
    exit_phase: AtomicU8,
}

impl Default for ApplicationShutdown {
    /// 使用产品约定的十五秒总 deadline 创建停止边界。
    fn default() -> Self {
        Self::with_deadline(APPLICATION_STOP_DEADLINE)
    }
}

impl ApplicationShutdown {
    /// 创建使用指定总 deadline 的停止边界；生产装配固定传入十五秒。
    fn with_deadline(deadline: Duration) -> Self {
        Self {
            execution: OnceLock::new(),
            deadline,
            exit_phase: AtomicU8::new(EXIT_IDLE),
        }
    }

    /// 运行或等待唯一一次应用清理，并向普通退出与更新重启返回同一结果。
    pub(crate) async fn stop_once<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        intent: ApplicationStopIntent,
    ) -> ApplicationStopOutcome {
        let steps = Arc::new(TauriApplicationStopSteps { app: app.clone() });
        self.stop_once_with_steps(intent, &steps).await
    }

    /// 以 owned 窄步骤接口启动唯一停止任务，调用方取消不会中止该任务。
    async fn stop_once_with_steps<S: ApplicationStopSteps + 'static>(
        &self,
        intent: ApplicationStopIntent,
        steps: &Arc<S>,
    ) -> ApplicationStopOutcome {
        let execution = self
            .execution
            .get_or_init(|| {
                let execution = Arc::new(ApplicationStopExecution::default());
                let task_execution = execution.clone();
                let deadline = self.deadline;
                let task_steps = steps.clone();
                let _ = intent;
                tauri::async_runtime::spawn(async move {
                    let outcome =
                        match tokio::time::timeout(deadline, run_stop_steps(task_steps.as_ref()))
                            .await
                        {
                            Ok(outcome) => outcome,
                            Err(_) => ApplicationStopOutcome::TimedOut,
                        };
                    task_execution.complete(outcome);
                });
                execution
            })
            .clone();
        execution.wait().await
    }

    /// 合并重复的系统退出/主窗口关闭事件，并只为首个事件启动异步清理。
    fn intercept_exit_request(&self) -> ExitRequestDecision {
        loop {
            match self.exit_phase.load(Ordering::Acquire) {
                EXIT_FINAL => return ExitRequestDecision::AllowFinalExit,
                EXIT_STOPPING => return ExitRequestDecision::Prevent,
                EXIT_IDLE => {
                    if self
                        .exit_phase
                        .compare_exchange(
                            EXIT_IDLE,
                            EXIT_STOPPING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        return ExitRequestDecision::PreventAndStart;
                    }
                }
                _ => unreachable!("application exit phase is internal"),
            }
        }
    }

    /// 清理返回后只放行一次内部最终退出，后续 Exit 不再触发清理。
    pub(crate) fn mark_final_exit(&self) {
        self.exit_phase.store(EXIT_FINAL, Ordering::Release);
    }
}

/// 独立停止任务的共享完成状态；等待者取消不会影响任务所有权。
#[derive(Default)]
struct ApplicationStopExecution {
    outcome: Mutex<Option<ApplicationStopOutcome>>,
    completed: Notify,
}

impl ApplicationStopExecution {
    /// 保存唯一结果并唤醒全部当前等待者。
    fn complete(&self, outcome: ApplicationStopOutcome) {
        *self
            .outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(outcome);
        self.completed.notify_waiters();
    }

    /// 等待独立任务发布结果，并在检查前注册通知以避免丢失唤醒。
    async fn wait(&self) -> ApplicationStopOutcome {
        loop {
            let completed = self.completed.notified();
            tokio::pin!(completed);
            completed.as_mut().enable();
            if let Some(outcome) = *self
                .outcome
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
            {
                return outcome;
            }
            completed.await;
        }
    }
}

/// 将应用现有生命周期 owner 投影为停止流程需要的窄步骤接口。
struct TauriApplicationStopSteps<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

#[async_trait::async_trait]
trait ApplicationStopSteps: Send + Sync {
    /// 关闭新操作入口、取消运行操作并等待注册归零。
    async fn close_operations(&self) -> Result<(), String>;

    /// 关闭全部 Babysit provider 资源。
    async fn shutdown_babysit(&self) -> Result<(), String>;

    /// 关闭 Review runtime 并清理受管 workspace。
    async fn shutdown_review(&self) -> Result<(), String>;

    /// 刷新并关闭 telemetry owner。
    async fn shutdown_telemetry(&self) -> Result<(), String>;
}

#[async_trait::async_trait]
impl<R: tauri::Runtime> ApplicationStopSteps for TauriApplicationStopSteps<R> {
    /// 使用共享 operation registry 执行 cancel/drain。
    async fn close_operations(&self) -> Result<(), String> {
        self.app
            .state::<operation::CancellableOperations>()
            .close_and_drain()
            .await
            .map_err(|error| error.to_string())
    }

    /// 通过 Babysit 聚合边界关闭 provider，而不识别具体 provider。
    async fn shutdown_babysit(&self) -> Result<(), String> {
        self.app
            .state::<babysit::BabysitApplication>()
            .shutdown()
            .await
            .map_err(|error| error.to_string())
    }

    /// 通过 Review 生命周期边界释放 runtime 与 workspace。
    async fn shutdown_review(&self) -> Result<(), String> {
        let warnings = self
            .app
            .state::<review::ReviewLifecycle>()
            .shutdown()
            .await
            .map_err(|error| error.to_string())?;
        if warnings.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "有 {} 个 Review workspace 未完成清理。",
                warnings.len()
            ))
        }
    }

    /// 通过 telemetry owner 完成最终 flush。
    async fn shutdown_telemetry(&self) -> Result<(), String> {
        self.app.state::<telemetry::Telemetry>().shutdown().await;
        Ok(())
    }
}

/// 在同一 deadline 内依序执行全部步骤；单步错误不跳过后续清理。
async fn run_stop_steps<S: ApplicationStopSteps>(steps: &S) -> ApplicationStopOutcome {
    let mut completed = true;
    for (name, result) in [
        ("operations", steps.close_operations().await),
        ("babysit", steps.shutdown_babysit().await),
        ("review", steps.shutdown_review().await),
        ("telemetry", steps.shutdown_telemetry().await),
    ] {
        if let Err(error) = result {
            completed = false;
            tracing::warn!(target: "narrant::shutdown", step = name, %error, "应用停止步骤失败");
        }
    }
    if completed {
        ApplicationStopOutcome::Completed
    } else {
        ApplicationStopOutcome::Failed
    }
}

/// 把 Tauri 退出事件转换为非阻塞共享停止与一次内部最终退出。
pub(super) fn handle_run_event<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    event: tauri::RunEvent,
) {
    match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            let decision = app.state::<ApplicationShutdown>().intercept_exit_request();
            if decision != ExitRequestDecision::AllowFinalExit {
                api.prevent_exit();
            }
            if decision == ExitRequestDecision::PreventAndStart {
                app.state::<Arc<PostStopCoordinator>>()
                    .request(app.clone(), PostStopIntent::OrdinaryExit);
            }
        }
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } if label == "main" => {
            let decision = app.state::<ApplicationShutdown>().intercept_exit_request();
            if decision != ExitRequestDecision::AllowFinalExit {
                api.prevent_close();
            }
            if decision == ExitRequestDecision::PreventAndStart {
                app.state::<Arc<PostStopCoordinator>>()
                    .request(app.clone(), PostStopIntent::OrdinaryExit);
            }
        }
        _ => {}
    }
}

/// 最终退出动作的可注入窄 seam；生产实现调用 Tauri `app.exit(0)`。
#[cfg(test)]
trait FinalExit: Send + Sync {
    /// 记录清理结果、放行内部退出并结束应用。
    fn exit(&self, outcome: ApplicationStopOutcome);
}

/// 在独立任务中等待停止结果，并无条件调用一次最终退出 seam。
#[cfg(test)]
fn spawn_final_exit_with<F>(
    stop: F,
    final_exit: Arc<dyn FinalExit>,
) -> tauri::async_runtime::JoinHandle<()>
where
    F: Future<Output = ApplicationStopOutcome> + Send + 'static,
{
    tauri::async_runtime::spawn(async move {
        let outcome = stop.await;
        if outcome != ApplicationStopOutcome::Completed {
            tracing::warn!(target: "narrant::shutdown", ?outcome, "应用清理未完整完成，仍继续退出");
        }
        final_exit.exit(outcome);
    })
}

#[cfg(test)]
#[path = "tests/shutdown.rs"]
mod tests;
