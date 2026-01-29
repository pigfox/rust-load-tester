// src/lib.rs
use anyhow::Context;
use clap::Parser;
use hdrhistogram::Histogram;
use reqwest::{Method, Url};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

/* ================================ CLI ================================ */

#[derive(Parser, Debug, Clone)]
#[command(name = "endpoint_tester")]
pub struct Args {
    #[arg(long)]
    pub url: String,

    #[arg(long, default_value = "GET")]
    pub method: String,

    #[arg(long, default_value_t = 4)]
    pub concurrency: usize,

    /// Run exactly N requests total (across all workers)
    #[arg(long)]
    pub requests: Option<u64>,

    /// Run for a duration like 500ms, 10s, 2m, 1h
    #[arg(long)]
    pub duration: Option<String>,

    /// Per-request timeout like 500ms, 2s
    #[arg(long, default_value = "2s")]
    pub timeout: String,

    /// Repeatable headers: --header 'Key: Value'
    #[arg(long = "header")]
    pub headers: Vec<String>,

    /// Optional API key convenience (adds Authorization: Bearer <token>)
    #[arg(long)]
    pub api_key: Option<String>,

    /// Inline JSON payload (for POST/PUT/PATCH)
    #[arg(long)]
    pub json: Option<String>,

    /// JSON payload file path (for POST/PUT/PATCH)
    #[arg(long)]
    pub json_file: Option<String>,

    /// Print progress every N completions (0 disables)
    #[arg(long, default_value_t = 1000)]
    pub progress_every: u64,
}

/* ============================= PUBLIC API ============================= */

pub async fn main_entry() -> anyhow::Result<()> {
    let args = Args::parse();
    let run_args = RunArgs::from(args);
    let result = run(run_args).await?;
    print!("{}", render_report(&result));
    Ok(())
}

#[derive(Debug, Clone)]
pub struct RunArgs {
    pub url: String,
    pub method: String,
    pub concurrency: usize,
    pub requests: Option<u64>,
    pub duration: Option<String>,
    pub timeout: String,
    pub headers: Vec<String>,
    pub api_key: Option<String>,
    pub json: Option<String>,
    pub json_file: Option<String>,
    pub progress_every: u64,
}

