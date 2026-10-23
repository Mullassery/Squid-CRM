//! REST handlers. Envelopes and error shapes follow the captured v2.49.0
//! contract (see `contracts/v2.49.0/examples/`):
//!   list   -> { data: { people: [...] }, totalCount, pageInfo }
//!   create -> { data: { createPerson: {...} } }
//!   get    -> { data: { person: {...} } }
//!   delete -> { data: { deletePerson: { id } } }
//!   error  -> { statusCode, error, messages, code? }

use std::collections::{BTreeMap, HashMap};

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::json;
use sha2::Sha256;
use squid_core::{
    AppError, AppResult, Company, CreateCompanyInput, CreateNoteInput, CreateNoteTargetInput,
    CreateOpportunityInput, CreatePersonInput, CreateTaskInput, CreateTaskTargetInput, DateOp,
    FilterExpr, FindManyResponse, Note, NoteTarget, Opportunity, PageInfo, StringOp, Task,
    TaskTarget, UpdateCompanyInput, UpdateNoteInput, UpdateNoteTargetInput, UpdateOpportunityInput,
    UpdatePersonInput, UpdateTaskInput, UpdateTaskTargetInput, UuidOp,
};
use squid_db::{OrderTerm, PersonColumn};
use uuid::Uuid;

use crate::filtering::{self, parse_generic_filter};
use crate::graphql::{
    sort_companies, sort_note_targets, sort_notes, sort_task_targets, sort_tasks,
};
use crate::sorting::{
    company_value, note_target_value, note_value, opportunity_value, person_value,
    task_target_value, task_value, SortTerm, SortValue,
};
use crate::{resolve_auth_with_policy, AppState};

#[derive(Debug, Default, Deserialize)]
pub struct ResponseShape {
    pub depth: Option<i32>,
    pub fields: Option<String>,
}

impl ResponseShape {
    fn validate(&self) -> AppResult<()> {
        if !matches!(self.depth, None | Some(0) | Some(1)) {
            return Err(AppError::BadRequest(format!(
                "'depth={}' parameter invalid. Allowed values are 0, 1",
                self.depth.unwrap_or_default()
            )));
        }
        Ok(())
    }

    fn project(
        &self,
        value: &mut serde_json::Value,
        object_name: &str,
        allowed: &[&str],
    ) -> AppResult<()> {
        let Some(fields) = self.fields.as_deref() else {
            return Ok(());
        };
        let requested: Vec<&str> = fields
            .split(',')
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .collect();
        let unknown: Vec<&str> = requested
            .iter()
            .copied()
            .filter(|field| !allowed.contains(field))
            .collect();
        if !unknown.is_empty() {
            return Err(AppError::BadRequest(format!(
                "'fields' parameter invalid. Unknown or unreadable fields on '{object_name}': {}",
                unknown.join(", ")
            )));
        }
        let Some(object) = value.as_object_mut() else {
            return Ok(());
        };
        object.retain(|key, _| key == "id" || requested.contains(&key.as_str()));
        Ok(())
    }

    fn project_array(
        &self,
        value: &mut serde_json::Value,
        object_name: &str,
        allowed: &[&str],
    ) -> AppResult<()> {
        if let Some(items) = value.as_array_mut() {
            for item in items {
                self.project(item, object_name, allowed)?;
            }
        }
        Ok(())
    }
}

const PERSON_FIELDS: &[&str] = &[
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
];
const COMPANY_FIELDS: &[&str] = &[
    "id",
    "createdAt",
    "updatedAt",
    "deletedAt",
    "name",
    "domainName",
    "address",
    "linkedinLink",
    "annualRevenue",
    "position",
    "createdBy",
    "updatedBy",
    "searchVector",
    "accountOwnerId",
];
const OPPORTUNITY_FIELDS: &[&str] = &[
    "id",
    "createdAt",
    "updatedAt",
    "deletedAt",
    "name",
    "amount",
    "closeDate",
    "stage",
    "position",
    "createdBy",
    "updatedBy",
    "searchVector",
    "companyId",
    "pointOfContactId",
    "ownerId",
];
const TASK_FIELDS: &[&str] = &[
    "id",
    "createdAt",
    "updatedAt",
    "deletedAt",
    "position",
    "title",
    "bodyV2",
    "dueAt",
    "status",
    "createdBy",
    "updatedBy",
    "searchVector",
    "assigneeId",
];
const NOTE_FIELDS: &[&str] = &[
    "id",
    "createdAt",
    "updatedAt",
    "deletedAt",
    "position",
    "title",
    "bodyV2",
    "createdBy",
    "updatedBy",
    "searchVector",
];

#[derive(Debug, Deserialize)]
pub struct ListPeopleParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub starting_after: Option<String>,
    pub ending_before: Option<String>,
    pub order_by: Option<String>,
    pub filter: Option<String>,
    #[serde(flatten)]
    pub shape: ResponseShape,
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

fn map_person_error(error: anyhow::Error) -> AppError {
    let message = error.to_string();
    if message.starts_with("File not found:") {
        AppError::BadRequest(message)
    } else {
        AppError::Internal(error)
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct FileTokenParams {
    pub expires: Option<i64>,
    pub token: Option<String>,
}

fn file_signature(secret: &str, action: &str, id: Uuid, expires: i64) -> String {
    squid_auth::hash_token(&format!("{secret}:{action}:{id}:{expires}"))
}

fn validate_file_token(
    state: &AppState,
    id: Uuid,
    action: &str,
    params: &FileTokenParams,
) -> AppResult<()> {
    let (Some(expires), Some(token)) = (params.expires, params.token.as_deref()) else {
        return Err(AppError::Forbidden(
            "Missing or invalid file token".to_string(),
        ));
    };
    if expires < chrono::Utc::now().timestamp() {
        return Err(AppError::Forbidden("File token expired".to_string()));
    }
    let expected = file_signature(&state.config.jwt_secret, action, id, expires);
    if token != expected {
        return Err(AppError::Forbidden(
            "Missing or invalid file token".to_string(),
        ));
    }
    Ok(())
}

pub async fn upload_file(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(params): Query<FileTokenParams>,
    body: axum::body::Bytes,
) -> AppResult<axum::http::StatusCode> {
    validate_file_token(&state, id, "upload", &params)?;
    if let Some(storage) = &state.storage {
        let file = state
            .db
            .file_content(id)
            .await
            .map_err(AppError::Internal)?
            .map(|(file, _)| file)
            .ok_or_else(|| AppError::NotFound("File not found".to_string()))?;
        storage
            .put(&file.path, &body, &file.content_type)
            .await
            .map_err(|error| AppError::Internal(anyhow::anyhow!(error)))?;
        state
            .db
            .mark_file_uploaded(id, body.len() as i64)
            .await
            .map_err(AppError::Internal)?;
        return Ok(axum::http::StatusCode::NO_CONTENT);
    }
    let stored = state
        .db
        .store_file_content(id, &body)
        .await
        .map_err(AppError::Internal)?;
    if stored {
        Ok(axum::http::StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound("File not found".to_string()))
    }
}

pub async fn download_file(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(params): Query<FileTokenParams>,
) -> AppResult<axum::response::Response> {
    use axum::response::IntoResponse;

    validate_file_token(&state, id, "download", &params)?;

    let Some((file, mut content)) = state
        .db
        .file_content(id)
        .await
        .map_err(AppError::Internal)?
    else {
        return Err(AppError::NotFound("File not found".to_string()));
    };
    if let Some(storage) = &state.storage {
        content = storage
            .get(&file.path)
            .await
            .map_err(|error| AppError::Internal(anyhow::anyhow!(error)))?;
    }
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, file.content_type.clone()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{}\"", file.filename),
            ),
        ],
        content,
    )
        .into_response())
}

pub async fn list_people(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListPeopleParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    params.shape.validate()?;

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

    let mut body = serde_json::to_value(FindManyResponse {
        data,
        total_count: page.total.max(0) as u64,
        page_info: PageInfo {
            has_next_page: page.has_next_page,
            has_previous_page: page.has_previous_page,
            start_cursor,
            end_cursor,
        },
    })
    .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    params
        .shape
        .project_array(&mut body["data"]["people"], "person", PERSON_FIELDS)?;
    Ok(Json(body))
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn csv_response(rows: Vec<Vec<String>>) -> AppResult<axum::response::Response> {
    use axum::response::IntoResponse;
    let body = rows
        .into_iter()
        .map(|row| {
            row.iter()
                .map(|value| csv_escape(value))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"export.csv\"",
            ),
        ],
        body,
    )
        .into_response())
}

pub async fn export_people(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<axum::response::Response> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let page = state
        .db
        .list_persons_page(auth.workspace_id, 10_000, 0, None, None, &[], None)
        .await
        .map_err(AppError::Internal)?;
    let mut rows = vec![
        "id,createdAt,updatedAt,firstName,lastName,primaryEmail,jobTitle,companyId"
            .split(',')
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
    ];
    for person in page.persons {
        rows.push(vec![
            person.id.to_string(),
            person.created_at.to_rfc3339(),
            person.updated_at.to_rfc3339(),
            person.name.first_name,
            person.name.last_name,
            person.emails.primary_email,
            person.job_title.unwrap_or_default(),
            person
                .company_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
        ]);
    }
    csv_response(rows)
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(current.clone());
                current.clear();
            }
            other => current.push(other),
        }
    }
    fields.push(current);
    fields
}

pub async fn import_people(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    let text = String::from_utf8(body.to_vec())
        .map_err(|_| AppError::BadRequest("CSV must be UTF-8".to_string()))?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| AppError::BadRequest("CSV header is required".to_string()))?;
    let columns = parse_csv_line(header)
        .into_iter()
        .map(|column| column.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    let index = |name: &str| columns.iter().position(|column| column == name);
    let first = index("firstname");
    let last = index("lastname");
    let email = index("primaryemail").or_else(|| index("email"));
    let job = index("jobtitle");
    let company = index("companyid");
    if first.is_none() || email.is_none() {
        return Err(AppError::BadRequest(
            "CSV must include firstName and primaryEmail columns".to_string(),
        ));
    }
    let mut imported = 0i64;
    for line in lines {
        let values = parse_csv_line(line);
        let value = |position: Option<usize>| {
            position
                .and_then(|position| values.get(position))
                .cloned()
                .unwrap_or_default()
        };
        let input = CreatePersonInput {
            name: squid_core::FullName {
                first_name: value(first),
                last_name: value(last),
            },
            emails: squid_core::Emails {
                primary_email: value(email),
                additional_emails: Vec::new(),
            },
            job_title: job
                .and_then(|position| values.get(position))
                .filter(|value| !value.is_empty())
                .cloned(),
            company_id: company
                .and_then(|position| values.get(position))
                .and_then(|value| Uuid::parse_str(value).ok()),
            ..Default::default()
        };
        if input.name.first_name.is_empty() && input.name.last_name.is_empty() {
            continue;
        }
        state
            .db
            .create_person(auth.workspace_id, &input)
            .await
            .map_err(AppError::Internal)?;
        imported += 1;
    }
    Ok(Json(json!({ "imported": imported })))
}

fn csv_import_rows(body: &[u8]) -> AppResult<(Vec<String>, Vec<Vec<String>>)> {
    let text = String::from_utf8(body.to_vec())
        .map_err(|_| AppError::BadRequest("CSV must be UTF-8".to_string()))?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| AppError::BadRequest("CSV header is required".to_string()))?;
    let columns = parse_csv_line(header)
        .into_iter()
        .map(|column| column.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    let rows = lines.map(parse_csv_line).collect();
    Ok((columns, rows))
}

