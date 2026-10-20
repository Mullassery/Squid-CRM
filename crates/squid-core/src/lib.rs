//! Shared configuration, domain models and error contract for Squid-CRM.
//!
//! The error envelope mirrors the audited upstream REST contract:
//! `{ "statusCode": u16, "error": String, "messages": [String], "code": Option<String> }`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

pub mod config;
pub mod error;
pub mod metadata;

pub use async_graphql::Json as GqlJson;
pub use config::Config;
pub use error::{AppError, AppResult};
pub use metadata::*;

/// A workspace member's role. Upstream models this as a role entity with
/// permission flags; the slice keeps the built-in roles as an enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, async_graphql::Enum)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Role {
    Owner,
    Admin,
    Member,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "OWNER",
            Role::Admin => "ADMIN",
            Role::Member => "MEMBER",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "OWNER" => Some(Role::Owner),
            "ADMIN" => Some(Role::Admin),
            "MEMBER" => Some(Role::Member),
            _ => None,
        }
    }

    /// Roles allowed to administer workspace settings.
    pub fn can_administer(self) -> bool {
        matches!(self, Role::Owner | Role::Admin)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: Uuid,
    pub display_name: String,
    pub subdomain: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMember {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub role: Role,
}

/// Workspace API key metadata (the secret token is only returned once and is
/// stored hashed).
#[derive(Debug, Clone, Serialize, Deserialize, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct ApiKey {
    pub id: Uuid,
    pub name: String,
    pub role: Role,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Composite `FullNameMetadata` shape used by upstream for `person.name`.
#[derive(
    Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, async_graphql::SimpleObject,
)]
pub struct FullName {
    #[serde(rename = "firstName")]
    pub first_name: String,
    #[serde(rename = "lastName")]
    pub last_name: String,
}

/// Composite `EmailsMetadata` shape used by upstream for `person.emails`.
#[derive(
    Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, async_graphql::SimpleObject,
)]
pub struct Emails {
    #[serde(rename = "primaryEmail")]
    pub primary_email: String,
    #[serde(rename = "additionalEmails", default)]
    pub additional_emails: Vec<String>,
}

/// Composite `LinksMetadata` shape (`linkedinLink`).
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, async_graphql::SimpleObject,
)]
pub struct Links {
    #[serde(rename = "primaryLinkLabel", default)]
    pub primary_link_label: String,
    #[serde(rename = "primaryLinkUrl", default)]
    pub primary_link_url: String,
    #[serde(rename = "secondaryLinks", default)]
    pub secondary_links: Vec<async_graphql::Json<serde_json::Value>>,
}

/// Composite `PhonesMetadata` shape.
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, async_graphql::SimpleObject,
)]
pub struct Phones {
    #[serde(rename = "primaryPhoneNumber", default)]
    pub primary_phone_number: String,
    #[serde(rename = "primaryPhoneCountryCode", default)]
    pub primary_phone_country_code: String,
    #[serde(rename = "primaryPhoneCallingCode", default)]
    pub primary_phone_calling_code: String,
    #[serde(rename = "additionalPhones", default)]
    pub additional_phones: Vec<async_graphql::Json<serde_json::Value>>,
}

/// `createdBy` / `updatedBy` actor shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, async_graphql::SimpleObject)]
pub struct Actor {
    pub source: String,
    #[serde(rename = "workspaceMemberId")]
    pub workspace_member_id: Option<Uuid>,
    pub name: String,
    pub context: async_graphql::Json<serde_json::Value>,
}

impl Default for Actor {
    fn default() -> Self {
        Self {
            source: "API".to_string(),
            workspace_member_id: None,
            name: "API".to_string(),
            context: async_graphql::Json(serde_json::json!({})),
        }
    }
}

/// Phase 4 subset of the standard `person` object, aligned with the captured
/// v2.49.0 REST/GraphQL field set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, async_graphql::SimpleObject)]
pub struct Person {
    pub id: Uuid,
    #[serde(rename = "createdAt")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "updatedAt")]
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "deletedAt")]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub name: FullName,
    pub emails: Emails,
    #[serde(rename = "linkedinLink")]
    pub linkedin_link: Links,
    #[serde(rename = "jobTitle")]
    pub job_title: Option<String>,
    pub phones: Phones,
    #[serde(rename = "avatarUrl")]
    pub avatar_url: String,
    #[serde(rename = "avatarFile")]
    pub avatar_file: Option<async_graphql::Json<serde_json::Value>>,
    pub position: f64,
    #[serde(rename = "createdBy")]
    pub created_by: Actor,
    #[serde(rename = "updatedBy")]
    pub updated_by: Actor,
    #[serde(rename = "searchVector")]
    pub search_vector: Option<String>,
    #[serde(rename = "companyId")]
    pub company_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CreatePersonInput {
    pub name: FullName,
    pub emails: Emails,
    #[serde(rename = "jobTitle", default)]
    pub job_title: Option<String>,
    #[serde(rename = "linkedinLink", default)]
    pub linkedin_link: Option<Links>,
    #[serde(default)]
    pub phones: Option<Phones>,
    #[serde(rename = "avatarUrl", default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(rename = "companyId", default)]
    pub company_id: Option<Uuid>,
}

