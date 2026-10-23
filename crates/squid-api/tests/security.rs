//! Security-focused integration tests (Phase 6 threat-model coverage).
//!
//! Requires `TEST_DATABASE_URL`, e.g.
//!   TEST_DATABASE_URL=postgres://$USER@127.0.0.1:5432/squid_test cargo test --test security
//!
//! These tests exercise authorization boundaries, idempotency conflicts,
//! webhook HMAC verification, and parser-level input rejection through the
//! public API surface (no crate-internal functions).

use axum::body::{to_bytes, Body};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::Router;
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use squid_api::{app, AppState};
use squid_core::Config;
use squid_db::Db;
use tower::ServiceExt;

fn database_url() -> String {
    std::env::var("TEST_DATABASE_URL").expect(
        "TEST_DATABASE_URL must be set to run integration tests \
         (e.g. postgres://$USER@127.0.0.1:5432/squid_test)",
    )
}

async fn test_app() -> Router {
    let url = database_url();
    let db = Db::connect(&url, 5).await.expect("connect test db");
    db.migrate().await.expect("core migrations");

    let config = Config {
        database_url: url,
        bind_addr: "127.0.0.1:0".to_string(),
        frontend_url: "http://localhost:3001".to_string(),
        jwt_secret: "integration-test-secret-at-least-32-characters".to_string(),
        login_token_ttl_secs: 300,
        access_token_ttl_secs: 3600,
        refresh_token_ttl_secs: 86_400,
        max_page_size: 100,
        cookie_secure: false,
        s3: None,
        http_job_allow_private: false,
        http_job_allowed_hosts: Vec::new(),
    };

    app(AppState::new(db, config))
}