fn csv_value(columns: &[String], values: &[String], name: &str) -> Option<String> {
    columns
        .iter()
        .position(|column| column == name)
        .and_then(|position| values.get(position))
        .filter(|value| !value.is_empty())
        .cloned()
}

pub async fn import_companies(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    let (columns, rows) = csv_import_rows(&body)?;
    let mut imported = 0i64;
    for values in rows {
        let input = CreateCompanyInput {
            name: csv_value(&columns, &values, "name"),
            account_owner_id: csv_value(&columns, &values, "accountownerid")
                .and_then(|value| Uuid::parse_str(&value).ok()),
            ..Default::default()
        };
        if input.name.as_deref().unwrap_or_default().is_empty() {
            continue;
        }
        state
            .db
            .create_company(auth.workspace_id, &input)
            .await
            .map_err(AppError::Internal)?;
        imported += 1;
    }
    Ok(Json(json!({ "imported": imported })))
}

pub async fn import_opportunities(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    let (columns, rows) = csv_import_rows(&body)?;
    let mut imported = 0i64;
    for values in rows {
        let input = CreateOpportunityInput {
            name: csv_value(&columns, &values, "name"),
            stage: csv_value(&columns, &values, "stage")
                .and_then(|value| squid_core::OpportunityStage::parse(&value)),
            close_date: csv_value(&columns, &values, "closedate")
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
                .map(|value| value.with_timezone(&chrono::Utc)),
            company_id: csv_value(&columns, &values, "companyid")
                .and_then(|value| Uuid::parse_str(&value).ok()),
            point_of_contact_id: csv_value(&columns, &values, "pointofcontactid")
                .and_then(|value| Uuid::parse_str(&value).ok()),
            owner_id: csv_value(&columns, &values, "ownerid")
                .and_then(|value| Uuid::parse_str(&value).ok()),
            ..Default::default()
        };
        if input.name.as_deref().unwrap_or_default().is_empty() {
            continue;
        }
        state
            .db
            .create_opportunity(auth.workspace_id, &input)
            .await
            .map_err(AppError::Internal)?;
        imported += 1;
    }
    Ok(Json(json!({ "imported": imported })))
}

pub async fn import_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    let (columns, rows) = csv_import_rows(&body)?;
    let mut imported = 0i64;
    for values in rows {
        let input = CreateTaskInput {
            title: csv_value(&columns, &values, "title"),
            status: csv_value(&columns, &values, "status")
                .and_then(|value| squid_core::TaskStatus::parse(&value)),
            due_at: csv_value(&columns, &values, "dueat")
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
                .map(|value| value.with_timezone(&chrono::Utc)),
            assignee_id: csv_value(&columns, &values, "assigneeid")
                .and_then(|value| Uuid::parse_str(&value).ok()),
            position: None,
            body_v2: None,
        };
        if input.title.as_deref().unwrap_or_default().is_empty() {
            continue;
        }
        state
            .db
            .create_task(auth.workspace_id, &input)
            .await
            .map_err(AppError::Internal)?;
        imported += 1;
    }
    Ok(Json(json!({ "imported": imported })))
}

pub async fn import_notes(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    let (columns, rows) = csv_import_rows(&body)?;
    let mut imported = 0i64;
    for values in rows {
        let input = CreateNoteInput {
            title: csv_value(&columns, &values, "title"),
            position: None,
            body_v2: None,
        };
        if input.title.as_deref().unwrap_or_default().is_empty() {
            continue;
        }
        state
            .db
            .create_note(auth.workspace_id, &input)
            .await
            .map_err(AppError::Internal)?;
        imported += 1;
    }
    Ok(Json(json!({ "imported": imported })))
}

pub async fn export_companies(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<axum::response::Response> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let (companies, _) = state
        .db
        .list_companies(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;
    let mut rows = vec![vec![
        "id".to_string(),
        "createdAt".to_string(),
        "updatedAt".to_string(),
        "name".to_string(),
        "accountOwnerId".to_string(),
    ]];
    for company in companies {
        rows.push(vec![
            company.id.to_string(),
            company.created_at.to_rfc3339(),
            company.updated_at.to_rfc3339(),
            company.name.unwrap_or_default(),
            company
                .account_owner_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
        ]);
    }
    csv_response(rows)
}

pub async fn export_opportunities(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<axum::response::Response> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let (opportunities, _) = state
        .db
        .list_opportunities(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;
    let mut rows = vec![vec![
        "id".to_string(),
        "createdAt".to_string(),
        "updatedAt".to_string(),
        "name".to_string(),
        "stage".to_string(),
        "closeDate".to_string(),
        "companyId".to_string(),
    ]];
    for opportunity in opportunities {
        rows.push(vec![
            opportunity.id.to_string(),
            opportunity.created_at.to_rfc3339(),
            opportunity.updated_at.to_rfc3339(),
            opportunity.name.unwrap_or_default(),
            opportunity.stage.as_str().to_string(),
            opportunity
                .close_date
                .map(|date| date.to_rfc3339())
                .unwrap_or_default(),
            opportunity
                .company_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
        ]);
    }
    csv_response(rows)
}

pub async fn export_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<axum::response::Response> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let (tasks, _) = state
        .db
        .list_tasks(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;
    let mut rows = vec![vec![
        "id".to_string(),
        "createdAt".to_string(),
        "updatedAt".to_string(),
        "title".to_string(),
        "status".to_string(),
        "dueAt".to_string(),
        "assigneeId".to_string(),
    ]];
    for task in tasks {
        rows.push(vec![
            task.id.to_string(),
            task.created_at.to_rfc3339(),
            task.updated_at.to_rfc3339(),
            task.title.unwrap_or_default(),
            task.status
                .map(|status| status.as_str().to_string())
                .unwrap_or_default(),
            task.due_at
                .map(|date| date.to_rfc3339())
                .unwrap_or_default(),
            task.assignee_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
        ]);
    }
    csv_response(rows)
}

pub async fn export_notes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<axum::response::Response> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let (notes, _) = state
        .db
        .list_notes(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;
    let mut rows = vec![vec![
        "id".to_string(),
        "createdAt".to_string(),
        "updatedAt".to_string(),
        "title".to_string(),
    ]];
    for note in notes {
        rows.push(vec![
            note.id.to_string(),
            note.created_at.to_rfc3339(),
            note.updated_at.to_rfc3339(),
            note.title.unwrap_or_default(),
        ]);
    }
    csv_response(rows)
}

pub async fn get_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    shape.validate()?;

    let person = state
        .db
        .get_person(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;

    let mut body = json!({ "data": { "person": person } });
    shape.project(&mut body["data"]["person"], "person", PERSON_FIELDS)?;
    Ok(Json(body))
}

pub async fn create_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(shape): Query<ResponseShape>,
    Json(input): Json<CreatePersonInput>,
) -> AppResult<(axum::http::StatusCode, Json<serde_json::Value>)> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    validate_person(&input)?;

    let person = state
        .db
        .create_person(auth.workspace_id, &input)
        .await
        .map_err(map_person_error)?;

    let mut body = json!({ "data": { "createPerson": person } });
    shape.project(&mut body["data"]["createPerson"], "person", PERSON_FIELDS)?;
    emit_webhooks(
        &state,
        auth.workspace_id,
        "person.created",
        body["data"]["createPerson"].clone(),
    )
    .await;
    Ok((axum::http::StatusCode::CREATED, Json(body)))
}

pub async fn update_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
    Json(input): Json<UpdatePersonInput>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;

    let person = state
        .db
        .update_person(auth.workspace_id, id, &input)
        .await
        .map_err(map_person_error)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;

    let mut body = json!({ "data": { "updatePerson": person } });
    shape.project(&mut body["data"]["updatePerson"], "person", PERSON_FIELDS)?;
    emit_webhooks(
        &state,
        auth.workspace_id,
        "person.updated",
        body["data"]["updatePerson"].clone(),
    )
    .await;
    Ok(Json(body))
}

pub async fn delete_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(params): Query<DeletePersonParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let capability = if params.soft_delete {
        Capability::SoftDelete
    } else {
        Capability::Destroy
    };
    if !role_can(&state, &auth, capability).await? {
        return Err(AppError::Forbidden(
            "You do not have permission to perform this action".to_string(),
        ));
    }

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

    emit_webhooks(
        &state,
        auth.workspace_id,
        if params.soft_delete {
            "person.soft_deleted"
        } else {
            "person.deleted"
        },
        json!({ "id": id }),
    )
    .await;
    Ok(Json(json!({ "data": { "deletePerson": { "id": id } } })))
}

pub async fn restore_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;

    let person = state
        .db
        .restore_person(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;

    let mut body = json!({ "data": { "restorePerson": person } });
    shape.project(&mut body["data"]["restorePerson"], "person", PERSON_FIELDS)?;
    Ok(Json(body))
}

#[derive(Debug, Deserialize)]
pub struct ListCompaniesParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub starting_after: Option<String>,
    pub ending_before: Option<String>,
    pub order_by: Option<String>,
    pub filter: Option<String>,
    #[serde(flatten)]
    pub shape: ResponseShape,
}