/// Pagination envelope identical to the audited `PageInfo` interface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, async_graphql::SimpleObject)]
pub struct PageInfo {
    #[serde(rename = "hasNextPage")]
    pub has_next_page: bool,
    #[serde(rename = "hasPreviousPage")]
    pub has_previous_page: bool,
    #[serde(rename = "startCursor")]
    pub start_cursor: Option<String>,
    #[serde(rename = "endCursor")]
    pub end_cursor: Option<String>,
}

/// Partial person update. `None` means "leave unchanged"; clearing a nullable
/// field requires the metadata update contract (not implemented yet).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdatePersonInput {
    #[serde(default)]
    pub name: Option<FullName>,
    #[serde(default)]
    pub emails: Option<Emails>,
    #[serde(rename = "jobTitle", default)]
    pub job_title: Option<String>,
    #[serde(rename = "linkedinLink", default)]
    pub linkedin_link: Option<Links>,
    #[serde(default)]
    pub phones: Option<Phones>,
    #[serde(rename = "avatarUrl", default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(rename = "companyId", default)]
    pub company_id: Option<Uuid>,
}

/// Backend-neutral filter AST. REST/GraphQL parsers produce this; the DB layer
/// compiles it to parameterised SQL (column names never come from input).
#[derive(Debug, Clone, PartialEq)]
pub enum FilterExpr {
    Id(UuidOp),
    CreatedAt(DateOp),
    UpdatedAt(DateOp),
    FirstName(StringOp),
    LastName(StringOp),
    PrimaryEmail(StringOp),
    JobTitle(StringOp),
    And(Vec<FilterExpr>),
    Or(Vec<FilterExpr>),
    Not(Box<FilterExpr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UuidOp {
    Eq(Uuid),
    Neq(Uuid),
    In(Vec<Uuid>),
    IsNull(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringOp {
    Eq(String),
    Neq(String),
    Gt(String),
    Gte(String),
    Lt(String),
    Lte(String),
    In(Vec<String>),
    StartsWith(String),
    EndsWith(String),
    Like(String),
    ILike(String),
    Regex(String),
    IRegex(String),
    IsNull(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub enum DateOp {
    Eq(chrono::DateTime<chrono::Utc>),
    Neq(chrono::DateTime<chrono::Utc>),
    Gt(chrono::DateTime<chrono::Utc>),
    Gte(chrono::DateTime<chrono::Utc>),
    Lt(chrono::DateTime<chrono::Utc>),
    Lte(chrono::DateTime<chrono::Utc>),
    In(Vec<chrono::DateTime<chrono::Utc>>),
    IsNull(bool),
}

/// REST find-many envelope: `{ data: { <plural>: [...] }, totalCount, pageInfo }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindManyResponse<T> {
    pub data: std::collections::BTreeMap<String, Vec<T>>,
    #[serde(rename = "totalCount")]
    pub total_count: u64,
    #[serde(rename = "pageInfo")]
    pub page_info: PageInfo,
}

/// Error body shipped on every REST failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    #[serde(rename = "statusCode")]
    pub status_code: u16,
    pub error: String,
    pub messages: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let name = status_name(status).to_string();
        let body = ErrorBody {
            status_code: status.as_u16(),
            error: name,
            messages: vec![self.public_message().to_string()],
            code: self.error_code().map(ToString::to_string),
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "request failed");
        }
        (status, Json(body)).into_response()
    }
}

fn status_name(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "BadRequestException",
        StatusCode::UNAUTHORIZED => "UnauthorizedException",
        StatusCode::FORBIDDEN => "FORBIDDEN_EXCEPTION",
        StatusCode::NOT_FOUND => "NotFoundException",
        StatusCode::CONFLICT => "ConflictException",
        StatusCode::UNPROCESSABLE_ENTITY => "UnprocessableEntityException",
        StatusCode::TOO_MANY_REQUESTS => "TooManyRequestsException",
        StatusCode::PAYLOAD_TOO_LARGE => "PayloadTooLargeException",
        _ => "InternalServerErrorException",
    }
}

/// Convenience helper used by tests and handlers to assert the contract shape.
pub fn error_json(status: StatusCode, message: &str) -> serde_json::Value {
    json!({
        "statusCode": status.as_u16(),
        "error": status_name(status),
        "messages": [message],
    })
}
