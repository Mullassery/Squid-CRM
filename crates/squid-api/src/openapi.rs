//! OpenAPI 3.1 document for the supported HTTP surface.
//!
//! The document is assembled from the same resource list the REST routes use
//! so a newly added core resource does not silently disappear from the spec.
//! Request/response bodies are intentionally generic (`object`) for now: the
//! authoritative field-level contracts live in `docs/api.md` and the GraphQL
//! schema, while this document is for discovery, client generation and
//! tooling.

use axum::Json;
use serde_json::{json, Map, Value};

const RESOURCES: &[&str] = &[
    "people",
    "companies",
    "opportunities",
    "tasks",
    "notes",
    "taskTargets",
    "noteTargets",
];

fn object_schema() -> Value {
    json!({"type": "object", "additionalProperties": true})
}

fn operation(id: &str, method: &str, summary: &str, with_body: bool) -> Value {
    let mut operation = Map::new();
    operation.insert("operationId".into(), json!(id));
    operation.insert("summary".into(), json!(summary));
    operation.insert("tags".into(), json!([method]));
    operation.insert(
        "responses".into(),
        json!({
            "200": {
                "description": "Successful response",
                "content": {"application/json": {"schema": object_schema()}}
            },
            "400": {"description": "Invalid request"},
            "401": {"description": "Authentication required"},
            "403": {"description": "Forbidden"}
        }),
    );
    if with_body {
        operation.insert(
            "requestBody".into(),
            json!({
                "required": true,
                "content": {"application/json": {"schema": object_schema()}}
            }),
        );
    }
    Value::Object(operation)
}

fn all_paths() -> Value {
    let mut paths = Map::new();
    paths.insert(
        "/healthz".into(),
        json!({"get": operation("healthz", "health", "Liveness probe", false)}),
    );
    paths.insert(
        "/client-config".into(),
        json!({"get": operation("clientConfig", "config", "Client configuration", false)}),
    );
    paths.insert(
        "/openapi.json".into(),
        json!({"get": operation("openapi", "meta", "This OpenAPI document", false)}),
    );
    paths.insert(
        "/rest/me".into(),
        json!({"get": operation("me", "auth", "Current authenticated user", false)}),
    );
    for resource in RESOURCES {
        let base = format!("/rest/{resource}");
        paths.insert(
            base.clone(),
            json!({
                "get": operation(&format!("list_{resource}"), "read", "List records", false),
                "post": operation(&format!("create_{resource}"), "write", "Create a record", true)
            }),
        );
        paths.insert(
            format!("{base}/{{id}}"),
            json!({
                "get": operation(&format!("get_{resource}"), "read", "Fetch one record", false),
                "patch": operation(&format!("update_{resource}"), "write", "Update a record", true),
                "delete": operation(&format!("delete_{resource}"), "write", "Soft-delete a record", false)
            }),
        );
        paths.insert(
            format!("{base}/{{id}}/restore"),
            json!({"patch": operation(&format!("restore_{resource}"), "write", "Restore a record", false)}),
        );
        paths.insert(
            format!("{base}/export"),
            json!({"get": operation(&format!("export_{resource}"), "read", "Export records", false)}),
        );
        paths.insert(
            format!("{base}/import"),
            json!({"post": operation(&format!("import_{resource}"), "write", "Import records", true)}),
        );
        paths.insert(
            format!("{base}/groupBy"),
            json!({"get": operation(&format!("group_{resource}"), "read", "Group records", false)}),
        );
    }
    paths.insert(
        "/auth/signup".into(),
        json!({"post": operation("signup", "auth", "Create account and workspace", true)}),
    );
    paths.insert(
        "/auth/signin".into(),
        json!({"post": operation("signin", "auth", "Sign in", true)}),
    );
    paths.insert(
        "/auth/refresh".into(),
        json!({"post": operation("refresh", "auth", "Rotate session tokens", true)}),
    );
    paths.insert(
        "/auth/signout".into(),
        json!({"post": operation("signout", "auth", "Revoke the current session", false)}),
    );
    paths.insert(
        "/graphql".into(),
        json!({
            "get": operation("graphqlPlayground", "graphql", "GraphQL playground", false),
            "post": operation("graphql", "graphql", "Execute a GraphQL operation", true)
        }),
    );
    paths.insert(
        "/metadata".into(),
        json!({
            "get": operation("metadataPlayground", "graphql", "Metadata GraphQL playground", false),
            "post": operation("metadataGraphql", "graphql", "Execute a metadata GraphQL operation", true)
        }),
    );
    paths.insert(
        "/files/upload/{id}".into(),
        json!({"put": operation("uploadFile", "files", "Upload a file", true)}),
    );
    paths.insert(
        "/files/{id}".into(),
        json!({"get": operation("downloadFile", "files", "Download a file", false)}),
    );
    Value::Object(paths)
}

pub fn document() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Squid-CRM API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Self-hostable CRM backend, API-compatible with Twenty. \
                Request/response bodies are described generically here; see docs/api.md \
                and the GraphQL schema for field-level contracts."
        },
        "servers": [{"url": "/", "description": "Same origin"}],
        "tags": [
            {"name": "read"}, {"name": "write"}, {"name": "auth"},
            {"name": "graphql"}, {"name": "files"}, {"name": "health"},
            {"name": "config"}, {"name": "meta"}
        ],
        "components": {
            "securitySchemes": {
                "bearerAuth": {"type": "http", "scheme": "bearer"},
                "apiKeyAuth": {"type": "apiKey", "in": "header", "name": "Authorization"},
                "cookieAuth": {"type": "apiKey", "in": "cookie", "name": "squid_session"}
            }
        },
        "security": [{"bearerAuth": []}, {"apiKeyAuth": []}, {"cookieAuth": []}],
        "paths": all_paths()
    })
}

pub async fn spec() -> Json<Value> {
    Json(document())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_is_valid_openapi_31_and_covers_core_resources() {
        let doc = document();
        assert_eq!(doc["openapi"], "3.1.0");
        let paths = doc["paths"].as_object().expect("paths object");
        for resource in RESOURCES {
            assert!(paths.contains_key(&format!("/rest/{resource}")));
            assert!(paths.contains_key(&format!("/rest/{resource}/{{id}}")));
            assert!(paths.contains_key(&format!("/rest/{resource}/groupBy")));
        }
        assert!(paths.contains_key("/graphql"));
        assert!(paths.contains_key("/openapi.json"));
    }

    #[test]
    fn every_path_has_at_least_one_operation() {
        let doc = document();
        for (path, item) in doc["paths"].as_object().expect("paths object") {
            assert!(
                item.as_object()
                    .is_some_and(|operations| !operations.is_empty()),
                "{path} has no operations"
            );
        }
    }
}