pub async fn list_companies(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListCompaniesParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    params.shape.validate()?;
    let filter = params
        .filter
        .as_deref()
        .map(|raw| {
            parse_generic_filter(
                raw,
                &[
                    "id",
                    "createdAt",
                    "updatedAt",
                    "deletedAt",
                    "name",
                    "position",
                    "accountOwnerId",
                    "searchVector",
                    "annualRevenue",
                    "annualRevenue.amountMicros",
                ],
            )
        })
        .transpose()?;
    let order = parse_generic_order_by(
        params.order_by.as_deref(),
        &[
            "id",
            "createdAt",
            "updatedAt",
            "deletedAt",
            "name",
            "position",
            "accountOwnerId",
            "accountOwner.id",
            "accountOwner.userId",
            "accountOwner.userEmail",
            "accountOwner.name.firstName",
            "accountOwner.name.lastName",
            "searchVector",
            "annualRevenue",
        ],
    )?;

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

    let (mut companies, _) = state
        .db
        .list_companies(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;
    if let Some(filter) = &filter {
        companies
            .retain(|company| filtering::matches(filter, &|field| company_value(company, field)));
    }
    let total = companies.len() as i64;
    if order
        .iter()
        .any(|term| term.field.starts_with("accountOwner."))
    {
        let members = workspace_member_map(&state, auth.workspace_id).await?;
        crate::sorting::sort_records(&mut companies, &order, |company, field| {
            crate::sorting::company_related_value(company, field, &members)
        });
    } else {
        sort_companies(&mut companies, &order);
    }

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

    let mut body = serde_json::to_value(FindManyResponse {
        data,
        total_count: total.max(0) as u64,
        page_info: PageInfo {
            has_next_page,
            has_previous_page,
            start_cursor,
            end_cursor,
        },
    })
    .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    params.shape.project_array(
        &mut body["data"]["companies"],
        "<class 'object'>",
        COMPANY_FIELDS,
    )?;
    Ok(Json(body))
}

pub async fn get_company(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    shape.validate()?;
    let company = state
        .db
        .get_company(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    let mut body = json!({ "data": { "company": company } });
    shape.project(&mut body["data"]["company"], "company", COMPANY_FIELDS)?;
    Ok(Json(body))
}

pub async fn create_company(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(shape): Query<ResponseShape>,
    Json(input): Json<CreateCompanyInput>,
) -> AppResult<(axum::http::StatusCode, Json<serde_json::Value>)> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    let company = state
        .db
        .create_company(auth.workspace_id, &input)
        .await
        .map_err(AppError::Internal)?;
    let mut body = json!({ "data": { "createCompany": company } });
    shape.project(
        &mut body["data"]["createCompany"],
        "company",
        COMPANY_FIELDS,
    )?;
    emit_webhooks(
        &state,
        auth.workspace_id,
        "company.created",
        body["data"]["createCompany"].clone(),
    )
    .await;
    Ok((axum::http::StatusCode::CREATED, Json(body)))
}

pub async fn update_company(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
    Json(input): Json<UpdateCompanyInput>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    let company = state
        .db
        .update_company(auth.workspace_id, id, &input)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    let mut body = json!({ "data": { "updateCompany": company } });
    shape.project(
        &mut body["data"]["updateCompany"],
        "company",
        COMPANY_FIELDS,
    )?;
    emit_webhooks(
        &state,
        auth.workspace_id,
        "company.updated",
        body["data"]["updateCompany"].clone(),
    )
    .await;
    Ok(Json(body))
}

pub async fn delete_company(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(params): Query<DeletePersonParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let capability = if params.soft_delete {
        Capability::SoftDelete
    } else {
        Capability::Destroy
    };
    if !role_can(&state, &auth, capability).await? {
        return Err(AppError::Forbidden(
            "You do not have permission to perform this action".to_string(),
        ));
    }
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
    emit_webhooks(
        &state,
        auth.workspace_id,
        if params.soft_delete {
            "company.soft_deleted"
        } else {
            "company.deleted"
        },
        json!({ "id": id }),
    )
    .await;
    Ok(Json(json!({ "data": { "deleteCompany": { "id": id } } })))
}

pub async fn restore_company(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    let company = state
        .db
        .restore_company(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    let mut body = json!({ "data": { "restoreCompany": company } });
    shape.project(
        &mut body["data"]["restoreCompany"],
        "company",
        COMPANY_FIELDS,
    )?;
    Ok(Json(body))
}

#[derive(Debug, Deserialize)]
pub struct ListOpportunitiesParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub starting_after: Option<String>,
    pub ending_before: Option<String>,
    pub order_by: Option<String>,
    pub filter: Option<String>,
    #[serde(flatten)]
    pub shape: ResponseShape,
}

pub async fn list_opportunities(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListOpportunitiesParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    params.shape.validate()?;
    let filter = params
        .filter
        .as_deref()
        .map(|raw| {
            parse_generic_filter(
                raw,
                &[
                    "id",
                    "createdAt",
                    "updatedAt",
                    "deletedAt",
                    "name",
                    "amount",
                    "closeDate",
                    "stage",
                    "position",
                    "companyId",
                    "company.id",
                    "company.createdAt",
                    "company.updatedAt",
                    "company.name",
                    "company.position",
                    "company.accountOwnerId",
                    "pointOfContactId",
                    "pointOfContact.id",
                    "pointOfContact.createdAt",
                    "pointOfContact.updatedAt",
                    "pointOfContact.name.firstName",
                    "pointOfContact.name.lastName",
                    "pointOfContact.emails.primaryEmail",
                    "pointOfContact.jobTitle",
                    "pointOfContact.companyId",
                    "ownerId",
                    "searchVector",
                    "amount.amountMicros",
                ],
            )
        })
        .transpose()?;
    let order = parse_generic_order_by(
        params.order_by.as_deref(),
        &[
            "id",
            "createdAt",
            "updatedAt",
            "deletedAt",
            "name",
            "amount",
            "closeDate",
            "stage",
            "position",
            "companyId",
            "company.id",
            "company.createdAt",
            "company.updatedAt",
            "company.name",
            "company.position",
            "company.accountOwnerId",
            "pointOfContactId",
            "pointOfContact.id",
            "pointOfContact.createdAt",
            "pointOfContact.updatedAt",
            "pointOfContact.name.firstName",
            "pointOfContact.name.lastName",
            "pointOfContact.emails.primaryEmail",
            "pointOfContact.jobTitle",
            "ownerId",
            "searchVector",
        ],
    )?;

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

    let (mut opportunities, _) = state
        .db
        .list_opportunities(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;
    let needs_companies = filter
        .as_ref()
        .is_some_and(|filter| filtering::references_prefix(filter, "company."))
        || order.iter().any(|term| {
            term.field.starts_with("company.") || term.field.starts_with("pointOfContact.")
        });
    let companies: HashMap<Uuid, Company> = if needs_companies {
        let (companies, _) = state
            .db
            .list_companies(auth.workspace_id, 10_000, 0)
            .await
            .map_err(AppError::Internal)?;
        companies
            .into_iter()
            .map(|company| (company.id, company))
            .collect()
    } else {
        HashMap::new()
    };
    let needs_people = filter
        .as_ref()
        .is_some_and(|filter| filtering::references_prefix(filter, "pointOfContact."))
        || order
            .iter()
            .any(|term| term.field.starts_with("pointOfContact."));
    let people: HashMap<Uuid, squid_core::Person> = if needs_people {
        state
            .db
            .list_persons_page(auth.workspace_id, 10_000, 0, None, None, &[], None)
            .await
            .map_err(AppError::Internal)?
            .persons
            .into_iter()
            .map(|person| (person.id, person))
            .collect()
    } else {
        HashMap::new()
    };
    if let Some(filter) = &filter {
        opportunities.retain(|opportunity| {
            filtering::matches(filter, &|field| {
                opportunity_relation_value(opportunity, field, &companies, &people)
            })
        });
    }
    let total = opportunities.len() as i64;
    crate::sorting::sort_records(&mut opportunities, &order, |opportunity, field| {
        opportunity_relation_value(opportunity, field, &companies, &people)
    });

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

    let mut body = serde_json::to_value(FindManyResponse {
        data,
        total_count: total.max(0) as u64,
        page_info: PageInfo {
            has_next_page,
            has_previous_page,
            start_cursor,
            end_cursor,
        },
    })
    .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    params.shape.project_array(
        &mut body["data"]["opportunities"],
        "<class 'object'>",
        OPPORTUNITY_FIELDS,
    )?;
    Ok(Json(body))
}

pub async fn get_opportunity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    shape.validate()?;
    let opportunity = state
        .db
        .get_opportunity(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    let mut body = json!({ "data": { "opportunity": opportunity } });
    shape.project(
        &mut body["data"]["opportunity"],
        "opportunity",
        OPPORTUNITY_FIELDS,
    )?;
    Ok(Json(body))
}

pub async fn create_opportunity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(shape): Query<ResponseShape>,
    Json(input): Json<CreateOpportunityInput>,
) -> AppResult<(axum::http::StatusCode, Json<serde_json::Value>)> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    let opportunity = state
        .db
        .create_opportunity(auth.workspace_id, &input)
        .await
        .map_err(AppError::Internal)?;
    let mut body = json!({ "data": { "createOpportunity": opportunity } });
    shape.project(
        &mut body["data"]["createOpportunity"],
        "opportunity",
        OPPORTUNITY_FIELDS,
    )?;
    emit_webhooks(
        &state,
        auth.workspace_id,
        "opportunity.created",
        body["data"]["createOpportunity"].clone(),
    )
    .await;
    Ok((axum::http::StatusCode::CREATED, Json(body)))
}

pub async fn update_opportunity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
    Json(input): Json<UpdateOpportunityInput>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    let opportunity = state
        .db
        .update_opportunity(auth.workspace_id, id, &input)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    let mut body = json!({ "data": { "updateOpportunity": opportunity } });
    shape.project(
        &mut body["data"]["updateOpportunity"],
        "opportunity",
        OPPORTUNITY_FIELDS,
    )?;
    emit_webhooks(
        &state,
        auth.workspace_id,
        "opportunity.updated",
        body["data"]["updateOpportunity"].clone(),
    )
    .await;
    Ok(Json(body))
}

pub async fn delete_opportunity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(params): Query<DeletePersonParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let capability = if params.soft_delete {
        Capability::SoftDelete
    } else {
        Capability::Destroy
    };
    if !role_can(&state, &auth, capability).await? {
        return Err(AppError::Forbidden(
            "You do not have permission to perform this action".to_string(),
        ));
    }
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
    Query(shape): Query<ResponseShape>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    let opportunity = state
        .db
        .restore_opportunity(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    let mut body = json!({ "data": { "restoreOpportunity": opportunity } });
    shape.project(
        &mut body["data"]["restoreOpportunity"],
        "opportunity",
        OPPORTUNITY_FIELDS,
    )?;
    Ok(Json(body))
}

pub async fn list_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListOpportunitiesParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    params.shape.validate()?;
    let filter = params
        .filter
        .as_deref()
        .map(|raw| {
            parse_generic_filter(
                raw,
                &[
                    "id",
                    "createdAt",
                    "updatedAt",
                    "deletedAt",
                    "position",
                    "title",
                    "dueAt",
                    "status",
                    "assigneeId",
                    "searchVector",
                ],
            )
        })
        .transpose()?;
    let order = parse_generic_order_by(
        params.order_by.as_deref(),
        &[
            "id",
            "createdAt",
            "updatedAt",
            "deletedAt",
            "position",
            "title",
            "dueAt",
            "status",
            "assigneeId",
            "assignee.id",
            "assignee.userId",
            "assignee.userEmail",
            "assignee.name.firstName",
            "assignee.name.lastName",
            "searchVector",
        ],
    )?;

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

    let (mut tasks, _) = state
        .db
        .list_tasks(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;
    if let Some(filter) = &filter {
        tasks.retain(|task| filtering::matches(filter, &|field| task_value(task, field)));
    }
    let total = tasks.len() as i64;
    if order.iter().any(|term| term.field.starts_with("assignee.")) {
        let members = workspace_member_map(&state, auth.workspace_id).await?;
        crate::sorting::sort_records(&mut tasks, &order, |task, field| {
            crate::sorting::task_related_value(task, field, &members)
        });
    } else {
        sort_tasks(&mut tasks, &order);
    }
    if let Some(after) = starting_after {
        if let Some(index) = tasks.iter().position(|row| row.id == after) {
            tasks.drain(..=index);
            offset = 0;
        }
    }
    if let Some(before) = ending_before {
        if let Some(index) = tasks.iter().position(|row| row.id == before) {
            tasks.truncate(index);
        }
    }
    let start = (offset as usize).min(tasks.len());
    let end = (start + limit as usize).min(tasks.len());
    let has_next_page = end < tasks.len();
    let has_previous_page = start > 0 || starting_after.is_some();
    let page: Vec<Task> = tasks[start..end].to_vec();
    let start_cursor = page.first().map(|row| encode_cursor(row.id));
    let end_cursor = page.last().map(|row| encode_cursor(row.id));

    let mut data = BTreeMap::new();
    data.insert("tasks".to_string(), page);
    let mut body = serde_json::to_value(FindManyResponse {
        data,
        total_count: total.max(0) as u64,
        page_info: PageInfo {
            has_next_page,
            has_previous_page,
            start_cursor,
            end_cursor,
        },
    })
    .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    params
        .shape
        .project_array(&mut body["data"]["tasks"], "task", TASK_FIELDS)?;
    Ok(Json(body))
}

pub async fn get_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    shape.validate()?;
    let task = state
        .db
        .get_task(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    let mut body = json!({ "data": { "task": task } });
    shape.project(&mut body["data"]["task"], "task", TASK_FIELDS)?;
    Ok(Json(body))
}