impl From<Args> for RunArgs {
    fn from(a: Args) -> Self {
        Self {
            url: a.url,
            method: a.method,
            concurrency: a.concurrency,
            requests: a.requests,
            duration: a.duration,
            timeout: a.timeout,
            headers: a.headers,
            api_key: a.api_key,
            json: a.json,
            json_file: a.json_file,
            progress_every: a.progress_every,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub url: String,
    pub method: String,
    pub concurrency: usize,
    pub requests_target: Option<u64>,
    pub duration_target: Option<String>,
    pub timeout: String,
    pub elapsed_sec: f64,
    pub sent: u64,
    pub completed: u64,
    pub aggregates: Aggregates,
}

/* ============================= AGGREGATES ============================= */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetErrKind {
    Timeout,
    Connect,
    Request,
    Body,
    Decode,
    Other,
}

#[derive(Debug, Default, Clone)]
pub struct StatusClassCounts {
    pub c1xx: u64,
    pub c2xx: u64,
    pub c3xx: u64,
    pub c4xx: u64,
    pub c5xx: u64,
    pub other: u64,
}

impl StatusClassCounts {
    pub fn record(&mut self, code: u16) {
        match code / 100 {
            1 => self.c1xx += 1,
            2 => self.c2xx += 1,
            3 => self.c3xx += 1,
            4 => self.c4xx += 1,
            5 => self.c5xx += 1,
            _ => self.other += 1,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct NetErrCounts {
    pub timeout: u64,
    pub connect: u64,
    pub request: u64,
    pub body: u64,
    pub decode: u64,
    pub other: u64,
}

impl NetErrCounts {
    pub fn record(&mut self, k: NetErrKind) {
        match k {
            NetErrKind::Timeout => self.timeout += 1,
            NetErrKind::Connect => self.connect += 1,
            NetErrKind::Request => self.request += 1,
            NetErrKind::Body => self.body += 1,
            NetErrKind::Decode => self.decode += 1,
            NetErrKind::Other => self.other += 1,
        }
    }

    pub fn total(&self) -> u64 {
        self.timeout + self.connect + self.request + self.body + self.decode + self.other
    }
}

#[derive(Debug, Clone)]
pub struct Aggregates {
    pub status_exact: BTreeMap<u16, u64>,
    pub status_class: StatusClassCounts,
    pub net_errors: NetErrCounts,
    pub latency_micros: Histogram<u64>,
}

impl Aggregates {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            status_exact: BTreeMap::new(),
            status_class: StatusClassCounts::default(),
            net_errors: NetErrCounts::default(),
            latency_micros: Histogram::<u64>::new(3)?,
        })
    }

    pub fn record_status(&mut self, code: u16) {
        *self.status_exact.entry(code).or_insert(0) += 1;
        self.status_class.record(code);
    }

    pub fn record_error(&mut self, kind: NetErrKind) {
        self.net_errors.record(kind);
    }

    pub fn record_latency(&mut self, micros: u64) {
        let _ = self.latency_micros.record(micros.max(1));
    }
}

pub fn classify_reqwest_error(e: &reqwest::Error) -> NetErrKind {
    if e.is_timeout() {
        NetErrKind::Timeout
    } else if e.is_connect() {
        NetErrKind::Connect
    } else if e.is_request() {
        NetErrKind::Request
    } else if e.is_body() {
        NetErrKind::Body
    } else if e.is_decode() {
        NetErrKind::Decode
    } else {
        NetErrKind::Other
    }
}

/* ================================ RUN ================================ */

pub async fn run(args: RunArgs) -> anyhow::Result<RunResult> {
    // validate url
    let url = Url::parse(&args.url).map_err(|e| anyhow::anyhow!("Invalid --url: {e}"))?;

    // validate method (explicit allow-list; reqwest accepts extension methods)
    let method = parse_http_method(&args.method)
        .ok_or_else(|| anyhow::anyhow!("Invalid --method: {}", args.method))?;

    if args.requests.is_none() && args.duration.is_none() {
        return Err(anyhow::anyhow!(
            "You must provide either --requests or --duration"
        ));
    }

    // validate timeout and optional duration
    let timeout_dur = parse_duration(&args.timeout)
        .ok_or_else(|| anyhow::anyhow!("Invalid --timeout: {}", args.timeout))?;

    let duration_target = if let Some(d) = &args.duration {
        Some(parse_duration(d).ok_or_else(|| anyhow::anyhow!("Invalid --duration: {d}"))?)
    } else {
        None
    };

    // parse headers
    let mut header_map: BTreeMap<String, String> = BTreeMap::new();
    for h in &args.headers {
        let (k, v) = parse_header(h).ok_or_else(|| {
            anyhow::anyhow!("Invalid --header format: {h} (expected \"Key: Value\")")
        })?;
        header_map.insert(k, v);
    }
    if let Some(token) = &args.api_key {
        header_map.insert("Authorization".to_string(), format!("Bearer {token}"));
    }

    // JSON payload
    let json_payload = load_json_payload(&args)?;

    // build client
    let client = reqwest::Client::builder()
        .timeout(timeout_dur)
        .build()
        .context("Failed to build reqwest client")?;

    // shared state
    let agg = Arc::new(Mutex::new(Aggregates::new()?));
    let sent = Arc::new(AtomicU64::new(0));
    let completed = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));

    let start = Instant::now();
    let deadline = duration_target.map(|d| start + d);

    let mut handles = Vec::with_capacity(args.concurrency.max(1));
    let conc = args.concurrency.max(1);

