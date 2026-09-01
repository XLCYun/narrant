use crate::{babysit, operation, review, telemetry};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;
use tauri::Manager;
use tokio::sync::OnceCell;

const APPLICATION_STOP_DEADLINE: Duration = Duration::from_secs(15);
const EXIT_IDLE: u8 = 0;
const EXIT_STOPPING: u8 = 1;
const EXIT_FINAL: u8 = 2;

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

impl ApplicationStopOutcome {
    /// 只有完整成功的停止结果才允许 updater 替换程序文件。
    #[allow(dead_code, reason = "Task 3 的 installer gate 将消费此结果")]
    pub(crate) fn allows_update_installation(self) -> bool {
        matches!(self, Self::Completed)
    }
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
    outcome: OnceCell<ApplicationStopOutcome>,
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
            outcome: OnceCell::new(),
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
        let steps = TauriApplicationStopSteps { app };
        self.stop_once_with_steps(intent, &steps).await
    }

    /// 以窄步骤接口执行唯一停止流程，便于确定性验证 deadline 与顺序。
    async fn stop_once_with_steps<S: ApplicationStopSteps>(
        &self,
        intent: ApplicationStopIntent,
        steps: &S,
    ) -> ApplicationStopOutcome {
        *self
            .outcome
            .get_or_init(|| async move {
                let _ = intent;
                match tokio::time::timeout(self.deadline, run_stop_steps(steps)).await {
                    Ok(outcome) => outcome,
                    Err(_) => ApplicationStopOutcome::TimedOut,
                }
            })
            .await
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
    fn mark_final_exit(&self) {
        self.exit_phase.store(EXIT_FINAL, Ordering::Release);
    }
}

/// 将应用现有生命周期 owner 投影为停止流程需要的窄步骤接口。
struct TauriApplicationStopSteps<'a, R: tauri::Runtime> {
    app: &'a tauri::AppHandle<R>,
}

#[async_trait::async_trait]
trait ApplicationStopSteps: Sync {
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
impl<R: tauri::Runtime> ApplicationStopSteps for TauriApplicationStopSteps<'_, R> {
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
                spawn_final_exit(app.clone());
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
                spawn_final_exit(app.clone());
            }
        }
        _ => {}
    }
}

/// 在 Tauri 异步 runtime 上完成停止后请求一次最终退出，绝不阻塞事件循环。
fn spawn_final_exit<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let outcome = app
            .state::<ApplicationShutdown>()
            .stop_once(&app, ApplicationStopIntent::OrdinaryExit)
            .await;
        if outcome != ApplicationStopOutcome::Completed {
            tracing::warn!(target: "narrant::shutdown", ?outcome, "应用清理未完整完成，仍继续退出");
        }
        app.state::<ApplicationShutdown>().mark_final_exit();
        app.exit(0);
    });
}

#[cfg(test)]
#[path = "tests/shutdown.rs"]
mod tests;