pub async fn create_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(shape): Query<ResponseShape>,
    Json(input): Json<CreateTaskInput>,
) -> AppResult<(axum::http::StatusCode, Json<serde_json::Value>)> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    let task = state
        .db
        .create_task(auth.workspace_id, &input)
        .await
        .map_err(AppError::Internal)?;
    let mut body = json!({ "data": { "createTask": task } });
    shape.project(&mut body["data"]["createTask"], "task", TASK_FIELDS)?;
    emit_webhooks(
        &state,
        auth.workspace_id,
        "task.created",
        body["data"]["createTask"].clone(),
    )
    .await;
    Ok((axum::http::StatusCode::CREATED, Json(body)))
}

pub async fn update_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
    Json(input): Json<UpdateTaskInput>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    let task = state
        .db
        .update_task(auth.workspace_id, id, &input)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    let mut body = json!({ "data": { "updateTask": task } });
    shape.project(&mut body["data"]["updateTask"], "task", TASK_FIELDS)?;
    emit_webhooks(
        &state,
        auth.workspace_id,
        "task.updated",
        body["data"]["updateTask"].clone(),
    )
    .await;
    Ok(Json(body))
}

pub async fn delete_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(params): Query<DeletePersonParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let capability = if params.soft_delete {
        Capability::SoftDelete
    } else {
        Capability::Destroy
    };
    if !role_can(&state, &auth, capability).await? {
        return Err(AppError::Forbidden(
            "You do not have permission to perform this action".to_string(),
        ));
    }
    let deleted = if params.soft_delete {
        state
            .db
            .soft_delete_task(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    } else {
        state
            .db
            .destroy_task(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    };
    if !deleted {
        return Err(AppError::NotFound("Record not found".to_string()));
    }
    emit_webhooks(
        &state,
        auth.workspace_id,
        if params.soft_delete {
            "task.soft_deleted"
        } else {
            "task.deleted"
        },
        json!({ "id": id }),
    )
    .await;
    Ok(Json(json!({ "data": { "deleteTask": { "id": id } } })))
}

pub async fn restore_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    let task = state
        .db
        .restore_task(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    let mut body = json!({ "data": { "restoreTask": task } });
    shape.project(&mut body["data"]["restoreTask"], "task", TASK_FIELDS)?;
    Ok(Json(body))
}

pub async fn list_notes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListOpportunitiesParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    params.shape.validate()?;
    let filter = params
        .filter
        .as_deref()
        .map(|raw| {
            parse_generic_filter(
                raw,
                &[
                    "id",
                    "createdAt",
                    "updatedAt",
                    "deletedAt",
                    "position",
                    "title",
                    "searchVector",
                ],
            )
        })
        .transpose()?;
    let order = parse_generic_order_by(
        params.order_by.as_deref(),
        &[
            "id",
            "createdAt",
            "updatedAt",
            "deletedAt",
            "position",
            "title",
            "searchVector",
        ],
    )?;

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

    let (mut notes, _) = state
        .db
        .list_notes(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;
    if let Some(filter) = &filter {
        notes.retain(|note| filtering::matches(filter, &|field| note_value(note, field)));
    }
    let total = notes.len() as i64;
    sort_notes(&mut notes, &order);
    if let Some(after) = starting_after {
        if let Some(index) = notes.iter().position(|row| row.id == after) {
            notes.drain(..=index);
            offset = 0;
        }
    }
    if let Some(before) = ending_before {
        if let Some(index) = notes.iter().position(|row| row.id == before) {
            notes.truncate(index);
        }
    }
    let start = (offset as usize).min(notes.len());
    let end = (start + limit as usize).min(notes.len());
    let has_next_page = end < notes.len();
    let has_previous_page = start > 0 || starting_after.is_some();
    let page: Vec<Note> = notes[start..end].to_vec();
    let start_cursor = page.first().map(|row| encode_cursor(row.id));
    let end_cursor = page.last().map(|row| encode_cursor(row.id));

    let mut data = BTreeMap::new();
    data.insert("notes".to_string(), page);
    let mut body = serde_json::to_value(FindManyResponse {
        data,
        total_count: total.max(0) as u64,
        page_info: PageInfo {
            has_next_page,
            has_previous_page,
            start_cursor,
            end_cursor,
        },
    })
    .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    params
        .shape
        .project_array(&mut body["data"]["notes"], "note", NOTE_FIELDS)?;
    Ok(Json(body))
}

pub async fn get_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    shape.validate()?;
    let note = state
        .db
        .get_note(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    let mut body = json!({ "data": { "note": note } });
    shape.project(&mut body["data"]["note"], "note", NOTE_FIELDS)?;
    Ok(Json(body))
}

pub async fn create_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(shape): Query<ResponseShape>,
    Json(input): Json<CreateNoteInput>,
) -> AppResult<(axum::http::StatusCode, Json<serde_json::Value>)> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    let note = state
        .db
        .create_note(auth.workspace_id, &input)
        .await
        .map_err(AppError::Internal)?;
    let mut body = json!({ "data": { "createNote": note } });
    shape.project(&mut body["data"]["createNote"], "note", NOTE_FIELDS)?;
    emit_webhooks(
        &state,
        auth.workspace_id,
        "note.created",
        body["data"]["createNote"].clone(),
    )
    .await;
    Ok((axum::http::StatusCode::CREATED, Json(body)))
}

pub async fn update_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
    Json(input): Json<UpdateNoteInput>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    let note = state
        .db
        .update_note(auth.workspace_id, id, &input)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    let mut body = json!({ "data": { "updateNote": note } });
    shape.project(&mut body["data"]["updateNote"], "note", NOTE_FIELDS)?;
    emit_webhooks(
        &state,
        auth.workspace_id,
        "note.updated",
        body["data"]["updateNote"].clone(),
    )
    .await;
    Ok(Json(body))
}

pub async fn delete_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(params): Query<DeletePersonParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let capability = if params.soft_delete {
        Capability::SoftDelete
    } else {
        Capability::Destroy
    };
    if !role_can(&state, &auth, capability).await? {
        return Err(AppError::Forbidden(
            "You do not have permission to perform this action".to_string(),
        ));
    }
    let deleted = if params.soft_delete {
        state
            .db
            .soft_delete_note(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    } else {
        state
            .db
            .destroy_note(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    };
    if !deleted {
        return Err(AppError::NotFound("Record not found".to_string()));
    }
    emit_webhooks(
        &state,
        auth.workspace_id,
        if params.soft_delete {
            "note.soft_deleted"
        } else {
            "note.deleted"
        },
        json!({ "id": id }),
    )
    .await;
    Ok(Json(json!({ "data": { "deleteNote": { "id": id } } })))
}

pub async fn restore_note(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(shape): Query<ResponseShape>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    shape.validate()?;
    let note = state
        .db
        .restore_note(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    let mut body = json!({ "data": { "restoreNote": note } });
    shape.project(&mut body["data"]["restoreNote"], "note", NOTE_FIELDS)?;
    Ok(Json(body))
}

fn list_target_page<T: Clone>(
    mut rows: Vec<T>,
    limit: i64,
    offset: i64,
    starting_after: Option<Uuid>,
    ending_before: Option<Uuid>,
    id_of: impl Fn(&T) -> Uuid,
) -> (Vec<T>, PageInfo) {
    let mut offset = offset;
    if let Some(after) = starting_after {
        if let Some(index) = rows.iter().position(|row| id_of(row) == after) {
            rows.drain(..=index);
            offset = 0;
        }
    }
    if let Some(before) = ending_before {
        if let Some(index) = rows.iter().position(|row| id_of(row) == before) {
            rows.truncate(index);
        }
    }
    let start = (offset as usize).min(rows.len());
    let end = (start + limit as usize).min(rows.len());
    let start_cursor = rows.get(start).map(|row| encode_cursor(id_of(row)));
    let end_cursor = rows
        .get(end.saturating_sub(1))
        .map(|row| encode_cursor(id_of(row)));
    let page = rows[start..end].to_vec();
    (
        page,
        PageInfo {
            has_next_page: end < rows.len(),
            has_previous_page: start > 0 || starting_after.is_some(),
            start_cursor,
            end_cursor,
        },
    )
}

pub async fn list_task_targets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListOpportunitiesParams>,
) -> AppResult<Json<FindManyResponse<TaskTarget>>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let filter = params
        .filter
        .as_deref()
        .map(|raw| {
            parse_generic_filter(
                raw,
                &[
                    "id",
                    "createdAt",
                    "updatedAt",
                    "deletedAt",
                    "position",
                    "searchVector",
                    "taskId",
                    "targetCompanyId",
                    "targetOpportunityId",
                    "targetPersonId",
                ],
            )
        })
        .transpose()?;
    let order = parse_generic_order_by(
        params.order_by.as_deref(),
        &[
            "id",
            "createdAt",
            "updatedAt",
            "deletedAt",
            "position",
            "searchVector",
            "taskId",
            "targetCompanyId",
            "targetOpportunityId",
            "targetPersonId",
            "task.id",
            "task.title",
            "task.status",
            "targetCompany.id",
            "targetCompany.name",
            "targetCompany.createdAt",
            "targetOpportunity.id",
            "targetOpportunity.name",
            "targetOpportunity.stage",
            "targetPerson.id",
            "targetPerson.name.firstName",
            "targetPerson.name.lastName",
            "targetPerson.emails.primaryEmail",
        ],
    )?;
    let limit = params
        .limit
        .unwrap_or(20)
        .clamp(1, state.config.max_page_size);
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
    let mut targets = state
        .db
        .list_task_targets(auth.workspace_id, None, None, None, None)
        .await
        .map_err(AppError::Internal)?;
    if let Some(filter) = &filter {
        targets
            .retain(|target| filtering::matches(filter, &|field| task_target_value(target, field)));
    }
    if order.iter().any(|term| {
        term.field.starts_with("task.")
            || term.field.starts_with("targetCompany.")
            || term.field.starts_with("targetOpportunity.")
            || term.field.starts_with("targetPerson.")
    }) {
        let (tasks, _) = state
            .db
            .list_tasks(auth.workspace_id, 10_000, 0)
            .await
            .map_err(AppError::Internal)?;
        let tasks: HashMap<Uuid, Task> = tasks.into_iter().map(|task| (task.id, task)).collect();
        let (companies, _) = state
            .db
            .list_companies(auth.workspace_id, 10_000, 0)
            .await
            .map_err(AppError::Internal)?;
        let companies: HashMap<Uuid, Company> = companies
            .into_iter()
            .map(|company| (company.id, company))
            .collect();
        let (opportunities, _) = state
            .db
            .list_opportunities(auth.workspace_id, 10_000, 0)
            .await
            .map_err(AppError::Internal)?;
        let opportunities: HashMap<Uuid, Opportunity> = opportunities
            .into_iter()
            .map(|opportunity| (opportunity.id, opportunity))
            .collect();
        let people: HashMap<Uuid, squid_core::Person> = state
            .db
            .list_persons_page(auth.workspace_id, 10_000, 0, None, None, &[], None)
            .await
            .map_err(AppError::Internal)?
            .persons
            .into_iter()
            .map(|person| (person.id, person))
            .collect();
        crate::sorting::sort_records(&mut targets, &order, |target, field| {
            crate::sorting::task_target_related_value(
                target,
                field,
                &tasks,
                &companies,
                &opportunities,
                &people,
            )
        });
    } else {
        sort_task_targets(&mut targets, &order);
    }
    let total = targets.len() as i64;
    let (page, page_info) = list_target_page(
        targets,
        limit,
        params.offset.unwrap_or(0).max(0),
        starting_after,
        ending_before,
        |target| target.id,
    );
    let mut data = BTreeMap::new();
    data.insert("taskTargets".to_string(), page);
    Ok(Json(FindManyResponse {
        data,
        total_count: total.max(0) as u64,
        page_info,
    }))
}

