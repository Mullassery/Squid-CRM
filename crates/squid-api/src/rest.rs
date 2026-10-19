//! REST handlers. Envelopes and error shapes follow the captured v2.49.0
//! contract (see `contracts/v2.49.0/examples/`):
//!   list   -> { data: { people: [...] }, totalCount, pageInfo }
//!   create -> { data: { createPerson: {...} } }
//!   get    -> { data: { person: {...} } }
//!   delete -> { data: { deletePerson: { id } } }
//!   error  -> { statusCode, error, messages, code? }

use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use base64::Engine;
use serde::Deserialize;
use serde_json::json;
use squid_core::{AppError, AppResult, CreatePersonInput, FindManyResponse, PageInfo, Person};
use squid_db::{OrderTerm, PersonColumn};
use uuid::Uuid;

use crate::{resolve_auth, AppState};

#[derive(Debug, Deserialize)]
pub struct ListPeopleParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub starting_after: Option<String>,
    pub ending_before: Option<String>,
    pub order_by: Option<String>,
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
    let starting_after = params
        .starting_after
        .as_deref()
        .map(decode_cursor)
        .transpose()?;
    let ending_before = params
        .ending_before
        .as_deref()
        .map(decode_cursor)
        .transpose()?;
    let order = parse_order_by(params.order_by.as_deref())?;

    let page = state
        .db
        .list_persons_page(
            auth.workspace_id,
            limit,
            offset,
            starting_after,
            ending_before,
            &order,
        )
        .await
        .map_err(AppError::Internal)?;

    let start_cursor = page.persons.first().map(|person| encode_cursor(person.id));
    let end_cursor = page.persons.last().map(|person| encode_cursor(person.id));

    let mut data = BTreeMap::new();
    data.insert("people".to_string(), page.persons);

    Ok(Json(FindManyResponse {
        data,
        total_count: page.total.max(0) as u64,
        page_info: PageInfo {
            has_next_page: page.has_next_page,
            has_previous_page: page.has_previous_page,
            start_cursor,
            end_cursor,
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
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;

    Ok(Json(json!({ "data": { "person": person } })))
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

    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({ "data": { "createPerson": person } })),
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
        return Err(AppError::NotFound("Record not found".to_string()));
    }

    Ok(Json(json!({ "data": { "deletePerson": { "id": id } } })))
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let user_id = auth
        .user_id
        .ok_or_else(|| AppError::Forbidden("API keys have no user profile".to_string()))?;

    let user = state
        .db
        .find_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Authentication required".to_string()))?;
    let workspace = state
        .db
        .find_workspace(auth.workspace_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;

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
        .ok_or_else(|| AppError::Forbidden("Missing authentication token".to_string()))
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

/// Encode a cursor exactly like upstream: base64 of `{"id":"<uuid>"}` (no pad).
pub(crate) fn encode_cursor(id: Uuid) -> String {
    base64::engine::general_purpose::STANDARD_NO_PAD.encode(format!("{{\"id\":\"{id}\"}}"))
}

pub(crate) fn decode_cursor(raw: &str) -> AppResult<Uuid> {
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(raw)
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(raw))
        .map_err(|_| AppError::BadRequest("Invalid cursor".to_string()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::BadRequest("Invalid cursor".to_string()))?;
    value
        .get("id")
        .and_then(|id| id.as_str())
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| AppError::BadRequest("Invalid cursor".to_string()))
}

/// `order_by=field[AscNullsFirst],name.firstName[DescNullsLast]`.
fn parse_order_by(raw: Option<&str>) -> AppResult<Vec<OrderTerm>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };

    let mut terms = Vec::new();
    for item in raw.split(',').filter(|item| !item.trim().is_empty()) {
        let (field, direction) = match item.split_once('[') {
            Some((field, rest)) => (field.trim(), rest.trim_end_matches(']').trim()),
            None => (item.trim(), "AscNullsFirst"),
        };
        let column = match field {
            "id" => PersonColumn::Id,
            "createdAt" => PersonColumn::CreatedAt,
            "updatedAt" => PersonColumn::UpdatedAt,
            "name.firstName" => PersonColumn::NameFirst,
            "name.lastName" => PersonColumn::NameLast,
            "emails.primaryEmail" => PersonColumn::PrimaryEmail,
            "jobTitle" => PersonColumn::JobTitle,
            _ => {
                return Err(AppError::BadRequest(format!(
                    "Field \"{field}\" does not exist or is not sortable"
                )))
            }
        };
        let (descending, nulls_first) = match direction {
            "AscNullsFirst" => (false, true),
            "AscNullsLast" => (false, false),
            "DescNullsFirst" => (true, true),
            "DescNullsLast" => (true, false),
            other => {
                return Err(AppError::BadRequest(format!(
                    "'order_by' direction '{other}' invalid. Allowed values are \
                     'AscNullsFirst', 'AscNullsLast', 'DescNullsFirst', 'DescNullsLast'."
                )))
            }
        };
        terms.push(OrderTerm {
            column,
            descending,
            nulls_first,
        });
    }
    Ok(terms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trips_and_matches_upstream_encoding() {
        let id = Uuid::parse_str("6c774d80-730f-4e23-b707-39053977cbff").unwrap();
        let cursor = encode_cursor(id);
        assert_eq!(
            cursor,
            "eyJpZCI6IjZjNzc0ZDgwLTczMGYtNGUyMy1iNzA3LTM5MDUzOTc3Y2JmZiJ9"
        );
        assert_eq!(decode_cursor(&cursor).unwrap(), id);
    }

    #[test]
    fn rejects_invalid_cursor() {
        assert!(decode_cursor("not-base64").is_err());
    }

    #[test]
    fn rejects_injected_order_by() {
        assert!(parse_order_by(Some("createdAt; DROP TABLE person")).is_err());
        assert!(parse_order_by(Some("createdAt[BadDirection]")).is_err());
        assert!(parse_order_by(Some("-createdAt")).is_err());
    }
}
