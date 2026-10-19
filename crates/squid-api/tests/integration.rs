//! Integration tests against a real PostgreSQL instance.
//!
//! Requires `TEST_DATABASE_URL`, e.g.
//!   TEST_DATABASE_URL=postgres://$USER@127.0.0.1:5432/squid_test cargo test
//!
//! Each test creates its own workspace/user, so tests are safe to run in
//! parallel against one database.

use axum::body::{to_bytes, Body};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
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
    };

    app(AppState::new(db, config))
}

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
    let token = response.body["tokens"]["accessOrWorkspaceAgnosticToken"]["token"]
        .as_str()
        .expect("access token")
        .to_string();
    let workspace_id = response.body["workspace"]["id"]
        .as_str()
        .expect("workspace id")
        .to_string();
    (token, workspace_id)
}

fn person_body(first: &str, last: &str, email: &str) -> Value {
    json!({
        "name": { "firstName": first, "lastName": last },
        "emails": { "primaryEmail": email },
        "jobTitle": "Engineer"
    })
}

#[tokio::test]
async fn signup_signin_and_person_crud_over_rest() {
    let app = test_app().await;
    let email = unique_email("rest-crud");
    let (token, _workspace) = signup(&app, &email).await;

    // Create
    let created = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(person_body("Ada", "Lovelace", "ada@example.com")),
        ),
    )
    .await;
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "body: {}",
        created.body
    );
    let person_id = created.body["data"]["person"]["id"]
        .as_str()
        .expect("person id")
        .to_string();
    assert_eq!(created.body["data"]["person"]["name"]["firstName"], "Ada");

    // List: exact upstream envelope
    let listed = send(
        &app,
        authorized("GET", "/rest/people?limit=10", &token, None),
    )
    .await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.body["totalCount"], 1);
    assert_eq!(listed.body["data"]["people"].as_array().unwrap().len(), 1);
    assert!(listed.body["pageInfo"]["startCursor"].is_null());

    // Get one
    let fetched = send(
        &app,
        authorized("GET", &format!("/rest/people/{person_id}"), &token, None),
    )
    .await;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.body["data"]["person"]["id"], person_id);

    // Delete (soft) then get -> 404
    let deleted = send(
        &app,
        authorized("DELETE", &format!("/rest/people/{person_id}"), &token, None),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK);

    let gone = send(
        &app,
        authorized("GET", &format!("/rest/people/{person_id}"), &token, None),
    )
    .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
    assert_eq!(gone.body["error"], "NotFoundException");

    // Sign in again works
    let signin = send(
        &app,
        json_request(
            "POST",
            "/auth/signin",
            json!({ "email": email, "password": "super-secret-password" }),
        ),
    )
    .await;
    assert_eq!(signin.status, StatusCode::OK, "body: {}", signin.body);
    assert!(signin.body["tokens"]["accessOrWorkspaceAgnosticToken"]["token"].is_string());
}

#[tokio::test]
async fn unauthenticated_requests_get_contract_401() {
    let app = test_app().await;
    let response = send(&app, json_request("GET", "/rest/people", json!({}))).await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.body["statusCode"], 401);
    assert_eq!(response.body["error"], "UnauthorizedException");
    assert!(response.body["messages"].is_array());
}

#[tokio::test]
async fn tenant_isolation_blocks_cross_workspace_reads() {
    let app = test_app().await;

    let (token_a, workspace_a) = signup(&app, &unique_email("tenant-a")).await;
    let (token_b, workspace_b) = signup(&app, &unique_email("tenant-b")).await;
    assert_ne!(workspace_a, workspace_b);

    let created = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token_a,
            Some(person_body("Grace", "Hopper", "grace@example.com")),
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let person_id = created.body["data"]["person"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Workspace B must not see or read workspace A's person.
    let list_b = send(&app, authorized("GET", "/rest/people", &token_b, None)).await;
    assert_eq!(list_b.status, StatusCode::OK);
    assert_eq!(list_b.body["totalCount"], 0);

    let get_b = send(
        &app,
        authorized("GET", &format!("/rest/people/{person_id}"), &token_b, None),
    )
    .await;
    assert_eq!(get_b.status, StatusCode::NOT_FOUND);

    let delete_b = send(
        &app,
        authorized(
            "DELETE",
            &format!("/rest/people/{person_id}"),
            &token_b,
            None,
        ),
    )
    .await;
    assert_eq!(delete_b.status, StatusCode::NOT_FOUND);

    // And workspace A still sees it.
    let get_a = send(
        &app,
        authorized("GET", &format!("/rest/people/{person_id}"), &token_a, None),
    )
    .await;
    assert_eq!(get_a.status, StatusCode::OK);
}

#[tokio::test]
async fn graphql_login_flow_and_people_query() {
    let app = test_app().await;
    let email = unique_email("graphql");
    let (rest_token, _workspace) = signup(&app, &email).await;

    // Seed a person via REST.
    let created = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &rest_token,
            Some(person_body("Alan", "Turing", "alan@example.com")),
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);

    // Step 1: credentials -> login token
    let login = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($email:String!, $password:String!) { getLoginTokenFromCredentials(email:$email, password:$password, origin:\"http://localhost:3001\") { loginToken { token } } }",
                "variables": { "email": email, "password": "super-secret-password" }
            }),
        ),
    )
    .await;
    assert_eq!(login.status, StatusCode::OK, "body: {}", login.body);
    let login_token = login.body["data"]["getLoginTokenFromCredentials"]["loginToken"]["token"]
        .as_str()
        .expect("login token")
        .to_string();

    // Step 2: login token -> auth tokens (+ session cookie)
    let tokens = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($t:String!) { getAuthTokensFromLoginToken(loginToken:$t, origin:\"http://localhost:3001\") { tokens { accessOrWorkspaceAgnosticToken { token } } } }",
                "variables": { "t": login_token }
            }),
        ),
    )
    .await;
    assert_eq!(tokens.status, StatusCode::OK, "body: {}", tokens.body);
    let access_token = tokens.body["data"]["getAuthTokensFromLoginToken"]["tokens"]
        ["accessOrWorkspaceAgnosticToken"]["token"]
        .as_str()
        .expect("access token")
        .to_string();
    assert!(
        tokens.headers.get(header::SET_COOKIE).is_some(),
        "session cookie should be set"
    );

    // Step 3: query people with the issued token.
    let people = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "{ people { id name { firstName lastName } emails { primaryEmail } } }"
            }),
        )
        .with_bearer(&access_token),
    )
    .await;
    assert_eq!(people.status, StatusCode::OK);
    let list = people.body["data"]["people"]
        .as_array()
        .expect("people array");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"]["firstName"], "Alan");

    // Step 4: create a person over GraphQL.
    let gql_create = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: CreatePersonInput!) { createPerson(input:$input) { id name { firstName } } }",
                "variables": {
                    "input": {
                        "name": { "firstName": "Katherine", "lastName": "Johnson" },
                        "emails": { "primaryEmail": "katherine@example.com" }
                    }
                }
            }),
        )
        .with_bearer(&access_token),
    )
    .await;
    assert_eq!(
        gql_create.status,
        StatusCode::OK,
        "body: {}",
        gql_create.body
    );
    assert_eq!(
        gql_create.body["data"]["createPerson"]["name"]["firstName"],
        "Katherine"
    );
}

trait WithBearer {
    fn with_bearer(self, token: &str) -> Request<Body>;
}

impl WithBearer for Request<Body> {
    fn with_bearer(mut self, token: &str) -> Request<Body> {
        use axum::http::HeaderValue;
        self.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        self
    }
}
