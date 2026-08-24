use super::*;

/// 默认设置必须让 Claude CLI 自行选择模型与 effort。
#[tokio::test]
async fn nullable_preference_omits_model_and_effort_flags() {
    let (provider, temp, events) = provider("success", Duration::from_secs(5));
    provider
        .generate(
            request_with_model(temp.path().to_string_lossy().into_owned(), None, None),
            OperationContext::background().cancellation(),
        )
        .await
        .unwrap();

    let events = std::fs::read_to_string(events).unwrap();
    assert!(!events.lines().any(|line| line == "--model"));
    assert!(!events.lines().any(|line| line == "--effort"));
}

/// repair 必须沿用 generation 开始时冻结的显式 CLI 覆盖。
#[tokio::test]
async fn repair_reuses_generation_resolved_model_preference() {
    let (provider, temp, events) = provider("repair", Duration::from_secs(5));
    let cancellation = OperationContext::background().cancellation().clone();
    let workspace = temp.path().to_string_lossy().into_owned();
    let generation = request_with_model(workspace, Some("opus"), Some("max"));
    let previous = provider
        .generate(generation.clone(), &cancellation)
        .await
        .unwrap();
    let mut changed = generation;
    changed.model = "haiku".to_owned();
    changed.reasoning_effort.clear();
    provider
        .repair(
            changed,
            previous,
            crate::babysit::ValidationFailure::MissingLine {
                side: FileSide::Before,
                line_number: 1,
            },
            &cancellation,
        )
        .await
        .unwrap();

    let events = std::fs::read_to_string(events).unwrap();
    assert_eq!(events.matches("--model\nopus").count(), 2);
    assert_eq!(events.matches("--effort\nmax").count(), 2);
    assert!(!events.lines().any(|line| line == "haiku"));
}

/// 内容相同的并发分析必须按 Narrant run identity 隔离各自冻结的模型偏好。
#[tokio::test]
async fn identical_requests_keep_independent_resolved_model_preferences() {
    let (provider, temp, events) = provider("repair", Duration::from_secs(5));
    let cancellation = OperationContext::background().cancellation().clone();
    let workspace = temp.path().to_string_lossy().into_owned();
    let mut first = request_with_model(workspace.clone(), Some("opus"), Some("max"));
    first.analysis_run_id = 41;
    let first_output = provider
        .generate(first.clone(), &cancellation)
        .await
        .unwrap();
    let mut second = request_with_model(workspace, Some("sonnet"), Some("low"));
    second.analysis_run_id = 42;
    provider.generate(second, &cancellation).await.unwrap();

    first.model = "haiku".to_owned();
    first.reasoning_effort.clear();
    provider
        .repair(
            first,
            first_output,
            crate::babysit::ValidationFailure::MissingLine {
                side: FileSide::Before,
                line_number: 1,
            },
            &cancellation,
        )
        .await
        .unwrap();

    let events = std::fs::read_to_string(events).unwrap();
    assert_eq!(events.matches("--model\nopus").count(), 2);
    assert_eq!(events.matches("--effort\nmax").count(), 2);
    assert_eq!(events.matches("--model\nsonnet").count(), 1);
    assert!(!events.lines().any(|line| line == "haiku"));
}
