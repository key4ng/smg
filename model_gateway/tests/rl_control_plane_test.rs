//! Gateway-level tests for the RL control plane (`/v1/rl/*`).

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, Version},
};
use common::{
    mock_worker::{
        set_request_recorder, HealthStatus, MockWorker, MockWorkerConfig, RequestRecorder,
        WorkerType,
    },
    AppTestContext, TestRouterConfig,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn mock(port: u16, fail_rate: f32) -> MockWorkerConfig {
    MockWorkerConfig {
        port,
        worker_type: WorkerType::Regular,
        health_status: HealthStatus::Healthy,
        response_delay_ms: 0,
        fail_rate,
    }
}

async fn ctx(enabled: bool, workers: Vec<MockWorkerConfig>) -> AppTestContext {
    let mut config = TestRouterConfig::round_robin(0);
    config.rl.enabled = enabled;
    config.rl.control_timeout_secs = 5;
    AppTestContext::new_with_config(config, workers).await
}

#[expect(
    clippy::unwrap_used,
    reason = "test helper - panicking on failure is intentional"
)]
async fn json_of(resp: axum::response::Response) -> Value {
    serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes())
        .unwrap_or(Value::Null)
}

#[tokio::test]
async fn flag_off_leaves_v1_rl_unmounted() {
    let ctx = ctx(false, vec![mock(18901, 0.0)]).await;
    let app = ctx.create_app();
    for (method, uri) in [
        ("GET", "/v1/rl/workers"),
        ("POST", "/v1/rl/engine/flush_cache?selector=engine%3Dsglang"),
    ] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{method} {uri}");
        // An unmounted route answers the JSON not-found envelope, never an empty body.
        let body = json_of(resp).await;
        assert_eq!(
            body["error"]["code"], "unknown_url",
            "{method} {uri}: {body}"
        );
    }
    assert!(ctx.app_context.rl.is_none());
    ctx.shutdown().await;
}