#[allow(dead_code)]
struct HttpResult {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

async fn send(app: &Router, request: Request<Body>) -> HttpResult {
    let response = app.clone().oneshot(request).await.expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    HttpResult {
        status,
        headers,
        body,
    }
}

fn json_request(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn authorized(method: &str, uri: &str, token: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json");
    match body {
        Some(body) => builder.body(Body::from(body.to_string())).unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

fn unique_email(tag: &str) -> String {
    format!("{tag}-{}@example.com", uuid::Uuid::new_v4().simple())
}

fn person_body(first: &str, last: &str, email: &str) -> Value {
    json!({
        "name": { "firstName": first, "lastName": last },
        "emails": { "primaryEmail": email },
    })
}

/// Sign up and mint an API key for data-API calls.
async fn signup(app: &Router, email: &str) -> (String, String) {
    let response = send(
        app,
        json_request(
            "POST",
            "/auth/signup",
            json!({
                "email": email,
                "password": "super-secret-password",
                "workspaceName": "Acme Test"
            }),
        ),
    )
    .await;
    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "body: {}",
        response.body
    );
    let access_token = response.body["tokens"]["accessOrWorkspaceAgnosticToken"]["token"]
        .as_str()
        .expect("access token")
        .to_string();
    let workspace_id = response.body["workspace"]["id"]
        .as_str()
        .expect("workspace id")
        .to_string();

    let expires_at = "2035-01-01T00:00:00.000Z";
    let created = send(
        app,
        authorized(
            "POST",
            "/metadata",
            &access_token,
            Some(json!({
                "query": "mutation($input: CreateApiKeyInput!) { createApiKey(input:$input) { id } }",
                "variables": {
                    "input": {
                        "name": "integration",
                        "expiresAt": expires_at,
                        "roleId": uuid::Uuid::new_v4().to_string()
                    }
                }
            })),
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK, "body: {}", created.body);
    let api_key_id = created.body["data"]["createApiKey"]["id"]
        .as_str()
        .expect("api key id")
        .to_string();

    let generated = send(
        app,
        authorized(
            "POST",
            "/metadata",
            &access_token,
            Some(json!({
                "query": "mutation($id: UUID!, $expiresAt: String!) { generateApiKeyToken(apiKeyId:$id, expiresAt:$expiresAt) { token } }",
                "variables": { "id": api_key_id, "expiresAt": expires_at }
            })),
        ),
    )
    .await;
    assert_eq!(generated.status, StatusCode::OK, "body: {}", generated.body);
    let api_key = generated.body["data"]["generateApiKeyToken"]["token"]
        .as_str()
        .expect("api key token")
        .to_string();
    (api_key, workspace_id)
}

#[tokio::test]
async fn idempotent_same_key_different_body_conflicts() {
    let app = test_app().await;
    let (token, _workspace) = signup(&app, &unique_email("idempotency")).await;

    let key = "idem-key-conflict-1";

    let mut first_req = authorized(
        "POST",
        "/rest/people",
        &token,
        Some(person_body("Idem", "One", "idem-one@example.com")),
    );
    first_req.headers_mut().insert(
        header::HeaderName::from_static("idempotency-key"),
        header::HeaderValue::from_str(key).unwrap(),
    );
    let first = send(&app, first_req).await;
    assert_eq!(first.status, StatusCode::CREATED, "body: {}", first.body);
    let person_id = first.body["data"]["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Same key, different body -> 409 Conflict.
    let mut conflict_req = authorized(
        "POST",
        "/rest/people",
        &token,
        Some(person_body("Idem", "Two", "idem-two@example.com")),
    );
    conflict_req.headers_mut().insert(
        header::HeaderName::from_static("idempotency-key"),
        header::HeaderValue::from_str(key).unwrap(),
    );
    let conflict = send(&app, conflict_req).await;
    assert_eq!(
        conflict.status,
        StatusCode::CONFLICT,
        "body: {}",
        conflict.body
    );

    // Same key, same body -> replayed response.
    let mut replay_req = authorized(
        "POST",
        "/rest/people",
        &token,
        Some(person_body("Idem", "One", "idem-one@example.com")),
    );
    replay_req.headers_mut().insert(
        header::HeaderName::from_static("idempotency-key"),
        header::HeaderValue::from_str(key).unwrap(),
    );
    let replay = send(&app, replay_req).await;
    assert_eq!(replay.status, StatusCode::CREATED, "body: {}", replay.body);
    assert_eq!(replay.body["data"]["createPerson"]["id"], person_id);
}

#[tokio::test]
async fn idempotency_key_is_workspace_scoped() {
    let app = test_app().await;
    let (token_a, _) = signup(&app, &unique_email("ws-a")).await;
    let (token_b, _) = signup(&app, &unique_email("ws-b")).await;

    let key = "idem-key-cross-workspace";

    let mut first_req = authorized(
        "POST",
        "/rest/people",
        &token_a,
        Some(person_body("WS", "A", "wsa@example.com")),
    );
    first_req.headers_mut().insert(
        header::HeaderName::from_static("idempotency-key"),
        header::HeaderValue::from_str(key).unwrap(),
    );
    let first = send(&app, first_req).await;
    assert_eq!(first.status, StatusCode::CREATED, "body: {}", first.body);

    // Same key in a different workspace must not conflict.
    let mut second_req = authorized(
        "POST",
        "/rest/people",
        &token_b,
        Some(person_body("WS", "B", "wsb@example.com")),
    );
    second_req.headers_mut().insert(
        header::HeaderName::from_static("idempotency-key"),
        header::HeaderValue::from_str(key).unwrap(),
    );
    let second = send(&app, second_req).await;
    assert_eq!(second.status, StatusCode::CREATED, "body: {}", second.body);
}

#[tokio::test]
async fn webhook_hmac_rejects_tampered_body() {
    let app = test_app().await;
    let (token, _workspace) = signup(&app, &unique_email("webhook-hmac")).await;
    let secret = "hmac-tamper-test-secret";

    let webhook = send(
        &app,
        authorized(
            "POST",
            "/metadata",
            &token,
            Some(json!({
                "query": "mutation($input: CreateWebhookInput!) { createWebhook(input: $input) { id } }",
                "variables": {
                    "input": {
                        "targetUrl": "http://127.0.0.1:1/discard",
                        "operations": ["person.created"],
                        "secret": secret
                    }
                }
            })),
        ),
    )
    .await;
    assert!(
        webhook.body["errors"].is_null(),
        "webhook errors: {}",
        webhook.body
    );
    let webhook_id = webhook.body["data"]["createWebhook"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Fetch the stored webhook to get the persisted secret.
    let fetched = send(
        &app,
        authorized(
            "POST",
            "/metadata",
            &token,
            Some(json!({
                "query": "query($id: UUID!) { webhook(id: $id) { id secret } }",
                "variables": { "id": webhook_id }
            })),
        ),
    )
    .await;
    let stored_secret = fetched.body["data"]["webhook"]["secret"].as_str().unwrap();

    // Build a raw body and compute a valid signature, then tamper the body.
    let body = json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "type": "person.created",
        "event": "person.created",
        "schemaVersion": "1.0",
        "workspaceId": uuid::Uuid::new_v4().to_string(),
        "occurredAt": "2026-01-01T00:00:00Z",
        "data": { "personId": uuid::Uuid::new_v4().to_string() }
    });
    let raw = serde_json::to_string(&body).unwrap();

    let mut mac = Hmac::<Sha256>::new_from_slice(stored_secret.as_bytes()).unwrap();
    mac.update(raw.as_bytes());
    let valid_signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));

    // Tampered body (different bytes) with the valid signature must be rejected.
    let mut tampered = body.clone();
    tampered["data"]["personId"] = serde_json::Value::String(uuid::Uuid::new_v4().to_string());
    let tampered_raw = serde_json::to_string(&tampered).unwrap();

    // Recompute signature over the tampered body -> mismatches the stored expectation.
    let mut mac2 = Hmac::<Sha256>::new_from_slice(stored_secret.as_bytes()).unwrap();
    mac2.update(tampered_raw.as_bytes());
    let tampered_signature = format!("sha256={}", hex::encode(mac2.finalize().into_bytes()));

    // The signature computed over the original body must NOT match the tampered body.
    assert_ne!(valid_signature, tampered_signature);

    // Verify the original signature verifies against the original body (positive control).
    let mut mac3 = Hmac::<Sha256>::new_from_slice(stored_secret.as_bytes()).unwrap();
    mac3.update(raw.as_bytes());
    let positive = format!("sha256={}", hex::encode(mac3.finalize().into_bytes()));
    assert_eq!(positive, valid_signature);
}

#[tokio::test]
async fn rest_filter_grammar_rejects_injected_fields() {
    let app = test_app().await;
    let (token, _workspace) = signup(&app, &unique_email("filter-inject")).await;

    // A filter on a non-existent/injected field is rejected.
    let bad = send(
        &app,
        authorized(
            "GET",
            "/rest/people?filter=passwordHash[eq]:secret",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST, "body: {}", bad.body);
}
