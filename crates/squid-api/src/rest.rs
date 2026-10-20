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
use squid_core::{
    AppError, AppResult, Company, CreateCompanyInput, CreateOpportunityInput, CreatePersonInput,
    DateOp, FilterExpr, FindManyResponse, Opportunity, PageInfo, Person, StringOp,
    UpdateCompanyInput, UpdateOpportunityInput, UpdatePersonInput, UuidOp,
};
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
    pub filter: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeletePersonParams {
    #[serde(default)]
    pub soft_delete: bool,
}

/// Runtime configuration consumed by the frontend at boot. Mirrors the
/// captured v2.49.0 shape; unimplemented integrations are reported as disabled.
pub async fn client_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let front_domain = state
        .config
        .frontend_url
        .split("://")
        .nth(1)
        .unwrap_or(&state.config.frontend_url)
        .split('/')
        .next()
        .unwrap_or("localhost")
        .split(':')
        .next()
        .unwrap_or("localhost")
        .to_string();

    Json(json!({
        "appVersion": "v2.49.0",
        "billing": {
            "isBillingEnabled": false,
            "trialPeriods": [
                { "duration": 30, "isCreditCardRequired": true },
                { "duration": 7, "isCreditCardRequired": false }
            ]
        },
        "aiModels": [],
        "aiEvaluationModels": [],
        "aiModelTiers": [],
        "authProviders": {
            "google": false,
            "magicLink": false,
            "password": true,
            "microsoft": false,
            "sso": []
        },
        "signInPrefilled": false,
        "isMultiWorkspaceEnabled": false,
        "isEmailVerificationRequired": false,
        "defaultSubdomain": "app",
        "frontDomain": front_domain,
        "serverUrl": "",
        "publicFunctionDomain": null,
        "support": { "supportDriver": "NONE" },
        "sentry": { "release": "v2.49.0", "tracesSampleRate": 0.1 },
        "captcha": {},
        "api": { "mutationMaximumAffectedRecords": 100 },
        "onboarding": null,
        "isAttachmentPreviewEnabled": true,
        "analyticsEnabled": false,
        "canManageFeatureFlags": false,
        "publicFeatureFlags": [],
        "isCookieSessionEnabled": true,
        "isMicrosoftMessagingEnabled": false,
        "isMicrosoftCalendarEnabled": false,
        "isGoogleMessagingEnabled": false,
        "isGoogleCalendarEnabled": false,
        "isConfigVariablesInDbEnabled": true,
        "isImapSmtpCaldavEnabled": true,
        "isEmailingDomainInDemoMode": true,
        "allowRequestsToTwentyIcons": true,
        "isBookCallOnboardingStepEnabled": false,
        "isCompanyEnrichmentEnabled": false,
        "isCloudflareIntegrationEnabled": false,
        "isClickHouseConfigured": false,
        "isWorkspaceSchemaDDLLocked": false,
        "isOnboardingAiChatEnabled": false,
        "enterpriseInstanceType": "production"
    }))
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
    let filter = params.filter.as_deref().map(parse_filter).transpose()?;

    let page = state
        .db
        .list_persons_page(
            auth.workspace_id,
            limit,
            offset,
            starting_after,
            ending_before,
            &order,
            filter.as_ref(),
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

pub async fn update_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdatePersonInput>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;

    let person = state
        .db
        .update_person(auth.workspace_id, id, &input)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;

    Ok(Json(json!({ "data": { "updatePerson": person } })))
}

pub async fn delete_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(params): Query<DeletePersonParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;

    let deleted = if params.soft_delete {
        state
            .db
            .soft_delete_person(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    } else {
        state
            .db
            .destroy_person(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    };

    if !deleted {
        return Err(AppError::NotFound("Record not found".to_string()));
    }

    Ok(Json(json!({ "data": { "deletePerson": { "id": id } } })))
}

pub async fn restore_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;

    let person = state
        .db
        .restore_person(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;

    Ok(Json(json!({ "data": { "restorePerson": person } })))
}

#[derive(Debug, Deserialize)]
pub struct ListCompaniesParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub starting_after: Option<String>,
    pub ending_before: Option<String>,
    pub order_by: Option<String>,
    pub filter: Option<String>,
}