pub async fn get_task_target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let target = state
        .db
        .get_task_target(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    Ok(Json(json!({ "data": { "taskTarget": target } })))
}

pub async fn create_task_target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateTaskTargetInput>,
) -> AppResult<(axum::http::StatusCode, Json<serde_json::Value>)> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    let target = state
        .db
        .create_task_target(auth.workspace_id, &input)
        .await
        .map_err(AppError::Internal)?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({ "data": { "createTaskTarget": target } })),
    ))
}

pub async fn update_task_target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateTaskTargetInput>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    let target = state
        .db
        .update_task_target(auth.workspace_id, id, &input)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    Ok(Json(json!({ "data": { "updateTaskTarget": target } })))
}

pub async fn delete_task_target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(params): Query<DeletePersonParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let capability = if params.soft_delete {
        Capability::SoftDelete
    } else {
        Capability::Destroy
    };
    if !role_can(&state, &auth, capability).await? {
        return Err(AppError::Forbidden(
            "You do not have permission to perform this action".to_string(),
        ));
    }
    let deleted = if params.soft_delete {
        state
            .db
            .delete_task_target(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    } else {
        state
            .db
            .destroy_task_target(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    };
    if !deleted {
        return Err(AppError::NotFound("Record not found".to_string()));
    }
    Ok(Json(
        json!({ "data": { "deleteTaskTarget": { "id": id } } }),
    ))
}

pub async fn restore_task_target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    let target = state
        .db
        .restore_task_target(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    Ok(Json(json!({ "data": { "restoreTaskTarget": target } })))
}

pub async fn list_note_targets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListOpportunitiesParams>,
) -> AppResult<Json<FindManyResponse<NoteTarget>>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let filter = params
        .filter
        .as_deref()
        .map(|raw| {
            parse_generic_filter(
                raw,
                &[
                    "id",
                    "createdAt",
                    "updatedAt",
                    "deletedAt",
                    "position",
                    "searchVector",
                    "noteId",
                    "targetCompanyId",
                    "targetOpportunityId",
                    "targetPersonId",
                ],
            )
        })
        .transpose()?;
    let order = parse_generic_order_by(
        params.order_by.as_deref(),
        &[
            "id",
            "createdAt",
            "updatedAt",
            "deletedAt",
            "position",
            "searchVector",
            "noteId",
            "targetCompanyId",
            "targetOpportunityId",
            "targetPersonId",
            "note.id",
            "note.title",
            "targetCompany.id",
            "targetCompany.name",
            "targetCompany.createdAt",
            "targetOpportunity.id",
            "targetOpportunity.name",
            "targetOpportunity.stage",
            "targetPerson.id",
            "targetPerson.name.firstName",
            "targetPerson.name.lastName",
            "targetPerson.emails.primaryEmail",
        ],
    )?;
    let limit = params
        .limit
        .unwrap_or(20)
        .clamp(1, state.config.max_page_size);
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
    let mut targets = state
        .db
        .list_note_targets(auth.workspace_id, None, None, None, None)
        .await
        .map_err(AppError::Internal)?;
    if let Some(filter) = &filter {
        targets
            .retain(|target| filtering::matches(filter, &|field| note_target_value(target, field)));
    }
    if order.iter().any(|term| {
        term.field.starts_with("note.")
            || term.field.starts_with("targetCompany.")
            || term.field.starts_with("targetOpportunity.")
            || term.field.starts_with("targetPerson.")
    }) {
        let (notes, _) = state
            .db
            .list_notes(auth.workspace_id, 10_000, 0)
            .await
            .map_err(AppError::Internal)?;
        let notes: HashMap<Uuid, Note> = notes.into_iter().map(|note| (note.id, note)).collect();
        let (companies, _) = state
            .db
            .list_companies(auth.workspace_id, 10_000, 0)
            .await
            .map_err(AppError::Internal)?;
        let companies: HashMap<Uuid, Company> = companies
            .into_iter()
            .map(|company| (company.id, company))
            .collect();
        let (opportunities, _) = state
            .db
            .list_opportunities(auth.workspace_id, 10_000, 0)
            .await
            .map_err(AppError::Internal)?;
        let opportunities: HashMap<Uuid, Opportunity> = opportunities
            .into_iter()
            .map(|opportunity| (opportunity.id, opportunity))
            .collect();
        let people: HashMap<Uuid, squid_core::Person> = state
            .db
            .list_persons_page(auth.workspace_id, 10_000, 0, None, None, &[], None)
            .await
            .map_err(AppError::Internal)?
            .persons
            .into_iter()
            .map(|person| (person.id, person))
            .collect();
        crate::sorting::sort_records(&mut targets, &order, |target, field| {
            crate::sorting::note_target_related_value(
                target,
                field,
                &notes,
                &companies,
                &opportunities,
                &people,
            )
        });
    } else {
        sort_note_targets(&mut targets, &order);
    }
    let total = targets.len() as i64;
    let (page, page_info) = list_target_page(
        targets,
        limit,
        params.offset.unwrap_or(0).max(0),
        starting_after,
        ending_before,
        |target| target.id,
    );
    let mut data = BTreeMap::new();
    data.insert("noteTargets".to_string(), page);
    Ok(Json(FindManyResponse {
        data,
        total_count: total.max(0) as u64,
        page_info,
    }))
}

pub async fn get_note_target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let target = state
        .db
        .get_note_target(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    Ok(Json(json!({ "data": { "noteTarget": target } })))
}

pub async fn create_note_target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateNoteTargetInput>,
) -> AppResult<(axum::http::StatusCode, Json<serde_json::Value>)> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    let target = state
        .db
        .create_note_target(auth.workspace_id, &input)
        .await
        .map_err(AppError::Internal)?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({ "data": { "createNoteTarget": target } })),
    ))
}

pub async fn update_note_target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateNoteTargetInput>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    let target = state
        .db
        .update_note_target(auth.workspace_id, id, &input)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    Ok(Json(json!({ "data": { "updateNoteTarget": target } })))
}

pub async fn delete_note_target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(params): Query<DeletePersonParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require(&state, &headers).await?;
    let capability = if params.soft_delete {
        Capability::SoftDelete
    } else {
        Capability::Destroy
    };
    if !role_can(&state, &auth, capability).await? {
        return Err(AppError::Forbidden(
            "You do not have permission to perform this action".to_string(),
        ));
    }
    let deleted = if params.soft_delete {
        state
            .db
            .delete_note_target(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    } else {
        state
            .db
            .destroy_note_target(auth.workspace_id, id)
            .await
            .map_err(AppError::Internal)?
    };
    if !deleted {
        return Err(AppError::NotFound("Record not found".to_string()));
    }
    Ok(Json(
        json!({ "data": { "deleteNoteTarget": { "id": id } } }),
    ))
}

pub async fn restore_note_target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Update).await?;
    let target = state
        .db
        .restore_note_target(auth.workspace_id, id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
    Ok(Json(json!({ "data": { "restoreNoteTarget": target } })))
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
    resolve_auth_with_policy(state, headers, false)
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
            "companyId" => PersonColumn::CompanyId,
            "company.name" => PersonColumn::CompanyName,
            "company.createdAt" => PersonColumn::CompanyCreatedAt,
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

fn parse_generic_order_by(raw: Option<&str>, allowed: &[&str]) -> AppResult<Vec<SortTerm>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };

    let mut terms = Vec::new();
    for item in raw.split(',').filter(|item| !item.trim().is_empty()) {
        let (field, direction) = match item.split_once('[') {
            Some((field, rest)) => (field.trim(), rest.trim_end_matches(']').trim()),
            None => (item.trim(), "AscNullsFirst"),
        };
        if !allowed.contains(&field) {
            return Err(AppError::BadRequest(format!(
                "Field \"{field}\" does not exist or is not sortable"
            )));
        }
        let term = SortTerm::parse(field, direction).ok_or_else(|| {
            AppError::BadRequest(format!(
                "'order_by' direction '{direction}' invalid. Allowed values are \
                 'AscNullsFirst', 'AscNullsLast', 'DescNullsFirst', 'DescNullsLast'."
            ))
        })?;
        terms.push(term);
    }
    Ok(terms)
}

/// Parse the REST filter mini-language, e.g.
/// `createdAt[gte]:"2024-01-01",name.firstName[startsWith]:A`.
async fn workspace_member_map(
    state: &AppState,
    workspace_id: Uuid,
) -> AppResult<HashMap<Uuid, (squid_core::WorkspaceMember, squid_core::User)>> {
    Ok(state
        .db
        .list_workspace_members_with_users(workspace_id)
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .into_iter()
        .map(|(member, user)| (member.user_id, (member, user)))
        .collect())
}

#[derive(Debug, Clone, Copy)]
enum Capability {
    Read,
    Update,
    SoftDelete,
    Destroy,
}

async fn role_can(
    state: &AppState,
    auth: &squid_auth::AuthContext,
    cap: Capability,
) -> AppResult<bool> {
    if let Some(role_id) = auth.role_id {
        if let Some(role) = state
            .db
            .get_role(auth.workspace_id, role_id)
            .await
            .map_err(AppError::Internal)?
        {
            return Ok(match cap {
                Capability::Read => role.can_read_all_object_records,
                Capability::Update => role.can_update_all_object_records,
                Capability::SoftDelete => role.can_soft_delete_all_object_records,
                Capability::Destroy => role.can_destroy_all_object_records,
            });
        }
    }
    Ok(match auth.role {
        squid_core::Role::Owner | squid_core::Role::Admin => true,
        squid_core::Role::Member => !matches!(cap, Capability::Destroy),
    })
}

const WEBHOOK_MAX_ATTEMPTS: i32 = 8;
const WEBHOOK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub(crate) async fn emit_webhooks(
    state: &AppState,
    workspace_id: Uuid,
    operation: &str,
    payload: serde_json::Value,
) {
    let Ok(hooks) = state.db.list_webhooks(workspace_id).await else {
        return;
    };
    let client = match reqwest::Client::builder()
        .timeout(WEBHOOK_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::error!(%error, "failed to build webhook HTTP client");
            return;
        }
    };

    for hook in hooks {
        if !hook
            .operations
            .iter()
            .any(|value| value == operation || value == "*")
        {
            continue;
        }
        let event_id = Uuid::new_v4();
        let occurred_at = chrono::Utc::now();
        let body = serde_json::json!({
            "id": event_id,
            "event": operation,
            "type": operation,
            "schemaVersion": "1.0",
            "workspaceId": workspace_id,
            "occurredAt": occurred_at.to_rfc3339(),
            "data": payload,
        });
        let body_bytes = match serde_json::to_vec(&body) {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::error!(%error, webhook_id = %hook.id, "failed to serialize webhook event");
                continue;
            }
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(hook.secret.as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(&body_bytes);
        let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        let event = operation.to_string();
        let hook_id = hook.id;
        let target_url = hook.target_url.clone();
        let client = client.clone();
        let delivery_id = match state
            .db
            .create_webhook_delivery(
                workspace_id,
                hook_id,
                &event,
                event_id,
                &hook.target_url,
                &body_bytes,
                &signature,
            )
            .await
        {
            Ok(id) => id,
            Err(error) => {
                tracing::error!(%error, webhook_id = %hook_id, "failed to record webhook delivery");
                continue;
            }
        };
        let state = state.clone();
        tokio::spawn(async move {
            deliver_webhook(
                &state,
                &client,
                delivery_id,
                0,
                hook_id,
                workspace_id,
                &target_url,
                &event,
                event_id,
                &body_bytes,
                &signature,
            )
            .await;
        });
    }
}

