use super::{UpdateFailure, UpdateSnapshot, UpdateState};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

/// 只持久化稳定分类、版本和 operation identity 的 updater 诊断仓储。
pub(crate) struct UpdateDiagnostics {
    path: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SafeDiagnostic<'a> {
    event: &'a str,
    sequence: u64,
    version: Option<&'a str>,
    category: Option<&'a str>,
    operation_id: Option<u64>,
}

impl UpdateDiagnostics {
    /// 创建固定于 Application Update 私有配置目录的 JSON Lines 诊断边界。
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// 记录公开快照，不写入包地址、签名、凭据、Release Notes 或用户路径。
    pub(crate) fn record_snapshot(&self, snapshot: &UpdateSnapshot) {
        let version = snapshot
            .candidate
            .as_ref()
            .map(|candidate| candidate.version.as_str());
        self.append(&SafeDiagnostic {
            event: if snapshot.state == UpdateState::Failed {
                "transition-failed"
            } else {
                "transition"
            },
            sequence: snapshot.sequence,
            version,
            category: snapshot
                .failure
                .as_ref()
                .map(|failure| match failure.category {
                    super::UpdateFailureCategory::Network => "network",
                    super::UpdateFailureCategory::RequestTimeout => "request_timeout",
                    super::UpdateFailureCategory::RateLimited => "rate_limited",
                    super::UpdateFailureCategory::Server => "server",
                    super::UpdateFailureCategory::Manifest => "manifest",
                    super::UpdateFailureCategory::Verification => "verification",
                    super::UpdateFailureCategory::Storage => "storage",
                    super::UpdateFailureCategory::Installer => "installer",
                }),
            operation_id: None,
        });
    }

    /// 记录 adapter 原始失败的稳定类别与 operation identity，原始文本只用于内存分类。
    pub(crate) fn record_failure(&self, operation_id: u64, failure: &UpdateFailure) {
        let _ = failure.diagnostic();
        self.append(&SafeDiagnostic {
            event: "adapter-failed",
            sequence: 0,
            version: None,
            category: Some(match failure.category() {
                super::UpdateFailureCategory::Network => "network",
                super::UpdateFailureCategory::RequestTimeout => "request_timeout",
                super::UpdateFailureCategory::RateLimited => "rate_limited",
                super::UpdateFailureCategory::Server => "server",
                super::UpdateFailureCategory::Manifest => "manifest",
                super::UpdateFailureCategory::Verification => "verification",
                super::UpdateFailureCategory::Storage => "storage",
                super::UpdateFailureCategory::Installer => "installer",
            }),
            operation_id: Some(operation_id),
        });
    }

    /// 以 append-only 方式写入单条脱敏记录；诊断不可用不阻断更新状态机。
    fn append(&self, record: &SafeDiagnostic<'_>) {
        let Some(parent) = self.path.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let Ok(line) = serde_json::to_string(record) else {
            return;
        };
        if let Ok(mut output) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = writeln!(output, "{line}");
        }
    }

    /// 返回测试可读取的诊断文件位置，不参与对外 command contract。
    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}
