// tests/e2e.rs  (REPLACE ENTIRE FILE)
use endpoint_tester::{run, RunArgs};

use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::time::Duration;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;

async fn spawn_test_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);

            tokio::spawn(async move {
                let service = hyper::service::service_fn(|req: Request<Incoming>| async move {
                    let path = req.uri().path().to_string();

                    match path.as_str() {
                        "/ok" => Ok::<_, hyper::Error>(
                            Response::builder()
                                .status(StatusCode::OK)
                                .body(Full::<Bytes>::from("ok").boxed())
                                .unwrap(),
                        ),
                        "/fail" => Ok::<_, hyper::Error>(
                            Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Full::<Bytes>::from("fail").boxed())
                                .unwrap(),
                        ),
                        "/sleep" => {
                            tokio::time::sleep(Duration::from_millis(250)).await;
                            Ok::<_, hyper::Error>(
                                Response::builder()
                                    .status(StatusCode::OK)
                                    .body(Full::<Bytes>::from("slow").boxed())
                                    .unwrap(),
                            )
                        }
                        "/echo_json" => {
                            let (_parts, body) = req.into_parts();

                            // BodyExt::collect is provided by http_body_util::BodyExt (now in scope)
                            let collected = body.collect().await?;
                            let bytes = collected.to_bytes();
                            let s = String::from_utf8_lossy(bytes.as_ref()).to_string();

                            let ok = s.trim_start().starts_with('{') && s.trim_end().ends_with('}');
                            let status = if ok {
                                StatusCode::OK
                            } else {
                                StatusCode::BAD_REQUEST
                            };

                            Ok::<_, hyper::Error>(
                                Response::builder()
                                    .status(status)
                                    .body(Full::<Bytes>::from(s).boxed())
                                    .unwrap(),
                            )
                        }
                        _ => Ok::<_, hyper::Error>(
                            Response::builder()
                                .status(StatusCode::NOT_IMPLEMENTED)
                                .body(Full::<Bytes>::from("no").boxed())
                                .unwrap(),
                        ),
                    }
                });

                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });

    addr
}

#[tokio::test]
async fn e2e_counts_200s() {
    let addr = spawn_test_server().await;
    let url = format!("http://{}/ok", addr);

    let args = RunArgs {
        url,
        method: "GET".into(),
        concurrency: 4,
        requests: Some(50),
        duration: None,
        timeout: "2s".into(),
        headers: vec![],
        api_key: None,
        json: None,
        json_file: None,
        progress_every: 0,
    };

    let res = run(args).await.unwrap();
    assert_eq!(res.completed, 50);
    assert_eq!(res.aggregates.status_class.c2xx, 50);
}

#[tokio::test]
async fn e2e_counts_500s() {
    let addr = spawn_test_server().await;
    let url = format!("http://{}/fail", addr);

    let args = RunArgs {
        url,
        method: "GET".into(),
        concurrency: 4,
        requests: Some(50),
        duration: None,
        timeout: "2s".into(),
        headers: vec![],
        api_key: None,
        json: None,
        json_file: None,
        progress_every: 0,
    };

    let res = run(args).await.unwrap();
    assert_eq!(res.completed, 50);
    assert_eq!(res.aggregates.status_class.c5xx, 50);
}

#[tokio::test]
async fn e2e_timeout_errors() {
    let addr = spawn_test_server().await;
    let url = format!("http://{}/sleep", addr);

    let args = RunArgs {
        url,
        method: "GET".into(),
        concurrency: 2,
        requests: Some(10),
        duration: None,
        timeout: "50ms".into(),
        headers: vec![],
        api_key: None,
        json: None,
        json_file: None,
        progress_every: 0,
    };

    let res = run(args).await.unwrap();
    assert_eq!(res.completed, 10);
    assert_eq!(res.aggregates.net_errors.timeout, 10);
}

#[tokio::test]
async fn e2e_post_json_works() {
    let addr = spawn_test_server().await;
    let url = format!("http://{}/echo_json", addr);

    let args = RunArgs {
        url,
        method: "POST".into(),
        concurrency: 1,
        requests: Some(5),
        duration: None,
        timeout: "2s".into(),
        headers: vec!["Content-Type: application/json".into()],
        api_key: None,
        json: Some(r#"{"hello":"world"}"#.into()),
        json_file: None,
        progress_every: 0,
    };

    let res = run(args).await.unwrap();
    assert_eq!(res.completed, 5);
    assert_eq!(res.aggregates.status_class.c2xx, 5);
}
