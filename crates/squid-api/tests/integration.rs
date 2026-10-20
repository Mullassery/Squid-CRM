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
                "query": "{ people { totalCount edges { node { id name { firstName lastName } emails { primaryEmail } } cursor } pageInfo { startCursor endCursor hasNextPage hasPreviousPage } } }"
            }),
        )
        .with_bearer(&access_token),
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
        .with_bearer(&access_token),
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
        .with_bearer(&access_token),
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
    let (token, _workspace) = signup(&app, &email).await;

    let created = send(
        &app,
        json_request(
            "POST",
            "/metadata",
            json!({
                "query": "mutation($input: CreateApiKeyInput!) { createApiKey(input:$input) { id name expiresAt role } }",
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
        assert_eq!(objects.len(), 5, "seed must be idempotent");
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
    let (token, _workspace) = signup(&app, &email).await;

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
        5,
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
        5,
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
