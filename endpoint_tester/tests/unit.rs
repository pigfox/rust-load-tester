// tests/unit.rs
use endpoint_tester::{
    parse_duration, parse_header, parse_http_method, Aggregates, NetErrCounts, NetErrKind,
    StatusClassCounts,
};
use reqwest::Method;
use std::time::Duration;

#[test]
fn parse_header_ok() {
    let (k, v) = parse_header("X-Test: hello").unwrap();
    assert_eq!(k, "X-Test");
    assert_eq!(v, "hello");
}

#[test]
fn parse_header_trims() {
    let (k, v) = parse_header("  A  :  B  ").unwrap();
    assert_eq!(k, "A");
    assert_eq!(v, "B");
}

#[test]
fn parse_header_invalid() {
    assert!(parse_header("NoColon").is_none());
    assert!(parse_header(": value").is_none());
}

#[test]
fn parse_duration_ms_s_m_h() {
    assert_eq!(parse_duration("500ms"), Some(Duration::from_millis(500)));
    assert_eq!(parse_duration("10s"), Some(Duration::from_secs(10)));
    assert_eq!(parse_duration("2m"), Some(Duration::from_secs(120)));
    assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
}

#[test]
fn parse_duration_invalid() {
    assert!(parse_duration("").is_none());
    assert!(parse_duration("10").is_none());
    assert!(parse_duration("xs").is_none());
    assert!(parse_duration("10d").is_none());
}

#[test]
fn parse_http_method_allowlist() {
    assert_eq!(parse_http_method("get"), Some(Method::GET));
    assert_eq!(parse_http_method("POST"), Some(Method::POST));
    assert_eq!(parse_http_method("NOPE"), None);
}

#[test]
fn status_class_counts() {
    let mut s = StatusClassCounts::default();
    s.record(200);
    s.record(204);
    s.record(302);
    s.record(404);
    s.record(500);
    s.record(503);
    assert_eq!(s.c2xx, 2);
    assert_eq!(s.c3xx, 1);
    assert_eq!(s.c4xx, 1);
    assert_eq!(s.c5xx, 2);
}

#[test]
fn net_err_counts_total() {
    let mut n = NetErrCounts::default();
    n.record(NetErrKind::Timeout);
    n.record(NetErrKind::Timeout);
    n.record(NetErrKind::Connect);
    assert_eq!(n.timeout, 2);
    assert_eq!(n.connect, 1);
    assert_eq!(n.total(), 3);
}

#[test]
fn aggregates_recording_paths() {
    let mut a = Aggregates::new().unwrap();
    a.record_status(200);
    a.record_status(500);
    a.record_error(NetErrKind::Timeout);
    a.record_latency(1500);

    assert_eq!(a.status_exact.get(&200), Some(&1));
    assert_eq!(a.status_exact.get(&500), Some(&1));
    assert_eq!(a.status_class.c2xx, 1);
    assert_eq!(a.status_class.c5xx, 1);
    assert_eq!(a.net_errors.timeout, 1);
    assert!(a.latency_micros.len() >= 1);
}