#[allow(clippy::too_many_arguments)]
async fn deliver_webhook(
    state: &AppState,
    client: &reqwest::Client,
    delivery_id: Uuid,
    start_attempt: i32,
    hook_id: Uuid,
    workspace_id: Uuid,
    target_url: &str,
    event: &str,
    event_id: Uuid,
    body: &[u8],
    signature: &str,
) {
    for attempt in (start_attempt + 1)..=WEBHOOK_MAX_ATTEMPTS {
        let response = client
            .post(target_url)
            .header("user-agent", "squid-crm-webhook/1.0")
            .header("content-type", "application/json")
            .header("x-squid-event", event)
            .header("x-squid-event-id", event_id.to_string())
            .header("x-squid-schema-version", "1.0")
            .header("x-squid-workspace-id", workspace_id.to_string())
            .header("x-squid-signature", signature)
            .header("idempotency-key", event_id.to_string())
            .header("x-squid-attempt", attempt.to_string())
            .body(body.to_vec())
            .send()
            .await;

        match response {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    let _ = state
                        .db
                        .record_webhook_delivery(
                            workspace_id,
                            delivery_id,
                            attempt,
                            Some(i32::from(status.as_u16())),
                            None,
                            "succeeded",
                        )
                        .await;
                    tracing::debug!(
                        webhook_id = %hook_id,
                        event_id = %event_id,
                        attempt,
                        "webhook delivered"
                    );
                    return;
                }

                let retry_after = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(parse_retry_after);
                if is_retryable_status(status) && attempt < WEBHOOK_MAX_ATTEMPTS {
                    let _ = state
                        .db
                        .record_webhook_delivery(
                            workspace_id,
                            delivery_id,
                            attempt,
                            Some(i32::from(status.as_u16())),
                            None,
                            "pending",
                        )
                        .await;
                    let delay = webhook_backoff(attempt, retry_after);
                    tracing::warn!(
                        webhook_id = %hook_id,
                        event_id = %event_id,
                        attempt,
                        %status,
                        ?delay,
                        "webhook delivery failed; retrying"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }

                let _ = state
                    .db
                    .record_webhook_delivery(
                        workspace_id,
                        delivery_id,
                        attempt,
                        Some(i32::from(status.as_u16())),
                        Some("non-success HTTP status"),
                        "failed",
                    )
                    .await;
                tracing::warn!(
                    webhook_id = %hook_id,
                    event_id = %event_id,
                    attempt,
                    %status,
                    "webhook delivery failed permanently"
                );
                return;
            }
            Err(error) => {
                if attempt < WEBHOOK_MAX_ATTEMPTS {
                    let _ = state
                        .db
                        .record_webhook_delivery(
                            workspace_id,
                            delivery_id,
                            attempt,
                            None,
                            Some(&error.to_string()),
                            "pending",
                        )
                        .await;
                    let delay = webhook_backoff(attempt, None);
                    tracing::warn!(
                        webhook_id = %hook_id,
                        event_id = %event_id,
                        attempt,
                        %error,
                        ?delay,
                        "webhook transport error; retrying"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
                tracing::warn!(
                    webhook_id = %hook_id,
                    event_id = %event_id,
                    attempt,
                    %error,
                    "webhook transport error; giving up"
                );
                return;
            }
        }
    }
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500..=599)
}

fn parse_retry_after(value: &str) -> Option<std::time::Duration> {
    value
        .trim()
        .parse::<u64>()
        .ok()
        .map(|seconds| std::time::Duration::from_secs(seconds.min(30)))
}

fn webhook_backoff(attempt: i32, retry_after: Option<std::time::Duration>) -> std::time::Duration {
    if let Some(delay) = retry_after {
        return delay;
    }
    let exponent = attempt.saturating_sub(1).clamp(0, 4) as u64;
    let millis = 250_u64.saturating_mul(1_u64 << exponent);
    std::time::Duration::from_millis(millis.min(5_000))
}

pub(crate) async fn replay_webhook_delivery(
    state: &AppState,
    workspace_id: Uuid,
    delivery_id: Uuid,
) -> AppResult<bool> {
    let delivery = state
        .db
        .get_webhook_delivery(workspace_id, delivery_id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Webhook delivery not found".to_string()))?;
    let hook = state
        .db
        .get_webhook(workspace_id, delivery.webhook_id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Webhook not found".to_string()))?;
    let target_url = delivery
        .target_url
        .clone()
        .unwrap_or_else(|| hook.target_url.clone());
    let body = delivery
        .request_body
        .clone()
        .ok_or_else(|| AppError::BadRequest("Delivery has no stored body".to_string()))?;
    let mut mac = Hmac::<Sha256>::new_from_slice(hook.secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(&body);
    let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    let new_delivery = state
        .db
        .create_webhook_delivery(
            workspace_id,
            hook.id,
            &delivery.event,
            delivery.event_id,
            &target_url,
            &body,
            &signature,
        )
        .await
        .map_err(AppError::Internal)?;
    let client = reqwest::Client::builder()
        .timeout(WEBHOOK_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| AppError::Internal(anyhow::anyhow!(error)))?;
    let state = state.clone();
    let event = delivery.event.clone();
    let event_id = delivery.event_id;
    tokio::spawn(async move {
        deliver_webhook(
            &state,
            &client,
            new_delivery,
            0,
            hook.id,
            workspace_id,
            &target_url,
            &event,
            event_id,
            &body,
            &signature,
        )
        .await;
    });
    Ok(true)
}

pub(crate) async fn requeue_webhook_delivery(
    state: &AppState,
    workspace_id: Uuid,
    delivery_id: Uuid,
) -> AppResult<()> {
    let delivery = state
        .db
        .get_webhook_delivery(workspace_id, delivery_id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Webhook delivery not found".to_string()))?;
    let hook = state
        .db
        .get_webhook(workspace_id, delivery.webhook_id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Webhook not found".to_string()))?;
    let target_url = delivery
        .target_url
        .clone()
        .unwrap_or_else(|| hook.target_url.clone());
    let body = delivery
        .request_body
        .clone()
        .ok_or_else(|| AppError::BadRequest("Delivery has no stored body".to_string()))?;
    let mut mac = Hmac::<Sha256>::new_from_slice(hook.secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(&body);
    let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    let client = reqwest::Client::builder()
        .timeout(WEBHOOK_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| AppError::Internal(anyhow::anyhow!(error)))?;
    let state = state.clone();
    let event = delivery.event.clone();
    let event_id = delivery.event_id;
    let start_attempt = delivery.attempts;
    tokio::spawn(async move {
        deliver_webhook(
            &state,
            &client,
            delivery_id,
            start_attempt,
            hook.id,
            workspace_id,
            &target_url,
            &event,
            event_id,
            &body,
            &signature,
        )
        .await;
    });
    Ok(())
}

pub(crate) fn http_job_url_allowed(
    url: &str,
    allowed_hosts: &[String],
    allow_private: bool,
) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if allowed_hosts
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(&host))
    {
        return true;
    }
    if allow_private {
        return true;
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return false;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        match ip {
            std::net::IpAddr::V4(ip) => {
                if ip.is_private()
                    || ip.is_loopback()
                    || ip.is_link_local()
                    || ip.is_unspecified()
                    || ip.is_broadcast()
                {
                    return false;
                }
            }
            std::net::IpAddr::V6(ip) => {
                if ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
                {
                    return false;
                }
            }
        }
    }
    true
}

async fn execute_job(state: &AppState, job: &squid_db::JobRecord) -> AppResult<()> {
    match job.kind.as_str() {
        "maintenance.prune" => {
            state
                .db
                .prune_expired_state()
                .await
                .map_err(AppError::Internal)?;
            Ok(())
        }
        "webhook.replay" => {
            let id = job
                .payload
                .get("deliveryId")
                .and_then(|value| value.as_str())
                .and_then(|value| Uuid::parse_str(value).ok())
                .ok_or_else(|| {
                    AppError::BadRequest("webhook.replay requires deliveryId".to_string())
                })?;
            let workspace_id = job
                .workspace_id
                .ok_or_else(|| AppError::BadRequest("workspace is required".to_string()))?;
            replay_webhook_delivery(state, workspace_id, id)
                .await
                .map(|_| ())
        }
        "http.request" => {
            let url = job
                .payload
                .get("url")
                .and_then(|value| value.as_str())
                .ok_or_else(|| AppError::BadRequest("http.request requires url".to_string()))?;
            if !http_job_url_allowed(
                url,
                &state.config.http_job_allowed_hosts,
                state.config.http_job_allow_private,
            ) {
                return Err(AppError::BadRequest(
                    "http.request destination is not allowed".to_string(),
                ));
            }
            let method = job
                .payload
                .get("method")
                .and_then(|value| value.as_str())
                .unwrap_or("POST")
                .to_ascii_uppercase();
            let method = reqwest::Method::from_bytes(method.as_bytes())
                .map_err(|_| AppError::BadRequest("Invalid HTTP method".to_string()))?;
            let mut request = reqwest::Client::new()
                .request(method, url)
                .timeout(std::time::Duration::from_secs(10));
            if let Some(headers) = job
                .payload
                .get("headers")
                .and_then(|value| value.as_object())
            {
                for (name, value) in headers {
                    if let Some(value) = value.as_str() {
                        request = request.header(name, value);
                    }
                }
            }
            if let Some(body) = job.payload.get("body") {
                request = request.json(body);
            }
            let response = request
                .send()
                .await
                .map_err(|error| AppError::Internal(anyhow::anyhow!(error)))?;
            if !response.status().is_success() {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "http.request returned {}",
                    response.status()
                )));
            }
            Ok(())
        }
        other => Err(AppError::BadRequest(format!("Unknown job kind '{other}'"))),
    }
}

pub(crate) async fn run_job_now(
    state: &AppState,
    workspace_id: Uuid,
    id: Uuid,
) -> AppResult<squid_db::JobRecord> {
    let job = state
        .db
        .get_job(id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Job not found".to_string()))?;
    if job.workspace_id.is_some() && job.workspace_id != Some(workspace_id) {
        return Err(AppError::NotFound("Job not found".to_string()));
    }
    match execute_job(state, &job).await {
        Ok(()) => {
            state
                .db
                .complete_job(id)
                .await
                .map_err(AppError::Internal)?;
        }
        Err(error) => {
            state
                .db
                .fail_job(id, &error.to_string())
                .await
                .map_err(AppError::Internal)?;
        }
    }
    state
        .db
        .get_job(id)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Job not found".to_string()))
}

pub async fn run_job_worker(state: AppState) {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
    loop {
        ticker.tick().await;
        let Ok(jobs) = state.db.claim_due_jobs(10).await else {
            continue;
        };
        for job in jobs {
            match execute_job(&state, &job).await {
                Ok(()) => {
                    let _ = state.db.complete_job(job.id).await;
                    if job.kind == "maintenance.prune" {
                        let _ = state
                            .db
                            .enqueue_job(
                                None,
                                "maintenance.prune",
                                &serde_json::json!({}),
                                chrono::Utc::now() + chrono::Duration::hours(1),
                                3,
                            )
                            .await;
                    }
                }
                Err(error) => {
                    let _ = state.db.fail_job(job.id, &error.to_string()).await;
                }
            }
        }
    }
}

pub async fn run_maintenance_worker(state: AppState) {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(3600));
    loop {
        ticker.tick().await;
        match state.db.prune_expired_state().await {
            Ok((idempotency, deliveries)) => {
                if idempotency > 0 || deliveries > 0 {
                    tracing::info!(
                        idempotency_keys = idempotency,
                        webhook_deliveries = deliveries,
                        "pruned expired integration state"
                    );
                }
            }
            Err(error) => tracing::warn!(%error, "maintenance worker failed"),
        }
    }
}

pub async fn run_webhook_replay_worker(state: AppState) {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        ticker.tick().await;
        match state.db.list_replayable_webhook_deliveries(20).await {
            Ok(due) => {
                for (workspace_id, delivery_id) in due {
                    if let Err(error) =
                        requeue_webhook_delivery(&state, workspace_id, delivery_id).await
                    {
                        tracing::warn!(
                            %error,
                            %workspace_id,
                            %delivery_id,
                            "failed to requeue webhook delivery"
                        );
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "webhook replay worker query failed"),
        }
    }
}

async fn require_capability(
    state: &AppState,
    headers: &HeaderMap,
    cap: Capability,
) -> AppResult<squid_auth::AuthContext> {
    let auth = require(state, headers).await?;
    if role_can(state, &auth, cap).await? {
        Ok(auth)
    } else {
        Err(AppError::Forbidden(
            "You do not have permission to perform this action".to_string(),
        ))
    }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn query_value(query: Option<&str>, key: &str) -> Option<String> {
    query?.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.replace("+", " ").replace("%20", " ").to_string())
    })
}

