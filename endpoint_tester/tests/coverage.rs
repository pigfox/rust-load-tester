// tests/coverage.rs
use endpoint_tester::{render_report, run, Aggregates, NetErrKind, RunArgs};

#[tokio::test]
async fn run_errors_on_invalid_url() {
    let args = RunArgs {
        url: "not a url".into(),
        method: "GET".into(),
        concurrency: 1,
        requests: Some(1),
        duration: None,
        timeout: "1s".into(),
        headers: vec![],
        api_key: None,
        json: None,
        json_file: None,
        progress_every: 0,
    };
    let err = run(args).await.unwrap_err();
    assert!(format!("{err}").contains("Invalid --url"));
}

#[tokio::test]
async fn run_errors_on_invalid_method() {
    let args = RunArgs {
        url: "http://127.0.0.1/ok".into(),
        method: "NOPE".into(),
        concurrency: 1,
        requests: Some(1),
        duration: None,
        timeout: "1s".into(),
        headers: vec![],
        api_key: None,
        json: None,
        json_file: None,
        progress_every: 0,
    };
    let err = run(args).await.unwrap_err();
    assert!(format!("{err}").contains("Invalid --method"));
}

#[tokio::test]
async fn run_errors_on_invalid_timeout() {
    let args = RunArgs {
        url: "http://127.0.0.1/ok".into(),
        method: "GET".into(),
        concurrency: 1,
        requests: Some(1),
        duration: None,
        timeout: "nope".into(),
        headers: vec![],
        api_key: None,
        json: None,
        json_file: None,
        progress_every: 0,
    };
    let err = run(args).await.unwrap_err();
    assert!(format!("{err}").contains("Invalid --timeout"));
}

#[tokio::test]
async fn run_errors_on_invalid_duration_string() {
    let args = RunArgs {
        url: "http://127.0.0.1/ok".into(),
        method: "GET".into(),
        concurrency: 1,
        requests: None,
        duration: Some("nope".into()),
        timeout: "1s".into(),
        headers: vec![],
        api_key: None,
        json: None,
        json_file: None,
        progress_every: 0,
    };
    let err = run(args).await.unwrap_err();
    assert!(format!("{err}").contains("Invalid --duration"));
}

#[tokio::test]
async fn run_errors_on_invalid_header_format() {
    let args = RunArgs {
        url: "http://127.0.0.1/ok".into(),
        method: "GET".into(),
        concurrency: 1,
        requests: Some(1),
        duration: None,
        timeout: "1s".into(),
        headers: vec!["badheader".into()],
        api_key: None,
        json: None,
        json_file: None,
        progress_every: 0,
    };
    let err = run(args).await.unwrap_err();
    assert!(format!("{err}").contains("Invalid --header format"));
}

#[tokio::test]
async fn run_errors_when_json_and_json_file_both_set() {
    let args = RunArgs {
        url: "http://127.0.0.1/ok".into(),
        method: "POST".into(),
        concurrency: 1,
        requests: Some(1),
        duration: None,
        timeout: "1s".into(),
        headers: vec![],
        api_key: None,
        json: Some(r#"{"a":1}"#.into()),
        json_file: Some("payload.json".into()),
        progress_every: 0,
    };
    let err = run(args).await.unwrap_err();
    assert!(format!("{err}").contains("Provide only one of --json or --json-file"));
}

#[tokio::test]
async fn run_errors_when_no_requests_or_duration() {
    let args = RunArgs {
        url: "http://127.0.0.1/ok".into(),
        method: "GET".into(),
        concurrency: 1,
        requests: None,
        duration: None,
        timeout: "1s".into(),
        headers: vec![],
        api_key: None,
        json: None,
        json_file: None,
        progress_every: 0,
    };
    let err = run(args).await.unwrap_err();
    assert!(format!("{err}").contains("either --requests or --duration"));
}

#[test]
fn aggregates_methods_exist_and_cover_paths() {
    let mut agg = Aggregates::new().unwrap();
    agg.record_error(NetErrKind::Timeout);
    agg.record_latency(1000);
    agg.record_status(200);

    assert_eq!(agg.net_errors.timeout, 1);
    assert!(agg.latency_micros.len() >= 1);
    assert_eq!(agg.status_exact.get(&200), Some(&1));
}

#[tokio::test]
async fn render_report_covers_formatting_paths() {
    // Build a minimal-ish RunArgs that will fail fast (connect error) but still yields a report
    // We can't call render_report without a RunResult, so we just build Aggregates and ensure
    // it formats correctly via a fake structure from run() by hitting an unroutable port.
    let args = RunArgs {
        url: "http://127.0.0.1:9/".into(),
        method: "GET".into(),
        concurrency: 1,
        requests: Some(1),
        duration: None,
        timeout: "200ms".into(),
        headers: vec![],
        api_key: None,
        json: None,
        json_file: None,
        progress_every: 0,
    };

    let res = run(args).await.unwrap();
    let out = render_report(&res);
    assert!(out.contains("== Results =="));
    assert!(out.contains("network_error_counts:"));
    assert!(out.contains("status_class_counts:"));
}
