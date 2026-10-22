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

async fn signup_with_access(app: &Router, email: &str) -> (String, String) {
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

/// Data APIs accept API keys, not user access tokens. Most tests therefore
/// sign up and then mint an API key for subsequent REST/GraphQL calls.
async fn signup(app: &Router, email: &str) -> (String, String) {
    let (access_token, workspace_id) = signup_with_access(app, email).await;
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
    let person_id = created.body["data"]["createPerson"]["id"]
        .as_str()
        .expect("person id")
        .to_string();
    assert_eq!(
        created.body["data"]["createPerson"]["name"]["firstName"],
        "Ada"
    );

    // List: exact upstream envelope
    let listed = send(
        &app,
        authorized("GET", "/rest/people?limit=10", &token, None),
    )
    .await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.body["totalCount"], 1);
    assert_eq!(listed.body["data"]["people"].as_array().unwrap().len(), 1);
    assert!(listed.body["pageInfo"]["startCursor"].is_string());
    assert!(listed.body["pageInfo"]["endCursor"].is_string());

    // Get one
    let fetched = send(
        &app,
        authorized("GET", &format!("/rest/people/{person_id}"), &token, None),
    )
    .await;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.body["data"]["person"]["id"], person_id);

    // Update
    let updated = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/people/{person_id}"),
            &token,
            Some(json!({ "jobTitle": "Countess", "position": 3 })),
        ),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "body: {}", updated.body);
    assert_eq!(updated.body["data"]["updatePerson"]["jobTitle"], "Countess");
    assert_eq!(updated.body["data"]["updatePerson"]["position"], 3.0);
    assert_eq!(
        updated.body["data"]["updatePerson"]["name"]["firstName"],
        "Ada"
    );

    // Delete (soft) then get -> 404
    let deleted = send(
        &app,
        authorized("DELETE", &format!("/rest/people/{person_id}"), &token, None),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK);
    assert_eq!(deleted.body["data"]["deletePerson"]["id"], person_id);

    let gone = send(
        &app,
        authorized("GET", &format!("/rest/people/{person_id}"), &token, None),
    )
    .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
    assert_eq!(gone.body["error"], "NotFoundException");
    assert_eq!(gone.body["messages"], json!(["Record not found"]));
    assert!(gone.body.get("code").is_none());

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
async fn unauthenticated_requests_get_contract_403() {
    let app = test_app().await;
    let response = send(&app, json_request("GET", "/rest/people", json!({}))).await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(response.body["statusCode"], 403);
    assert_eq!(response.body["error"], "FORBIDDEN_EXCEPTION");
    assert_eq!(
        response.body["messages"],
        json!(["Missing authentication token"])
    );
}

#[tokio::test]
async fn refresh_tokens_rotate_and_signout_revokes_the_session() {
    let app = test_app().await;
    let email = unique_email("refresh");
    let signup = send(
        &app,
        json_request(
            "POST",
            "/auth/signup",
            json!({
                "email": email,
                "password": "super-secret-password",
                "workspaceName": "Rotate Co"
            }),
        ),
    )
    .await;
    assert_eq!(signup.status, StatusCode::CREATED, "body: {}", signup.body);
    let access = signup.body["tokens"]["accessOrWorkspaceAgnosticToken"]["token"]
        .as_str()
        .unwrap()
        .to_string();
    let refresh = signup.body["tokens"]["refreshToken"]["token"]
        .as_str()
        .unwrap()
        .to_string();

    let renewed = send(
        &app,
        json_request("POST", "/auth/refresh", json!({ "refreshToken": refresh })),
    )
    .await;
    assert_eq!(renewed.status, StatusCode::OK, "body: {}", renewed.body);
    let new_access = renewed.body["tokens"]["accessOrWorkspaceAgnosticToken"]["token"]
        .as_str()
        .unwrap()
        .to_string();
    let new_refresh = renewed.body["tokens"]["refreshToken"]["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(access, new_access);
    assert_ne!(refresh, new_refresh);

    // Rotation consumes the old token; replay is rejected.
    let replay = send(
        &app,
        json_request("POST", "/auth/refresh", json!({ "refreshToken": refresh })),
    )
    .await;
    assert_eq!(replay.status, StatusCode::UNAUTHORIZED);

    // The new access token works on the metadata API.
    let me = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({ "query": "{ currentWorkspace { id } }" }),
        )
        .with_bearer(&new_access),
    )
    .await;
    assert_eq!(me.status, StatusCode::OK, "body: {}", me.body);
    assert!(me.body["data"]["currentWorkspace"]["id"].is_string());

    // signOut(refreshToken:) revokes the session, invalidating access tokens
    // that carry its `sid`.
    let signed_out = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($r:String) { signOut(refreshToken:$r) }",
                "variables": { "r": new_refresh }
            }),
        ),
    )
    .await;
    assert_eq!(
        signed_out.status,
        StatusCode::OK,
        "body: {}",
        signed_out.body
    );

    let revoked = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({ "query": "{ currentWorkspace { id } }" }),
        )
        .with_bearer(&new_access),
    )
    .await;
    assert_eq!(revoked.status, StatusCode::OK);
    assert!(
        revoked.body["data"]["currentWorkspace"].is_null(),
        "revoked access token must not authenticate: {}",
        revoked.body
    );
}

#[tokio::test]
async fn graphql_renew_token_rotates_refresh_tokens() {
    let app = test_app().await;
    let email = unique_email("renew");
    let signup = send(
        &app,
        json_request(
            "POST",
            "/auth/signup",
            json!({
                "email": email,
                "password": "super-secret-password",
                "workspaceName": "Renew Co"
            }),
        ),
    )
    .await;
    let refresh = signup.body["tokens"]["refreshToken"]["token"]
        .as_str()
        .unwrap()
        .to_string();

    let renewed = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($t:String!) { renewToken(appToken:$t) { tokens { accessOrWorkspaceAgnosticToken { token } refreshToken { token } } } }",
                "variables": { "t": refresh }
            }),
        ),
    )
    .await;
    assert_eq!(renewed.status, StatusCode::OK, "body: {}", renewed.body);
    assert!(renewed.body["errors"].is_null(), "body: {}", renewed.body);
    assert!(renewed.body["data"]["renewToken"]["tokens"]["refreshToken"]["token"].is_string());

    let replay = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($t:String!) { renewToken(appToken:$t) { tokens { refreshToken { token } } } }",
                "variables": { "t": refresh }
            }),
        ),
    )
    .await;
    assert_eq!(
        replay.body["errors"][0]["extensions"]["code"], "UNAUTHENTICATED",
        "body: {}",
        replay.body
    );
}

#[tokio::test]
async fn cookie_requests_reject_foreign_origins() {
    let app = test_app().await;
    let email = unique_email("csrf");
    let signup = send(
        &app,
        json_request(
            "POST",
            "/auth/signup",
            json!({
                "email": email,
                "password": "super-secret-password",
                "workspaceName": "CSRF Co"
            }),
        ),
    )
    .await;
    let cookie = signup
        .headers
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let request = json_request(
        "POST",
        "/graphql",
        json!({ "query": "{ people { totalCount } }" }),
    )
    .with_cookie(&cookie);
    let mut request = request;
    request.headers_mut().insert(
        header::ORIGIN,
        axum::http::HeaderValue::from_static("https://evil.example"),
    );
    let blocked = send(&app, request).await;
    assert_eq!(blocked.status, StatusCode::FORBIDDEN);

    let mut allowed = json_request(
        "POST",
        "/graphql",
        json!({ "query": "{ people { totalCount } }" }),
    )
    .with_cookie(&cookie);
    allowed.headers_mut().insert(
        header::ORIGIN,
        axum::http::HeaderValue::from_static("http://localhost:3001"),
    );
    let response = send(&app, allowed).await;
    assert_eq!(response.status, StatusCode::OK, "body: {}", response.body);
    assert!(response.body["errors"].is_null());
}

#[tokio::test]
async fn rest_fields_and_depth_match_upstream_validation() {
    let app = test_app().await;
    let email = unique_email("shape");
    let (token, _workspace) = signup(&app, &email).await;

    let created = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(person_body("Grace", "Hopper", "grace@example.com")),
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let person_id = created.body["data"]["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let selected = send(
        &app,
        authorized(
            "GET",
            &format!("/rest/people/{person_id}?fields=name,id"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(selected.status, StatusCode::OK, "body: {}", selected.body);
    assert_eq!(
        selected.body["data"]["person"],
        json!({
            "id": person_id,
            "name": { "firstName": "Grace", "lastName": "Hopper" }
        })
    );

    let unknown = send(
        &app,
        authorized(
            "GET",
            &format!("/rest/people/{person_id}?fields=name.firstName"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    assert!(unknown.body["messages"][0]
        .as_str()
        .unwrap()
        .contains("Unknown or unreadable fields on 'person': name.firstName"));

    let bad_depth = send(
        &app,
        authorized(
            "GET",
            &format!("/rest/people/{person_id}?depth=2"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(bad_depth.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        bad_depth.body["messages"][0],
        "'depth=2' parameter invalid. Allowed values are 0, 1"
    );

    let list = send(
        &app,
        authorized("GET", "/rest/people?fields=name", &token, None),
    )
    .await;
    assert_eq!(list.status, StatusCode::OK, "body: {}", list.body);
    let first = &list.body["data"]["people"][0];
    assert!(first.get("jobTitle").is_none());
    assert_eq!(first["name"]["firstName"], "Grace");
    assert!(first["id"].is_string());
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
    let person_id = created.body["data"]["createPerson"]["id"]
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

    let update_b = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/people/{person_id}"),
            &token_b,
            Some(json!({ "jobTitle": "Hijacked" })),
        ),
    )
    .await;
    assert_eq!(update_b.status, StatusCode::NOT_FOUND);

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
    let _access_token = tokens.body["data"]["getAuthTokensFromLoginToken"]["tokens"]
        ["accessOrWorkspaceAgnosticToken"]["token"]
        .as_str()
        .expect("access token")
        .to_string();
    let session_cookie = tokens
        .headers
        .get(header::SET_COOKIE)
        .expect("session cookie should be set")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    // User access tokens are rejected by the data API.
    let rejected = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({ "query": "{ people { totalCount } }" }),
        )
        .with_bearer(&_access_token),
    )
    .await;
    assert_eq!(rejected.status, StatusCode::OK);
    assert_eq!(
        rejected.body["errors"][0]["extensions"]["code"], "UNAUTHENTICATED",
        "access token must not authenticate the data API: {}",
        rejected.body
    );

    // Step 3: query people with the session cookie issued alongside the token.
    let people = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "{ people { totalCount edges { node { id name { firstName lastName } emails { primaryEmail } } cursor } pageInfo { startCursor endCursor hasNextPage hasPreviousPage } } }"
            }),
        )
        .with_cookie(&session_cookie),
    )
    .await;
    assert_eq!(people.status, StatusCode::OK);
    let list = people.body["data"]["people"]["edges"]
        .as_array()
        .expect("people edges");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["node"]["name"]["firstName"], "Alan");
    assert_eq!(people.body["data"]["people"]["totalCount"], 1);
    assert!(list[0]["cursor"].is_string());

    // Step 4: create a person over GraphQL.
    let gql_create = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($data: PersonCreateInput!) { createPerson(data:$data) { id name { firstName } } }",
                "variables": {
                    "data": {
                        "name": { "firstName": "Katherine", "lastName": "Johnson" },
                        "emails": { "primaryEmail": "katherine@example.com" }
                    }
                }
            }),
        )
        .with_cookie(&session_cookie),
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

    // Step 5: updatePerson works.
    let created_id = gql_create.body["data"]["createPerson"]["id"]
        .as_str()
        .expect("created id");
    let gql_update = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($id: UUID!, $data: PersonUpdateInput!) { updatePerson(id:$id, data:$data) { id jobTitle } }",
                "variables": { "id": created_id, "data": { "jobTitle": "Computer" } }
            }),
        )
        .with_cookie(&session_cookie),
    )
    .await;
    assert_eq!(
        gql_update.status,
        StatusCode::OK,
        "body: {}",
        gql_update.body
    );
    assert_eq!(
        gql_update.body["data"]["updatePerson"]["jobTitle"],
        "Computer"
    );

    // Step 6: person(filter:) works.
    let first_id = list[0]["node"]["id"].as_str().unwrap();
    let by_filter = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "query($id: UUID!) { person(filter: { id: { eq: $id } }) { id name { firstName } } }",
                "variables": { "id": first_id }
            }),
        )
        .with_cookie(&session_cookie),
    )
    .await;
    assert_eq!(by_filter.status, StatusCode::OK, "body: {}", by_filter.body);
    assert_eq!(
        by_filter.body["data"]["person"]["name"]["firstName"],
        "Alan"
    );
}