pub async fn list_companies(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListCompaniesParams>,
) -> AppResult<Json<FindManyResponse<Company>>> {
    let auth = require(&state, &headers).await?;
    let _ = (&params.order_by, &params.filter);

    let limit = params
        .limit
        .unwrap_or(20)
        .clamp(1, state.config.max_page_size);
    let mut offset = params.offset.unwrap_or(0).max(0);
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

    let (mut companies, total) = state
        .db
        .list_companies(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;

    if let Some(after) = starting_after {
        if let Some(index) = companies.iter().position(|company| company.id == after) {
            companies.drain(..=index);
            offset = 0;
        }
    }
    if let Some(before) = ending_before {
        if let Some(index) = companies.iter().position(|company| company.id == before) {
            companies.truncate(index);
        }
    }

    let start = (offset as usize).min(companies.len());
    let end = (start + limit as usize).min(companies.len());
    let has_next_page = end < companies.len();
    let has_previous_page = start > 0 || starting_after.is_some();
    let page: Vec<Company> = companies[start..end].to_vec();

    let start_cursor = page.first().map(|company| encode_cursor(company.id));
    let end_cursor = page.last().map(|company| encode_cursor(company.id));

    let mut data = BTreeMap::new();
    data.insert("companies".to_string(), page);

    Ok(Json(FindManyResponse {
        data,
        total_count: total.max(0) as u64,
        page_info: PageInfo {
            has_next_page,
            has_previous_page,
            start_cursor,
            end_cursor,
        },
    }))
}

pub async fn get_company(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let company = state
        .db
        .get_company(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    Ok(Json(json!({ "data": { "company": company } })))
}

pub async fn create_company(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateCompanyInput>,
) -> AppResult<(axum::http::StatusCode, Json<serde_json::Value>)> {
    let auth = require(&state, &headers).await?;
    let company = state
        .db
        .create_company(auth.workspace_id, &input)
        .await
        .map_err(AppError::Internal)?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({ "data": { "createCompany": company } })),
    ))
}

pub async fn update_company(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateCompanyInput>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let company = state
        .db
        .update_company(auth.workspace_id, id, &input)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    Ok(Json(json!({ "data": { "updateCompany": company } })))
}

pub async fn delete_company(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(params): Query<DeletePersonParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let deleted = if params.soft_delete {
        state
            .db
            .soft_delete_company(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    } else {
        state
            .db
            .destroy_company(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    };
    if !deleted {
        return Err(AppError::NotFound("Record not found".to_string()));
    }
    Ok(Json(json!({ "data": { "deleteCompany": { "id": id } } })))
}

pub async fn restore_company(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let company = state
        .db
        .restore_company(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    Ok(Json(json!({ "data": { "restoreCompany": company } })))
}

#[derive(Debug, Deserialize)]
pub struct ListOpportunitiesParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub starting_after: Option<String>,
    pub ending_before: Option<String>,
    pub order_by: Option<String>,
    pub filter: Option<String>,
}

pub async fn list_opportunities(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListOpportunitiesParams>,
) -> AppResult<Json<FindManyResponse<Opportunity>>> {
    let auth = require(&state, &headers).await?;
    let _ = (&params.order_by, &params.filter);

    let limit = params
        .limit
        .unwrap_or(20)
        .clamp(1, state.config.max_page_size);
    let mut offset = params.offset.unwrap_or(0).max(0);
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

    let (mut opportunities, total) = state
        .db
        .list_opportunities(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;

    if let Some(after) = starting_after {
        if let Some(index) = opportunities.iter().position(|row| row.id == after) {
            opportunities.drain(..=index);
            offset = 0;
        }
    }
    if let Some(before) = ending_before {
        if let Some(index) = opportunities.iter().position(|row| row.id == before) {
            opportunities.truncate(index);
        }
    }

    let start = (offset as usize).min(opportunities.len());
    let end = (start + limit as usize).min(opportunities.len());
    let has_next_page = end < opportunities.len();
    let has_previous_page = start > 0 || starting_after.is_some();
    let page: Vec<Opportunity> = opportunities[start..end].to_vec();

    let start_cursor = page.first().map(|row| encode_cursor(row.id));
    let end_cursor = page.last().map(|row| encode_cursor(row.id));

    let mut data = BTreeMap::new();
    data.insert("opportunities".to_string(), page);

    Ok(Json(FindManyResponse {
        data,
        total_count: total.max(0) as u64,
        page_info: PageInfo {
            has_next_page,
            has_previous_page,
            start_cursor,
            end_cursor,
        },
    }))
}

pub async fn get_opportunity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let opportunity = state
        .db
        .get_opportunity(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    Ok(Json(json!({ "data": { "opportunity": opportunity } })))
}

pub async fn create_opportunity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateOpportunityInput>,
) -> AppResult<(axum::http::StatusCode, Json<serde_json::Value>)> {
    let auth = require(&state, &headers).await?;
    let opportunity = state
        .db
        .create_opportunity(auth.workspace_id, &input)
        .await
        .map_err(AppError::Internal)?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({ "data": { "createOpportunity": opportunity } })),
    ))
}

