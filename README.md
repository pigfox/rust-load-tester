# rust-load-tester

A fast, configurable HTTP endpoint load testing tool written in Rust.  
Designed for API and service testing with precise control over concurrency, duration, request counts, headers, authentication, and JSON payloads.  
Built on **Tokio**, **Reqwest**, and async Rust.

---

## Features

- High-concurrency async execution
- Duration-based **or** request-count-based runs
- Full HTTP method support (GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS)
- Custom headers (repeatable)
- API key / Bearer token support
- Inline JSON payloads or JSON from file
- Per-request timeouts
- Detailed result aggregation:
  - Exact HTTP status counts
  - Status class counts (2xx / 4xx / 5xx)
  - Network error breakdown (timeouts, connect errors, etc.)
  - Latency histogram and percentiles
- Clean separation between library and binary
- 80%+ test coverage with integration tests

---

## Project Layout
```text
rust-load-tester/
├── README.md
└── endpoint_tester/
    ├── Cargo.toml
    ├── src/
    │   ├── lib.rs        # Core implementation
    │   └── main.rs       # Thin wrapper calling main_entry()
    ├── tests/
    │   ├── unit.rs       # Unit tests
    │   ├── coverage.rs   # Coverage-driven integration tests
    │   └── e2e.rs        # End-to-end HTTP tests
    └── test.sh           # Test + coverage runner
```

---

## Run Tests
```bash
cd endpoint_tester
./test.sh
```

---

## Usage Examples

### Basic health check (duration-based)
```bash
cargo run --release -- \
  --url "https://example.com/health" \
  --method GET \
  --concurrency 200 \
  --duration 10s
```

### Request-count-based run with custom headers
```bash
cargo run --release -- \
  --url "https://example.com/api/v1/items" \
  --method GET \
  --concurrency 100 \
  --requests 50000 \
  --header "X-Env: staging" \
  --header "X-Client: endpoint_tester"
```

### POST with inline JSON and API key
```bash
cargo run --release -- \
  --url "https://example.com/api/v1/login" \
  --method POST \
  --concurrency 50 \
  --requests 2000 \
  --api-key "YOUR_TOKEN" \
  --header "Content-Type: application/json" \
  --json '{"email":"a@b.com","password":"pw"}'
```

### POST with JSON from file
```bash
cargo run --release -- \
  --url "https://example.com/api/v1/events" \
  --method POST \
  --concurrency 25 \
  --duration 30s \
  --json-file ./payload.json
```

---