fn custom_values(
    object: &squid_core::ObjectMetadata,
    body: &serde_json::Value,
) -> Vec<(String, squid_core::FieldMetadataType, serde_json::Value)> {
    let Some(map) = body.as_object() else {
        return Vec::new();
    };
    object
        .fields
        .iter()
        .filter(|field| {
            !field.is_system && field.field_type != squid_core::FieldMetadataType::MorphRelation
        })
        .filter_map(|field| {
            map.get(&field.name)
                .cloned()
                .map(|value| (field.name.clone(), field.field_type, value))
        })
        .collect()
}

pub async fn custom_rest_fallback(
    State(state): State<AppState>,
    method: axum::http::Method,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> AppResult<axum::response::Response> {
    use axum::response::IntoResponse;

    let path = uri.path().trim_start_matches('/');
    let Some(rest) = path.strip_prefix("rest/") else {
        return Err(AppError::NotFound("Route not found".to_string()));
    };
    let mut segments = rest.split('/').filter(|segment| !segment.is_empty());
    let Some(object_name) = segments.next() else {
        return Err(AppError::NotFound("Route not found".to_string()));
    };
    let id_segment = segments.next();
    if segments.next().is_some() {
        return Err(AppError::NotFound("Route not found".to_string()));
    }
    if object_name.ends_with("groupBy") {
        return Err(AppError::NotFound("Route not found".to_string()));
    }

    let auth = require(&state, &headers).await?;
    let object = state
        .db
        .find_object_by_name(auth.workspace_id, object_name)
        .await
        .map_err(AppError::Internal)?
        .ok_or_else(|| AppError::NotFound("Object not found".to_string()))?;
    if object.is_system {
        return Err(AppError::NotFound("Object not found".to_string()));
    }

    if method == axum::http::Method::POST && id_segment == Some("upsert") {
        let auth = require_capability(&state, &headers, Capability::Update).await?;
        let payload: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|_| AppError::BadRequest("Invalid JSON body".to_string()))?;
        let records = payload
            .as_array()
            .cloned()
            .or_else(|| {
                payload
                    .get("records")
                    .and_then(|value| value.as_array())
                    .cloned()
            })
            .ok_or_else(|| {
                AppError::BadRequest("Expected a JSON array or { records: [...] }".to_string())
            })?;
        let mut upserted = Vec::new();
        for record in records {
            let id = record
                .get("id")
                .and_then(|value| value.as_str())
                .map(|value| {
                    Uuid::parse_str(value)
                        .map_err(|_| AppError::BadRequest("Invalid record id".to_string()))
                })
                .transpose()?;
            let values = custom_values(&object, &record);
            let result = if let Some(id) = id {
                state
                    .db
                    .update_custom_record(auth.workspace_id, &object.name_singular, id, &values)
                    .await
                    .map_err(AppError::Internal)?
            } else {
                if values.is_empty() {
                    continue;
                }
                Some(
                    state
                        .db
                        .create_custom_record(auth.workspace_id, &object.name_singular, &values)
                        .await
                        .map_err(AppError::Internal)?,
                )
            };
            if let Some(result) = result {
                upserted.push(result);
            }
        }
        let mut data = BTreeMap::new();
        let count = upserted.len();
        data.insert(object.name_plural.clone(), upserted);
        return Ok(Json(json!({ "data": data, "upserted": count })).into_response());
    }

    if method == axum::http::Method::POST && id_segment == Some("import") {
        let auth = require_capability(&state, &headers, Capability::Update).await?;
        let (columns, rows) = csv_import_rows(&body)?;
        let lower_columns = columns
            .iter()
            .map(|column| column.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let mut imported = 0i64;
        for values in rows {
            let mut record = Vec::new();
            for field in &object.fields {
                if field.is_system
                    || field.field_type == squid_core::FieldMetadataType::MorphRelation
                {
                    continue;
                }
                let Some(index) = lower_columns
                    .iter()
                    .position(|column| column == &field.name.to_ascii_lowercase())
                else {
                    continue;
                };
                let Some(value) = values.get(index).filter(|value| !value.is_empty()) else {
                    continue;
                };
                record.push((
                    field.name.clone(),
                    field.field_type,
                    serde_json::Value::String(value.clone()),
                ));
            }
            if record.is_empty() {
                continue;
            }
            state
                .db
                .create_custom_record(auth.workspace_id, &object.name_singular, &record)
                .await
                .map_err(AppError::Internal)?;
            imported += 1;
        }
        return Ok(Json(json!({ "imported": imported })).into_response());
    }

    let id = id_segment
        .map(|value| {
            Uuid::parse_str(value).map_err(|_| AppError::BadRequest("Invalid id".to_string()))
        })
        .transpose()?;
    let query = uri.query();

    match (method, id) {
        (axum::http::Method::GET, None) => {
            let auth = require_capability(&state, &headers, Capability::Read).await?;
            let limit = query_value(query, "limit")
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(20)
                .clamp(1, state.config.max_page_size);
            let offset = query_value(query, "offset")
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0)
                .max(0);
            let (records, total) = state
                .db
                .list_custom_records(auth.workspace_id, &object.name_singular, limit, offset)
                .await
                .map_err(AppError::Internal)?;
            let mut data = BTreeMap::new();
            data.insert(object.name_plural.clone(), records);
            Ok(Json(serde_json::json!({
                "data": data,
                "totalCount": total,
                "pageInfo": {
                    "hasNextPage": offset + limit < total,
                    "hasPreviousPage": offset > 0
                }
            }))
            .into_response())
        }
        (axum::http::Method::GET, Some(id)) => {
            let auth = require_capability(&state, &headers, Capability::Read).await?;
            let record = state
                .db
                .get_custom_record(auth.workspace_id, &object.name_singular, id)
                .await
                .map_err(AppError::Internal)?
                .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
            Ok(Json(serde_json::json!({
                "data": { object.name_singular.clone(): record }
            }))
            .into_response())
        }
        (axum::http::Method::POST, None) => {
            let auth = require_capability(&state, &headers, Capability::Update).await?;
            let payload: serde_json::Value = serde_json::from_slice(&body)
                .map_err(|_| AppError::BadRequest("Invalid JSON body".to_string()))?;
            let values = custom_values(&object, &payload);
            if values.is_empty() {
                return Err(AppError::BadRequest(
                    "No writable fields supplied".to_string(),
                ));
            }
            let record = state
                .db
                .create_custom_record(auth.workspace_id, &object.name_singular, &values)
                .await
                .map_err(AppError::Internal)?;
            let key = format!("create{}", capitalize(&object.name_singular));
            Ok((
                axum::http::StatusCode::CREATED,
                Json(serde_json::json!({ "data": { key: record } })),
            )
                .into_response())
        }
        (axum::http::Method::PATCH, Some(id)) => {
            let auth = require_capability(&state, &headers, Capability::Update).await?;
            let payload: serde_json::Value = serde_json::from_slice(&body)
                .map_err(|_| AppError::BadRequest("Invalid JSON body".to_string()))?;
            let values = custom_values(&object, &payload);
            let record = state
                .db
                .update_custom_record(auth.workspace_id, &object.name_singular, id, &values)
                .await
                .map_err(AppError::Internal)?
                .ok_or_else(|| AppError::NotFound("Record not found".to_string()))?;
            let key = format!("update{}", capitalize(&object.name_singular));
            Ok(Json(serde_json::json!({ "data": { key: record } })).into_response())
        }
        (axum::http::Method::DELETE, Some(id)) => {
            let soft = query_value(query, "soft_delete").as_deref() == Some("true");
            let capability = if soft {
                Capability::SoftDelete
            } else {
                Capability::Destroy
            };
            let auth = require_capability(&state, &headers, capability).await?;
            let deleted = state
                .db
                .delete_custom_record(auth.workspace_id, &object.name_singular, id, soft)
                .await
                .map_err(AppError::Internal)?;
            if !deleted {
                return Err(AppError::NotFound("Record not found".to_string()));
            }
            let key = format!("delete{}", capitalize(&object.name_singular));
            Ok(Json(serde_json::json!({ "data": { key: { "id": id } } })).into_response())
        }
        _ => Err(AppError::NotFound("Route not found".to_string())),
    }
}