#[tokio::test]
async fn rest_cursor_pagination_and_order_by_match_contract() {
    let app = test_app().await;
    let email = unique_email("rest-page");
    let (token, _workspace) = signup(&app, &email).await;

    for (first, last) in [("Zoe", "Zulu"), ("Amy", "Alpha"), ("Mia", "Mike")] {
        let created = send(
            &app,
            authorized(
                "POST",
                "/rest/people",
                &token,
                Some(person_body(first, last, &format!("{first}@example.com"))),
            ),
        )
        .await;
        assert_eq!(created.status, StatusCode::CREATED);
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }

    let first_page = send(
        &app,
        authorized("GET", "/rest/people?limit=2", &token, None),
    )
    .await;
    assert_eq!(
        first_page.status,
        StatusCode::OK,
        "body: {}",
        first_page.body
    );
    assert_eq!(first_page.body["totalCount"], 3);
    assert_eq!(
        first_page.body["data"]["people"].as_array().unwrap().len(),
        2
    );
    assert_eq!(first_page.body["pageInfo"]["hasNextPage"], true);
    assert_eq!(first_page.body["pageInfo"]["hasPreviousPage"], false);
    let cursor = first_page.body["pageInfo"]["endCursor"]
        .as_str()
        .expect("end cursor")
        .to_string();

    let second_page = send(
        &app,
        authorized(
            "GET",
            &format!("/rest/people?limit=2&starting_after={cursor}"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(second_page.status, StatusCode::OK);
    assert_eq!(second_page.body["totalCount"], 3);
    assert_eq!(
        second_page.body["data"]["people"].as_array().unwrap().len(),
        1
    );
    assert_eq!(second_page.body["pageInfo"]["hasNextPage"], false);
    assert_eq!(second_page.body["pageInfo"]["hasPreviousPage"], true);

    let ordered = send(
        &app,
        authorized(
            "GET",
            "/rest/people?order_by=name.firstName[AscNullsLast]",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(ordered.status, StatusCode::OK);
    let names: Vec<&str> = ordered.body["data"]["people"]
        .as_array()
        .unwrap()
        .iter()
        .map(|person| person["name"]["firstName"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Amy", "Mia", "Zoe"]);

    let filtered = send(
        &app,
        authorized(
            "GET",
            "/rest/people?filter=name.firstName%5BstartsWith%5D%3AA",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(filtered.status, StatusCode::OK, "body: {}", filtered.body);
    assert_eq!(filtered.body["totalCount"], 1);
    assert_eq!(
        filtered.body["data"]["people"][0]["name"]["firstName"],
        "Amy"
    );

    let filtered_page = send(
        &app,
        authorized(
            "GET",
            "/rest/people?limit=1&filter=jobTitle%5Beq%5D%3AEngineer",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        filtered_page.status,
        StatusCode::OK,
        "body: {}",
        filtered_page.body
    );
    assert_eq!(filtered_page.body["totalCount"], 3);
    assert_eq!(filtered_page.body["pageInfo"]["hasNextPage"], true);
    let filtered_cursor = filtered_page.body["pageInfo"]["endCursor"]
        .as_str()
        .unwrap()
        .to_string();
    let filtered_next = send(
        &app,
        authorized(
            "GET",
            &format!(
                "/rest/people?limit=1&filter=jobTitle%5Beq%5D%3AEngineer&starting_after={filtered_cursor}"
            ),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        filtered_next.status,
        StatusCode::OK,
        "body: {}",
        filtered_next.body
    );
    assert_eq!(
        filtered_next.body["data"]["people"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(filtered_next.body["pageInfo"]["hasPreviousPage"], true);

    let either = send(
        &app,
        authorized(
            "GET",
            "/rest/people?filter=or%28name.firstName%5Beq%5D%3AZoe%2Cname.firstName%5Beq%5D%3AMia%29",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(either.status, StatusCode::OK, "body: {}", either.body);
    assert_eq!(either.body["totalCount"], 2);

    let bad_filter = send(
        &app,
        authorized("GET", "/rest/people?filter=bogus%5Beq%5D%3Ax", &token, None),
    )
    .await;
    assert_eq!(bad_filter.status, StatusCode::BAD_REQUEST);

    let bad_order = send(
        &app,
        authorized("GET", "/rest/people?order_by=-createdAt", &token, None),
    )
    .await;
    assert_eq!(bad_order.status, StatusCode::BAD_REQUEST);
    assert_eq!(bad_order.body["error"], "BadRequestException");
    assert_eq!(bad_order.body["statusCode"], 400);

    let bad_cursor = send(
        &app,
        authorized(
            "GET",
            "/rest/people?starting_after=not-base64",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(bad_cursor.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn graphql_connection_pagination() {
    let app = test_app().await;
    let email = unique_email("gql-page");
    let (token, _workspace) = signup(&app, &email).await;

    for (first, last) in [("Zoe", "Zulu"), ("Amy", "Alpha"), ("Mia", "Mike")] {
        let created = send(
            &app,
            authorized(
                "POST",
                "/rest/people",
                &token,
                Some(person_body(first, last, &format!("{first}@example.com"))),
            ),
        )
        .await;
        assert_eq!(created.status, StatusCode::CREATED);
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }

    let first_page = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "{ people(first: 2) { totalCount edges { node { name { firstName } } cursor } pageInfo { hasNextPage hasPreviousPage endCursor } } }"
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        first_page.status,
        StatusCode::OK,
        "body: {}",
        first_page.body
    );
    let connection = &first_page.body["data"]["people"];
    assert_eq!(connection["totalCount"], 3);
    assert_eq!(connection["edges"].as_array().unwrap().len(), 2);
    assert_eq!(connection["pageInfo"]["hasNextPage"], true);
    assert_eq!(connection["pageInfo"]["hasPreviousPage"], false);
    let cursor = connection["pageInfo"]["endCursor"]
        .as_str()
        .expect("end cursor")
        .to_string();

    let second_page = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "query($after: String!) { people(first: 2, after: $after) { totalCount edges { node { name { firstName } } } pageInfo { hasNextPage hasPreviousPage } } }",
                "variables": { "after": cursor }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(second_page.status, StatusCode::OK);
    let connection = &second_page.body["data"]["people"];
    assert_eq!(connection["edges"].as_array().unwrap().len(), 1);
    assert_eq!(connection["pageInfo"]["hasNextPage"], false);
    assert_eq!(connection["pageInfo"]["hasPreviousPage"], true);

    let filtered = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "{ people(filter: { or: [ { name: { firstName: { eq: \"Amy\" } } }, { name: { firstName: { eq: \"Mia\" } } } ] }) { totalCount edges { node { name { firstName } } } } }"
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(filtered.status, StatusCode::OK, "body: {}", filtered.body);
    let connection = &filtered.body["data"]["people"];
    assert_eq!(connection["totalCount"], 2);
    assert_eq!(connection["edges"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn metadata_mount_serves_auth_mutations() {
    let app = test_app().await;
    let email = unique_email("metadata");

    let signup = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($email:String!, $password:String!) { signUp(email:$email, password:$password, workspaceName:\"Metadata Co\") { tokens { accessOrWorkspaceAgnosticToken { token } } workspace { id } } }",
                "variables": { "email": email, "password": "super-secret-password" }
            }),
        ),
    )
    .await;
    assert_eq!(signup.status, StatusCode::OK, "body: {}", signup.body);
    let token = signup.body["data"]["signUp"]["tokens"]["accessOrWorkspaceAgnosticToken"]["token"]
        .as_str()
        .expect("access token");

    let me = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({ "query": "{ currentWorkspace { id } }" }),
        )
        .with_bearer(token),
    )
    .await;
    assert_eq!(me.status, StatusCode::OK, "body: {}", me.body);
    assert!(me.body["data"]["currentWorkspace"]["id"].is_string());
}

#[tokio::test]
async fn api_key_lifecycle_authenticates_data_api() {
    let app = test_app().await;
    let email = unique_email("api-key");
    let (token, _workspace) = signup_with_access(&app, &email).await;

    let created = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($input: CreateApiKeyInput!) { createApiKey(input:$input) { id name expiresAt role { label } } }",
                "variables": {
                    "input": {
                        "name": "CI key",
                        "expiresAt": "2030-01-01T00:00:00Z",
                        "roleId": uuid::Uuid::new_v4()
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK, "body: {}", created.body);
    let api_key_id = created.body["data"]["createApiKey"]["id"]
        .as_str()
        .expect("api key id")
        .to_string();

    let generated = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($id: UUID!) { generateApiKeyToken(apiKeyId:$id, expiresAt:\"2030-01-01T00:00:00Z\") { token } }",
                "variables": { "id": api_key_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(generated.status, StatusCode::OK, "body: {}", generated.body);
    let api_key_token = generated.body["data"]["generateApiKeyToken"]["token"]
        .as_str()
        .expect("api key token")
        .to_string();

    // The key authenticates reads and writes on the data API.
    let list = send(
        &app,
        authorized("GET", "/rest/people", &api_key_token, None),
    )
    .await;
    assert_eq!(list.status, StatusCode::OK, "body: {}", list.body);

    let create = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &api_key_token,
            Some(person_body("Api", "Key", "api-key@example.com")),
        ),
    )
    .await;
    assert_eq!(create.status, StatusCode::CREATED, "body: {}", create.body);

    let gql = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({ "query": "{ people(first: 1) { totalCount } }" }),
        )
        .with_bearer(&api_key_token),
    )
    .await;
    assert_eq!(gql.status, StatusCode::OK, "body: {}", gql.body);
    assert_eq!(gql.body["data"]["people"]["totalCount"], 1);

    let roles = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({ "query": "{ getApiKeyRoles { id label } }" }),
        )
        .with_bearer(&token),
    )
    .await;
    let admin_role_id = roles.body["data"]["getApiKeyRoles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|role| role["label"] == "Admin")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let assigned = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($apiKeyId: UUID!, $roleId: UUID!) { assignRoleToApiKey(apiKeyId: $apiKeyId, roleId: $roleId) }",
                "variables": { "apiKeyId": api_key_id, "roleId": admin_role_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(assigned.status, StatusCode::OK, "body: {}", assigned.body);
    assert_eq!(assigned.body["data"]["assignRoleToApiKey"], true);
    assert!(
        assigned.body["errors"].is_null(),
        "errors: {}",
        assigned.body
    );

    // Revocation takes effect for the already-issued token.
    let revoked = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($input: RevokeApiKeyInput!) { revokeApiKey(input:$input) { id revokedAt } }",
                "variables": { "input": { "id": api_key_id } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(revoked.status, StatusCode::OK, "body: {}", revoked.body);
    assert!(revoked.body["data"]["revokeApiKey"]["revokedAt"].is_string());

    let denied = send(
        &app,
        authorized("GET", "/rest/people", &api_key_token, None),
    )
    .await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn client_config_matches_frontend_boot_contract() {
    let app = test_app().await;
    let response = send(&app, json_request("GET", "/client-config", json!({}))).await;
    assert_eq!(response.status, StatusCode::OK, "body: {}", response.body);
    assert_eq!(response.body["appVersion"], "v2.49.0");
    assert_eq!(response.body["authProviders"]["password"], true);
    assert_eq!(response.body["isCookieSessionEnabled"], true);
    assert_eq!(response.body["frontDomain"], "localhost");
    assert!(response.body["api"]["mutationMaximumAffectedRecords"].is_number());
}

#[tokio::test]
async fn soft_delete_and_restore_round_trip() {
    let app = test_app().await;
    let email = unique_email("restore");
    let (token, _workspace) = signup(&app, &email).await;

    let created = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(person_body("Soft", "Delete", "soft@example.com")),
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let person_id = created.body["data"]["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let soft = send(
        &app,
        authorized(
            "DELETE",
            &format!("/rest/people/{person_id}?soft_delete=true"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(soft.status, StatusCode::OK, "body: {}", soft.body);

    let gone = send(
        &app,
        authorized("GET", &format!("/rest/people/{person_id}"), &token, None),
    )
    .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);

    let restored = send(
        &app,
        json_request(
            "PATCH",
            &format!("/rest/people/{person_id}/restore"),
            json!({}),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(restored.status, StatusCode::OK, "body: {}", restored.body);
    assert_eq!(restored.body["data"]["restorePerson"]["id"], person_id);

    let back = send(
        &app,
        authorized("GET", &format!("/rest/people/{person_id}"), &token, None),
    )
    .await;
    assert_eq!(back.status, StatusCode::OK);

    // Default DELETE is a hard delete (upstream `soft_delete=false`).
    let hard = send(
        &app,
        authorized("DELETE", &format!("/rest/people/{person_id}"), &token, None),
    )
    .await;
    assert_eq!(hard.status, StatusCode::OK);
    let gone = send(
        &app,
        authorized("GET", &format!("/rest/people/{person_id}"), &token, None),
    )
    .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn graphql_soft_delete_restore_and_destroy() {
    let app = test_app().await;
    let email = unique_email("gql-destroy");
    let (token, _workspace) = signup(&app, &email).await;

    let created = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(person_body("Graph", "Lifecycle", "graph@example.com")),
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let person_id = created.body["data"]["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let deleted = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($id: UUID!) { deletePerson(id:$id) { id } }",
                "variables": { "id": person_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK, "body: {}", deleted.body);
    assert_eq!(deleted.body["data"]["deletePerson"]["id"], person_id);

    let restored = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($id: UUID!) { restorePerson(id:$id) { id } }",
                "variables": { "id": person_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(restored.status, StatusCode::OK, "body: {}", restored.body);
    assert_eq!(restored.body["data"]["restorePerson"]["id"], person_id);

    let destroyed = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($id: UUID!) { destroyPerson(id:$id) { id } }",
                "variables": { "id": person_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(destroyed.status, StatusCode::OK, "body: {}", destroyed.body);
    assert_eq!(destroyed.body["data"]["destroyPerson"]["id"], person_id);
}

trait WithBearer {
    fn with_bearer(self, token: &str) -> Request<Body>;
    fn with_cookie(self, cookie: &str) -> Request<Body>;
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

    fn with_cookie(mut self, cookie: &str) -> Request<Body> {
        use axum::http::HeaderValue;
        self.headers_mut()
            .insert(header::COOKIE, HeaderValue::from_str(cookie).unwrap());
        self
    }
}

#[tokio::test]
async fn graphql_batch_person_mutations() {
    let app = test_app().await;
    let email = unique_email("batch");
    let (token, _workspace) = signup(&app, &email).await;

    let created = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation { createPeople(data: [ { name: { firstName: \"Ada\", lastName: \"Lovelace\" }, emails: { primaryEmail: \"ada@example.com\" }, jobTitle: \"Engineer\" }, { name: { firstName: \"Alan\", lastName: \"Turing\" }, emails: { primaryEmail: \"alan@example.com\" }, jobTitle: \"Engineer\" }, { name: { firstName: \"Grace\", lastName: \"Hopper\" }, emails: { primaryEmail: \"grace@example.com\" }, jobTitle: \"Engineer\" } ]) { id name { firstName } } }"
            })),
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK, "body: {}", created.body);
    let people = created.body["data"]["createPeople"].as_array().unwrap();
    assert_eq!(people.len(), 3);

    let updated = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation { updatePeople(data: { jobTitle: \"Staff\" }, filter: { jobTitle: { eq: \"Engineer\" } }) { id jobTitle } }"
            })),
        ),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "body: {}", updated.body);
    let people = updated.body["data"]["updatePeople"].as_array().unwrap();
    assert_eq!(people.len(), 3);
    assert!(people.iter().all(|person| person["jobTitle"] == "Staff"));

    let deleted = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation { deletePeople(filter: { jobTitle: { eq: \"Staff\" } }) { id deletedAt } }"
            })),
        ),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK, "body: {}", deleted.body);
    assert_eq!(
        deleted.body["data"]["deletePeople"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    let live = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({ "query": "{ people { totalCount } }" })),
        ),
    )
    .await;
    assert_eq!(live.body["data"]["people"]["totalCount"], 0);

    let restored = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation { restorePeople(filter: { jobTitle: { eq: \"Staff\" } }) { id deletedAt } }"
            })),
        ),
    )
    .await;
    assert_eq!(restored.status, StatusCode::OK, "body: {}", restored.body);
    assert_eq!(
        restored.body["data"]["restorePeople"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    let destroyed = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation { destroyPeople(filter: { jobTitle: { eq: \"Staff\" } }) { id } }"
            })),
        ),
    )
    .await;
    assert_eq!(destroyed.status, StatusCode::OK, "body: {}", destroyed.body);
    assert_eq!(
        destroyed.body["data"]["destroyPeople"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    let empty = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({ "query": "{ people { totalCount } }" })),
        ),
    )
    .await;
    assert_eq!(empty.body["data"]["people"]["totalCount"], 0);
}

#[tokio::test]
async fn metadata_objects_match_frontend_fragment() {
    let app = test_app().await;
    let email = unique_email("metadata-objects");
    let (token, _workspace) = signup(&app, &email).await;

    let query = r#"
        query ObjectMetadataItems {
          objects(paging: { first: 1000 }) {
            edges {
              node {
                id
                universalIdentifier
                nameSingular
                namePlural
                labelSingular
                labelPlural
                color
                description
                icon
                isRemote
                isActive
                isSystem
                isUIEditable
                isUICreatable
                writability
                createdAt
                updatedAt
                labelIdentifierFieldMetadataId
                imageIdentifierFieldMetadataId
                applicationId
                shortcut
                isLabelSyncedWithName
                isSearchable
                openRecordIn
                sharingReach
                readability
                duplicateCriteria
                searchFieldMetadataList {
                  id
                  fieldMetadataId
                  tsVectorFieldMetadataId
                  position
                  createdAt
                  updatedAt
                }
                indexMetadataList {
                  id
                  createdAt
                  updatedAt
                  name
                  indexWhereClause
                  indexType
                  isUnique
                  isCustom
                  indexFieldMetadataList {
                    id
                    fieldMetadataId
                    subFieldName
                    createdAt
                    updatedAt
                    order
                  }
                }
                fieldsList {
                  id
                  universalIdentifier
                  type
                  name
                  label
                  description
                  icon
                  isActive
                  isSystem
                  isUIEditable
                  writability
                  isNullable
                  isUnique
                  isSearchable
                  createdAt
                  updatedAt
                  defaultValue
                  options
                  settings
                  isLabelSyncedWithName
                  morphId
                  applicationId
                  relation {
                    type
                    sourceObjectMetadata { id nameSingular namePlural }
                    targetObjectMetadata { id nameSingular namePlural }
                    sourceFieldMetadata { id name }
                    targetFieldMetadata { id name }
                  }
                  morphRelations {
                    type
                    sourceObjectMetadata { id nameSingular namePlural }
                    targetObjectMetadata { id nameSingular namePlural }
                    sourceFieldMetadata { id name }
                    targetFieldMetadata { id name }
                  }
                }
              }
            }
            pageInfo { hasNextPage hasPreviousPage startCursor endCursor }
          }
        }
    "#;

    let mut first_object_id = String::new();
    for _ in 0..2 {
        let response = send(
            &app,
            authorized("POST", "/metadata", &token, Some(json!({ "query": query }))),
        )
        .await;
        assert_eq!(response.status, StatusCode::OK);
        assert!(
            response.body["errors"].is_null(),
            "metadata query errors: {}",
            response.body
        );
        let objects = response.body["data"]["objects"]["edges"]
            .as_array()
            .unwrap();
        assert_eq!(objects.len(), 8, "seed must be idempotent");
        let object = objects
            .iter()
            .map(|edge| &edge["node"])
            .find(|node| node["nameSingular"] == "person")
            .expect("person object seeded");
        if first_object_id.is_empty() {
            first_object_id = object["id"].as_str().unwrap().to_string();
        }
        assert_eq!(object["nameSingular"], "person");
        assert_eq!(object["namePlural"], "people");
        assert_eq!(object["labelSingular"], "Person");
        assert_eq!(object["isSystem"], true);
        assert_eq!(object["openRecordIn"], "SIDE_PANEL");

        let fields = object["fieldsList"].as_array().unwrap();
        let names: Vec<&str> = fields.iter().map(|f| f["name"].as_str().unwrap()).collect();
        for expected in [
            "id",
            "createdAt",
            "updatedAt",
            "deletedAt",
            "name",
            "emails",
            "linkedinLink",
            "jobTitle",
            "phones",
            "avatarUrl",
            "avatarFile",
            "position",
            "createdBy",
            "updatedBy",
            "searchVector",
            "companyId",
        ] {
            assert!(
                names.contains(&expected),
                "missing field {expected}: {names:?}"
            );
        }
        let name_field = fields.iter().find(|f| f["name"] == "name").unwrap();
        assert_eq!(name_field["type"], "FULL_NAME");
        assert_eq!(object["labelIdentifierFieldMetadataId"], name_field["id"]);
        let avatar_field = fields.iter().find(|f| f["name"] == "avatarFile").unwrap();
        assert_eq!(avatar_field["type"], "FILES");
        assert_eq!(object["imageIdentifierFieldMetadataId"], avatar_field["id"]);

        let search_fields = object["searchFieldMetadataList"].as_array().unwrap();
        assert_eq!(search_fields.len(), 3);
        let vector_field_id = fields.iter().find(|f| f["name"] == "searchVector").unwrap()["id"]
            .as_str()
            .unwrap();
        for (position, entry) in search_fields.iter().enumerate() {
            assert_eq!(entry["position"], json!(position as f64));
            assert_eq!(entry["tsVectorFieldMetadataId"], vector_field_id);
            let field_id = entry["fieldMetadataId"].as_str().unwrap();
            let field = fields
                .iter()
                .find(|field| field["id"] == field_id)
                .expect("search field references a seeded field");
            assert_eq!(field["isSearchable"], true);
            assert_ne!(field["type"], "TS_VECTOR");
        }

        let company_field = fields.iter().find(|f| f["name"] == "companyId").unwrap();
        let relation = &company_field["relation"];
        assert_eq!(relation["type"], "MANY_TO_ONE");
        assert_eq!(relation["sourceObjectMetadata"]["nameSingular"], "person");
        assert_eq!(relation["targetObjectMetadata"]["nameSingular"], "company");
        assert_eq!(relation["sourceFieldMetadata"]["name"], "companyId");
        assert_eq!(relation["targetFieldMetadata"]["name"], "people");
        let company = objects
            .iter()
            .map(|edge| &edge["node"])
            .find(|node| node["nameSingular"] == "company")
            .expect("company object seeded");
        let people_field = company["fieldsList"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["name"] == "people")
            .expect("company.people reverse field");
        assert_eq!(relation["targetFieldMetadata"]["id"], people_field["id"]);
        let reverse = &people_field["relation"];
        assert_eq!(reverse["type"], "ONE_TO_MANY");
        assert_eq!(reverse["sourceObjectMetadata"]["nameSingular"], "company");
        assert_eq!(reverse["targetObjectMetadata"]["nameSingular"], "person");
        assert_eq!(reverse["sourceFieldMetadata"]["name"], "people");
        assert_eq!(reverse["targetFieldMetadata"]["name"], "companyId");

        let task_target = objects
            .iter()
            .map(|edge| &edge["node"])
            .find(|node| node["nameSingular"] == "taskTarget")
            .expect("taskTarget object seeded");
        let target_company_id = task_target["fieldsList"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["name"] == "targetCompanyId")
            .expect("taskTarget.targetCompanyId");
        assert_eq!(target_company_id["relation"], json!(null));
        let target_company = task_target["fieldsList"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["name"] == "targetCompany")
            .expect("taskTarget.targetCompany");
        assert_eq!(target_company["relation"]["type"], "MANY_TO_ONE");
        assert_eq!(
            target_company["relation"]["sourceObjectMetadata"]["nameSingular"],
            "taskTarget"
        );
        assert_eq!(
            target_company["relation"]["targetObjectMetadata"]["nameSingular"],
            "company"
        );
        assert_eq!(
            target_company["relation"]["targetFieldMetadata"]["name"],
            "taskTargets"
        );
        assert_eq!(
            target_company["morphId"],
            task_target["fieldsList"]
                .as_array()
                .unwrap()
                .iter()
                .find(|field| field["name"] == "target")
                .unwrap()["morphId"]
        );
        let company_task_targets = company["fieldsList"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["name"] == "taskTargets")
            .expect("company.taskTargets reverse field");
        assert_eq!(
            target_company["relation"]["targetFieldMetadata"]["id"],
            company_task_targets["id"]
        );
        assert_eq!(company_task_targets["relation"]["type"], "ONE_TO_MANY");

        let target = task_target["fieldsList"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["name"] == "target")
            .expect("taskTarget.target morph head");
        assert_eq!(target["relation"], json!(null));
        let morph_relations = target["morphRelations"].as_array().unwrap();
        assert_eq!(morph_relations.len(), 3);
        let morph_targets: Vec<&str> = morph_relations
            .iter()
            .map(|relation| {
                relation["targetObjectMetadata"]["nameSingular"]
                    .as_str()
                    .unwrap()
            })
            .collect();
        assert_eq!(morph_targets, ["company", "opportunity", "person"]);
        for relation in morph_relations {
            assert_eq!(relation["type"], "MANY_TO_ONE");
            assert_eq!(relation["targetFieldMetadata"]["name"], "taskTargets");
        }

        let task = objects
            .iter()
            .map(|edge| &edge["node"])
            .find(|node| node["nameSingular"] == "task")
            .expect("task object seeded");
        let task_relation = task_target["fieldsList"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["name"] == "task")
            .expect("taskTarget.task relation");
        assert_eq!(
            task_relation["relation"]["targetObjectMetadata"]["nameSingular"],
            "task"
        );
        assert_eq!(
            task_relation["relation"]["targetFieldMetadata"]["name"],
            "taskTargets"
        );
        let task_targets = task["fieldsList"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["name"] == "taskTargets")
            .expect("task.taskTargets reverse field");
        assert_eq!(
            task_relation["relation"]["targetFieldMetadata"]["id"],
            task_targets["id"]
        );

        let workspace_member = objects
            .iter()
            .map(|edge| &edge["node"])
            .find(|node| node["nameSingular"] == "workspaceMember")
            .expect("workspaceMember object seeded");
        assert_eq!(workspace_member["isUICreatable"], false);
        let workspace_member_fields = workspace_member["fieldsList"].as_array().unwrap();
        for (field_name, target_object) in [
            ("assignedTasks", "task"),
            ("accountOwnerForCompanies", "company"),
        ] {
            let field = workspace_member_fields
                .iter()
                .find(|field| field["name"] == field_name)
                .unwrap_or_else(|| panic!("workspaceMember.{field_name}"));
            assert_eq!(field["relation"]["type"], "ONE_TO_MANY");
            assert_eq!(
                field["relation"]["targetObjectMetadata"]["nameSingular"],
                target_object
            );
        }
        let assignee = task["fieldsList"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["name"] == "assigneeId")
            .expect("task.assigneeId");
        let assigned_tasks = workspace_member_fields
            .iter()
            .find(|field| field["name"] == "assignedTasks")
            .unwrap();
        assert_eq!(
            assignee["relation"]["targetFieldMetadata"]["id"],
            assigned_tasks["id"]
        );
        assert_eq!(
            assignee["relation"]["targetFieldMetadata"]["name"],
            "assignedTasks"
        );

        assert!(
            objects
                .iter()
                .any(|edge| edge["node"]["nameSingular"] == "noteTarget"),
            "noteTarget object seeded"
        );
        assert_eq!(name_field["relation"], json!(null));
        assert_eq!(name_field["morphRelations"], json!([]));
    }
}

#[tokio::test]
async fn metadata_default_view_matches_frontend_fragment() {
    let app = test_app().await;
    let email = unique_email("views");
    let (token, _workspace) = signup(&app, &email).await;

    let objects = send(
        &app,
        authorized(
            "POST",
            "/metadata",
            &token,
            Some(json!({
                "query": "{ objects(paging: { first: 10 }) { edges { node { id nameSingular } } } }"
            })),
        ),
    )
    .await;
    let object_id = objects.body["data"]["objects"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| &edge["node"])
        .find(|node| node["nameSingular"] == "person")
        .and_then(|node| node["id"].as_str())
        .expect("person object id")
        .to_string();

    let query = r#"
        query FindManyViews($objectMetadataId: String, $viewTypes: [ViewType!]) {
          getViews(objectMetadataId: $objectMetadataId, viewTypes: $viewTypes) {
            id
            name
            objectMetadataId
            type
            key
            icon
            position
            isCompact
            kanbanAggregateOperation
            kanbanAggregateOperationFieldMetadataId
            mainGroupByFieldMetadataId
            shouldHideEmptyGroups
            kanbanColumnWidth
            groupLoadLimit
            anyFieldFilterValue
            calendarFieldMetadataId
            calendarEndFieldMetadataId
            calendarLayout
            visibility
            createdByUserWorkspaceId
            applicationId
            isActive
            viewFields {
              id
              fieldMetadataId
              viewId
              isVisible
              position
              size
              aggregateOperation
              viewFieldGroupId
              isActive
              createdAt
              updatedAt
              deletedAt
            }
            viewFieldGroups {
              id
              name
              position
              isVisible
              viewId
              isActive
              createdAt
              updatedAt
              deletedAt
              viewFields {
                id
                fieldMetadataId
                viewId
                isVisible
                position
                size
                aggregateOperation
                viewFieldGroupId
                isActive
                createdAt
                updatedAt
                deletedAt
              }
            }
            viewFilters {
              id
              fieldMetadataId
              operand
              value
              viewFilterGroupId
              positionInViewFilterGroup
              subFieldName
              relationTargetFieldMetadataId
              viewId
              createdAt
              updatedAt
              deletedAt
            }
            viewFilterGroups {
              id
              parentViewFilterGroupId
              logicalOperator
              positionInViewFilterGroup
              viewId
            }
            viewSorts {
              id
              fieldMetadataId
              direction
              subFieldName
              viewId
              createdAt
              deletedAt
              updatedAt
            }
            viewGroups {
              id
              isVisible
              fieldValue
              position
              viewId
              createdAt
              updatedAt
              deletedAt
            }
          }
        }
    "#;

    for _ in 0..2 {
        let response = send(
            &app,
            authorized(
                "POST",
                "/metadata",
                &token,
                Some(json!({
                    "query": query,
                    "variables": { "objectMetadataId": object_id, "viewTypes": ["TABLE"] }
                })),
            ),
        )
        .await;
        assert_eq!(response.status, StatusCode::OK);
        assert!(
            response.body["errors"].is_null(),
            "view query errors: {}",
            response.body
        );
        let views = response.body["data"]["getViews"].as_array().unwrap();
        assert_eq!(views.len(), 1, "default view seed must be idempotent");
        let view = &views[0];
        assert_eq!(view["name"], "All people");
        assert_eq!(view["objectMetadataId"], object_id);
        assert_eq!(view["type"], "TABLE");
        assert_eq!(view["key"], "INDEX");
        assert_eq!(view["visibility"], "WORKSPACE");
        assert_eq!(view["isActive"], true);

        let fields = view["viewFields"].as_array().unwrap();
        assert_eq!(fields.len(), 5);
        assert!(view["viewFilters"].as_array().unwrap().is_empty());
        assert!(view["viewSorts"].as_array().unwrap().is_empty());
        assert!(view["viewGroups"].as_array().unwrap().is_empty());
        assert!(view["viewFieldGroups"].as_array().unwrap().is_empty());
    }

    let views = send(
        &app,
        authorized(
            "POST",
            "/metadata",
            &token,
            Some(json!({
                "query": format!(
                    "{{ getViews(objectMetadataId: \"{object_id}\") {{ id }} }}"
                )
            })),
        ),
    )
    .await;
    let view_id = views.body["data"]["getViews"][0]["id"]
        .as_str()
        .expect("view id")
        .to_string();

    let single = send(
        &app,
        authorized(
            "POST",
            "/metadata",
            &token,
            Some(json!({
                "query": "query($id: String!) { getView(id: $id) { id name } }",
                "variables": { "id": view_id }
            })),
        ),
    )
    .await;
    assert_eq!(single.status, StatusCode::OK);
    assert_eq!(single.body["data"]["getView"]["name"], "All people");
}

#[tokio::test]
async fn current_user_matches_frontend_boot_fragment() {
    let app = test_app().await;
    let email = unique_email("current-user");
    let (token, _workspace) = signup_with_access(&app, &email).await;

    let query = r#"
        query GetCurrentUser {
          currentUser {
            id
            firstName
            lastName
            email
            hasPassword
            canAccessFullAdminPanel
            canImpersonate
            supportUserHash
            onboardingStatus
            previousOnboardingStatus
            isWorkspaceCreator
            workspaceMember {
              id
              name { firstName lastName }
              colorScheme
              uiScale
              openRecordIn
              avatarUrl
              locale
              userEmail
              userId
              userWorkspaceId
              timeZone
              dateFormat
              timeFormat
              calendarStartDay
              numberFormat
            }
            workspaceMembers {
              id
              name { firstName lastName }
              avatarUrl
              userEmail
              userId
              userWorkspaceId
            }
            deletedWorkspaceMembers {
              id
              name { firstName lastName }
              avatarUrl
              userEmail
            }
            currentUserWorkspace {
              id
              permissionFlags
              isImpersonating
              objectsPermissions {
                objectMetadataId
                canReadObjectRecords
                canUpdateObjectRecords
                canSoftDeleteObjectRecords
                canDestroyObjectRecords
                restrictedFields
                rowLevelPermissionPredicates {
                  id
                  fieldMetadataId
                  objectMetadataId
                  operand
                  subFieldName
                  workspaceMemberFieldMetadataId
                  workspaceMemberSubFieldName
                  rowLevelPermissionPredicateGroupId
                  positionInRowLevelPermissionPredicateGroup
                  roleId
                  value
                }
                rowLevelPermissionPredicateGroups {
                  id
                  parentRowLevelPermissionPredicateGroupId
                  logicalOperator
                  positionInRowLevelPermissionPredicateGroup
                  roleId
                  objectMetadataId
                }
              }
              twoFactorAuthenticationMethodSummary {
                twoFactorAuthenticationMethodId
                status
                strategy
              }
            }
            currentWorkspace {
              id
              displayName
              logo
              inviteHash
              allowImpersonation
              activationStatus
              isPublicInviteLinkEnabled
              isCampaignClickTrackingEnabled
              isCampaignOpenTrackingEnabled
              workspaceDiscoverability
              isGoogleAuthEnabled
              isMicrosoftAuthEnabled
              isPasswordAuthEnabled
              isGoogleAuthBypassEnabled
              isMicrosoftAuthBypassEnabled
              isPasswordAuthBypassEnabled
              subdomain
              customDomain
              hasValidSignedEnterpriseKey
              hasValidEnterpriseValidityToken
              workspaceCustomApplication { id }
              installedApplications {
                id
                name
                universalIdentifier
                logoUrl
                createdAt
              }
              isCustomDomainEnabled
              workspaceUrls { subdomainUrl customUrl }
              featureFlags { key value }
              currentBillingSubscription {
                id
                status
                interval
                metadata
                currentPeriodEnd
                cancelAt
                collectionMethod
                phases {
                  start_date
                  end_date
                  items { price quantity }
                }
                billingSubscriptionItems {
                  id
                  hasReachedCurrentPeriodCap
                  quantity
                  stripePriceId
                  unitAmount
                  creditAmount
                  billingProduct {
                    name
                    description
                    images
                    metadata {
                      productKey
                      planKey
                      priceUsageBased
                      isLegacy
                    }
                  }
                }
              }
              billingCustomer {
                id
                hasPaymentMethod
                userCreditsFallback
              }
              billingSubscriptions {
                id
                status
                metadata
                cancelAt
                phases {
                  start_date
                  end_date
                  items { price quantity }
                }
              }
              billingEntitlements { key value }
              workspaceMembersCount
              defaultRole {
                id
                label
                description
                icon
                canUpdateAllSettings
                canAccessAllTools
                isEditable
                canReadAllObjectRecords
                canUpdateAllObjectRecords
                canSoftDeleteAllObjectRecords
                canDestroyAllObjectRecords
                canBeAssignedToUsers
                canBeAssignedToAgents
                canBeAssignedToApiKeys
                applicationId
                universalIdentifier
              }
              aiChatModelTier
              aiAgentModelTier
              isAutoModelSelectionEnabled
              aiModelIdByTier
              aiAdditionalInstructions
              isTwoFactorAuthenticationEnforced
              trashRetentionDays
              eventLogRetentionDays
              editableProfileFields
              isInternalMessagesImportEnabled
            }
            availableWorkspaces {
              availableWorkspacesForSignIn {
                id
                displayName
                loginToken
                inviteHash
                personalInviteToken
                workspaceUrls { subdomainUrl customUrl }
                logo
                sso { type id issuer name status }
              }
              availableWorkspacesForSignUp {
                id
                displayName
                loginToken
                inviteHash
                personalInviteToken
                workspaceUrls { subdomainUrl customUrl }
                logo
                sso { type id issuer name status }
              }
            }
            userVars
          }
        }
    "#;

    let response = send(
        &app,
        authorized("POST", "/metadata", &token, Some(json!({ "query": query }))),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    assert!(
        response.body["errors"].is_null(),
        "currentUser query errors: {}",
        response.body
    );
    let user = &response.body["data"]["currentUser"];
    assert_eq!(user["email"], email);
    assert!(!user["firstName"].as_str().unwrap().is_empty());
    assert_eq!(user["hasPassword"], true);
    assert_eq!(user["onboardingStatus"], "COMPLETED");
    assert_eq!(user["isWorkspaceCreator"], true);
    assert_eq!(user["workspaceMembers"].as_array().unwrap().len(), 1);
    assert_eq!(user["deletedWorkspaceMembers"].as_array().unwrap().len(), 0);

    let workspace = &user["currentWorkspace"];
    assert_eq!(workspace["activationStatus"], "ACTIVE");
    assert_eq!(
        workspace["subdomain"],
        user["currentWorkspace"]["subdomain"]
    );
    assert_eq!(workspace["workspaceMembersCount"], 1.0);
    assert_eq!(workspace["defaultRole"]["label"], "Owner");
    assert_eq!(
        workspace["workspaceUrls"]["subdomainUrl"],
        "http://localhost:3001"
    );
}

#[tokio::test]
async fn company_crud_rest_and_graphql() {
    let app = test_app().await;
    let email = unique_email("company");
    let (token, _workspace) = signup(&app, &email).await;

    let created = send(
        &app,
        authorized(
            "POST",
            "/rest/companies",
            &token,
            Some(json!({
                "name": "Acme",
                "domainName": { "primaryLinkLabel": "acme.com", "primaryLinkUrl": "https://acme.com", "secondaryLinks": [] },
                "address": { "addressCity": "Paris", "addressCountry": "France" },
                "annualRevenue": { "amountMicros": 1200000, "currencyCode": "EUR" }
            })),
        ),
    )
    .await;
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "body: {}",
        created.body
    );
    let company = &created.body["data"]["createCompany"];
    assert_eq!(company["name"], "Acme");
    assert_eq!(company["domainName"]["primaryLinkUrl"], "https://acme.com");
    assert_eq!(company["address"]["addressCity"], "Paris");
    assert_eq!(company["annualRevenue"]["amountMicros"], 1200000);
    let id = company["id"].as_str().expect("company id").to_string();

    let listed = send(&app, authorized("GET", "/rest/companies", &token, None)).await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.body["totalCount"], 1);
    assert_eq!(
        listed.body["data"]["companies"].as_array().unwrap().len(),
        1
    );

    let fetched = send(
        &app,
        authorized("GET", &format!("/rest/companies/{id}"), &token, None),
    )
    .await;
    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.body["data"]["company"]["name"], "Acme");

    let updated = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/companies/{id}"),
            &token,
            Some(json!({ "name": "Acme Inc" })),
        ),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "body: {}", updated.body);
    assert_eq!(updated.body["data"]["updateCompany"]["name"], "Acme Inc");
    assert_eq!(
        updated.body["data"]["updateCompany"]["address"]["addressCity"],
        "Paris"
    );

    let graphql = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "{{ companies {{ totalCount edges {{ node {{ id name }} }} }} company(filter: {{ id: {{ eq: \"{id}\" }} }}) {{ name domainName {{ primaryLinkUrl }} }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(graphql.status, StatusCode::OK, "body: {}", graphql.body);
    assert_eq!(graphql.body["data"]["companies"]["totalCount"], 1);
    assert_eq!(graphql.body["data"]["company"]["name"], "Acme Inc");

    let deleted = send(
        &app,
        authorized(
            "DELETE",
            &format!("/rest/companies/{id}?soft_delete=true"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK);
    assert_eq!(deleted.body["data"]["deleteCompany"]["id"], id);
    let after_delete = send(
        &app,
        authorized("GET", &format!("/rest/companies/{id}"), &token, None),
    )
    .await;
    assert_eq!(after_delete.status, StatusCode::NOT_FOUND);

    let restored = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/companies/{id}/restore"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(restored.status, StatusCode::OK, "body: {}", restored.body);
    assert_eq!(restored.body["data"]["restoreCompany"]["name"], "Acme Inc");

    let destroyed = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!("mutation {{ destroyCompany(id: \"{id}\") {{ id }} }}")
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(destroyed.status, StatusCode::OK, "body: {}", destroyed.body);
    assert_eq!(destroyed.body["data"]["destroyCompany"]["id"], id);
}

#[tokio::test]
async fn opportunity_crud_rest_and_graphql() {
    let app = test_app().await;
    let email = unique_email("opportunity");
    let (token, _workspace) = signup(&app, &email).await;

    let created = send(
        &app,
        authorized(
            "POST",
            "/rest/opportunities",
            &token,
            Some(json!({
                "name": "Enterprise renewal",
                "amount": { "amountMicros": 9000000, "currencyCode": "USD" },
                "closeDate": "2026-12-31T00:00:00Z",
                "stage": "MEETING"
            })),
        ),
    )
    .await;
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "body: {}",
        created.body
    );
    let opportunity = &created.body["data"]["createOpportunity"];
    assert_eq!(opportunity["name"], "Enterprise renewal");
    assert_eq!(opportunity["amount"]["amountMicros"], 9000000);
    assert_eq!(opportunity["stage"], "MEETING");
    assert!(opportunity["closeDate"]
        .as_str()
        .unwrap()
        .starts_with("2026-12-31T00:00:00"));
    let id = opportunity["id"]
        .as_str()
        .expect("opportunity id")
        .to_string();

    let listed = send(&app, authorized("GET", "/rest/opportunities", &token, None)).await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.body["totalCount"], 1);
    assert_eq!(
        listed.body["data"]["opportunities"][0]["id"], id,
        "list envelope: {}",
        listed.body
    );

    let updated = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/opportunities/{id}"),
            &token,
            Some(json!({ "stage": "PROPOSAL", "name": "Enterprise renewal 2027" })),
        ),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "body: {}", updated.body);
    assert_eq!(
        updated.body["data"]["updateOpportunity"]["stage"],
        "PROPOSAL"
    );
    assert_eq!(
        updated.body["data"]["updateOpportunity"]["amount"]["currencyCode"],
        "USD"
    );

    let filtered = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "{ opportunities(filter: { stage: { eq: PROPOSAL } }) { totalCount edges { node { id name stage closeDate amount { amountMicros currencyCode } } } } }"
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(filtered.status, StatusCode::OK, "body: {}", filtered.body);
    assert_eq!(filtered.body["data"]["opportunities"]["totalCount"], 1);
    let node = &filtered.body["data"]["opportunities"]["edges"][0]["node"];
    assert_eq!(node["stage"], "PROPOSAL");
    assert_eq!(node["amount"]["amountMicros"], 9000000);
    assert!(node["closeDate"]
        .as_str()
        .unwrap()
        .starts_with("2026-12-31T00:00:00"));

    let filtered_out = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "{ opportunities(filter: { stage: { eq: CUSTOMER } }) { totalCount } }"
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        filtered_out.body["data"]["opportunities"]["totalCount"], 0,
        "body: {}",
        filtered_out.body
    );

    let destroyed = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!("mutation {{ destroyOpportunity(id: \"{id}\") {{ id stage }} }}")
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(destroyed.status, StatusCode::OK, "body: {}", destroyed.body);
    assert_eq!(destroyed.body["data"]["destroyOpportunity"]["id"], id);

    let gone = send(
        &app,
        authorized("GET", &format!("/rest/opportunities/{id}"), &token, None),
    )
    .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn task_and_note_crud_rest_and_graphql() {
    let app = test_app().await;
    let email = unique_email("tasks-notes");
    let (token, _workspace) = signup(&app, &email).await;

    let task = send(
        &app,
        authorized(
            "POST",
            "/rest/tasks",
            &token,
            Some(json!({
                "title": "Follow up with Acme",
                "status": "IN_PROGRESS",
                "dueAt": "2026-11-15T09:00:00Z",
                "bodyV2": { "markdown": "Send the proposal" }
            })),
        ),
    )
    .await;
    assert_eq!(task.status, StatusCode::CREATED, "body: {}", task.body);
    let task_id = task.body["data"]["createTask"]["id"]
        .as_str()
        .expect("task id")
        .to_string();
    assert_eq!(task.body["data"]["createTask"]["status"], "IN_PROGRESS");
    assert_eq!(
        task.body["data"]["createTask"]["bodyV2"]["markdown"],
        "Send the proposal"
    );

    let updated = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "mutation {{ updateTask(id: \"{task_id}\", data: {{ status: DONE, bodyV2: {{ blocknote: \"[\\\"done\\\"]\", markdown: \"Updated markdown\" }} }}) {{ id status bodyV2 {{ blocknote markdown }} }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "body: {}", updated.body);
    let node = &updated.body["data"]["updateTask"];
    assert_eq!(node["status"], "DONE");
    assert_eq!(node["bodyV2"]["blocknote"], "[\"done\"]");
    assert_eq!(node["bodyV2"]["markdown"], "Updated markdown");

    let filtered = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "{ tasks(filter: { status: { eq: DONE } }) { totalCount edges { node { id title status } } } }"
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(filtered.status, StatusCode::OK, "body: {}", filtered.body);
    assert_eq!(filtered.body["data"]["tasks"]["totalCount"], 1);
    assert_eq!(
        filtered.body["data"]["tasks"]["edges"][0]["node"]["title"],
        "Follow up with Acme"
    );

    let note = send(
        &app,
        authorized(
            "POST",
            "/rest/notes",
            &token,
            Some(json!({
                "title": "Discovery notes",
                "bodyV2": { "markdown": "Budget approved" }
            })),
        ),
    )
    .await;
    assert_eq!(note.status, StatusCode::CREATED, "body: {}", note.body);
    let note_id = note.body["data"]["createNote"]["id"]
        .as_str()
        .expect("note id")
        .to_string();
    assert_eq!(note.body["data"]["createNote"]["title"], "Discovery notes");

    let notes = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "{ notes(filter: { title: { ilike: \"%discovery%\" } }) { totalCount edges { node { id title bodyV2 { markdown } } } } }"
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(notes.status, StatusCode::OK, "body: {}", notes.body);
    assert_eq!(notes.body["data"]["notes"]["totalCount"], 1);
    assert_eq!(
        notes.body["data"]["notes"]["edges"][0]["node"]["bodyV2"]["markdown"],
        "Budget approved"
    );

    let soft_deleted = send(
        &app,
        authorized(
            "DELETE",
            &format!("/rest/notes/{note_id}?soft_delete=true"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(soft_deleted.status, StatusCode::OK);
    assert_eq!(soft_deleted.body["data"]["deleteNote"]["id"], note_id);
    let restored = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/notes/{note_id}/restore"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(restored.status, StatusCode::OK, "body: {}", restored.body);
    assert_eq!(
        restored.body["data"]["restoreNote"]["title"],
        "Discovery notes"
    );

    let destroyed = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "mutation {{ destroyTask(id: \"{task_id}\") {{ id }} destroyNote(id: \"{note_id}\") {{ id }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(destroyed.status, StatusCode::OK, "body: {}", destroyed.body);
    assert_eq!(destroyed.body["data"]["destroyTask"]["id"], task_id);
    assert_eq!(destroyed.body["data"]["destroyNote"]["id"], note_id);
}

#[tokio::test]
async fn task_and_note_target_relations() {
    let app = test_app().await;
    let email = unique_email("targets");
    let (token, _workspace) = signup(&app, &email).await;

    let person = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(person_body("Ada", "Target", "ada.target@example.com")),
        ),
    )
    .await;
    let person_id = person.body["data"]["createPerson"]["id"]
        .as_str()
        .expect("person id")
        .to_string();

    let company = send(
        &app,
        authorized(
            "POST",
            "/rest/companies",
            &token,
            Some(json!({ "name": "Target Co" })),
        ),
    )
    .await;
    let company_id = company.body["data"]["createCompany"]["id"]
        .as_str()
        .expect("company id")
        .to_string();

    let task = send(
        &app,
        authorized(
            "POST",
            "/rest/tasks",
            &token,
            Some(json!({ "title": "Linked task" })),
        ),
    )
    .await;
    let task_id = task.body["data"]["createTask"]["id"]
        .as_str()
        .expect("task id")
        .to_string();

    let created = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "mutation {{ createTaskTarget(data: {{ taskId: \"{task_id}\", targetPersonId: \"{person_id}\" }}) {{ id taskId targetPersonId }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK, "body: {}", created.body);
    let target_id = created.body["data"]["createTaskTarget"]["id"]
        .as_str()
        .expect("target id")
        .to_string();
    assert_eq!(
        created.body["data"]["createTaskTarget"]["targetPersonId"],
        person_id
    );

    let company_target = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "mutation {{ createTaskTarget(data: {{ taskId: \"{task_id}\", targetCompanyId: \"{company_id}\" }}) {{ id targetCompanyId }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        company_target.body["data"]["createTaskTarget"]["targetCompanyId"],
        company_id
    );

    let nested = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "{{ task(filter: {{ id: {{ eq: \"{task_id}\" }} }}) {{ id taskTargets {{ edges {{ node {{ id taskId targetPersonId targetCompanyId }} }} }} }} taskTargets(filter: {{ targetCompanyId: {{ eq: \"{company_id}\" }} }}) {{ edges {{ node {{ id }} }} }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(nested.status, StatusCode::OK, "body: {}", nested.body);
    let edges = nested.body["data"]["task"]["taskTargets"]["edges"]
        .as_array()
        .unwrap();
    assert_eq!(edges.len(), 2, "body: {}", nested.body);
    assert_eq!(
        nested.body["data"]["taskTargets"]["edges"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let invalid = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "mutation {{ createTaskTarget(data: {{ taskId: \"{task_id}\" }}) {{ id }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(invalid.status, StatusCode::OK);
    assert!(!invalid.body["errors"].is_null(), "body: {}", invalid.body);

    let deleted = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "mutation {{ deleteTaskTarget(id: \"{target_id}\") {{ id targetPersonId }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK, "body: {}", deleted.body);
    assert_eq!(
        deleted.body["data"]["deleteTaskTarget"]["targetPersonId"],
        person_id
    );

    let note = send(
        &app,
        authorized(
            "POST",
            "/rest/notes",
            &token,
            Some(json!({ "title": "Linked note" })),
        ),
    )
    .await;
    let note_id = note.body["data"]["createNote"]["id"]
        .as_str()
        .expect("note id")
        .to_string();
    let note_target = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "mutation {{ createNoteTarget(data: {{ noteId: \"{note_id}\", targetCompanyId: \"{company_id}\" }}) {{ id noteId targetCompanyId }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        note_target.status,
        StatusCode::OK,
        "body: {}",
        note_target.body
    );
    assert_eq!(
        note_target.body["data"]["createNoteTarget"]["targetCompanyId"],
        company_id
    );

    let note_nested = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "{{ note(filter: {{ id: {{ eq: \"{note_id}\" }} }}) {{ id noteTargets {{ edges {{ node {{ noteId targetCompanyId }} }} }} }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        note_nested.status,
        StatusCode::OK,
        "body: {}",
        note_nested.body
    );
    assert_eq!(
        note_nested.body["data"]["note"]["noteTargets"]["edges"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn task_and_note_target_rest_and_batch_surfaces() {
    let app = test_app().await;
    let email = unique_email("target-surfaces");
    let (token, _workspace) = signup(&app, &email).await;

    let person = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(person_body("Grace", "Target", "grace.target@example.com")),
        ),
    )
    .await;
    let person_id = person.body["data"]["createPerson"]["id"]
        .as_str()
        .expect("person id")
        .to_string();

    let company = send(
        &app,
        authorized(
            "POST",
            "/rest/companies",
            &token,
            Some(json!({ "name": "Surface Co" })),
        ),
    )
    .await;
    let company_id = company.body["data"]["createCompany"]["id"]
        .as_str()
        .expect("company id")
        .to_string();

    let task = send(
        &app,
        authorized(
            "POST",
            "/rest/tasks",
            &token,
            Some(json!({ "title": "Surface task" })),
        ),
    )
    .await;
    let task_id = task.body["data"]["createTask"]["id"]
        .as_str()
        .expect("task id")
        .to_string();

    let created = send(
        &app,
        authorized(
            "POST",
            "/rest/taskTargets",
            &token,
            Some(json!({ "taskId": task_id, "targetPersonId": person_id })),
        ),
    )
    .await;
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "body: {}",
        created.body
    );
    let target_id = created.body["data"]["createTaskTarget"]["id"]
        .as_str()
        .expect("target id")
        .to_string();

    let listed = send(&app, authorized("GET", "/rest/taskTargets", &token, None)).await;
    assert_eq!(listed.status, StatusCode::OK, "body: {}", listed.body);
    assert_eq!(listed.body["totalCount"], 1);
    assert_eq!(
        listed.body["data"]["taskTargets"][0]["targetPersonId"],
        person_id
    );

    let fetched = send(
        &app,
        authorized(
            "GET",
            &format!("/rest/taskTargets/{target_id}"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(fetched.status, StatusCode::OK, "body: {}", fetched.body);
    assert_eq!(fetched.body["data"]["taskTarget"]["taskId"], task_id);

    let patched = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/taskTargets/{target_id}"),
            &token,
            Some(json!({ "position": 3.5, "targetCompanyId": company_id })),
        ),
    )
    .await;
    assert_eq!(patched.status, StatusCode::OK, "body: {}", patched.body);
    assert_eq!(patched.body["data"]["updateTaskTarget"]["position"], 3.5);
    assert_eq!(
        patched.body["data"]["updateTaskTarget"]["targetCompanyId"],
        company_id
    );

    let nested = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "{{ taskTargets(filter: {{ taskId: {{ eq: \"{task_id}\" }} }}) {{ totalCount edges {{ node {{ id taskId position task {{ id title }} targetPerson {{ id }} targetCompany {{ id name }} }} }} }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(nested.status, StatusCode::OK, "body: {}", nested.body);
    let connection = &nested.body["data"]["taskTargets"];
    assert_eq!(connection["totalCount"], 1);
    let node = &connection["edges"][0]["node"];
    assert_eq!(node["task"]["title"], "Surface task");
    assert_eq!(node["targetPerson"]["id"], person_id.as_str());
    assert_eq!(node["targetCompany"]["name"], "Surface Co");

    let batch = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "mutation {{ createTaskTargets(data: [{{ taskId: \"{task_id}\", targetCompanyId: \"{company_id}\" }}]) {{ id targetCompanyId }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(batch.status, StatusCode::OK, "body: {}", batch.body);
    assert_eq!(
        batch.body["data"]["createTaskTargets"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let batch_update = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "mutation {{ updateTaskTargets(data: {{ position: 9.0 }}, filter: {{ taskId: {{ eq: \"{task_id}\" }} }}) {{ id position }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        batch_update.status,
        StatusCode::OK,
        "body: {}",
        batch_update.body
    );
    let updated = batch_update.body["data"]["updateTaskTargets"]
        .as_array()
        .unwrap();
    assert_eq!(updated.len(), 2);
    assert!(updated.iter().all(|target| target["position"] == 9.0));

    let batch_delete = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "mutation {{ deleteTaskTargets(filter: {{ taskId: {{ eq: \"{task_id}\" }} }}) {{ id }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        batch_delete.status,
        StatusCode::OK,
        "body: {}",
        batch_delete.body
    );
    assert_eq!(
        batch_delete.body["data"]["deleteTaskTargets"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let after_delete = send(&app, authorized("GET", "/rest/taskTargets", &token, None)).await;
    assert_eq!(after_delete.body["totalCount"], 0);

    let restored = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "mutation {{ restoreTaskTarget(id: \"{target_id}\") {{ id deletedAt }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(restored.status, StatusCode::OK, "body: {}", restored.body);
    assert!(restored.body["data"]["restoreTaskTarget"]["deletedAt"].is_null());

    let destroyed = send(
        &app,
        authorized(
            "DELETE",
            &format!("/rest/taskTargets/{target_id}"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(destroyed.status, StatusCode::OK, "body: {}", destroyed.body);
    assert_eq!(
        destroyed.body["data"]["deleteTaskTarget"]["id"],
        target_id.as_str()
    );

    let note = send(
        &app,
        authorized(
            "POST",
            "/rest/notes",
            &token,
            Some(json!({ "title": "Surface note" })),
        ),
    )
    .await;
    let note_id = note.body["data"]["createNote"]["id"]
        .as_str()
        .expect("note id")
        .to_string();

    let note_target = send(
        &app,
        authorized(
            "POST",
            "/rest/noteTargets",
            &token,
            Some(json!({ "noteId": note_id, "targetCompanyId": company_id })),
        ),
    )
    .await;
    assert_eq!(
        note_target.status,
        StatusCode::CREATED,
        "body: {}",
        note_target.body
    );
    assert_eq!(
        note_target.body["data"]["createNoteTarget"]["targetCompanyId"],
        company_id
    );

    let note_connection = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "{{ noteTargets(filter: {{ noteId: {{ eq: \"{note_id}\" }} }}) {{ totalCount edges {{ node {{ noteId note {{ id title }} targetCompany {{ id }} }} }} }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        note_connection.status,
        StatusCode::OK,
        "body: {}",
        note_connection.body
    );
    let note_node = &note_connection.body["data"]["noteTargets"]["edges"][0]["node"];
    assert_eq!(note_node["note"]["title"], "Surface note");
    assert_eq!(note_node["targetCompany"]["id"], company_id.as_str());
}

#[tokio::test]
async fn scalar_ordering_rest_and_graphql() {
    let app = test_app().await;
    let email = unique_email("ordering");
    let (token, _workspace) = signup(&app, &email).await;

    for name in ["Zebra Corp", "Alpha Corp", "Mango Corp"] {
        let response = send(
            &app,
            authorized(
                "POST",
                "/rest/companies",
                &token,
                Some(json!({ "name": name })),
            ),
        )
        .await;
        assert_eq!(
            response.status,
            StatusCode::CREATED,
            "body: {}",
            response.body
        );
    }

    let descending = send(
        &app,
        authorized(
            "GET",
            "/rest/companies?order_by=name%5BDescNullsLast%5D",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        descending.status,
        StatusCode::OK,
        "body: {}",
        descending.body
    );
    let names: Vec<&str> = descending.body["data"]["companies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|company| company["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Zebra Corp", "Mango Corp", "Alpha Corp"]);

    let page = send(
        &app,
        authorized(
            "GET",
            "/rest/companies?limit=2&order_by=name%5BAscNullsLast%5D",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(page.status, StatusCode::OK, "body: {}", page.body);
    assert_eq!(page.body["data"]["companies"][0]["name"], "Alpha Corp");
    assert_eq!(page.body["data"]["companies"][1]["name"], "Mango Corp");
    assert_eq!(page.body["pageInfo"]["hasNextPage"], true);

    let graphql = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "query($order: [JSON!]) { companies(orderBy: $order) { edges { node { name } } } }",
                "variables": { "order": [{ "name": "DescNullsLast" }] }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(graphql.status, StatusCode::OK, "body: {}", graphql.body);
    let edges = graphql.body["data"]["companies"]["edges"]
        .as_array()
        .unwrap();
    assert_eq!(edges[0]["node"]["name"], "Zebra Corp");
    assert_eq!(edges[2]["node"]["name"], "Alpha Corp");

    for title in ["First task", "Third task", "Second task"] {
        let response = send(
            &app,
            authorized(
                "POST",
                "/rest/tasks",
                &token,
                Some(json!({ "title": title })),
            ),
        )
        .await;
        assert_eq!(
            response.status,
            StatusCode::CREATED,
            "body: {}",
            response.body
        );
    }

    let tasks = send(
        &app,
        authorized(
            "GET",
            "/rest/tasks?order_by=title%5BAscNullsLast%5D",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(tasks.status, StatusCode::OK, "body: {}", tasks.body);
    let titles: Vec<&str> = tasks.body["data"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["First task", "Second task", "Third task"]);

    let invalid = send(
        &app,
        authorized(
            "GET",
            "/rest/companies?order_by=doesNotExist%5BAscNullsFirst%5D",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        invalid.status,
        StatusCode::BAD_REQUEST,
        "body: {}",
        invalid.body
    );

    let invalid_direction = send(
        &app,
        authorized(
            "GET",
            "/rest/companies?order_by=name%5BSideways%5D",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        invalid_direction.status,
        StatusCode::BAD_REQUEST,
        "body: {}",
        invalid_direction.body
    );
}

#[tokio::test]
async fn person_company_filter_matches_scalar_contract() {
    let app = test_app().await;
    let email = unique_email("person-company-filter");
    let (token, _workspace) = signup(&app, &email).await;

    let company = send(
        &app,
        authorized(
            "POST",
            "/rest/companies",
            &token,
            Some(json!({ "name": "Filter Corp" })),
        ),
    )
    .await;
    assert_eq!(
        company.status,
        StatusCode::CREATED,
        "body: {}",
        company.body
    );
    let company_id = company.body["data"]["createCompany"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let mut attached = person_body("Ada", "Lovelace", "ada@example.com");
    attached["companyId"] = json!(company_id);
    let person = send(
        &app,
        authorized("POST", "/rest/people", &token, Some(attached)),
    )
    .await;
    assert_eq!(person.status, StatusCode::CREATED, "body: {}", person.body);

    let unattached = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(person_body("Alan", "Turing", "alan@example.com")),
        ),
    )
    .await;
    assert_eq!(
        unattached.status,
        StatusCode::CREATED,
        "body: {}",
        unattached.body
    );

    let filtered = send(
        &app,
        authorized(
            "GET",
            &format!("/rest/people?filter=companyId%5Beq%5D:{company_id}"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(filtered.status, StatusCode::OK, "body: {}", filtered.body);
    assert_eq!(filtered.body["totalCount"], 1);
    assert_eq!(
        filtered.body["data"]["people"][0]["name"]["firstName"],
        "Ada"
    );

    let empty = send(
        &app,
        authorized(
            "GET",
            "/rest/people?filter=companyId%5Bis%5D:NULL",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(empty.status, StatusCode::OK, "body: {}", empty.body);
    assert_eq!(empty.body["totalCount"], 1);
    assert_eq!(empty.body["data"]["people"][0]["name"]["firstName"], "Alan");

    let gql = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": format!(
                    "{{ people(filter: {{ companyId: {{ eq: \"{company_id}\" }} }}) {{ totalCount edges {{ node {{ name {{ firstName }} }} }} }} }}"
                )
            })),
        ),
    )
    .await;
    assert_eq!(gql.status, StatusCode::OK, "body: {}", gql.body);
    assert!(gql.body["errors"].is_null(), "errors: {}", gql.body);
    assert_eq!(gql.body["data"]["people"]["totalCount"], 1);
    assert_eq!(
        gql.body["data"]["people"]["edges"][0]["node"]["name"]["firstName"],
        "Ada"
    );
}

#[tokio::test]
async fn relation_filters_match_nested_company_and_contact() {
    let app = test_app().await;
    let email = unique_email("relation-filters");
    let (token, _workspace) = signup(&app, &email).await;

    let company = send(
        &app,
        authorized(
            "POST",
            "/rest/companies",
            &token,
            Some(json!({ "name": "Relation Corp" })),
        ),
    )
    .await;
    assert_eq!(
        company.status,
        StatusCode::CREATED,
        "body: {}",
        company.body
    );
    let company_id = company.body["data"]["createCompany"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let other_company = send(
        &app,
        authorized(
            "POST",
            "/rest/companies",
            &token,
            Some(json!({ "name": "Other Corp" })),
        ),
    )
    .await;
    assert_eq!(
        other_company.status,
        StatusCode::CREATED,
        "body: {}",
        other_company.body
    );

    let mut attached = person_body("Ada", "Lovelace", "ada-relation@example.com");
    attached["companyId"] = json!(company_id);
    let person = send(
        &app,
        authorized("POST", "/rest/people", &token, Some(attached)),
    )
    .await;
    assert_eq!(person.status, StatusCode::CREATED, "body: {}", person.body);
    let person_id = person.body["data"]["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let mut detached = person_body("Alan", "Turing", "alan-relation@example.com");
    detached["companyId"] = json!(null);
    let other_person = send(
        &app,
        authorized("POST", "/rest/people", &token, Some(detached)),
    )
    .await;
    assert_eq!(
        other_person.status,
        StatusCode::CREATED,
        "body: {}",
        other_person.body
    );

    let opportunity = send(
        &app,
        authorized(
            "POST",
            "/rest/opportunities",
            &token,
            Some(json!({
                "name": "Relation Deal",
                "companyId": company_id,
                "pointOfContactId": person_id,
                "stage": "NEW"
            })),
        ),
    )
    .await;
    assert_eq!(
        opportunity.status,
        StatusCode::CREATED,
        "body: {}",
        opportunity.body
    );

    let other_opportunity = send(
        &app,
        authorized(
            "POST",
            "/rest/opportunities",
            &token,
            Some(json!({ "name": "Detached Deal", "stage": "NEW" })),
        ),
    )
    .await;
    assert_eq!(
        other_opportunity.status,
        StatusCode::CREATED,
        "body: {}",
        other_opportunity.body
    );

    let alpha_company = send(
        &app,
        authorized(
            "POST",
            "/rest/companies",
            &token,
            Some(json!({ "name": "Alpha Corp" })),
        ),
    )
    .await;
    assert_eq!(
        alpha_company.status,
        StatusCode::CREATED,
        "body: {}",
        alpha_company.body
    );
    let alpha_company_id = alpha_company.body["data"]["createCompany"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let alpha_opportunity = send(
        &app,
        authorized(
            "POST",
            "/rest/opportunities",
            &token,
            Some(json!({
                "name": "Alpha Deal",
                "companyId": alpha_company_id,
                "stage": "NEW"
            })),
        ),
    )
    .await;
    assert_eq!(
        alpha_opportunity.status,
        StatusCode::CREATED,
        "body: {}",
        alpha_opportunity.body
    );

    let people_rest = send(
        &app,
        authorized(
            "GET",
            "/rest/people?filter=company.name%5Beq%5D:Relation%20Corp",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        people_rest.status,
        StatusCode::OK,
        "body: {}",
        people_rest.body
    );
    assert_eq!(people_rest.body["totalCount"], 1);
    assert_eq!(
        people_rest.body["data"]["people"][0]["name"]["firstName"],
        "Ada"
    );

    let people_gql = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "{ people(filter: { company: { name: { eq: \"Relation Corp\" } } }) { totalCount edges { node { name { firstName } } } } }"
            })),
        ),
    )
    .await;
    assert_eq!(
        people_gql.status,
        StatusCode::OK,
        "body: {}",
        people_gql.body
    );
    assert!(
        people_gql.body["errors"].is_null(),
        "errors: {}",
        people_gql.body
    );
    assert_eq!(people_gql.body["data"]["people"]["totalCount"], 1);
    assert_eq!(
        people_gql.body["data"]["people"]["edges"][0]["node"]["name"]["firstName"],
        "Ada"
    );

    let opportunities_rest = send(
        &app,
        authorized(
            "GET",
            "/rest/opportunities?filter=company.name%5Beq%5D:Relation%20Corp",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        opportunities_rest.status,
        StatusCode::OK,
        "body: {}",
        opportunities_rest.body
    );
    assert_eq!(opportunities_rest.body["totalCount"], 1);
    assert_eq!(
        opportunities_rest.body["data"]["opportunities"][0]["name"],
        "Relation Deal"
    );

    let contacts_rest = send(
        &app,
        authorized(
            "GET",
            "/rest/opportunities?filter=pointOfContact.name.firstName%5Beq%5D:Ada",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        contacts_rest.status,
        StatusCode::OK,
        "body: {}",
        contacts_rest.body
    );
    assert_eq!(contacts_rest.body["totalCount"], 1);

    let opportunities_gql = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "{ opportunities(filter: { company: { name: { ilike: \"%relation%\" } }, pointOfContact: { name: { firstName: { eq: \"Ada\" } } } }) { totalCount edges { node { name } } } }"
            })),
        ),
    )
    .await;
    assert_eq!(
        opportunities_gql.status,
        StatusCode::OK,
        "body: {}",
        opportunities_gql.body
    );
    assert!(
        opportunities_gql.body["errors"].is_null(),
        "errors: {}",
        opportunities_gql.body
    );
    assert_eq!(
        opportunities_gql.body["data"]["opportunities"]["totalCount"],
        1
    );
    assert_eq!(
        opportunities_gql.body["data"]["opportunities"]["edges"][0]["node"]["name"],
        "Relation Deal"
    );

    let ordered_rest = send(
        &app,
        authorized(
            "GET",
            "/rest/opportunities?order_by=company.name%5BDescNullsLast%5D",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        ordered_rest.status,
        StatusCode::OK,
        "body: {}",
        ordered_rest.body
    );
    assert_eq!(
        ordered_rest.body["data"]["opportunities"][0]["name"],
        "Relation Deal"
    );
    assert_eq!(
        ordered_rest.body["data"]["opportunities"][1]["name"],
        "Alpha Deal"
    );

    let ordered_gql = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "{ opportunities(orderBy: [{ company: { name: DescNullsLast } }]) { edges { node { name } } } }"
            })),
        ),
    )
    .await;
    assert!(
        ordered_gql.body["errors"].is_null(),
        "errors: {}",
        ordered_gql.body
    );
    assert_eq!(
        ordered_gql.body["data"]["opportunities"]["edges"][0]["node"]["name"],
        "Relation Deal"
    );
    assert_eq!(
        ordered_gql.body["data"]["opportunities"]["edges"][1]["node"]["name"],
        "Alpha Deal"
    );

    let people_ordered_rest = send(
        &app,
        authorized(
            "GET",
            "/rest/people?order_by=company.name%5BAscNullsLast%5D",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        people_ordered_rest.status,
        StatusCode::OK,
        "body: {}",
        people_ordered_rest.body
    );
    assert_eq!(
        people_ordered_rest.body["data"]["people"][0]["name"]["firstName"],
        "Ada"
    );

    let people_ordered_gql = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "{ people(orderBy: [{ company: { name: AscNullsLast } }]) { edges { node { name { firstName } } } } }"
            })),
        ),
    )
    .await;
    assert!(
        people_ordered_gql.body["errors"].is_null(),
        "errors: {}",
        people_ordered_gql.body
    );
    assert_eq!(
        people_ordered_gql.body["data"]["people"]["edges"][0]["node"]["name"]["firstName"],
        "Ada"
    );
}

#[tokio::test]
async fn search_returns_ranked_records_with_object_filters() {
    let app = test_app().await;
    let email = unique_email("search");
    let (token, _workspace) = signup(&app, &email).await;

    let company = send(
        &app,
        authorized(
            "POST",
            "/rest/companies",
            &token,
            Some(json!({ "name": "Searchable Acme" })),
        ),
    )
    .await;
    assert_eq!(
        company.status,
        StatusCode::CREATED,
        "body: {}",
        company.body
    );
    let company_id = company.body["data"]["createCompany"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let person = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(person_body("Zed", "Searchable", "zed-search@example.com")),
        ),
    )
    .await;
    assert_eq!(person.status, StatusCode::CREATED, "body: {}", person.body);

    let query = r#"
        query($searchInput: String!, $limit: Int!, $after: String, $included: [String!], $excluded: [String!]) {
          search(searchInput: $searchInput, limit: $limit, after: $after,
                 includedObjectNameSingulars: $included,
                 excludedObjectNameSingulars: $excluded) {
            edges {
              node { recordId objectNameSingular objectLabelSingular label imageUrl tsRankCD tsRank }
              cursor
            }
            pageInfo { hasNextPage endCursor }
          }
        }
    "#;

    let all = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": query,
                "variables": { "searchInput": "Searchable", "limit": 10 }
            })),
        ),
    )
    .await;
    assert_eq!(all.status, StatusCode::OK, "body: {}", all.body);
    assert!(all.body["errors"].is_null(), "errors: {}", all.body);
    let edges = all.body["data"]["search"]["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 2);
    let objects: Vec<&str> = edges
        .iter()
        .map(|edge| edge["node"]["objectNameSingular"].as_str().unwrap())
        .collect();
    assert!(objects.contains(&"company"));
    assert!(objects.contains(&"person"));
    assert!(edges[0]["node"]["tsRankCD"].as_f64().unwrap() > 0.0);

    let filtered = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "query($searchInput: String!, $limit: Int!, $filter: ObjectRecordFilterInput) { search(searchInput: $searchInput, limit: $limit, filter: $filter) { edges { node { recordId objectNameSingular } } } }",
                "variables": {
                    "searchInput": "Searchable",
                    "limit": 10,
                    "filter": { "id": { "eq": company_id } }
                }
            })),
        ),
    )
    .await;
    assert!(
        filtered.body["errors"].is_null(),
        "errors: {}",
        filtered.body
    );
    let filtered_edges = filtered.body["data"]["search"]["edges"].as_array().unwrap();
    assert_eq!(filtered_edges.len(), 1);
    assert_eq!(filtered_edges[0]["node"]["objectNameSingular"], "company");

    let people_only = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": query,
                "variables": { "searchInput": "Searchable", "limit": 10, "included": ["person"] }
            })),
        ),
    )
    .await;
    assert!(
        people_only.body["errors"].is_null(),
        "errors: {}",
        people_only.body
    );
    let person_edges = people_only.body["data"]["search"]["edges"]
        .as_array()
        .unwrap();
    assert_eq!(person_edges.len(), 1);
    assert_eq!(person_edges[0]["node"]["objectNameSingular"], "person");

    let no_company = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": query,
                "variables": { "searchInput": "Searchable", "limit": 10, "excluded": ["company"] }
            })),
        ),
    )
    .await;
    assert!(
        no_company.body["errors"].is_null(),
        "errors: {}",
        no_company.body
    );
    let remaining = no_company.body["data"]["search"]["edges"]
        .as_array()
        .unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["node"]["objectNameSingular"], "person");

    let first_page = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": query,
                "variables": { "searchInput": "Searchable", "limit": 1 }
            })),
        ),
    )
    .await;
    assert!(
        first_page.body["errors"].is_null(),
        "errors: {}",
        first_page.body
    );
    assert_eq!(
        first_page.body["data"]["search"]["pageInfo"]["hasNextPage"],
        true
    );
    let cursor = first_page.body["data"]["search"]["pageInfo"]["endCursor"]
        .as_str()
        .unwrap()
        .to_string();
    let second_page = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": query,
                "variables": { "searchInput": "Searchable", "limit": 1, "after": cursor }
            })),
        ),
    )
    .await;
    assert!(
        second_page.body["errors"].is_null(),
        "errors: {}",
        second_page.body
    );
    assert_eq!(
        second_page.body["data"]["search"]["edges"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn rest_filter_grammar_for_companies_and_tasks() {
    let app = test_app().await;
    let email = unique_email("rest-filters");
    let (token, _workspace) = signup(&app, &email).await;

    for name in ["Alpha Corp", "Zebra Corp", "Mango Ltd"] {
        let response = send(
            &app,
            authorized(
                "POST",
                "/rest/companies",
                &token,
                Some(json!({ "name": name })),
            ),
        )
        .await;
        assert_eq!(
            response.status,
            StatusCode::CREATED,
            "body: {}",
            response.body
        );
    }

    let exact = send(
        &app,
        authorized(
            "GET",
            "/rest/companies?filter=name%5Beq%5D:Alpha%20Corp",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(exact.status, StatusCode::OK, "body: {}", exact.body);
    assert_eq!(exact.body["totalCount"], 1);

    let contains = send(
        &app,
        authorized(
            "GET",
            "/rest/companies?filter=name%5Bcontains%5D:Corp",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(contains.status, StatusCode::OK, "body: {}", contains.body);
    assert_eq!(contains.body["totalCount"], 2);

    let ilike = send(
        &app,
        authorized(
            "GET",
            "/rest/companies?filter=name%5Bilike%5D:%25corp",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(ilike.status, StatusCode::OK, "body: {}", ilike.body);
    assert_eq!(ilike.body["totalCount"], 2);

    let any_of = send(
        &app,
        authorized(
            "GET",
            "/rest/companies?filter=name%5Bin%5D:(Alpha%20Corp,Mango%20Ltd)",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(any_of.status, StatusCode::OK, "body: {}", any_of.body);
    assert_eq!(any_of.body["totalCount"], 2);

    let grouped = send(
        &app,
        authorized(
            "GET",
            "/rest/companies?filter=or(name%5Beq%5D:%22Alpha%20Corp%22,not(name%5Bcontains%5D:Corp))",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(grouped.status, StatusCode::OK, "body: {}", grouped.body);
    let names: Vec<&str> = grouped.body["data"]["companies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|company| company["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Alpha Corp", "Mango Ltd"]);

    let recent = send(
        &app,
        authorized(
            "GET",
            "/rest/companies?filter=createdAt%5Bgte%5D:%222020-01-01%22&order_by=name%5BAscNullsLast%5D",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(recent.status, StatusCode::OK, "body: {}", recent.body);
    assert_eq!(recent.body["totalCount"], 3);
    assert_eq!(recent.body["data"]["companies"][0]["name"], "Alpha Corp");

    for (title, status) in [("First task", "TODO"), ("Done task", "DONE")] {
        let response = send(
            &app,
            authorized(
                "POST",
                "/rest/tasks",
                &token,
                Some(json!({ "title": title, "status": status })),
            ),
        )
        .await;
        assert_eq!(
            response.status,
            StatusCode::CREATED,
            "body: {}",
            response.body
        );
    }

    let todo = send(
        &app,
        authorized(
            "GET",
            "/rest/tasks?filter=status%5Beq%5D:TODO",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(todo.status, StatusCode::OK, "body: {}", todo.body);
    assert_eq!(todo.body["totalCount"], 1);
    assert_eq!(todo.body["data"]["tasks"][0]["title"], "First task");

    let invalid = send(
        &app,
        authorized(
            "GET",
            "/rest/companies?filter=unknown%5Beq%5D:value",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        invalid.status,
        StatusCode::BAD_REQUEST,
        "body: {}",
        invalid.body
    );
}

#[tokio::test]
async fn metadata_object_and_field_writes() {
    let app = test_app().await;
    let email = unique_email("metadata-writes");
    let (token, _workspace) = signup(&app, &email).await;

    let objects = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({ "query": "{ objects { edges { node { id nameSingular fieldsList { id name isSystem } } } } }" }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(objects.status, StatusCode::OK, "body: {}", objects.body);
    assert_eq!(
        objects.body["data"]["objects"]["edges"]
            .as_array()
            .unwrap()
            .len(),
        8,
        "body: {}",
        objects.body
    );
    let person = objects.body["data"]["objects"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["node"]["nameSingular"] == "person")
        .expect("person object");
    let person_object_id = person["node"]["id"].as_str().unwrap().to_string();
    let person_name_field_id = person["node"]["fieldsList"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "name")
        .and_then(|field| field["id"].as_str())
        .expect("person name field")
        .to_string();
    let company_object_id = objects.body["data"]["objects"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["node"]["nameSingular"] == "company")
        .and_then(|edge| edge["node"]["id"].as_str())
        .expect("company object id")
        .to_string();

    let created = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: CreateOneObjectInput!) { createOneObject(input: $input) { id nameSingular namePlural labelSingular isSystem labelIdentifierFieldMetadataId fieldsList { id name label type isSystem } } }",
                "variables": {
                    "input": {
                        "object": {
                            "nameSingular": "project",
                            "namePlural": "projects",
                            "labelSingular": "Project",
                            "labelPlural": "Projects",
                            "description": "Custom project object",
                            "icon": "IconBriefcase"
                        }
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK, "body: {}", created.body);
    let object = &created.body["data"]["createOneObject"];
    assert_eq!(object["nameSingular"], "project", "body: {}", created.body);
    assert_eq!(object["isSystem"], false);
    let object_id = object["id"].as_str().expect("object id").to_string();
    let name_field = object["fieldsList"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "name")
        .expect("default name field");
    assert_eq!(name_field["type"], "TEXT");
    assert_eq!(name_field["isSystem"], false);

    let field = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: CreateOneFieldMetadataInput!) { createOneField(input: $input) { id name label type isNullable objectMetadataId } }",
                "variables": {
                    "input": {
                        "field": {
                            "type": "TEXT",
                            "name": "description",
                            "label": "Description",
                            "objectMetadataId": object_id
                        }
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(field.status, StatusCode::OK, "body: {}", field.body);
    let field_id = field.body["data"]["createOneField"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("field id: {}", field.body))
        .to_string();
    assert_eq!(field.body["data"]["createOneField"]["type"], "TEXT");

    let relation_field = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: CreateOneFieldMetadataInput!) { createOneField(input: $input) { id name type settings relation { type targetObjectMetadata { nameSingular } targetFieldMetadata { name } } } }",
                "variables": {
                    "input": {
                        "field": {
                            "type": "RELATION",
                            "name": "company",
                            "label": "Company",
                            "objectMetadataId": object_id,
                            "relationCreationPayload": {
                                "targetObjectMetadataId": company_object_id,
                                "targetFieldLabel": "Projects",
                                "targetFieldIcon": "IconBuildingSkyscraper",
                                "type": "MANY_TO_ONE"
                            }
                        }
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        relation_field.status,
        StatusCode::OK,
        "body: {}",
        relation_field.body
    );
    assert!(
        relation_field.body["errors"].is_null(),
        "errors: {}",
        relation_field.body
    );
    let relation = &relation_field.body["data"]["createOneField"]["relation"];
    assert_eq!(
        relation["type"], "MANY_TO_ONE",
        "body: {}",
        relation_field.body
    );
    assert_eq!(relation["targetObjectMetadata"]["nameSingular"], "company");
    assert_eq!(relation["targetFieldMetadata"]["name"], "projects");

    let company_rel = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "query($id: UUID!) { object(id: $id) { fieldsList { name type relation { type targetFieldMetadata { name } } } } }",
                "variables": { "id": company_object_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    let company_projects = company_rel.body["data"]["object"]["fieldsList"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "projects")
        .expect("company.projects reverse field");
    assert_eq!(company_projects["type"], "RELATION");
    assert_eq!(company_projects["relation"]["type"], "ONE_TO_MANY");
    assert_eq!(
        company_projects["relation"]["targetFieldMetadata"]["name"],
        "company"
    );

    let custom_create = send(
        &app,
        authorized(
            "POST",
            "/rest/projects",
            &token,
            Some(json!({ "name": "Apollo", "description": "Moon mission", "company": company_object_id })),
        ),
    )
    .await;
    assert_eq!(
        custom_create.status,
        StatusCode::CREATED,
        "body: {}",
        custom_create.body
    );
    let custom_id = custom_create.body["data"]["createProject"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        custom_create.body["data"]["createProject"]["name"],
        "Apollo"
    );

    let custom_list = send(&app, authorized("GET", "/rest/projects", &token, None)).await;
    assert_eq!(
        custom_list.status,
        StatusCode::OK,
        "body: {}",
        custom_list.body
    );
    assert_eq!(custom_list.body["totalCount"], 1);
    assert_eq!(custom_list.body["data"]["projects"][0]["name"], "Apollo");

    let custom_get = send(
        &app,
        authorized("GET", &format!("/rest/projects/{custom_id}"), &token, None),
    )
    .await;
    assert_eq!(
        custom_get.status,
        StatusCode::OK,
        "body: {}",
        custom_get.body
    );
    assert_eq!(
        custom_get.body["data"]["project"]["description"],
        "Moon mission"
    );

    let custom_update = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/projects/{custom_id}"),
            &token,
            Some(json!({ "description": "Lunar mission" })),
        ),
    )
    .await;
    assert_eq!(
        custom_update.status,
        StatusCode::OK,
        "body: {}",
        custom_update.body
    );
    assert_eq!(
        custom_update.body["data"]["updateProject"]["description"],
        "Lunar mission"
    );

    let custom_delete = send(
        &app,
        authorized(
            "DELETE",
            &format!("/rest/projects/{custom_id}?soft_delete=true"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        custom_delete.status,
        StatusCode::OK,
        "body: {}",
        custom_delete.body
    );
    let custom_empty = send(&app, authorized("GET", "/rest/projects", &token, None)).await;
    assert_eq!(custom_empty.body["totalCount"], 0);

    let updated_field = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: UpdateOneFieldMetadataInput!) { updateOneField(input: $input) { id label } }",
                "variables": {
                    "input": {
                        "id": field_id,
                        "update": { "label": "Project description" }
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        updated_field.status,
        StatusCode::OK,
        "body: {}",
        updated_field.body
    );
    assert_eq!(
        updated_field.body["data"]["updateOneField"]["label"],
        "Project description"
    );

    let updated_object = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: UpdateOneObjectInput!) { updateOneObject(input: $input) { id labelSingular description } }",
                "variables": {
                    "input": {
                        "id": object_id,
                        "update": {
                            "labelSingular": "Initiative",
                            "description": "Renamed object"
                        }
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        updated_object.status,
        StatusCode::OK,
        "body: {}",
        updated_object.body
    );
    assert_eq!(
        updated_object.body["data"]["updateOneObject"]["labelSingular"],
        "Initiative"
    );

    let duplicate = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: CreateOneObjectInput!) { createOneObject(input: $input) { id } }",
                "variables": {
                    "input": {
                        "object": {
                            "nameSingular": "project",
                            "namePlural": "projects",
                            "labelSingular": "Project",
                            "labelPlural": "Projects"
                        }
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(duplicate.status, StatusCode::OK);
    assert!(
        !duplicate.body["errors"].is_null(),
        "body: {}",
        duplicate.body
    );

    let system_object = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: DeleteOneObjectInput!) { deleteOneObject(input: $input) { id } }",
                "variables": { "input": { "id": person_object_id } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(system_object.status, StatusCode::OK);
    assert!(
        !system_object.body["errors"].is_null(),
        "body: {}",
        system_object.body
    );

    let system_delete = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: DeleteOneFieldInput!) { deleteOneField(input: $input) { id } }",
                "variables": {
                    "input": {
                        "id": person_name_field_id
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(system_delete.status, StatusCode::OK);
    assert!(
        !system_delete.body["errors"].is_null(),
        "body: {}",
        system_delete.body
    );

    let deleted_field = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: DeleteOneFieldInput!) { deleteOneField(input: $input) { id name } }",
                "variables": { "input": { "id": field_id } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        deleted_field.status,
        StatusCode::OK,
        "body: {}",
        deleted_field.body
    );
    assert_eq!(
        deleted_field.body["data"]["deleteOneField"]["name"],
        "description"
    );

    let deleted_object = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: DeleteOneObjectInput!) { deleteOneObject(input: $input) { id nameSingular } }",
                "variables": { "input": { "id": object_id } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        deleted_object.status,
        StatusCode::OK,
        "body: {}",
        deleted_object.body
    );
    assert_eq!(
        deleted_object.body["data"]["deleteOneObject"]["nameSingular"],
        "project"
    );

    let after = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({ "query": "{ objects { edges { node { id } } } }" }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        after.body["data"]["objects"]["edges"]
            .as_array()
            .unwrap()
            .len(),
        8,
        "body: {}",
        after.body
    );
}

#[tokio::test]
async fn view_and_view_child_writes() {
    let app = test_app().await;
    let email = unique_email("view-writes");
    let (token, _workspace) = signup(&app, &email).await;

    let objects = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({ "query": "{ objects { edges { node { id nameSingular fieldsList { id name } } } } }" }),
        )
        .with_bearer(&token),
    )
    .await;
    let person = objects.body["data"]["objects"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["node"]["nameSingular"] == "person")
        .expect("person object");
    let object_id = person["node"]["id"].as_str().unwrap().to_string();
    let name_field_id = person["node"]["fieldsList"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "name")
        .and_then(|field| field["id"].as_str())
        .unwrap()
        .to_string();

    let created = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: CreateViewInput!) { createView(input: $input) { id name type icon isCustom viewFields { id } viewFilters { id } viewSorts { id } viewGroups { id } } }",
                "variables": {
                    "input": {
                        "name": "My people",
                        "objectMetadataId": object_id,
                        "icon": "IconList"
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK, "body: {}", created.body);
    let view = &created.body["data"]["createView"];
    assert_eq!(view["name"], "My people");
    assert_eq!(view["type"], "TABLE");
    assert_eq!(view["isCustom"], true);
    let view_id = view["id"].as_str().unwrap().to_string();
    assert_eq!(view["viewFields"].as_array().unwrap().len(), 0);

    let updated = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($id: String!, $input: UpdateViewInput!) { updateView(id: $id, input: $input) { id name position visibility } }",
                "variables": {
                    "id": view_id,
                    "input": { "name": "Renamed people", "position": 4.5, "visibility": "UNLISTED" }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "body: {}", updated.body);
    assert_eq!(updated.body["data"]["updateView"]["name"], "Renamed people");
    assert_eq!(updated.body["data"]["updateView"]["visibility"], "UNLISTED");

    let cleared = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($id: String!, $input: UpdateViewInput!) { updateView(id: $id, input: $input) { id name anyFieldFilterValue calendarLayout } }",
                "variables": {
                    "id": view_id,
                    "input": { "anyFieldFilterValue": "ada", "calendarLayout": "MONTH" }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(cleared.status, StatusCode::OK, "body: {}", cleared.body);
    assert!(cleared.body["errors"].is_null(), "body: {}", cleared.body);
    assert_eq!(
        cleared.body["data"]["updateView"]["anyFieldFilterValue"],
        "ada"
    );
    assert_eq!(
        cleared.body["data"]["updateView"]["calendarLayout"],
        "MONTH"
    );

    let cleared = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($id: String!, $input: UpdateViewInput!) { updateView(id: $id, input: $input) { id name anyFieldFilterValue calendarLayout } }",
                "variables": {
                    "id": view_id,
                    "input": { "anyFieldFilterValue": null, "calendarLayout": null }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(cleared.status, StatusCode::OK, "body: {}", cleared.body);
    assert!(cleared.body["errors"].is_null(), "body: {}", cleared.body);
    assert!(cleared.body["data"]["updateView"]["anyFieldFilterValue"].is_null());
    assert!(cleared.body["data"]["updateView"]["calendarLayout"].is_null());
    assert_eq!(cleared.body["data"]["updateView"]["name"], "Renamed people");

    let view_field = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: CreateViewFieldInput!) { createViewField(input: $input) { id fieldMetadataId viewId isVisible size position } }",
                "variables": {
                    "input": {
                        "fieldMetadataId": name_field_id,
                        "viewId": view_id,
                        "isVisible": true,
                        "size": 210,
                        "position": 1
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        view_field.status,
        StatusCode::OK,
        "body: {}",
        view_field.body
    );
    let view_field_id = view_field.body["data"]["createViewField"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(view_field.body["data"]["createViewField"]["size"], 210.0);

    let updated_field = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: UpdateViewFieldInput!) { updateViewField(input: $input) { id size position isVisible } }",
                "variables": {
                    "input": {
                        "id": view_field_id,
                        "update": { "size": 150, "position": 3, "isVisible": false }
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        updated_field.status,
        StatusCode::OK,
        "body: {}",
        updated_field.body
    );
    assert_eq!(updated_field.body["data"]["updateViewField"]["size"], 150.0);
    assert_eq!(
        updated_field.body["data"]["updateViewField"]["isVisible"],
        false
    );

    let filter = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: CreateViewFilterInput!) { createViewFilter(input: $input) { id operand value viewId } }",
                "variables": {
                    "input": {
                        "fieldMetadataId": name_field_id,
                        "viewId": view_id,
                        "operand": "CONTAINS",
                        "value": "Ada"
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(filter.status, StatusCode::OK, "body: {}", filter.body);
    let filter_id = filter.body["data"]["createViewFilter"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(filter.body["data"]["createViewFilter"]["value"], "Ada");

    let updated_filter = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: UpdateViewFilterInput!) { updateViewFilter(input: $input) { id operand } }",
                "variables": {
                    "input": { "id": filter_id, "update": { "operand": "IS_NOT" } }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        updated_filter.status,
        StatusCode::OK,
        "body: {}",
        updated_filter.body
    );
    assert_eq!(
        updated_filter.body["data"]["updateViewFilter"]["operand"],
        "IS_NOT"
    );

    let sort = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: CreateViewSortInput!) { createViewSort(input: $input) { id direction viewId } }",
                "variables": {
                    "input": {
                        "fieldMetadataId": name_field_id,
                        "viewId": view_id,
                        "direction": "DESC"
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(sort.status, StatusCode::OK, "body: {}", sort.body);
    let sort_id = sort.body["data"]["createViewSort"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let updated_sort = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: UpdateViewSortInput!) { updateViewSort(input: $input) { id direction } }",
                "variables": {
                    "input": { "id": sort_id, "update": { "direction": "ASC" } }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        updated_sort.status,
        StatusCode::OK,
        "body: {}",
        updated_sort.body
    );
    assert_eq!(
        updated_sort.body["data"]["updateViewSort"]["direction"],
        "ASC"
    );

    let group = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: CreateViewGroupInput!) { createViewGroup(input: $input) { id fieldValue isVisible position viewId } }",
                "variables": {
                    "input": { "viewId": view_id, "fieldValue": "VIP", "position": 2 }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(group.status, StatusCode::OK, "body: {}", group.body);
    let group_id = group.body["data"]["createViewGroup"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let updated_group = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: UpdateViewGroupInput!) { updateViewGroup(input: $input) { id fieldValue isVisible position } }",
                "variables": {
                    "input": { "id": group_id, "update": { "fieldValue": "Regular", "isVisible": false } }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        updated_group.status,
        StatusCode::OK,
        "body: {}",
        updated_group.body
    );
    assert_eq!(
        updated_group.body["data"]["updateViewGroup"]["fieldValue"],
        "Regular"
    );

    let embedded = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!(
                    "{{ getView(id: \"{view_id}\") {{ id name viewFields {{ id size }} viewFilters {{ id operand }} viewSorts {{ id direction }} viewGroups {{ id fieldValue }} }} getViewFields(viewId: \"{view_id}\") {{ id }} getViewFilters(viewId: \"{view_id}\") {{ id }} getViewSorts(viewId: \"{view_id}\") {{ id }} getViewGroups(viewId: \"{view_id}\") {{ id }} }}"
                )
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(embedded.status, StatusCode::OK, "body: {}", embedded.body);
    let embedded_view = &embedded.body["data"]["getView"];
    assert_eq!(embedded_view["viewFields"].as_array().unwrap().len(), 1);
    assert_eq!(embedded_view["viewFilters"].as_array().unwrap().len(), 1);
    assert_eq!(embedded_view["viewSorts"].as_array().unwrap().len(), 1);
    assert_eq!(embedded_view["viewGroups"].as_array().unwrap().len(), 1);
    assert_eq!(
        embedded.body["data"]["getViewFields"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let deleted_filter = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: DeleteViewFilterInput!) { deleteViewFilter(input: $input) { id operand } }",
                "variables": { "input": { "id": filter_id } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        deleted_filter.status,
        StatusCode::OK,
        "body: {}",
        deleted_filter.body
    );
    assert_eq!(
        deleted_filter.body["data"]["deleteViewFilter"]["operand"],
        "IS_NOT"
    );

    let deleted_sort = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: DeleteViewSortInput!) { deleteViewSort(input: $input) }",
                "variables": { "input": { "id": sort_id } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        deleted_sort.status,
        StatusCode::OK,
        "body: {}",
        deleted_sort.body
    );
    assert_eq!(deleted_sort.body["data"]["deleteViewSort"], true);

    let destroyed_field = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: DestroyViewFieldInput!) { destroyViewField(input: $input) { id } }",
                "variables": { "input": { "id": view_field_id } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        destroyed_field.status,
        StatusCode::OK,
        "body: {}",
        destroyed_field.body
    );

    let deleted_group = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: DeleteViewGroupInput!) { deleteViewGroup(input: $input) { id fieldValue } }",
                "variables": { "input": { "id": group_id } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        deleted_group.status,
        StatusCode::OK,
        "body: {}",
        deleted_group.body
    );

    let deleted_view = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($id: String!) { deleteView(id: $id) }",
                "variables": { "id": view_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        deleted_view.status,
        StatusCode::OK,
        "body: {}",
        deleted_view.body
    );
    assert_eq!(deleted_view.body["data"]["deleteView"], true);

    let gone = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!("{{ getView(id: \"{view_id}\") {{ id }} }}")
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert!(
        gone.body["data"]["getView"].is_null(),
        "body: {}",
        gone.body
    );
}

#[tokio::test]
async fn view_batches_and_groups() {
    let app = test_app().await;
    let email = unique_email("view-groups");
    let (token, _workspace) = signup(&app, &email).await;

    let objects = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({ "query": "{ objects { edges { node { id nameSingular fieldsList { id name } } } } }" }),
        )
        .with_bearer(&token),
    )
    .await;
    let person = objects.body["data"]["objects"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["node"]["nameSingular"] == "person")
        .expect("person object");
    let object_id = person["node"]["id"].as_str().unwrap().to_string();
    let field_id = |name: &str| -> String {
        person["node"]["fieldsList"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["name"] == name)
            .and_then(|field| field["id"].as_str())
            .unwrap()
            .to_string()
    };
    let name_field_id = field_id("name");
    let emails_field_id = field_id("emails");

    let view = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: CreateViewInput!) { createView(input: $input) { id } }",
                "variables": { "input": { "name": "Grouped", "objectMetadataId": object_id, "icon": "IconList" } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(view.status, StatusCode::OK, "body: {}", view.body);
    let view_id = view.body["data"]["createView"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let field_groups = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($inputs: [CreateViewFieldGroupInput!]!) { createManyViewFieldGroups(inputs: $inputs) { id name position isVisible viewId } }",
                "variables": {
                    "inputs": [
                        { "name": "Main", "viewId": view_id, "position": 1 },
                        { "name": "Secondary", "viewId": view_id, "position": 2 }
                    ]
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        field_groups.status,
        StatusCode::OK,
        "body: {}",
        field_groups.body
    );
    let field_group_list = field_groups.body["data"]["createManyViewFieldGroups"]
        .as_array()
        .unwrap();
    assert_eq!(field_group_list.len(), 2);
    let field_group_id = field_group_list[0]["id"].as_str().unwrap().to_string();

    let fields = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($inputs: [CreateViewFieldInput!]!) { createManyViewFields(inputs: $inputs) { id fieldMetadataId viewFieldGroupId size } }",
                "variables": {
                    "inputs": [
                        { "fieldMetadataId": name_field_id, "viewId": view_id, "viewFieldGroupId": field_group_id },
                        { "fieldMetadataId": emails_field_id, "viewId": view_id, "size": 240 }
                    ]
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(fields.status, StatusCode::OK, "body: {}", fields.body);
    assert_eq!(
        fields.body["data"]["createManyViewFields"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let fetched_groups = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!("{{ getViewFieldGroups(viewId: \"{view_id}\") {{ id name position viewFields {{ id fieldMetadataId }} }} }}")
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        fetched_groups.status,
        StatusCode::OK,
        "body: {}",
        fetched_groups.body
    );
    let main_group = fetched_groups.body["data"]["getViewFieldGroups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| group["name"] == "Main")
        .unwrap();
    assert_eq!(main_group["viewFields"].as_array().unwrap().len(), 1);

    let updated_group = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: UpdateViewFieldGroupInput!) { updateViewFieldGroup(input: $input) { id name position isVisible } }",
                "variables": {
                    "input": {
                        "id": field_group_id,
                        "update": { "name": "Primary", "position": 5.0, "isVisible": false }
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        updated_group.status,
        StatusCode::OK,
        "body: {}",
        updated_group.body
    );
    assert_eq!(
        updated_group.body["data"]["updateViewFieldGroup"]["name"],
        "Primary"
    );

    let groups = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($inputs: [CreateViewGroupInput!]!) { createManyViewGroups(inputs: $inputs) { id fieldValue position } }",
                "variables": {
                    "inputs": [
                        { "viewId": view_id, "fieldValue": "One", "position": 1 },
                        { "viewId": view_id, "fieldValue": "Two", "position": 2 }
                    ]
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(groups.status, StatusCode::OK, "body: {}", groups.body);
    let group_list = groups.body["data"]["createManyViewGroups"]
        .as_array()
        .unwrap();
    assert_eq!(group_list.len(), 2);

    let updated_groups = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($inputs: [UpdateViewGroupInput!]!) { updateManyViewGroups(inputs: $inputs) { id fieldValue position isVisible } }",
                "variables": {
                    "inputs": [
                        { "id": group_list[0]["id"], "update": { "fieldValue": "Uno", "position": 9.0 } },
                        { "id": group_list[1]["id"], "update": { "isVisible": false } }
                    ]
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        updated_groups.status,
        StatusCode::OK,
        "body: {}",
        updated_groups.body
    );
    let updated_list = updated_groups.body["data"]["updateManyViewGroups"]
        .as_array()
        .unwrap();
    assert_eq!(updated_list.len(), 2);
    assert_eq!(updated_list[0]["fieldValue"], "Uno");
    assert_eq!(updated_list[1]["isVisible"], false);

    let filter_group = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: CreateViewFilterGroupInput!) { createViewFilterGroup(input: $input) { id logicalOperator positionInViewFilterGroup viewId } }",
                "variables": {
                    "input": { "viewId": view_id, "logicalOperator": "OR", "positionInViewFilterGroup": 1.0 }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        filter_group.status,
        StatusCode::OK,
        "body: {}",
        filter_group.body
    );
    let filter_group_id = filter_group.body["data"]["createViewFilterGroup"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let updated_filter_group = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($id: String!, $input: UpdateViewFilterGroupInput!) { updateViewFilterGroup(id: $id, input: $input) { id logicalOperator positionInViewFilterGroup } }",
                "variables": {
                    "id": filter_group_id,
                    "input": { "logicalOperator": "AND", "positionInViewFilterGroup": 2.0 }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        updated_filter_group.status,
        StatusCode::OK,
        "body: {}",
        updated_filter_group.body
    );
    assert_eq!(
        updated_filter_group.body["data"]["updateViewFilterGroup"]["logicalOperator"],
        "AND"
    );

    let fetched_filter_groups = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": format!("{{ getViewFilterGroups(viewId: \"{view_id}\") {{ id logicalOperator }} }}")
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        fetched_filter_groups.status,
        StatusCode::OK,
        "body: {}",
        fetched_filter_groups.body
    );
    assert_eq!(
        fetched_filter_groups.body["data"]["getViewFilterGroups"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let deleted_filter_group = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($id: String!) { deleteViewFilterGroup(id: $id) }",
                "variables": { "id": filter_group_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        deleted_filter_group.status,
        StatusCode::OK,
        "body: {}",
        deleted_filter_group.body
    );
    assert_eq!(
        deleted_filter_group.body["data"]["deleteViewFilterGroup"],
        true
    );

    let destroyed_filter_group = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($id: String!) { destroyViewFilterGroup(id: $id) }",
                "variables": { "id": filter_group_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        destroyed_filter_group.status,
        StatusCode::OK,
        "body: {}",
        destroyed_filter_group.body
    );
    assert_eq!(
        destroyed_filter_group.body["data"]["destroyViewFilterGroup"],
        true
    );

    let deleted_field_group = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: DeleteViewFieldGroupInput!) { deleteViewFieldGroup(input: $input) { id name } }",
                "variables": { "input": { "id": field_group_id } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        deleted_field_group.status,
        StatusCode::OK,
        "body: {}",
        deleted_field_group.body
    );
    assert_eq!(
        deleted_field_group.body["data"]["deleteViewFieldGroup"]["name"],
        "Primary"
    );

    let destroyed_field_group = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($input: DestroyViewFieldGroupInput!) { destroyViewFieldGroup(input: $input) { id } }",
                "variables": { "input": { "id": field_group_id } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        destroyed_field_group.status,
        StatusCode::OK,
        "body: {}",
        destroyed_field_group.body
    );
}

#[tokio::test]
async fn merge_people_repoints_relations_and_honours_dry_run() {
    let app = test_app().await;
    let email = unique_email("merge");
    let (token, _workspace) = signup(&app, &email).await;

    let target = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(json!({
                "name": { "firstName": "Ada", "lastName": "Target" },
                "emails": { "primaryEmail": "ada.target@example.com" }
            })),
        ),
    )
    .await;
    let target_id = target.body["data"]["createPerson"]["id"]
        .as_str()
        .expect("target id")
        .to_string();

    let source = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(person_body("Grace", "Source", "grace.source@example.com")),
        ),
    )
    .await;
    let source_id = source.body["data"]["createPerson"]["id"]
        .as_str()
        .expect("source id")
        .to_string();

    let company = send(
        &app,
        authorized(
            "POST",
            "/rest/companies",
            &token,
            Some(json!({ "name": "Merged Co" })),
        ),
    )
    .await;
    let company_id = company.body["data"]["createCompany"]["id"]
        .as_str()
        .expect("company id")
        .to_string();

    let source_update = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/people/{source_id}"),
            &token,
            Some(json!({ "jobTitle": "CTO", "companyId": company_id })),
        ),
    )
    .await;
    assert_eq!(
        source_update.status,
        StatusCode::OK,
        "body: {}",
        source_update.body
    );

    let task = send(
        &app,
        authorized(
            "POST",
            "/rest/tasks",
            &token,
            Some(json!({ "title": "Merge relation" })),
        ),
    )
    .await;
    let task_id = task.body["data"]["createTask"]["id"]
        .as_str()
        .expect("task id")
        .to_string();
    let target_link = send(
        &app,
        authorized(
            "POST",
            "/rest/taskTargets",
            &token,
            Some(json!({ "taskId": task_id, "targetPersonId": source_id })),
        ),
    )
    .await;
    assert_eq!(
        target_link.status,
        StatusCode::CREATED,
        "body: {}",
        target_link.body
    );

    let opportunity = send(
        &app,
        authorized(
            "POST",
            "/rest/opportunities",
            &token,
            Some(json!({
                "name": "Merge deal",
                "amount": { "amountMicros": 1000000, "currencyCode": "USD" },
                "closeDate": "2026-12-31T00:00:00Z",
                "stage": "PROPOSAL",
                "pointOfContactId": source_id
            })),
        ),
    )
    .await;
    assert_eq!(
        opportunity.status,
        StatusCode::CREATED,
        "body: {}",
        opportunity.body
    );
    let opportunity_id = opportunity.body["data"]["createOpportunity"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let preview = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($ids: [UUID!]!) { mergePeople(ids: $ids, conflictPriorityIndex: 1, dryRun: true) { id name { firstName lastName } jobTitle emails { primaryEmail additionalEmails } companyId } }",
                "variables": { "ids": [source_id, target_id] }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(preview.status, StatusCode::OK, "body: {}", preview.body);
    let preview_person = &preview.body["data"]["mergePeople"];
    assert_eq!(preview_person["id"], target_id);
    assert_eq!(preview_person["jobTitle"], "CTO");
    assert_eq!(preview_person["companyId"], company_id);
    let preview_emails = preview_person["emails"]["additionalEmails"]
        .as_array()
        .unwrap();
    assert!(preview_emails
        .iter()
        .any(|value| value == "grace.source@example.com"));

    let untouched = send(
        &app,
        authorized("GET", &format!("/rest/people/{target_id}"), &token, None),
    )
    .await;
    assert!(untouched.body["data"]["person"]["jobTitle"].is_null());
    let source_still_active = send(
        &app,
        authorized("GET", &format!("/rest/people/{source_id}"), &token, None),
    )
    .await;
    assert!(source_still_active.body["data"]["person"]["deletedAt"].is_null());

    let merged = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($ids: [UUID!]!) { mergePeople(ids: $ids, conflictPriorityIndex: 1) { id jobTitle companyId emails { primaryEmail additionalEmails } } }",
                "variables": { "ids": [source_id, target_id] }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(merged.status, StatusCode::OK, "body: {}", merged.body);
    assert_eq!(merged.body["data"]["mergePeople"]["id"], target_id);
    assert_eq!(merged.body["data"]["mergePeople"]["jobTitle"], "CTO");
    assert_eq!(merged.body["data"]["mergePeople"]["companyId"], company_id);

    let source_deleted = send(
        &app,
        authorized("GET", &format!("/rest/people/{source_id}"), &token, None),
    )
    .await;
    assert_eq!(source_deleted.status, StatusCode::NOT_FOUND);

    let linked_targets = send(&app, authorized("GET", "/rest/taskTargets", &token, None)).await;
    assert_eq!(linked_targets.status, StatusCode::OK);
    assert_eq!(linked_targets.body["totalCount"], 1);
    assert_eq!(
        linked_targets.body["data"]["taskTargets"][0]["targetPersonId"],
        target_id
    );

    let linked_opportunity = send(
        &app,
        authorized(
            "GET",
            &format!("/rest/opportunities/{opportunity_id}"),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(
        linked_opportunity.body["data"]["opportunity"]["pointOfContactId"],
        target_id
    );

    let invalid = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($ids: [UUID!]!) { mergePeople(ids: $ids, conflictPriorityIndex: 4) { id } }",
                "variables": { "ids": [source_id, target_id] }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(invalid.status, StatusCode::OK);
    assert_eq!(
        invalid.body["errors"][0]["extensions"]["code"],
        "BAD_USER_INPUT"
    );
}

#[tokio::test]
async fn person_updates_clear_nullable_fields_but_keep_omitted_ones() {
    let app = test_app().await;
    let email = unique_email("null-clearing");
    let (token, _workspace) = signup(&app, &email).await;

    let company = send(
        &app,
        authorized(
            "POST",
            "/rest/companies",
            &token,
            Some(json!({ "name": "Clear Co" })),
        ),
    )
    .await;
    let company_id = company.body["data"]["createCompany"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let person = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(person_body("Null", "Clearing", "null.clearing@example.com")),
        ),
    )
    .await;
    let person_id = person.body["data"]["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let assigned = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/people/{person_id}"),
            &token,
            Some(json!({ "companyId": company_id })),
        ),
    )
    .await;
    assert_eq!(
        assigned.body["data"]["updatePerson"]["companyId"], company_id,
        "body: {}",
        assigned.body
    );
    assert_eq!(
        assigned.body["data"]["updatePerson"]["jobTitle"],
        "Engineer"
    );

    let cleared = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/people/{person_id}"),
            &token,
            Some(json!({ "jobTitle": null, "companyId": null })),
        ),
    )
    .await;
    assert_eq!(cleared.status, StatusCode::OK, "body: {}", cleared.body);
    assert!(
        cleared.body["data"]["updatePerson"]["jobTitle"].is_null(),
        "body: {}",
        cleared.body
    );
    assert!(
        cleared.body["data"]["updatePerson"]["companyId"].is_null(),
        "body: {}",
        cleared.body
    );

    let omitted_keeps = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/people/{person_id}"),
            &token,
            Some(json!({ "position": 4.0 })),
        ),
    )
    .await;
    assert!(omitted_keeps.body["data"]["updatePerson"]["jobTitle"].is_null());
    assert!(omitted_keeps.body["data"]["updatePerson"]["companyId"].is_null());

    let set_again = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/people/{person_id}"),
            &token,
            Some(json!({ "jobTitle": "Staff" })),
        ),
    )
    .await;
    assert_eq!(set_again.body["data"]["updatePerson"]["jobTitle"], "Staff");

    let gql_cleared = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "mutation($id: UUID!, $data: PersonUpdateInput!) { updatePerson(id: $id, data: $data) { id jobTitle } }",
                "variables": {
                    "id": person_id,
                    "data": { "jobTitle": null }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(
        gql_cleared.status,
        StatusCode::OK,
        "body: {}",
        gql_cleared.body
    );
    assert!(gql_cleared.body["data"]["updatePerson"]["jobTitle"].is_null());
}

#[tokio::test]
async fn object_updates_clear_nullable_fields() {
    let app = test_app().await;
    let email = unique_email("object-null-clearing");
    let (token, _workspace) = signup(&app, &email).await;

    let person = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(person_body("Clear", "Owner", "clear.owner@example.com")),
        ),
    )
    .await;
    let person_id = person.body["data"]["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let company = send(
        &app,
        authorized(
            "POST",
            "/rest/companies",
            &token,
            Some(json!({ "name": "Null Co" })),
        ),
    )
    .await;
    let company_id = company.body["data"]["createCompany"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let company_owner = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/companies/{company_id}"),
            &token,
            Some(json!({ "accountOwnerId": person_id })),
        ),
    )
    .await;
    assert_eq!(
        company_owner.body["data"]["updateCompany"]["accountOwnerId"],
        person_id
    );
    let company_cleared = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/companies/{company_id}"),
            &token,
            Some(json!({ "name": null, "accountOwnerId": null })),
        ),
    )
    .await;
    assert_eq!(
        company_cleared.status,
        StatusCode::OK,
        "body: {}",
        company_cleared.body
    );
    assert!(company_cleared.body["data"]["updateCompany"]["name"].is_null());
    assert!(company_cleared.body["data"]["updateCompany"]["accountOwnerId"].is_null());

    let opportunity = send(
        &app,
        authorized(
            "POST",
            "/rest/opportunities",
            &token,
            Some(json!({
                "name": "Null deal",
                "amount": { "amountMicros": 1000000, "currencyCode": "USD" },
                "closeDate": "2026-12-31T00:00:00Z",
                "stage": "PROPOSAL",
                "pointOfContactId": person_id
            })),
        ),
    )
    .await;
    let opportunity_id = opportunity.body["data"]["createOpportunity"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let opportunity_cleared = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/opportunities/{opportunity_id}"),
            &token,
            Some(json!({ "closeDate": null, "pointOfContactId": null })),
        ),
    )
    .await;
    assert_eq!(
        opportunity_cleared.status,
        StatusCode::OK,
        "body: {}",
        opportunity_cleared.body
    );
    assert!(opportunity_cleared.body["data"]["updateOpportunity"]["closeDate"].is_null());
    assert!(opportunity_cleared.body["data"]["updateOpportunity"]["pointOfContactId"].is_null());

    let task = send(
        &app,
        authorized(
            "POST",
            "/rest/tasks",
            &token,
            Some(json!({ "title": "Null task" })),
        ),
    )
    .await;
    let task_id = task.body["data"]["createTask"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let task_updated = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/tasks/{task_id}"),
            &token,
            Some(json!({ "dueAt": "2026-11-01T00:00:00Z", "assigneeId": person_id })),
        ),
    )
    .await;
    assert_eq!(
        task_updated.body["data"]["updateTask"]["assigneeId"],
        person_id
    );
    let task_cleared = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/tasks/{task_id}"),
            &token,
            Some(json!({ "title": null, "dueAt": null, "assigneeId": null })),
        ),
    )
    .await;
    assert_eq!(
        task_cleared.status,
        StatusCode::OK,
        "body: {}",
        task_cleared.body
    );
    assert!(task_cleared.body["data"]["updateTask"]["title"].is_null());
    assert!(task_cleared.body["data"]["updateTask"]["dueAt"].is_null());
    assert!(task_cleared.body["data"]["updateTask"]["assigneeId"].is_null());

    let note = send(
        &app,
        authorized(
            "POST",
            "/rest/notes",
            &token,
            Some(json!({ "title": "Null note" })),
        ),
    )
    .await;
    let note_id = note.body["data"]["createNote"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let note_cleared = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/notes/{note_id}"),
            &token,
            Some(json!({ "title": null })),
        ),
    )
    .await;
    assert!(note_cleared.body["data"]["updateNote"]["title"].is_null());

    let target = send(
        &app,
        authorized(
            "POST",
            "/rest/taskTargets",
            &token,
            Some(json!({ "taskId": task_id, "targetCompanyId": company_id })),
        ),
    )
    .await;
    let target_id = target.body["data"]["createTaskTarget"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let target_cleared = send(
        &app,
        authorized(
            "PATCH",
            &format!("/rest/taskTargets/{target_id}"),
            &token,
            Some(json!({ "targetCompanyId": null })),
        ),
    )
    .await;
    assert_eq!(
        target_cleared.status,
        StatusCode::OK,
        "body: {}",
        target_cleared.body
    );
    assert!(target_cleared.body["data"]["updateTaskTarget"]["targetCompanyId"].is_null());
}

#[tokio::test]
async fn webhook_crud_matches_metadata_contract() {
    let app = test_app().await;
    let email = unique_email("webhooks");
    let (token, _workspace) = signup_with_access(&app, &email).await;

    let created = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($input: CreateWebhookInput!) { createWebhook(input: $input) { id targetUrl operations description secret applicationId createdAt } }",
                "variables": {
                    "input": {
                        "targetUrl": "https://example.com/hook",
                        "operations": ["person.created", "company.updated"],
                        "description": "Test hook"
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK, "body: {}", created.body);
    assert!(created.body["errors"].is_null(), "errors: {}", created.body);
    let webhook = &created.body["data"]["createWebhook"];
    let webhook_id = webhook["id"].as_str().unwrap().to_string();
    assert_eq!(webhook["targetUrl"], "https://example.com/hook");
    assert_eq!(webhook["operations"][0], "person.created");
    assert_eq!(webhook["description"], "Test hook");
    assert!(webhook["secret"].as_str().unwrap().len() >= 16);

    let listed = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({ "query": "{ webhooks { id targetUrl operations } }" }),
        )
        .with_bearer(&token),
    )
    .await;
    assert!(listed.body["errors"].is_null(), "errors: {}", listed.body);
    assert_eq!(listed.body["data"]["webhooks"].as_array().unwrap().len(), 1);

    let fetched = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "query($id: UUID!) { webhook(id: $id) { id targetUrl } }",
                "variables": { "id": webhook_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert!(fetched.body["errors"].is_null(), "errors: {}", fetched.body);
    assert_eq!(fetched.body["data"]["webhook"]["id"], webhook_id);

    let updated = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($input: UpdateWebhookInput!) { updateWebhook(input: $input) { id targetUrl operations } }",
                "variables": {
                    "input": {
                        "id": webhook_id,
                        "update": { "targetUrl": "https://example.com/hook2", "operations": ["person.updated"] }
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert!(updated.body["errors"].is_null(), "errors: {}", updated.body);
    assert_eq!(
        updated.body["data"]["updateWebhook"]["targetUrl"],
        "https://example.com/hook2"
    );
    assert_eq!(
        updated.body["data"]["updateWebhook"]["operations"][0],
        "person.updated"
    );

    let deleted = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($id: UUID!) { deleteWebhook(id: $id) { id deletedAt } }",
                "variables": { "id": webhook_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert!(deleted.body["errors"].is_null(), "errors: {}", deleted.body);
    assert!(deleted.body["data"]["deleteWebhook"]["deletedAt"].is_string());

    let empty = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({ "query": "{ webhooks { id } }" }),
        )
        .with_bearer(&token),
    )
    .await;
    assert!(empty.body["errors"].is_null(), "errors: {}", empty.body);
    assert!(empty.body["data"]["webhooks"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn role_assignment_uses_metadata_roles() {
    let app = test_app().await;
    let email = unique_email("roles");
    let (token, _workspace) = signup_with_access(&app, &email).await;

    let roles = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({ "query": "{ getRoles { id label isEditable } getApiKeyRoles { id label } }" }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(roles.status, StatusCode::OK, "body: {}", roles.body);
    assert!(roles.body["errors"].is_null(), "errors: {}", roles.body);
    let role_list = roles.body["data"]["getRoles"].as_array().unwrap();
    assert_eq!(role_list.len(), 3);
    let admin_role_id = role_list
        .iter()
        .find(|role| role["label"] == "Admin")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        roles.body["data"]["getApiKeyRoles"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    let members = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({ "query": "{ workspaceMembers { edges { node { id roles { label } } } } }" }),
        )
        .with_bearer(&token),
    )
    .await;
    assert!(members.body["errors"].is_null(), "errors: {}", members.body);
    let member_id = members.body["data"]["workspaceMembers"]["edges"][0]["node"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        members.body["data"]["workspaceMembers"]["edges"][0]["node"]["roles"][0]["label"],
        "Owner"
    );

    let updated = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($memberId: UUID!, $roleId: UUID!) { updateWorkspaceMemberRole(workspaceMemberId: $memberId, roleId: $roleId) { id roles { label } } }",
                "variables": { "memberId": member_id, "roleId": admin_role_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(updated.status, StatusCode::OK, "body: {}", updated.body);
    assert!(updated.body["errors"].is_null(), "errors: {}", updated.body);
    assert_eq!(
        updated.body["data"]["updateWorkspaceMemberRole"]["roles"][0]["label"],
        "Admin"
    );

    let role = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "query($id: UUID!) { getRole(id: $id) { id label canUpdateAllSettings } }",
                "variables": { "id": admin_role_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert!(role.body["errors"].is_null(), "errors: {}", role.body);
    assert_eq!(role.body["data"]["getRole"]["label"], "Admin");
    assert_eq!(role.body["data"]["getRole"]["canUpdateAllSettings"], true);

    let custom = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($input: CreateRoleInput!) { createOneRole(createRoleInput: $input) { id label isEditable } }",
                "variables": { "input": { "label": "Support", "canBeAssignedToApiKeys": true } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(custom.status, StatusCode::OK, "body: {}", custom.body);
    assert!(custom.body["errors"].is_null(), "errors: {}", custom.body);
    assert_eq!(custom.body["data"]["createOneRole"]["label"], "Support");
    assert_eq!(custom.body["data"]["createOneRole"]["isEditable"], true);
    let custom_role_id = custom.body["data"]["createOneRole"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let flags = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($input: UpsertPermissionFlagsInput!) { upsertPermissionFlags(upsertPermissionFlagsInput: $input) { roleId flag } }",
                "variables": {
                    "input": {
                        "roleId": custom_role_id,
                        "permissionFlagKeys": ["VIEWS", "EXPORT_CSV"]
                    }
                }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert!(flags.body["errors"].is_null(), "errors: {}", flags.body);
    let flags = flags.body["data"]["upsertPermissionFlags"]
        .as_array()
        .unwrap();
    assert_eq!(flags.len(), 2);
    assert!(flags.iter().any(|flag| flag["flag"] == "EXPORT_CSV"));

    let renamed = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($input: UpdateRoleInput!) { updateOneRole(updateRoleInput: $input) { id label } }",
                "variables": { "input": { "id": custom_role_id, "update": { "label": "Support Plus" } } }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert!(renamed.body["errors"].is_null(), "errors: {}", renamed.body);
    assert_eq!(
        renamed.body["data"]["updateOneRole"]["label"],
        "Support Plus"
    );

    let deleted = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($id: UUID!) { deleteOneRole(roleId: $id) }",
                "variables": { "id": custom_role_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert!(deleted.body["errors"].is_null(), "errors: {}", deleted.body);
    assert_eq!(deleted.body["data"]["deleteOneRole"], custom_role_id);

    let member_role_id = role_list
        .iter()
        .find(|role| role["label"] == "Member")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let demoted = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($memberId: UUID!, $roleId: UUID!) { updateWorkspaceMemberRole(workspaceMemberId: $memberId, roleId: $roleId) { id roles { label } } }",
                "variables": { "memberId": member_id, "roleId": member_role_id }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert!(demoted.body["errors"].is_null(), "errors: {}", demoted.body);

    let forbidden = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation { createOneRole(createRoleInput: { label: \"Nope\" }) { id } }"
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(forbidden.status, StatusCode::OK);
    assert_eq!(
        forbidden.body["errors"][0]["extensions"]["code"],
        "FORBIDDEN"
    );
}

#[tokio::test]
async fn workspace_members_query_and_boot_list() {
    let app = test_app().await;
    let email = unique_email("workspace-members");
    let (token, _workspace) = signup_with_access(&app, &email).await;

    let response = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "query($email: String!) { workspaceMember(filter: { userEmail: { eq: $email } }) { id name { firstName lastName } userEmail colorScheme locale } workspaceMembers { totalCount edges { node { id userEmail } } pageInfo { hasNextPage hasPreviousPage } } currentUser { workspaceMembers { id userEmail } } }",
                "variables": { "email": email }
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "body: {}", response.body);
    assert!(
        response.body["errors"].is_null(),
        "errors: {}",
        response.body
    );

    let member = &response.body["data"]["workspaceMember"];
    assert_eq!(member["userEmail"], email);
    assert_eq!(member["colorScheme"], "Light");
    assert_eq!(member["locale"], "en");

    let connection = &response.body["data"]["workspaceMembers"];
    assert_eq!(connection["totalCount"], 1);
    assert_eq!(connection["edges"].as_array().unwrap().len(), 1);
    assert_eq!(
        connection["edges"][0]["node"]["userEmail"].as_str(),
        Some(email.as_str())
    );
    assert_eq!(connection["pageInfo"]["hasNextPage"], false);
    assert_eq!(connection["pageInfo"]["hasPreviousPage"], false);

    let boot_members = response.body["data"]["currentUser"]["workspaceMembers"]
        .as_array()
        .expect("boot workspace members");
    assert_eq!(boot_members.len(), 1);
    assert_eq!(boot_members[0]["userEmail"], email);

    let missing = send(
        &app,
        json_request(
            "POST",
            "/graphql",
            json!({
                "query": "{ workspaceMember(filter: { userEmail: { eq: \"nobody@example.com\" } }) { id } }"
            }),
        )
        .with_bearer(&token),
    )
    .await;
    assert_eq!(missing.status, StatusCode::OK);
    assert!(missing.body["data"]["workspaceMember"].is_null());
}

#[tokio::test]
async fn rest_group_by_matches_captured_contract() {
    let app = test_app().await;
    let email = unique_email("group-by");
    let (token, _workspace) = signup(&app, &email).await;

    for (first, job) in [("Ada", "Engineer"), ("Grace", "Engineer"), ("Linus", "")] {
        let body = json!({
            "name": { "firstName": first, "lastName": "Group" },
            "emails": { "primaryEmail": format!("{first}@example.com") },
            "jobTitle": job
        });
        let created = send(&app, authorized("POST", "/rest/people", &token, Some(body))).await;
        assert_eq!(
            created.status,
            StatusCode::CREATED,
            "body: {}",
            created.body
        );
    }

    let objects = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "{ objects(paging:{first:20}) { edges { node { id nameSingular fieldsList { id name } } } } }"
            })),
        ),
    )
    .await;
    let person = objects.body["data"]["objects"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["node"]["nameSingular"] == "person")
        .expect("person metadata");
    let object_id = person["node"]["id"].as_str().unwrap().to_string();
    let name_field_id = person["node"]["fieldsList"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "name")
        .expect("name field")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let view = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation($input: CreateViewInput!) { createView(input:$input) { id } }",
                "variables": { "input": { "name": "Ada only", "objectMetadataId": object_id, "icon": "IconList" } }
            })),
        ),
    )
    .await;
    assert!(view.body["errors"].is_null(), "view body: {}", view.body);
    let view_id = view.body["data"]["createView"]["id"]
        .as_str()
        .expect("view id")
        .to_string();
    let filter = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation($input: CreateViewFilterInput!) { createViewFilter(input:$input) { id } }",
                "variables": {
                    "input": {
                        "fieldMetadataId": name_field_id,
                        "viewId": view_id,
                        "operand": "CONTAINS",
                        "value": "Ada",
                        "subFieldName": "firstName"
                    }
                }
            })),
        ),
    )
    .await;
    assert_eq!(filter.status, StatusCode::OK, "body: {}", filter.body);
    assert!(filter.body["errors"].is_null(), "errors: {}", filter.body);

    let second_view = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation($input: CreateViewInput!) { createView(input:$input) { id } }",
                "variables": { "input": { "name": "Sorted", "objectMetadataId": object_id, "icon": "IconList" } }
            })),
        ),
    )
    .await;
    assert!(
        second_view.body["errors"].is_null(),
        "view body: {}",
        second_view.body
    );
    let second_view_id = second_view.body["data"]["createView"]["id"]
        .as_str()
        .expect("view id")
        .to_string();
    let view_sort = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation($input: CreateViewSortInput!) { createViewSort(input: $input) { id direction } }",
                "variables": {
                    "input": {
                        "fieldMetadataId": name_field_id,
                        "viewId": second_view_id,
                        "direction": "DESC",
                        "subFieldName": "firstName"
                    }
                }
            })),
        ),
    )
    .await;
    assert!(
        view_sort.body["errors"].is_null(),
        "sort: {}",
        view_sort.body
    );

    let response = send(
        &app,
        authorized(
            "GET",
            "/rest/people/groupBy?group_by=%5B%7B%22jobTitle%22%3Atrue%7D%5D&aggregate=%5B%22countNotEmptyId%22%2C%22percentageNotEmptyId%22%5D&include_records_sample=true&order_by_for_records=createdAt%5BAscNullsLast%5D",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK, "body: {}", response.body);
    let groups = response
        .body
        .as_array()
        .expect("groupBy returns a bare array");
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["groupByDimensionValues"], json!(["Engineer"]));
    assert_eq!(groups[0]["countNotEmptyId"], "2");
    assert_eq!(groups[0]["percentageNotEmptyId"], "1.00000000000000000000");
    assert_eq!(groups[0]["records"].as_array().unwrap().len(), 2);
    assert_eq!(groups[1]["groupByDimensionValues"], json!([null]));
    assert_eq!(groups[1]["countNotEmptyId"], "1");

    let offset = send(
        &app,
        authorized(
            "GET",
            "/rest/people/groupBy?group_by=%5B%7B%22jobTitle%22%3Atrue%7D%5D&aggregate=%5B%22countNotEmptyId%22%5D&include_records_sample=true&order_by_for_records=createdAt%5BAscNullsLast%5D&offsetForRecords=1",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(offset.status, StatusCode::OK, "body: {}", offset.body);
    let offset_groups = offset.body.as_array().expect("offset groups");
    assert_eq!(offset_groups[0]["records"].as_array().unwrap().len(), 1);
    assert_eq!(offset_groups[0]["records"][0]["name"]["firstName"], "Grace");

    let sorted = send(
        &app,
        authorized(
            "GET",
            &format!(
                "/rest/people/groupBy?group_by=%5B%7B%22jobTitle%22%3Atrue%7D%5D&aggregate=%5B%22countNotEmptyId%22%5D&include_records_sample=true&viewId={second_view_id}"
            ),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(sorted.status, StatusCode::OK, "body: {}", sorted.body);
    let sorted_groups = sorted.body.as_array().expect("sorted groups");
    assert_eq!(sorted_groups[0]["records"][0]["name"]["firstName"], "Grace");
    assert_eq!(sorted_groups[0]["records"][1]["name"]["firstName"], "Ada");

    let filtered = send(
        &app,
        authorized(
            "GET",
            &format!(
                "/rest/people/groupBy?group_by=%5B%7B%22jobTitle%22%3Atrue%7D%5D&viewId={view_id}"
            ),
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(filtered.status, StatusCode::OK, "body: {}", filtered.body);
    let filtered_groups = filtered.body.as_array().expect("view-filtered groups");
    assert_eq!(filtered_groups.len(), 1);
    assert_eq!(
        filtered_groups[0]["groupByDimensionValues"],
        json!(["Engineer"])
    );

    let invalid = send(
        &app,
        authorized(
            "GET",
            "/rest/people/groupBy?group_by=%5B%7B%22nope%22%3Atrue%7D%5D",
            &token,
            None,
        ),
    )
    .await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        invalid.body["messages"][0],
        "Unidentified field in groupBy: nope"
    );
}

#[tokio::test]
async fn file_upload_and_avatar_file_write_round_trip() {
    let app = test_app().await;
    let email = unique_email("avatar-file");
    let (token, _workspace) = signup(&app, &email).await;

    let upload = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation($filename: String!, $size: Float!, $folder: FileFolder!) { createFileUpload(filename:$filename, size:$size, fileFolder:$folder) { fileId uploadUrl contentType } }",
                "variables": { "filename": "portrait.png", "size": 4.0, "folder": "CorePicture" }
            })),
        ),
    )
    .await;
    assert_eq!(upload.status, StatusCode::OK, "body: {}", upload.body);
    assert!(upload.body["errors"].is_null(), "errors: {}", upload.body);
    let file_id = upload.body["data"]["createFileUpload"]["fileId"]
        .as_str()
        .expect("file id")
        .to_string();
    let upload_url = upload.body["data"]["createFileUpload"]["uploadUrl"]
        .as_str()
        .expect("upload url")
        .to_string();
    assert_eq!(
        upload.body["data"]["createFileUpload"]["contentType"],
        "image/png"
    );

    let unsigned_upload = send(
        &app,
        Request::builder()
            .method("PUT")
            .uri(format!("/files/upload/{file_id}"))
            .header(header::CONTENT_TYPE, "image/png")
            .body(Body::from(vec![0x89, b'P', b'N', b'G']))
            .unwrap(),
    )
    .await;
    assert_eq!(
        unsigned_upload.status,
        StatusCode::FORBIDDEN,
        "body: {}",
        unsigned_upload.body
    );

    let unsigned_download = send(
        &app,
        Request::builder()
            .method("GET")
            .uri(format!("/files/{file_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        unsigned_download.status,
        StatusCode::FORBIDDEN,
        "body: {}",
        unsigned_download.body
    );

    let stored = send(
        &app,
        Request::builder()
            .method("PUT")
            .uri(&upload_url)
            .header(header::CONTENT_TYPE, "image/png")
            .body(Body::from(vec![0x89, b'P', b'N', b'G']))
            .unwrap(),
    )
    .await;
    assert_eq!(
        stored.status,
        StatusCode::NO_CONTENT,
        "body: {}",
        stored.body
    );

    let completed = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation($id: String!) { completeFileUpload(fileId:$id) { id path size url } }",
                "variables": { "id": file_id }
            })),
        ),
    )
    .await;
    assert_eq!(completed.status, StatusCode::OK, "body: {}", completed.body);
    assert!(
        completed.body["errors"].is_null(),
        "errors: {}",
        completed.body
    );
    assert_eq!(completed.body["data"]["completeFileUpload"]["size"], 4.0);

    let created = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(json!({
                "name": { "firstName": "Ada", "lastName": "File" },
                "emails": { "primaryEmail": "ada-file@example.com" },
                "avatarFile": [{ "fileId": file_id, "label": "Portrait" }]
            })),
        ),
    )
    .await;
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "body: {}",
        created.body
    );
    let avatar = &created.body["data"]["createPerson"]["avatarFile"][0];
    assert_eq!(avatar["label"], "Portrait");
    assert_eq!(avatar["extension"], "png");
    assert_eq!(avatar["fileId"], file_id);

    let missing_id = uuid::Uuid::new_v4();
    let rejected = send(
        &app,
        authorized(
            "POST",
            "/rest/people",
            &token,
            Some(json!({
                "name": { "firstName": "Missing", "lastName": "File" },
                "emails": { "primaryEmail": "missing-file@example.com" },
                "avatarFile": [{ "fileId": missing_id, "label": "Missing" }]
            })),
        ),
    )
    .await;
    assert_eq!(rejected.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        rejected.body["messages"][0],
        format!("File not found: {missing_id}")
    );
}