    for _ in 0..conc {
        let client = client.clone();
        let url = url.clone();
        let method = method.clone();
        let headers = header_map.clone();
        let json_payload = json_payload.clone();
        let agg = agg.clone();
        let sent = sent.clone();
        let completed = completed.clone();
        let stop = stop.clone();
        let limit = args.requests;
        let progress_every = args.progress_every;
        let deadline = deadline;

        handles.push(tokio::spawn(async move {
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                if let Some(dl) = deadline {
                    if Instant::now() >= dl {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }

                // exact limit without overshoot
                if let Some(n) = limit {
                    let cur = sent.load(Ordering::Relaxed);
                    if cur >= n {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                    // reserve one slot
                    if sent
                        .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::Relaxed)
                        .is_err()
                    {
                        continue; // retry
                    }
                } else {
                    sent.fetch_add(1, Ordering::Relaxed);
                }

                let t0 = Instant::now();
                let mut req = client.request(method.clone(), url.clone());

                for (k, v) in &headers {
                    req = req.header(k, v);
                }
                if let Some(j) = &json_payload {
                    req = req.json(j);
                }

                let resp = req.send().await;
                let micros = t0.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;

                let mut a = agg.lock().await;
                a.record_latency(micros);

                match resp {
                    Ok(r) => a.record_status(r.status().as_u16()),
                    Err(e) => a.record_error(classify_reqwest_error(&e)),
                }

                drop(a);

                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if progress_every > 0 && done % progress_every == 0 {
                    eprintln!("progress: completed={done}");
                }
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    let aggregates = {
        let guard = agg.lock().await;
        guard.clone()
    };

    Ok(RunResult {
        url: args.url,
        method: args.method,
        concurrency: conc,
        requests_target: args.requests,
        duration_target: args.duration,
        timeout: args.timeout,
        elapsed_sec: start.elapsed().as_secs_f64(),
        sent: sent.load(Ordering::Relaxed),
        completed: completed.load(Ordering::Relaxed),
        aggregates,
    })
}

/* ============================== REPORT ============================== */

pub fn render_report(r: &RunResult) -> String {
    let mut s = String::new();
    s.push_str("== Results ==\n");
    s.push_str(&format!("url: {}\n", r.url));
    s.push_str(&format!("method: {}\n", r.method));
    s.push_str(&format!("concurrency: {}\n", r.concurrency));
    if let Some(n) = r.requests_target {
        s.push_str(&format!("requests_target: {n}\n"));
    }
    if let Some(d) = &r.duration_target {
        s.push_str(&format!("duration_target: {d}\n"));
    }
    s.push_str(&format!("timeout: {}\n\n", r.timeout));

    s.push_str(&format!("elapsed_sec: {:.3}\n", r.elapsed_sec));
    s.push_str(&format!("sent: {}\n", r.sent));
    s.push_str(&format!("completed: {}\n", r.completed));
    if r.elapsed_sec > 0.0 {
        s.push_str(&format!(
            "throughput_rps: {:.2}\n",
            (r.completed as f64) / r.elapsed_sec
        ));
    }
    s.push('\n');

    s.push_str("status_class_counts:\n");
    s.push_str(&format!("  1xx: {}\n", r.aggregates.status_class.c1xx));
    s.push_str(&format!("  2xx: {}\n", r.aggregates.status_class.c2xx));
    s.push_str(&format!("  3xx: {}\n", r.aggregates.status_class.c3xx));
    s.push_str(&format!("  4xx: {}\n", r.aggregates.status_class.c4xx));
    s.push_str(&format!("  5xx: {}\n", r.aggregates.status_class.c5xx));
    s.push_str(&format!("  other: {}\n\n", r.aggregates.status_class.other));

    s.push_str("status_exact_counts:\n");
    for (code, count) in &r.aggregates.status_exact {
        s.push_str(&format!("  {code}: {count}\n"));
    }
    s.push('\n');

    s.push_str("network_error_counts:\n");
    s.push_str(&format!("  timeout: {}\n", r.aggregates.net_errors.timeout));
    s.push_str(&format!("  connect: {}\n", r.aggregates.net_errors.connect));
    s.push_str(&format!("  request: {}\n", r.aggregates.net_errors.request));
    s.push_str(&format!("  body: {}\n", r.aggregates.net_errors.body));
    s.push_str(&format!("  decode: {}\n", r.aggregates.net_errors.decode));
    s.push_str(&format!("  other: {}\n", r.aggregates.net_errors.other));
    s.push_str(&format!("  total: {}\n\n", r.aggregates.net_errors.total()));

    let h = &r.aggregates.latency_micros;
    if h.len() > 0 {
        s.push_str("latency_ms:\n");
        s.push_str(&format!("  min: {:.3}\n", (h.min() as f64) / 1000.0));
        s.push_str(&format!(
            "  p50: {:.3}\n",
            (h.value_at_quantile(0.50) as f64) / 1000.0
        ));
        s.push_str(&format!(
            "  p90: {:.3}\n",
            (h.value_at_quantile(0.90) as f64) / 1000.0
        ));
        s.push_str(&format!(
            "  p95: {:.3}\n",
            (h.value_at_quantile(0.95) as f64) / 1000.0
        ));
        s.push_str(&format!(
            "  p99: {:.3}\n",
            (h.value_at_quantile(0.99) as f64) / 1000.0
        ));
        s.push_str(&format!("  max: {:.3}\n", (h.max() as f64) / 1000.0));
    }
    s
}

/* ============================== HELPERS ============================== */

pub fn parse_http_method(s: &str) -> Option<Method> {
    match s.trim().to_ascii_uppercase().as_str() {
        "GET" => Some(Method::GET),
        "POST" => Some(Method::POST),
        "PUT" => Some(Method::PUT),
        "DELETE" => Some(Method::DELETE),
        "PATCH" => Some(Method::PATCH),
        "HEAD" => Some(Method::HEAD),
        "OPTIONS" => Some(Method::OPTIONS),
        "TRACE" => Some(Method::TRACE),
        "CONNECT" => Some(Method::CONNECT),
        _ => None,
    }
}

pub fn parse_header(s: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = s.splitn(2, ':').collect();
    if parts.len() != 2 {
        return None;
    }
    let k = parts[0].trim();
    let v = parts[1].trim();
    if k.is_empty() {
        return None;
    }
    Some((k.to_string(), v.to_string()))
}

/// supports suffixes "ms", "s", "m", "h"
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }

    let (num, unit) = if s.ends_with("ms") {
        (&s[..s.len() - 2], "ms")
    } else if s.ends_with('s') {
        (&s[..s.len() - 1], "s")
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], "m")
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], "h")
    } else {
        return None;
    };

    let n = num.trim().parse::<u64>().ok()?;
    match unit {
        "ms" => Some(Duration::from_millis(n)),
        "s" => Some(Duration::from_secs(n)),
        "m" => Some(Duration::from_secs(n * 60)),
        "h" => Some(Duration::from_secs(n * 60 * 60)),
        _ => None,
    }
}

pub fn load_json_payload(args: &RunArgs) -> anyhow::Result<Option<Value>> {
    match (args.json.as_deref(), args.json_file.as_deref()) {
        (Some(_), Some(_)) => Err(anyhow::anyhow!(
            "Provide only one of --json or --json-file."
        )),
        (Some(s), None) => {
            let v: Value =
                serde_json::from_str(s).map_err(|e| anyhow::anyhow!("Invalid --json: {e}"))?;
            Ok(Some(v))
        }
        (None, Some(path)) => {
            let bytes = std::fs::read(path)
                .map_err(|e| anyhow::anyhow!("Failed to read --json-file {path}: {e}"))?;
            let v: Value = serde_json::from_slice(&bytes)
                .map_err(|e| anyhow::anyhow!("Invalid JSON in --json-file {path}: {e}"))?;
            Ok(Some(v))
        }
        (None, None) => Ok(None),
    }
}