#[tokio::test]
async fn discovery_lists_mock_workers() {
    let ctx = ctx(true, vec![mock(18902, 0.0), mock(18903, 0.0)]).await;
    let app = ctx.create_app();
    let resp = app
        .oneshot(Request::get("/v1/rl/workers").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_of(resp).await;
    assert_eq!(body["total"], 2);
    for w in body["workers"].as_array().unwrap() {
        assert_eq!(w["engine"], "sglang");
        assert_eq!(w["tp_size"], 1);
        assert_eq!(w["health"], "ready");
        assert_eq!(w["connection_mode"], "http");
        assert_eq!(w["capabilities"]["source"], "static");
        assert!(w["id"].as_str().unwrap().len() >= 32);
    }
    ctx.shutdown().await;
}

#[tokio::test]
async fn proxy_forwards_body_verbatim() {
    let recorder = RequestRecorder::new();
    set_request_recorder(18904, recorder.clone());
    let ctx = ctx(true, vec![mock(18904, 0.0)]).await;
    let app = ctx.create_app();
    let id = {
        let resp = app
            .clone()
            .oneshot(Request::get("/v1/rl/workers").body(Body::empty()).unwrap())
            .await
            .unwrap();
        json_of(resp).await["workers"][0]["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let payload = json!({"model_path": "/models/x", "weight_version": "42", "flush_cache": true});
    let resp = app
        .oneshot(
            Request::post(format!(
                "/v1/rl/workers/{id}/engine/update_weights_from_disk"
            ))
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_of(resp).await;
    assert_eq!(body["worker_id"], id);
    assert_eq!(body["body"]["success"], true);
    assert_eq!(recorder.only_body(), payload);
    ctx.shutdown().await;
}

#[tokio::test]
async fn fanout_hits_every_worker_and_reports_failures_without_touching_breakers() {
    let mut ctx = ctx(true, vec![mock(18905, 0.0), mock(18906, 0.0)]).await;
    // Take the second engine down *after* registration: a mock with
    // `fail_rate: 1.0` would also fail its registration probes, so that
    // worker would never reach the registry at all.
    ctx.workers[1].stop().await;
    let app = ctx.create_app();
    let resp = app
        .oneshot(
            Request::post("/v1/rl/engine/pause_generation?selector=engine%3Dsglang")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"mode":"abort"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::MULTI_STATUS);
    let body = json_of(resp).await;
    assert_eq!(body["total"], 2);
    assert_eq!(body["succeeded"], 1);
    assert_eq!(body["failed"].as_array().unwrap().len(), 1);
    assert!(body["failed"][0]["url"].as_str().unwrap().contains("18906"));
    assert_eq!(body["failed"][0]["error"], "upstream_unreachable");

    // Control calls borrow each worker's HTTP client but bypass its breaker
    // and load counter, however the engine answered.
    for worker in ctx.app_context.worker_registry.get_all() {
        assert!(worker.circuit_breaker_can_execute(), "{}", worker.url());
        assert_eq!(worker.load(), 0, "{}", worker.url());
    }
    ctx.shutdown().await;
}

#[tokio::test]
async fn control_plane_auth_guards_v1_rl() {
    let mut config = TestRouterConfig::round_robin(0);
    config.rl.enabled = true;
    config.api_key = Some("admin-secret".to_string());
    let ctx = AppTestContext::new_with_config(config, vec![mock(18907, 0.0)]).await;
    let app = ctx.create_app();

    let resp = app
        .clone()
        .oneshot(Request::get("/v1/rl/workers").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = app
        .oneshot(
            Request::get("/v1/rl/workers")
                .header("authorization", "Bearer admin-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    ctx.shutdown().await;
}

/// Control calls go through each worker's own negotiated HTTP client, so a
/// worker that resolved to HTTP/1.1 under `--upstream-http2` is spoken to
/// on HTTP/1.1 while a worker that negotiated h2c gets HTTP/2.
#[tokio::test]
async fn control_calls_follow_each_worker_negotiated_http_version() {
    let mut config = TestRouterConfig::round_robin(0);
    config.rl.enabled = true;
    config.rl.control_timeout_secs = 5;
    config.upstream_http2 = true;
    let negotiated = RequestRecorder::new();
    set_request_recorder(18908, negotiated.clone());
    let pinned = RequestRecorder::new();
    set_request_recorder(18909, pinned.clone());

    // 18908 registers at startup and negotiates h2c; 18909 is registered
    // afterwards with `http_pool.http2 = false`, which pins it to HTTP/1.1.
    let ctx = AppTestContext::new_with_config(config, vec![mock(18908, 0.0)]).await;
    let app = ctx.create_app();
    let mut pinned_worker = MockWorker::new(mock(18909, 0.0));
    let pinned_url = pinned_worker.start().await.unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::post("/workers")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "url": pinned_url, "http_pool": { "http2": false } }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // Registration runs in the background; wait until discovery lists both.
    let mut workers = Vec::new();
    for _ in 0..50 {
        let resp = app
            .clone()
            .oneshot(Request::get("/v1/rl/workers").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = json_of(resp).await;
        workers = body["workers"].as_array().cloned().unwrap_or_default();
        if workers.len() == 2 && workers.iter().all(|w| w["health"] == "ready") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(workers.len(), 2, "{workers:?}");

    for w in &workers {
        let id = w["id"].as_str().unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::post(format!("/v1/rl/workers/{id}/engine/pause_generation"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"mode":"abort"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{}", w["url"]);
    }
    assert_eq!(negotiated.versions(), vec![Version::HTTP_2]);
    assert_eq!(pinned.versions(), vec![Version::HTTP_11]);

    pinned_worker.stop().await;
    ctx.shutdown().await;
}

#[expect(
    clippy::unwrap_used,
    reason = "test helper - panicking on failure is intentional"
)]
async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let req = Request::builder().method(method).uri(path);
    let req = match body {
        Some(b) => req
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    (status, json_of(resp).await)
}

/// A worker reads Ready before its registration job finishes; a repeat in that
/// window is answered 202, so the test waits for the job to settle first.
async fn job_settles(app: &axum::Router, id: &str) -> bool {
    for _ in 0..50 {
        let (_, worker) = send(app, "GET", &format!("/workers/{id}"), None).await;
        let status = &worker["job_status"]["status"];
        if worker["id"] == id && status != "pending" && status != "processing" {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    false
}

/// slime registers each engine with `POST /workers {url, worker_type}` on
/// every run and never deletes external engines, so its second run repeats the
/// request that registered the worker. That repeat answers 200 with the same
/// id; any other POST for the URL is still a conflict. A PUT or DELETE ends
/// the registration, so the old request is no longer a repeat after either.
#[tokio::test]
async fn a_repeat_of_the_registering_post_is_answered_as_done() {
    let mut config = TestRouterConfig::round_robin(0);
    config.rl.enabled = true;
    let ctx = AppTestContext::new_with_config(config, vec![]).await;
    let app = ctx.create_app();
    let mut worker = MockWorker::new(mock(18910, 0.0));
    let url = worker.start().await.unwrap();
    let spec = json!({ "url": url, "worker_type": "regular" });

    let (status, first) = send(&app, "POST", "/workers", Some(spec.clone())).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let id = first["worker_id"].as_str().unwrap().to_string();
    assert!(
        job_settles(&app, &id).await,
        "the registration never settled"
    );

    let (status, again) = send(&app, "POST", "/workers", Some(spec.clone())).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["worker_id"], first["worker_id"]);
    assert_eq!(again["location"], first["location"]);

    let other = json!({ "url": url, "labels": { "run": "2" } });
    let (status, body) = send(&app, "POST", "/workers", Some(other.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "WORKER_ALREADY_EXISTS");

    let (status, body) = send(&app, "PUT", &format!("/workers/{id}"), Some(other)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert!(
        job_settles(&app, &id).await,
        "the replacement never settled"
    );
    let (status, body) = send(&app, "POST", "/workers", Some(spec.clone())).await;
    assert_eq!(status, StatusCode::CONFLICT, "after a PUT: {body}");

    let (status, body) = send(&app, "DELETE", &format!("/workers/{id}"), None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let (status, body) = send(&app, "POST", "/workers", Some(spec)).await;
    assert_ne!(status, StatusCode::OK, "after a DELETE: {body}");

    worker.stop().await;
    ctx.shutdown().await;
}
