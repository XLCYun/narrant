use super::DEFAULT_LOG_FILTER;

#[test]
fn default_filter_keeps_narrant_and_rig_at_info() {
    assert_eq!(DEFAULT_LOG_FILTER, "narrant=info,rig=info");
}

#[test]
fn default_filter_is_valid() {
    assert!(tracing_subscriber::EnvFilter::try_new(DEFAULT_LOG_FILTER).is_ok());
}