fn opportunity_relation_value(
    opportunity: &Opportunity,
    field: &str,
    companies: &HashMap<Uuid, Company>,
    people: &HashMap<Uuid, squid_core::Person>,
) -> Option<SortValue> {
    if let Some(rest) = field.strip_prefix("company.") {
        let company = companies.get(&opportunity.company_id?)?;
        return company_value(company, rest);
    }
    if let Some(rest) = field.strip_prefix("pointOfContact.") {
        let person = people.get(&opportunity.point_of_contact_id?)?;
        return person_value(person, rest);
    }
    opportunity_value(opportunity, field)
}

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
        ("companyId", "eq") => Ok(FilterExpr::CompanyId(UuidOp::Eq(parse_uuid(&value)?))),
        ("companyId", "neq") => Ok(FilterExpr::CompanyId(UuidOp::Neq(parse_uuid(&value)?))),
        ("companyId", "in") => Ok(FilterExpr::CompanyId(UuidOp::In(
            values
                .iter()
                .map(|value| parse_uuid(value))
                .collect::<AppResult<_>>()?,
        ))),
        ("companyId", "is") => Ok(FilterExpr::CompanyId(UuidOp::IsNull(
            is_null.expect("is operator"),
        ))),
        ("company.id", "eq") => Ok(FilterExpr::RelatedCompany(Box::new(FilterExpr::Id(
            UuidOp::Eq(parse_uuid(&value)?),
        )))),
        ("company.id", "neq") => Ok(FilterExpr::RelatedCompany(Box::new(FilterExpr::Id(
            UuidOp::Neq(parse_uuid(&value)?),
        )))),
        ("company.id", "in") => Ok(FilterExpr::RelatedCompany(Box::new(FilterExpr::Id(
            UuidOp::In(
                values
                    .iter()
                    .map(|value| parse_uuid(value))
                    .collect::<AppResult<_>>()?,
            ),
        )))),
        ("company.id", "is") => Ok(FilterExpr::RelatedCompany(Box::new(FilterExpr::Id(
            UuidOp::IsNull(is_null.expect("is operator")),
        )))),
        ("company.createdAt", op) => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyCreatedAt(build_date_op(op, values, is_null)?),
        ))),
        ("company.updatedAt", op) => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyUpdatedAt(build_date_op(op, values, is_null)?),
        ))),
        ("company.name", "eq") => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyName(StringOp::Eq(value.clone())),
        ))),
        ("company.name", "neq") => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyName(StringOp::Neq(value.clone())),
        ))),
        ("company.name", "contains") => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyName(StringOp::Like(format!("%{value}%"))),
        ))),
        ("company.name", "startsWith") => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyName(StringOp::StartsWith(value.clone())),
        ))),
        ("company.name", "endsWith") => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyName(StringOp::EndsWith(value.clone())),
        ))),
        ("company.name", "like") => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyName(StringOp::Like(value.clone())),
        ))),
        ("company.name", "ilike") => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyName(StringOp::ILike(value.clone())),
        ))),
        ("company.name", "is") => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyName(StringOp::IsNull(is_null.expect("is operator"))),
        ))),
        ("company.accountOwnerId", "eq") => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyAccountOwnerId(UuidOp::Eq(parse_uuid(&value)?)),
        ))),
        ("company.accountOwnerId", "neq") => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyAccountOwnerId(UuidOp::Neq(parse_uuid(&value)?)),
        ))),
        ("company.accountOwnerId", "in") => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyAccountOwnerId(UuidOp::In(
                values
                    .iter()
                    .map(|value| parse_uuid(value))
                    .collect::<AppResult<_>>()?,
            )),
        ))),
        ("company.accountOwnerId", "is") => Ok(FilterExpr::RelatedCompany(Box::new(
            FilterExpr::CompanyAccountOwnerId(UuidOp::IsNull(is_null.expect("is operator"))),
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
        assert!(matches!(
            parse_filter("companyId[eq]:6c774d80-730f-4e23-b707-39053977cbff").unwrap(),
            FilterExpr::CompanyId(UuidOp::Eq(_))
        ));
        assert_eq!(
            parse_filter("companyId[is]:NULL").unwrap(),
            FilterExpr::CompanyId(UuidOp::IsNull(true))
        );
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

const GROUP_PERSON_FIELDS: &[&str] = &[
    "id",
    "createdAt",
    "updatedAt",
    "deletedAt",
    "name.firstName",
    "name.lastName",
    "emails.primaryEmail",
    "linkedinLink.primaryLinkUrl",
    "linkedinLink.primaryLinkLabel",
    "jobTitle",
    "phones.primaryPhoneNumber",
    "phones.primaryPhoneCountryCode",
    "phones.primaryPhoneCallingCode",
    "avatarUrl",
    "companyId",
];
const GROUP_COMPANY_FIELDS: &[&str] = &[
    "id",
    "createdAt",
    "updatedAt",
    "deletedAt",
    "name",
    "domainName.primaryLinkUrl",
    "domainName.primaryLinkLabel",
    "address.addressStreet1",
    "address.addressStreet2",
    "address.addressCity",
    "address.addressState",
    "address.addressPostcode",
    "address.addressCountry",
    "linkedinLink.primaryLinkUrl",
    "linkedinLink.primaryLinkLabel",
    "annualRevenue.amountMicros",
    "annualRevenue.currencyCode",
    "accountOwnerId",
];
const GROUP_OPPORTUNITY_FIELDS: &[&str] = &[
    "id",
    "createdAt",
    "updatedAt",
    "deletedAt",
    "name",
    "amount.amountMicros",
    "amount.currencyCode",
    "closeDate",
    "stage",
    "companyId",
    "pointOfContactId",
    "ownerId",
];
const GROUP_TASK_FIELDS: &[&str] = &[
    "id",
    "createdAt",
    "updatedAt",
    "deletedAt",
    "title",
    "bodyV2.markdown",
    "dueAt",
    "status",
    "assigneeId",
];
const GROUP_NOTE_FIELDS: &[&str] = &[
    "id",
    "createdAt",
    "updatedAt",
    "deletedAt",
    "title",
    "bodyV2.markdown",
];
const GROUP_TASK_TARGET_FIELDS: &[&str] = &[
    "id",
    "createdAt",
    "updatedAt",
    "deletedAt",
    "taskId",
    "targetCompanyId",
    "targetOpportunityId",
    "targetPersonId",
];
const GROUP_NOTE_TARGET_FIELDS: &[&str] = &[
    "id",
    "createdAt",
    "updatedAt",
    "deletedAt",
    "noteId",
    "targetCompanyId",
    "targetOpportunityId",
    "targetPersonId",
];

#[derive(Debug, Deserialize)]
pub struct GroupByParams {
    pub group_by: Option<String>,
    pub aggregate: Option<String>,
    pub include_records_sample: Option<bool>,
    pub order_by_for_records: Option<String>,
    pub limit: Option<i64>,
    #[serde(rename = "offsetForRecords", alias = "offset_for_records")]
    pub offset_for_records: Option<i64>,
    pub filter: Option<String>,
    #[serde(rename = "viewId", alias = "view_id")]
    pub view_id: Option<String>,
}

async fn group_by_response(
    state: &AppState,
    workspace_id: Uuid,
    params: &GroupByParams,
    object_name: &str,
    allowed: &[&str],
    mut records: Vec<serde_json::Value>,
) -> AppResult<Json<serde_json::Value>> {
    let mut request = crate::group_by::parse_request(
        params.group_by.as_deref(),
        params.aggregate.as_deref(),
        params.include_records_sample,
        params.order_by_for_records.as_deref(),
        params.limit,
        params.offset_for_records,
        allowed,
    )?;
    let date_fields = crate::group_by::date_field_set();

    if let Some(raw) = params.filter.as_deref() {
        let filter = parse_generic_filter(raw, allowed)?;
        records
            .retain(|record| filtering::matches(&filter, &|field| group_by_value(record, field)));
    }

    if let Some(view_id) = params
        .view_id
        .as_deref()
        .and_then(|id| Uuid::parse_str(id).ok())
    {
        if let Some(view) = state
            .db
            .get_view(workspace_id, view_id)
            .await
            .map_err(AppError::Internal)?
        {
            let fields = state
                .db
                .list_fields(workspace_id, None)
                .await
                .map_err(AppError::Internal)?;
            let field_names: std::collections::HashMap<Uuid, String> = fields
                .into_iter()
                .filter(|field| field.object_metadata_id == view.object_metadata_id)
                .map(|field| (field.id, field.name))
                .collect();
            if let Some(node) = crate::group_by::view_filter_node(&view, &field_names) {
                records.retain(|record| crate::group_by::matches(&node, record, &date_fields));
            }
            if params.order_by_for_records.is_none() {
                request.order_by_for_records = view
                    .view_sorts
                    .iter()
                    .filter_map(|sort| {
                        let name = field_names.get(&sort.field_metadata_id)?;
                        let field = match &sort.sub_field_name {
                            Some(sub_field) => format!("{name}.{sub_field}"),
                            None => name.clone(),
                        };
                        Some(crate::group_by::SortTerm {
                            field,
                            descending: sort.direction == squid_core::ViewSortDirection::Desc,
                            nulls_first: false,
                        })
                    })
                    .collect();
            }
        }
    }
    let _ = object_name;

    Ok(Json(serde_json::Value::Array(crate::group_by::execute(
        &records,
        &request,
        &date_fields,
    )?)))
}

fn group_by_value(record: &serde_json::Value, field: &str) -> Option<crate::sorting::SortValue> {
    let mut value = record;
    for part in field.split('.') {
        value = value.get(part)?;
    }
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(value) if value.is_empty() => None,
        serde_json::Value::String(value) => Some(crate::sorting::SortValue::Text(value.clone())),
        serde_json::Value::Number(value) => value.as_f64().map(crate::sorting::SortValue::Number),
        serde_json::Value::Bool(value) => Some(crate::sorting::SortValue::Bool(*value)),
        _ => None,
    }
}

fn to_group_records<T: serde::Serialize>(records: &[T]) -> AppResult<Vec<serde_json::Value>> {
    records
        .iter()
        .map(|record| {
            serde_json::to_value(record).map_err(|error| AppError::Internal(anyhow::anyhow!(error)))
        })
        .collect()
}

async fn all_people(state: &AppState, workspace_id: Uuid) -> AppResult<Vec<serde_json::Value>> {
    let mut records = Vec::new();
    let mut offset = 0;
    loop {
        let page = state
            .db
            .list_persons_page(workspace_id, 1000, offset, None, None, &[], None)
            .await
            .map_err(AppError::Internal)?;
        let count = page.persons.len();
        records.extend(to_group_records(&page.persons)?);
        if !page.has_next_page || count == 0 {
            break;
        }
        offset += count as i64;
    }
    Ok(records)
}

pub async fn group_by_people(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GroupByParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let records = all_people(&state, auth.workspace_id).await?;
    group_by_response(
        &state,
        auth.workspace_id,
        &params,
        "person",
        GROUP_PERSON_FIELDS,
        records,
    )
    .await
}

pub async fn group_by_companies(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GroupByParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let (companies, _) = state
        .db
        .list_companies(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;
    let records = to_group_records(&companies)?;
    group_by_response(
        &state,
        auth.workspace_id,
        &params,
        "company",
        GROUP_COMPANY_FIELDS,
        records,
    )
    .await
}

pub async fn group_by_opportunities(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GroupByParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let (opportunities, _) = state
        .db
        .list_opportunities(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;
    let records = to_group_records(&opportunities)?;
    group_by_response(
        &state,
        auth.workspace_id,
        &params,
        "opportunity",
        GROUP_OPPORTUNITY_FIELDS,
        records,
    )
    .await
}

pub async fn group_by_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GroupByParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let (tasks, _) = state
        .db
        .list_tasks(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;
    let records = to_group_records(&tasks)?;
    group_by_response(
        &state,
        auth.workspace_id,
        &params,
        "task",
        GROUP_TASK_FIELDS,
        records,
    )
    .await
}

pub async fn group_by_notes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GroupByParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let (notes, _) = state
        .db
        .list_notes(auth.workspace_id, 10_000, 0)
        .await
        .map_err(AppError::Internal)?;
    let records = to_group_records(&notes)?;
    group_by_response(
        &state,
        auth.workspace_id,
        &params,
        "note",
        GROUP_NOTE_FIELDS,
        records,
    )
    .await
}

pub async fn group_by_task_targets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GroupByParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let targets = state
        .db
        .list_task_targets(auth.workspace_id, None, None, None, None)
        .await
        .map_err(AppError::Internal)?;
    let records = to_group_records(&targets)?;
    group_by_response(
        &state,
        auth.workspace_id,
        &params,
        "taskTarget",
        GROUP_TASK_TARGET_FIELDS,
        records,
    )
    .await
}

pub async fn group_by_note_targets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GroupByParams>,
) -> AppResult<Json<serde_json::Value>> {
    let auth = require_capability(&state, &headers, Capability::Read).await?;
    let targets = state
        .db
        .list_note_targets(auth.workspace_id, None, None, None, None)
        .await
        .map_err(AppError::Internal)?;
    let records = to_group_records(&targets)?;
    group_by_response(
        &state,
        auth.workspace_id,
        &params,
        "noteTarget",
        GROUP_NOTE_TARGET_FIELDS,
        records,
    )
    .await
}