#[tokio::test]
async fn metadata_index_roles_and_permission_flags() {
    let app = test_app().await;
    let email = unique_email("metadata-index");
    let (token, _workspace) = signup(&app, &email).await;

    let objects = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "{ objects(paging:{first:20}) { edges { node { id nameSingular fieldsList { id name } } } } }"
            })),
        ),
    )
    .await;
    assert_eq!(objects.status, StatusCode::OK, "body: {}", objects.body);
    assert!(objects.body["errors"].is_null(), "errors: {}", objects.body);
    let person = objects.body["data"]["objects"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["node"]["nameSingular"] == "person")
        .expect("person metadata");
    let object_id = person["node"]["id"].as_str().unwrap().to_string();
    let field_id = person["node"]["fieldsList"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "jobTitle")
        .expect("jobTitle field")["id"]
        .as_str()
        .unwrap()
        .to_string();

    let created = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation($input: CreateOneIndexInput!) { createOneIndex(input:$input) { id name indexType indexFieldMetadataList { fieldMetadataId order } } }",
                "variables": {
                    "input": {
                        "index": {
                            "objectMetadataId": object_id,
                            "fields": [{ "fieldMetadataId": field_id }]
                        }
                    }
                }
            })),
        ),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK, "body: {}", created.body);
    assert!(created.body["errors"].is_null(), "errors: {}", created.body);
    let index = &created.body["data"]["createOneIndex"];
    assert_eq!(index["indexType"], "BTREE");
    assert_eq!(index["indexFieldMetadataList"].as_array().unwrap().len(), 1);
    let index_id = index["id"].as_str().unwrap().to_string();

    let roles = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "query($id: UUID!) { getRoles { id label } getPermissionFlags { key permissionType } mostlyEmptyFieldMetadataIds(objectMetadataId:$id) }",
                "variables": { "id": object_id }
            })),
        ),
    )
    .await;
    assert_eq!(roles.status, StatusCode::OK, "body: {}", roles.body);
    assert!(roles.body["errors"].is_null(), "errors: {}", roles.body);
    assert_eq!(roles.body["data"]["getRoles"].as_array().unwrap().len(), 3);
    assert_eq!(
        roles.body["data"]["getPermissionFlags"]
            .as_array()
            .unwrap()
            .len(),
        26
    );
    assert_eq!(roles.body["data"]["mostlyEmptyFieldMetadataIds"], json!([]));

    let deleted = send(
        &app,
        authorized(
            "POST",
            "/graphql",
            &token,
            Some(json!({
                "query": "mutation($input: DeleteOneIndexInput!) { deleteOneIndex(input:$input) { id } }",
                "variables": { "input": { "id": index_id } }
            })),
        ),
    )
    .await;
    assert_eq!(deleted.status, StatusCode::OK, "body: {}", deleted.body);
    assert!(deleted.body["errors"].is_null(), "errors: {}", deleted.body);
    assert_eq!(deleted.body["data"]["deleteOneIndex"]["id"], index_id);
}