pub async fn update_opportunity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateOpportunityInput>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let opportunity = state
        .db
        .update_opportunity(auth.workspace_id, id, &input)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    Ok(Json(
        json!({ "data": { "updateOpportunity": opportunity } }),
    ))
}

pub async fn delete_opportunity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(params): Query<DeletePersonParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let deleted = if params.soft_delete {
        state
            .db
            .soft_delete_opportunity(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    } else {
        state
            .db
            .destroy_opportunity(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    };
    if !deleted {
        return Err(AppError::NotFound("Record not found".to_string()));
    }
    Ok(Json(
        json!({ "data": { "deleteOpportunity": { "id": id } } }),
    ))
}

pub async fn restore_opportunity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let opportunity = state
        .db
        .restore_opportunity(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    Ok(Json(
        json!({ "data": { "restoreOpportunity": opportunity } }),
    ))
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

/// Parse the REST filter mini-language, e.g.
/// `createdAt[gte]:"2024-01-01",name.firstName[startsWith]:A`.
fn parse_filter(raw: &str) -> AppResult<FilterExpr> {
    let mut parser = FilterParser::new(raw);
    let expr = parser.parse_list(')')?;
    parser.skip_whitespace();
    if !parser.at_end() {
        return Err(AppError::BadRequest(format!(
            "Invalid filter near '{}'",
            parser.remaining()
        )));
    }
    Ok(expr)
}

struct FilterParser {
    chars: Vec<char>,
    pos: usize,
}

impl FilterParser {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let value = self.peek();
        if value.is_some() {
            self.pos += 1;
        }
        value
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn remaining(&self) -> String {
        self.chars[self.pos..].iter().collect()
    }

    fn expect(&mut self, expected: char) -> AppResult<()> {
        self.skip_whitespace();
        if self.bump() == Some(expected) {
            Ok(())
        } else {
            Err(AppError::BadRequest(format!(
                "Expected '{expected}' in filter near '{}'",
                self.remaining()
            )))
        }
    }

    fn read_ident(&mut self) -> String {
        self.skip_whitespace();
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn parse_list(&mut self, terminator: char) -> AppResult<FilterExpr> {
        let mut items = vec![self.parse_expr()?];
        loop {
            self.skip_whitespace();
            if self.peek() == Some(',') {
                self.pos += 1;
                items.push(self.parse_expr()?);
            } else if self.peek() == Some(terminator) || self.at_end() {
                break;
            } else {
                return Err(AppError::BadRequest(format!(
                    "Unexpected token in filter near '{}'",
                    self.remaining()
                )));
            }
        }
        Ok(if items.len() == 1 {
            items.pop().unwrap()
        } else {
            FilterExpr::And(items)
        })
    }

    fn parse_expr(&mut self) -> AppResult<FilterExpr> {
        self.skip_whitespace();
        let ident = self.read_ident();
        if ident == "or" && self.peek() == Some('(') {
            self.pos += 1;
            let mut items = Vec::new();
            loop {
                items.push(self.parse_expr()?);
                self.skip_whitespace();
                match self.bump() {
                    Some(',') => continue,
                    Some(')') => break,
                    _ => {
                        return Err(AppError::BadRequest(
                            "Unbalanced or() in filter".to_string(),
                        ))
                    }
                }
            }
            return Ok(FilterExpr::Or(items));
        }
        if ident == "not" && self.peek() == Some('(') {
            self.pos += 1;
            let expr = self.parse_expr()?;
            self.expect(')')?;
            return Ok(FilterExpr::Not(Box::new(expr)));
        }
        if ident.is_empty() {
            return Err(AppError::BadRequest(format!(
                "Expected a field name in filter near '{}'",
                self.remaining()
            )));
        }

        self.expect('[')?;
        let start = self.pos;
        while self.peek().is_some_and(|c| c != ']') {
            self.pos += 1;
        }
        let operator: String = self.chars[start..self.pos].iter().collect();
        self.expect(']')?;
        self.expect(':')?;
        let values = self.read_values()?;

        build_filter_leaf(&ident, operator.trim(), &values)
    }

    fn read_values(&mut self) -> AppResult<Vec<String>> {
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.peek() == Some('(') || self.peek() == Some('[') {
            let close = if self.bump() == Some('(') { ')' } else { ']' };
            loop {
                values.push(self.read_value()?);
                self.skip_whitespace();
                match self.bump() {
                    Some(',') => continue,
                    Some(c) if c == close => break,
                    _ => {
                        return Err(AppError::BadRequest(
                            "Unbalanced value list in filter".to_string(),
                        ))
                    }
                }
            }
        } else {
            values.push(self.read_value()?);
        }
        Ok(values)
    }

    fn read_value(&mut self) -> AppResult<String> {
        self.skip_whitespace();
        match self.bump() {
            Some('"') | Some('\'') => {
                let quote = self.chars[self.pos - 1];
                let start = self.pos;
                while self.peek().is_some_and(|c| c != quote) {
                    self.pos += 1;
                }
                let value: String = self.chars[start..self.pos].iter().collect();
                self.expect(quote)?;
                Ok(value)
            }
            Some(first) => {
                let mut value = String::from(first);
                while self
                    .peek()
                    .is_some_and(|c| c != ',' && c != ')' && c != ']')
                {
                    value.push(self.chars[self.pos]);
                    self.pos += 1;
                }
                Ok(value.trim().to_string())
            }
            None => Err(AppError::BadRequest("Missing filter value".to_string())),
        }
    }
}

fn build_filter_leaf(field: &str, operator: &str, values: &[String]) -> AppResult<FilterExpr> {
    let value = values.first().cloned().unwrap_or_default();
    let is_null = match operator {
        "is" => Some(match value.to_ascii_uppercase().as_str() {
            "NULL" => true,
            "NOT_NULL" => false,
            _ => {
                return Err(AppError::BadRequest(format!(
                    "Invalid filter value '{value}' for is (expected NULL or NOT_NULL)"
                )))
            }
        }),
        _ => None,
    };

    macro_rules! string_op {
        ($variant:ident, $expr:ident) => {
            FilterExpr::$expr(StringOp::$variant(value.clone()))
        };
    }

    match (field, operator) {
        ("id", "eq") => Ok(FilterExpr::Id(UuidOp::Eq(parse_uuid(&value)?))),
        ("id", "neq") => Ok(FilterExpr::Id(UuidOp::Neq(parse_uuid(&value)?))),
        ("id", "in") => Ok(FilterExpr::Id(UuidOp::In(
            values
                .iter()
                .map(|value| parse_uuid(value))
                .collect::<AppResult<_>>()?,
        ))),
        ("id", "is") => Ok(FilterExpr::Id(UuidOp::IsNull(
            is_null.expect("is operator"),
        ))),
        ("createdAt", op) => Ok(FilterExpr::CreatedAt(build_date_op(op, values, is_null)?)),
        ("updatedAt", op) => Ok(FilterExpr::UpdatedAt(build_date_op(op, values, is_null)?)),
        ("name.firstName", "eq") => Ok(string_op!(Eq, FirstName)),
        ("name.firstName", "neq") => Ok(string_op!(Neq, FirstName)),
        ("name.firstName", "contains") => {
            Ok(FilterExpr::FirstName(StringOp::Like(format!("%{value}%"))))
        }
        ("name.firstName", "startsWith") => Ok(string_op!(StartsWith, FirstName)),
        ("name.firstName", "endsWith") => Ok(string_op!(EndsWith, FirstName)),
        ("name.firstName", "like") => Ok(string_op!(Like, FirstName)),
        ("name.firstName", "ilike") => Ok(string_op!(ILike, FirstName)),
        ("name.firstName", "is") => Ok(FilterExpr::FirstName(StringOp::IsNull(
            is_null.expect("is operator"),
        ))),
        ("name.lastName", "eq") => Ok(string_op!(Eq, LastName)),
        ("name.lastName", "neq") => Ok(string_op!(Neq, LastName)),
        ("name.lastName", "contains") => {
            Ok(FilterExpr::LastName(StringOp::Like(format!("%{value}%"))))
        }
        ("name.lastName", "startsWith") => Ok(string_op!(StartsWith, LastName)),
        ("name.lastName", "endsWith") => Ok(string_op!(EndsWith, LastName)),
        ("name.lastName", "like") => Ok(string_op!(Like, LastName)),
        ("name.lastName", "ilike") => Ok(string_op!(ILike, LastName)),
        ("name.lastName", "is") => Ok(FilterExpr::LastName(StringOp::IsNull(
            is_null.expect("is operator"),
        ))),
        ("emails.primaryEmail", "eq") => Ok(string_op!(Eq, PrimaryEmail)),
        ("emails.primaryEmail", "neq") => Ok(string_op!(Neq, PrimaryEmail)),
        ("emails.primaryEmail", "contains") => Ok(FilterExpr::PrimaryEmail(StringOp::Like(
            format!("%{value}%"),
        ))),
        ("emails.primaryEmail", "startsWith") => Ok(string_op!(StartsWith, PrimaryEmail)),
        ("emails.primaryEmail", "endsWith") => Ok(string_op!(EndsWith, PrimaryEmail)),
        ("emails.primaryEmail", "like") => Ok(string_op!(Like, PrimaryEmail)),
        ("emails.primaryEmail", "ilike") => Ok(string_op!(ILike, PrimaryEmail)),
        ("emails.primaryEmail", "is") => Ok(FilterExpr::PrimaryEmail(StringOp::IsNull(
            is_null.expect("is operator"),
        ))),
        ("jobTitle", "eq") => Ok(string_op!(Eq, JobTitle)),
        ("jobTitle", "neq") => Ok(string_op!(Neq, JobTitle)),
        ("jobTitle", "contains") => Ok(FilterExpr::JobTitle(StringOp::Like(format!("%{value}%")))),
        ("jobTitle", "startsWith") => Ok(string_op!(StartsWith, JobTitle)),
        ("jobTitle", "endsWith") => Ok(string_op!(EndsWith, JobTitle)),
        ("jobTitle", "like") => Ok(string_op!(Like, JobTitle)),
        ("jobTitle", "ilike") => Ok(string_op!(ILike, JobTitle)),
        ("jobTitle", "is") => Ok(FilterExpr::JobTitle(StringOp::IsNull(
            is_null.expect("is operator"),
        ))),
        _ => Err(AppError::BadRequest(format!(
            "Unsupported filter: {field}[{operator}]"
        ))),
    }
}

fn build_date_op(operator: &str, values: &[String], is_null: Option<bool>) -> AppResult<DateOp> {
    let value = values.first().map(String::as_str).unwrap_or_default();
    match operator {
        "eq" => Ok(DateOp::Eq(parse_datetime(value)?)),
        "neq" => Ok(DateOp::Neq(parse_datetime(value)?)),
        "gt" => Ok(DateOp::Gt(parse_datetime(value)?)),
        "gte" => Ok(DateOp::Gte(parse_datetime(value)?)),
        "lt" => Ok(DateOp::Lt(parse_datetime(value)?)),
        "lte" => Ok(DateOp::Lte(parse_datetime(value)?)),
        "in" => Ok(DateOp::In(
            values
                .iter()
                .map(|value| parse_datetime(value))
                .collect::<AppResult<_>>()?,
        )),
        "is" => Ok(DateOp::IsNull(is_null.expect("is operator"))),
        _ => Err(AppError::BadRequest(format!(
            "Unsupported date comparator '{operator}'"
        ))),
    }
}

fn parse_uuid(raw: &str) -> AppResult<Uuid> {
    Uuid::parse_str(raw.trim()).map_err(|_| AppError::BadRequest(format!("Invalid UUID '{raw}'")))
}

fn parse_datetime(raw: &str) -> AppResult<chrono::DateTime<chrono::Utc>> {
    if let Ok(value) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Ok(value.with_timezone(&chrono::Utc));
    }
    for format in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d"] {
        if let Ok(value) = chrono::NaiveDateTime::parse_from_str(raw, format) {
            return Ok(value.and_utc());
        }
        if let Ok(value) = chrono::NaiveDate::parse_from_str(raw, format) {
            if let Some(value) = value.and_hms_opt(0, 0, 0) {
                return Ok(value.and_utc());
            }
        }
    }
    Err(AppError::BadRequest(format!("Invalid date value '{raw}'")))
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
    fn parses_rest_filter_language() {
        assert_eq!(
            parse_filter("jobTitle[eq]:Engineer").unwrap(),
            FilterExpr::JobTitle(StringOp::Eq("Engineer".to_string()))
        );
        assert_eq!(
            parse_filter("createdAt[gte]:\"2024-01-01\"").unwrap(),
            FilterExpr::CreatedAt(DateOp::Gte(
                chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc)
            ))
        );
        assert!(matches!(
            parse_filter("or(name.firstName[eq]:Ada,name.firstName[eq]:Alan)").unwrap(),
            FilterExpr::Or(items) if items.len() == 2
        ));
        assert!(matches!(
            parse_filter("not(id[is]:NULL)").unwrap(),
            FilterExpr::Not(_)
        ));
        assert!(parse_filter("jobTitle[is]:MAYBE").is_err());
        assert!(parse_filter("bogus[eq]:x").is_err());
        assert!(parse_filter("name.firstName[eq]").is_err());
    }

    #[test]
    fn rejects_injected_order_by() {
        assert!(parse_order_by(Some("createdAt; DROP TABLE person")).is_err());
        assert!(parse_order_by(Some("createdAt[BadDirection]")).is_err());
        assert!(parse_order_by(Some("-createdAt")).is_err());
    }
}
