//! REST handlers. The response envelope and error shape match the audited
//! upstream contract:
//!   list  -> { data: { people: [...] }, totalCount, pageInfo }
//!   error -> { statusCode, error, messages, code? }

use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use squid_core::{AppError, AppResult, CreatePersonInput, FindManyResponse, PageInfo, Person};
use uuid::Uuid;

use crate::{resolve_auth, AppState};

#[derive(Debug, Deserialize)]
pub struct ListPeopleParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn healthz() -> &'static str {
    "ok"
}

pub async fn list_people(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListPeopleParams>,
) -> AppResult<Json<FindManyResponse<Person>>> {
    let auth = require(&state, &headers).await?;

    let limit = params
        .limit
        .unwrap_or(20)
        .clamp(1, state.config.max_page_size);
    let offset = params.offset.unwrap_or(0).max(0);

    let people = state
        .db
        .list_persons(auth.workspace_id, limit, offset)
        .await
        .map_err(AppError::Internal)?;
    let total = state
        .db
        .count_persons(auth.workspace_id)
        .await
        .map_err(AppError::Internal)?;

    let mut data = BTreeMap::new();
    data.insert("people".to_string(), people);

    Ok(Json(FindManyResponse {
        data,
        total_count: total.max(0) as u64,
        page_info: PageInfo {
            has_next_page: Some(offset + limit < total),
            has_previous_page: Some(offset > 0),
            start_cursor: None,
            end_cursor: None,
        },
    }))
}

pub async fn get_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;

    let person = state
        .db
        .get_person(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Person not found".to_string()))?;

    let mut data = BTreeMap::new();
    data.insert("person".to_string(), person);

    Ok(Json(json!({ "data": data })))
}

pub async fn create_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreatePersonInput>,
) -> AppResult<(axum::http::StatusCode, Json<serde_json::Value>)> {
    let auth = require(&state, &headers).await?;
    validate_person(&input)?;

    let person = state
        .db
        .create_person(auth.workspace_id, &input)
        .await
        .map_err(AppError::Internal)?;

    let mut data = BTreeMap::new();
    data.insert("person".to_string(), person);

    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({ "data": data })),
    ))
}

pub async fn delete_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;

    let deleted = state
        .db
        .soft_delete_person(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?;

    if !deleted {
        return Err(AppError::NotFound("Person not found".to_string()));
    }

    Ok(Json(json!({ "data": { "id": id } })))
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;

    let user = state
        .db
        .find_user_by_id(auth.user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Authentication required".to_string()))?;
    let workspace = state
        .db
        .find_workspace(auth.workspace_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Workspace not found".to_string()))?;

    Ok(Json(json!({
        "user": user,
        "workspace": workspace,
        "role": auth.role,
    })))
}

pub(crate) async fn require(
    state: &AppState,
    headers: &HeaderMap,
) -> AppResult<squid_auth::AuthContext> {
    resolve_auth(state, headers)
        .await
        .ok_or_else(|| AppError::Unauthorized("Authentication required".to_string()))
}

fn validate_person(input: &CreatePersonInput) -> AppResult<()> {
    if input.name.first_name.trim().is_empty() && input.name.last_name.trim().is_empty() {
        return Err(AppError::BadRequest(
            "A first or last name is required".to_string(),
        ));
    }
    if input.emails.primary_email.len() > 320 {
        return Err(AppError::BadRequest("Email is too long".to_string()));
    }
    Ok(())
}
