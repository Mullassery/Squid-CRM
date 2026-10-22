//! GraphQL surface. The Phase 4 slice exposes the auth flow the frontend uses
//! (`getLoginTokenFromCredentials` -> `getAuthTokensFromLoginToken`) plus the
//! `person` object, so the existing React client can authenticate and read
//! people against the Rust backend.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use async_graphql::ErrorExtensions;
use async_graphql::{
    ComplexObject, Context, EmptySubscription, InputObject, Object, Schema, SimpleObject,
};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{Html, IntoResponse, Response};
use chrono::{DateTime, Utc};
use squid_auth::AuthToken;
use squid_auth::{hash_password, hash_token, verify_password, AuthContext};
use squid_core::{
    Address, ApiKey, AppError, AppResult, Company, CreateCompanyInput, CreateFieldMetadataInput,
    CreateNoteInput, CreateNoteTargetInput, CreateObjectMetadataInput, CreateOneIndexInput,
    CreateOpportunityInput, CreatePersonInput, CreateTaskInput, CreateTaskTargetInput,
    CreateViewFieldGroupInput, CreateViewFieldInput, CreateViewFilterGroupInput,
    CreateViewFilterInput, CreateViewGroupInput, CreateViewInput, CreateViewSortInput, Currency,
    CursorPagingInput, DateOp, DeleteOneIndexInput, FieldConnection, FieldFilterInput,
    FieldMetadata, FileFolder, FileItemInput, FilterExpr, GqlJson, Links, MaybeUndefined, Note,
    NoteTarget, ObjectConnection, ObjectEdge, ObjectFilterInput, ObjectMetadata, Opportunity,
    OpportunityStage, PageInfo, Person, RichText, Role, StringOp, Task, TaskStatus, TaskTarget,
    UpdateCompanyInput, UpdateFieldMetadataInput, UpdateNoteInput, UpdateObjectMetadataInput,
    UpdateOpportunityInput, UpdatePersonInput, UpdateTaskInput, UpdateViewFieldGroupInput,
    UpdateViewFieldInput, UpdateViewFilterGroupInput, UpdateViewFilterInput, UpdateViewGroupInput,
    UpdateViewInput, UpdateViewSortInput, User as CoreUser, UuidOp, View, ViewField,
    ViewFieldGroup, ViewFilter, ViewFilterGroup, ViewGroup, ViewSort, ViewType,
};
use squid_db::{OrderTerm, PersonColumn};
use uuid::Uuid;

use crate::auth::{normalize_email, unique_subdomain};
use crate::current_user::{self, User, Workspace, WorkspaceMemberGql};
use crate::sorting::{sort_records, SortTerm};
use crate::{
    clear_session_cookie, resolve_auth_with_policy, session_cookie, AppState, AuthTokenPair,
};

pub type SquidSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

#[derive(Clone)]
pub struct GqlCtx {
    pub state: AppState,
    pub auth: Option<AuthContext>,
    pub cookie: Arc<Mutex<Option<CookieAction>>>,
}

#[derive(Clone)]
pub enum CookieAction {
    Set {
        token: String,
        expires_at: DateTime<Utc>,
    },
    Clear,
}

#[derive(Debug, SimpleObject)]
pub struct AuthPayload {
    pub tokens: AuthTokenPair,
    pub user: User,
    pub workspace: Workspace,
}

#[derive(Debug, SimpleObject)]
pub struct AuthTokensPayload {
    pub tokens: AuthTokenPair,
}

#[derive(Debug, SimpleObject)]
pub struct LoginTokenPayload {
    pub login_token: AuthToken,
}

/// Upstream exposes connection cursors as a dedicated `Cursor` scalar.
#[derive(Debug, Clone)]
pub struct Cursor(pub String);

#[async_graphql::Scalar(name = "Cursor")]
impl async_graphql::ScalarType for Cursor {
    fn parse(value: async_graphql::Value) -> async_graphql::InputValueResult<Self> {
        match value {
            async_graphql::Value::String(value) => Ok(Self(value)),
            _ => Err(async_graphql::InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> async_graphql::Value {
        async_graphql::Value::String(self.0.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, async_graphql::Enum)]
pub enum RecordShareAccessLevelEnum {
    Read,
    ReadWrite,
    Full,
    None,
}

#[derive(Debug, InputObject)]
pub struct ShareWithInput {
    pub workspace_member_id: Option<uuid::Uuid>,
    pub role_id: Option<uuid::Uuid>,
    pub everyone: Option<bool>,
    pub access_level: Option<RecordShareAccessLevelEnum>,
}

#[derive(Debug, InputObject)]
pub struct CreateApiKeyInput {
    pub name: String,
    pub expires_at: String,
    pub revoked_at: Option<String>,
    pub role_id: uuid::Uuid,
}

#[derive(Debug, InputObject)]
pub struct RevokeApiKeyInput {
    pub id: uuid::Uuid,
}

#[derive(Debug, InputObject)]
pub struct GetApiKeyInput {
    pub id: uuid::Uuid,
}

#[derive(Debug, SimpleObject)]
pub struct ApiKeyToken {
    pub token: String,
}

#[derive(Debug, InputObject)]
pub struct FullNameCreateInput {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Debug, InputObject)]
pub struct EmailsCreateInput {
    pub primary_email: Option<String>,
    pub additional_emails: Option<Vec<String>>,
}

#[derive(Debug, InputObject)]
pub struct LinksCreateInput {
    pub primary_link_label: Option<String>,
    pub primary_link_url: Option<String>,
    pub secondary_links: Option<Vec<GqlJson<serde_json::Value>>>,
}

#[derive(Debug, InputObject)]
pub struct PhonesCreateInput {
    pub primary_phone_number: Option<String>,
    pub primary_phone_country_code: Option<String>,
    pub primary_phone_calling_code: Option<String>,
    pub additional_phones: Option<Vec<GqlJson<serde_json::Value>>>,
}

#[derive(Debug, InputObject)]
#[graphql(name = "FileItemInput")]
pub struct FileItemInputGql {
    pub file_id: Uuid,
    pub label: String,
}

impl From<FileItemInputGql> for FileItemInput {
    fn from(input: FileItemInputGql) -> Self {
        Self {
            file_id: input.file_id,
            label: input.label,
        }
    }
}

#[derive(Debug, SimpleObject)]
pub struct FileUploadTarget {
    pub file_id: Uuid,
    pub upload_url: String,
    pub content_type: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, SimpleObject)]
pub struct FileWithSignedUrl {
    pub id: Uuid,
    pub path: String,
    pub size: f64,
    pub created_at: DateTime<Utc>,
    pub url: String,
}

#[derive(Debug, SimpleObject)]
#[graphql(name = "PermissionFlag")]
pub struct PermissionFlagGql {
    pub id: Uuid,
    pub universal_identifier: Uuid,
    pub key: String,
    pub label: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub permission_type: String,
    pub application_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const PERMISSION_FLAGS: &[(&str, &str)] = &[
    ("API_KEYS_AND_WEBHOOKS", "API keys and webhooks"),
    ("WORKSPACE", "Workspace"),
    ("WORKSPACE_MEMBERS", "Workspace members"),
    ("ROLES", "Roles"),
    ("DATA_MODEL", "Data model"),
    ("SECURITY", "Security"),
    ("WORKFLOWS", "Workflows"),
    ("IMPERSONATE", "Impersonate"),
    ("SSO_BYPASS", "SSO bypass"),
    ("APPLICATIONS", "Applications"),
    ("MARKETPLACE_APPS", "Marketplace apps"),
    ("LAYOUTS", "Layouts"),
    ("BILLING", "Billing"),
    ("AI_SETTINGS", "AI settings"),
    ("AI", "AI"),
    ("VIEWS", "Views"),
    ("UPLOAD_FILE", "Upload file"),
    ("DOWNLOAD_FILE", "Download file"),
    ("SEND_EMAIL_TOOL", "Send email tool"),
    ("CREATE_CALENDAR_EVENT_TOOL", "Create calendar event tool"),
    ("HTTP_REQUEST_TOOL", "HTTP request tool"),
    ("CODE_INTERPRETER_TOOL", "Code interpreter tool"),
    ("IMPORT_CSV", "Import CSV"),
    ("EXPORT_CSV", "Export CSV"),
    ("CONNECTED_ACCOUNTS", "Connected accounts"),
    ("PROFILE_INFORMATION", "Profile information"),
];

#[derive(Debug, InputObject)]
#[graphql(name = "PersonCreateInput")]
pub struct PersonCreateInputGql {
    pub name: Option<FullNameCreateInput>,
    pub emails: Option<EmailsCreateInput>,
    pub job_title: Option<String>,
    pub linkedin_link: Option<LinksCreateInput>,
    pub phones: Option<PhonesCreateInput>,
    pub avatar_url: Option<String>,
    pub avatar_file: Option<Vec<FileItemInputGql>>,
    pub position: Option<f64>,
    pub company_id: Option<uuid::Uuid>,
}

impl From<PersonCreateInputGql> for CreatePersonInput {
    fn from(input: PersonCreateInputGql) -> Self {
        CreatePersonInput {
            name: input
                .name
                .map(|name| squid_core::FullName {
                    first_name: name.first_name.unwrap_or_default(),
                    last_name: name.last_name.unwrap_or_default(),
                })
                .unwrap_or_default(),
            emails: input
                .emails
                .map(|emails| squid_core::Emails {
                    primary_email: emails.primary_email.unwrap_or_default(),
                    additional_emails: emails.additional_emails.unwrap_or_default(),
                })
                .unwrap_or_default(),
            job_title: input.job_title,
            linkedin_link: input.linkedin_link.map(|links| squid_core::Links {
                primary_link_label: links.primary_link_label.unwrap_or_default(),
                primary_link_url: links.primary_link_url.unwrap_or_default(),
                secondary_links: links.secondary_links.unwrap_or_default(),
            }),
            phones: input.phones.map(|phones| squid_core::Phones {
                primary_phone_number: phones.primary_phone_number.unwrap_or_default(),
                primary_phone_country_code: phones.primary_phone_country_code.unwrap_or_default(),
                primary_phone_calling_code: phones.primary_phone_calling_code.unwrap_or_default(),
                additional_phones: phones.additional_phones.unwrap_or_default(),
            }),
            avatar_url: input.avatar_url,
            avatar_file: input
                .avatar_file
                .map(|items| items.into_iter().map(Into::into).collect()),
            position: input.position,
            company_id: input.company_id,
        }
    }
}

#[derive(Debug, InputObject)]
#[graphql(name = "PersonUpdateInput")]
pub struct PersonUpdateInputGql {
    pub name: MaybeUndefined<FullNameCreateInput>,
    pub emails: MaybeUndefined<EmailsCreateInput>,
    pub job_title: MaybeUndefined<String>,
    pub linkedin_link: MaybeUndefined<LinksCreateInput>,
    pub phones: MaybeUndefined<PhonesCreateInput>,
    pub avatar_url: MaybeUndefined<String>,
    pub avatar_file: MaybeUndefined<Vec<FileItemInputGql>>,
    pub position: MaybeUndefined<f64>,
    pub company_id: MaybeUndefined<uuid::Uuid>,
}

impl From<PersonUpdateInputGql> for UpdatePersonInput {
    fn from(input: PersonUpdateInputGql) -> Self {
        UpdatePersonInput {
            name: input.name.map_value(|name| squid_core::FullName {
                first_name: name.first_name.unwrap_or_default(),
                last_name: name.last_name.unwrap_or_default(),
            }),
            emails: input.emails.map_value(|emails| squid_core::Emails {
                primary_email: emails.primary_email.unwrap_or_default(),
                additional_emails: emails.additional_emails.unwrap_or_default(),
            }),
            job_title: input.job_title,
            linkedin_link: input.linkedin_link.map_value(|links| squid_core::Links {
                primary_link_label: links.primary_link_label.unwrap_or_default(),
                primary_link_url: links.primary_link_url.unwrap_or_default(),
                secondary_links: links.secondary_links.unwrap_or_default(),
            }),
            phones: input.phones.map_value(|phones| squid_core::Phones {
                primary_phone_number: phones.primary_phone_number.unwrap_or_default(),
                primary_phone_country_code: phones.primary_phone_country_code.unwrap_or_default(),
                primary_phone_calling_code: phones.primary_phone_calling_code.unwrap_or_default(),
                additional_phones: phones.additional_phones.unwrap_or_default(),
            }),
            avatar_url: input.avatar_url,
            avatar_file: input
                .avatar_file
                .map_value(|items| items.into_iter().map(Into::into).collect()),
            position: input.position,
            company_id: input.company_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, async_graphql::Enum)]
pub enum FilterIs {
    #[graphql(name = "NULL")]
    Null,
    #[graphql(name = "NOT_NULL")]
    NotNull,
}

#[derive(Debug, Default, InputObject)]
pub struct UuidFilter {
    pub eq: Option<uuid::Uuid>,
    pub neq: Option<uuid::Uuid>,
    pub r#in: Option<Vec<uuid::Uuid>>,
    pub is: Option<FilterIs>,
}

#[derive(Debug, Default, InputObject)]
pub struct StringFilter {
    pub eq: Option<String>,
    pub neq: Option<String>,
    pub gt: Option<String>,
    pub gte: Option<String>,
    pub lt: Option<String>,
    pub lte: Option<String>,
    pub r#in: Option<Vec<String>>,
    pub starts_with: Option<String>,
    pub ends_with: Option<String>,
    pub like: Option<String>,
    pub ilike: Option<String>,
    pub regex: Option<String>,
    pub iregex: Option<String>,
    pub is: Option<FilterIs>,
}

#[derive(Debug, Default, InputObject)]
pub struct DateTimeFilter {
    pub eq: Option<chrono::DateTime<chrono::Utc>>,
    pub neq: Option<chrono::DateTime<chrono::Utc>>,
    pub gt: Option<chrono::DateTime<chrono::Utc>>,
    pub gte: Option<chrono::DateTime<chrono::Utc>>,
    pub lt: Option<chrono::DateTime<chrono::Utc>>,
    pub lte: Option<chrono::DateTime<chrono::Utc>>,
    pub r#in: Option<Vec<chrono::DateTime<chrono::Utc>>>,
    pub is: Option<FilterIs>,
}

#[derive(Debug, Default, InputObject)]
pub struct FullNameFilterInput {
    pub first_name: Option<StringFilter>,
    pub last_name: Option<StringFilter>,
}

#[derive(Debug, Default, InputObject)]
#[graphql(name = "WorkspaceMemberFilterInput")]
pub struct WorkspaceMemberFilterInput {
    pub id: Option<UuidFilter>,
    pub created_at: Option<DateTimeFilter>,
    pub updated_at: Option<DateTimeFilter>,
    pub name: Option<FullNameFilterInput>,
    pub color_scheme: Option<StringFilter>,
    pub locale: Option<StringFilter>,
    pub time_zone: Option<StringFilter>,
    pub user_email: Option<StringFilter>,
    pub job_title: Option<StringFilter>,
    pub user_id: Option<UuidFilter>,
    pub and: Option<Vec<WorkspaceMemberFilterInput>>,
    pub or: Option<Vec<WorkspaceMemberFilterInput>>,
    pub not: Option<Box<WorkspaceMemberFilterInput>>,
}

#[derive(Debug, Default, InputObject)]
pub struct EmailsFilterInput {
    pub primary_email: Option<StringFilter>,
}

#[derive(Debug, Default, InputObject)]
pub struct PersonFilterInput {
    pub id: Option<UuidFilter>,
    pub created_at: Option<DateTimeFilter>,
    pub updated_at: Option<DateTimeFilter>,
    pub name: Option<FullNameFilterInput>,
    pub emails: Option<EmailsFilterInput>,
    pub job_title: Option<StringFilter>,
    pub company_id: Option<UuidFilter>,
    pub company: Option<CompanyFilterInput>,
    pub and: Option<Vec<PersonFilterInput>>,
    pub or: Option<Vec<PersonFilterInput>>,
    pub not: Option<Box<PersonFilterInput>>,
}

fn is_null(is: FilterIs) -> bool {
    matches!(is, FilterIs::Null)
}

fn string_ops(filter: &StringFilter) -> Vec<StringOp> {
    let mut ops = Vec::new();
    if let Some(value) = &filter.eq {
        ops.push(StringOp::Eq(value.clone()));
    }
    if let Some(value) = &filter.neq {
        ops.push(StringOp::Neq(value.clone()));
    }
    if let Some(value) = &filter.gt {
        ops.push(StringOp::Gt(value.clone()));
    }
    if let Some(value) = &filter.gte {
        ops.push(StringOp::Gte(value.clone()));
    }
    if let Some(value) = &filter.lt {
        ops.push(StringOp::Lt(value.clone()));
    }
    if let Some(value) = &filter.lte {
        ops.push(StringOp::Lte(value.clone()));
    }
    if let Some(values) = &filter.r#in {
        ops.push(StringOp::In(values.clone()));
    }
    if let Some(value) = &filter.starts_with {
        ops.push(StringOp::StartsWith(value.clone()));
    }
    if let Some(value) = &filter.ends_with {
        ops.push(StringOp::EndsWith(value.clone()));
    }
    if let Some(value) = &filter.like {
        ops.push(StringOp::Like(value.clone()));
    }
    if let Some(value) = &filter.ilike {
        ops.push(StringOp::ILike(value.clone()));
    }
    if let Some(value) = &filter.regex {
        ops.push(StringOp::Regex(value.clone()));
    }
    if let Some(value) = &filter.iregex {
        ops.push(StringOp::IRegex(value.clone()));
    }
    if let Some(is) = filter.is {
        ops.push(StringOp::IsNull(is_null(is)));
    }
    ops
}

fn uuid_ops(filter: &UuidFilter) -> Vec<UuidOp> {
    let mut ops = Vec::new();
    if let Some(value) = filter.eq {
        ops.push(UuidOp::Eq(value));
    }
    if let Some(value) = filter.neq {
        ops.push(UuidOp::Neq(value));
    }
    if let Some(values) = &filter.r#in {
        ops.push(UuidOp::In(values.clone()));
    }
    if let Some(is) = filter.is {
        ops.push(UuidOp::IsNull(is_null(is)));
    }
    ops
}

fn date_ops(filter: &DateTimeFilter) -> Vec<DateOp> {
    let mut ops = Vec::new();
    if let Some(value) = filter.eq {
        ops.push(DateOp::Eq(value));
    }
    if let Some(value) = filter.neq {
        ops.push(DateOp::Neq(value));
    }
    if let Some(value) = filter.gt {
        ops.push(DateOp::Gt(value));
    }
    if let Some(value) = filter.gte {
        ops.push(DateOp::Gte(value));
    }
    if let Some(value) = filter.lt {
        ops.push(DateOp::Lt(value));
    }
    if let Some(value) = filter.lte {
        ops.push(DateOp::Lte(value));
    }
    if let Some(values) = &filter.r#in {
        ops.push(DateOp::In(values.clone()));
    }
    if let Some(is) = filter.is {
        ops.push(DateOp::IsNull(is_null(is)));
    }
    ops
}

impl PersonFilterInput {
    fn to_expr(&self) -> Option<FilterExpr> {
        let mut items = Vec::new();
        if let Some(id) = &self.id {
            items.extend(uuid_ops(id).into_iter().map(FilterExpr::Id));
        }
        if let Some(created_at) = &self.created_at {
            items.extend(date_ops(created_at).into_iter().map(FilterExpr::CreatedAt));
        }
        if let Some(updated_at) = &self.updated_at {
            items.extend(date_ops(updated_at).into_iter().map(FilterExpr::UpdatedAt));
        }
        if let Some(name) = &self.name {
            if let Some(first_name) = &name.first_name {
                items.extend(
                    string_ops(first_name)
                        .into_iter()
                        .map(FilterExpr::FirstName),
                );
            }
            if let Some(last_name) = &name.last_name {
                items.extend(string_ops(last_name).into_iter().map(FilterExpr::LastName));
            }
        }
        if let Some(emails) = &self.emails {
            if let Some(primary_email) = &emails.primary_email {
                items.extend(
                    string_ops(primary_email)
                        .into_iter()
                        .map(FilterExpr::PrimaryEmail),
                );
            }
        }
        if let Some(job_title) = &self.job_title {
            items.extend(string_ops(job_title).into_iter().map(FilterExpr::JobTitle));
        }
        if let Some(company_id) = &self.company_id {
            items.extend(uuid_ops(company_id).into_iter().map(FilterExpr::CompanyId));
        }
        if let Some(company) = &self.company {
            if let Some(expr) = company.to_expr() {
                items.push(FilterExpr::RelatedCompany(Box::new(expr)));
            }
        }
        if let Some(and) = &self.and {
            let exprs: Vec<FilterExpr> =
                and.iter().filter_map(PersonFilterInput::to_expr).collect();
            if !exprs.is_empty() {
                items.push(FilterExpr::And(exprs));
            }
        }
        if let Some(or) = &self.or {
            let exprs: Vec<FilterExpr> = or.iter().filter_map(PersonFilterInput::to_expr).collect();
            if !exprs.is_empty() {
                items.push(FilterExpr::Or(exprs));
            }
        }
        if let Some(not) = &self.not {
            if let Some(expr) = not.to_expr() {
                items.push(FilterExpr::Not(Box::new(expr)));
            }
        }

        match items.len() {
            0 => None,
            1 => items.pop(),
            _ => Some(FilterExpr::And(items)),
        }
    }
}

impl CompanyFilterInput {
    fn to_expr(&self) -> Option<FilterExpr> {
        let mut items = Vec::new();
        if let Some(id) = &self.id {
            items.extend(uuid_ops(id).into_iter().map(FilterExpr::Id));
        }
        if let Some(created_at) = &self.created_at {
            items.extend(
                date_ops(created_at)
                    .into_iter()
                    .map(FilterExpr::CompanyCreatedAt),
            );
        }
        if let Some(updated_at) = &self.updated_at {
            items.extend(
                date_ops(updated_at)
                    .into_iter()
                    .map(FilterExpr::CompanyUpdatedAt),
            );
        }
        if let Some(name) = &self.name {
            items.extend(string_ops(name).into_iter().map(FilterExpr::CompanyName));
        }
        if let Some(account_owner_id) = &self.account_owner_id {
            items.extend(
                uuid_ops(account_owner_id)
                    .into_iter()
                    .map(FilterExpr::CompanyAccountOwnerId),
            );
        }
        if let Some(and) = &self.and {
            let exprs: Vec<FilterExpr> =
                and.iter().filter_map(CompanyFilterInput::to_expr).collect();
            if !exprs.is_empty() {
                items.push(FilterExpr::And(exprs));
            }
        }
        if let Some(or) = &self.or {
            let exprs: Vec<FilterExpr> =
                or.iter().filter_map(CompanyFilterInput::to_expr).collect();
            if !exprs.is_empty() {
                items.push(FilterExpr::Or(exprs));
            }
        }
        if let Some(not) = &self.not {
            if let Some(expr) = not.to_expr() {
                items.push(FilterExpr::Not(Box::new(expr)));
            }
        }
        match items.len() {
            0 => None,
            1 => items.pop(),
            _ => Some(FilterExpr::And(items)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, async_graphql::Enum)]
pub enum OrderByDirection {
    AscNullsFirst,
    AscNullsLast,
    DescNullsFirst,
    DescNullsLast,
}

fn json_order_terms(raw: &[serde_json::Value]) -> Vec<SortTerm> {
    let mut terms = Vec::new();
    for item in raw {
        let Some(object) = item.as_object() else {
            continue;
        };
        for (field, direction) in object {
            let Some(direction) = direction.as_str() else {
                continue;
            };
            if let Some(term) = SortTerm::parse(field, direction) {
                terms.push(term);
            }
        }
    }
    terms
}

pub(crate) fn sort_companies(items: &mut [Company], terms: &[SortTerm]) {
    sort_records(items, terms, crate::sorting::company_value);
}

pub(crate) fn sort_opportunities(items: &mut [Opportunity], terms: &[SortTerm]) {
    sort_records(items, terms, crate::sorting::opportunity_value);
}

pub(crate) fn sort_tasks(items: &mut [Task], terms: &[SortTerm]) {
    sort_records(items, terms, crate::sorting::task_value);
}

pub(crate) fn sort_notes(items: &mut [Note], terms: &[SortTerm]) {
    sort_records(items, terms, crate::sorting::note_value);
}

pub(crate) fn sort_task_targets(items: &mut [TaskTarget], terms: &[SortTerm]) {
    sort_records(items, terms, crate::sorting::task_target_value);
}

pub(crate) fn sort_note_targets(items: &mut [NoteTarget], terms: &[SortTerm]) {
    sort_records(items, terms, crate::sorting::note_target_value);
}

#[derive(Debug, InputObject)]
pub struct FullNameOrderByInput {
    pub first_name: Option<OrderByDirection>,
    pub last_name: Option<OrderByDirection>,
}

#[derive(Debug, InputObject)]
pub struct EmailsOrderByInput {
    pub primary_email: Option<OrderByDirection>,
}

#[derive(Debug, InputObject)]
pub struct PersonOrderByInput {
    pub id: Option<OrderByDirection>,
    pub created_at: Option<OrderByDirection>,
    pub updated_at: Option<OrderByDirection>,
    pub name: Option<FullNameOrderByInput>,
    pub emails: Option<EmailsOrderByInput>,
    pub job_title: Option<OrderByDirection>,
}

#[derive(Debug, InputObject)]
pub struct AddressInput {
    pub address_street_1: Option<String>,
    pub address_street_2: Option<String>,
    pub address_city: Option<String>,
    pub address_postcode: Option<String>,
    pub address_state: Option<String>,
    pub address_country: Option<String>,
    pub address_lat: Option<f64>,
    pub address_lng: Option<f64>,
}

impl From<AddressInput> for Address {
    fn from(input: AddressInput) -> Self {
        Address {
            address_street_1: input.address_street_1,
            address_street_2: input.address_street_2,
            address_city: input.address_city,
            address_postcode: input.address_postcode,
            address_state: input.address_state,
            address_country: input.address_country,
            address_lat: input.address_lat,
            address_lng: input.address_lng,
        }
    }
}

#[derive(Debug, InputObject)]
pub struct CurrencyInput {
    pub amount_micros: Option<i64>,
    pub currency_code: Option<String>,
}

impl From<CurrencyInput> for Currency {
    fn from(input: CurrencyInput) -> Self {
        Currency {
            amount_micros: input.amount_micros,
            currency_code: input.currency_code.unwrap_or_default(),
        }
    }
}

#[derive(Debug, InputObject)]
pub struct CreateCompanyInputGql {
    pub name: Option<String>,
    pub domain_name: Option<Links>,
    pub address: Option<AddressInput>,
    pub linkedin_link: Option<Links>,
    pub annual_revenue: Option<CurrencyInput>,
    pub position: Option<f64>,
    pub account_owner_id: Option<Uuid>,
}

impl From<CreateCompanyInputGql> for CreateCompanyInput {
    fn from(input: CreateCompanyInputGql) -> Self {
        CreateCompanyInput {
            name: input.name,
            domain_name: input.domain_name,
            address: input.address.map(Into::into),
            linkedin_link: input.linkedin_link,
            annual_revenue: input.annual_revenue.map(Into::into),
            position: input.position,
            account_owner_id: input.account_owner_id,
        }
    }
}

#[derive(Debug, InputObject)]
pub struct UpdateCompanyInputGql {
    pub name: MaybeUndefined<String>,
    pub domain_name: MaybeUndefined<Links>,
    pub address: MaybeUndefined<AddressInput>,
    pub linkedin_link: MaybeUndefined<Links>,
    pub annual_revenue: MaybeUndefined<CurrencyInput>,
    pub position: MaybeUndefined<f64>,
    pub account_owner_id: MaybeUndefined<Uuid>,
}

impl From<UpdateCompanyInputGql> for UpdateCompanyInput {
    fn from(input: UpdateCompanyInputGql) -> Self {
        UpdateCompanyInput {
            name: input.name,
            domain_name: input.domain_name,
            address: input.address.map_value(Into::into),
            linkedin_link: input.linkedin_link,
            annual_revenue: input.annual_revenue.map_value(Into::into),
            position: input.position,
            account_owner_id: input.account_owner_id,
        }
    }
}

#[derive(Debug, InputObject)]
pub struct CreateOpportunityInputGql {
    pub name: Option<String>,
    pub amount: Option<CurrencyInput>,
    pub close_date: Option<DateTime<Utc>>,
    pub stage: Option<OpportunityStage>,
    pub position: Option<f64>,
    pub company_id: Option<Uuid>,
    pub point_of_contact_id: Option<Uuid>,
    pub owner_id: Option<Uuid>,
}

impl From<CreateOpportunityInputGql> for CreateOpportunityInput {
    fn from(input: CreateOpportunityInputGql) -> Self {
        CreateOpportunityInput {
            name: input.name,
            amount: input.amount.map(Into::into),
            close_date: input.close_date,
            stage: input.stage,
            position: input.position,
            company_id: input.company_id,
            point_of_contact_id: input.point_of_contact_id,
            owner_id: input.owner_id,
        }
    }
}

#[derive(Debug, InputObject)]
pub struct UpdateOpportunityInputGql {
    pub name: MaybeUndefined<String>,
    pub amount: MaybeUndefined<CurrencyInput>,
    pub close_date: MaybeUndefined<DateTime<Utc>>,
    pub stage: MaybeUndefined<OpportunityStage>,
    pub position: MaybeUndefined<f64>,
    pub company_id: MaybeUndefined<Uuid>,
    pub point_of_contact_id: MaybeUndefined<Uuid>,
    pub owner_id: MaybeUndefined<Uuid>,
}

impl From<UpdateOpportunityInputGql> for UpdateOpportunityInput {
    fn from(input: UpdateOpportunityInputGql) -> Self {
        UpdateOpportunityInput {
            name: input.name,
            amount: input.amount.map_value(Into::into),
            close_date: input.close_date,
            stage: input.stage,
            position: input.position,
            company_id: input.company_id,
            point_of_contact_id: input.point_of_contact_id,
            owner_id: input.owner_id,
        }
    }
}

#[derive(Debug, Default, InputObject)]
pub struct OpportunityStageFilter {
    pub eq: Option<OpportunityStage>,
    pub neq: Option<OpportunityStage>,
    pub r#in: Option<Vec<OpportunityStage>>,
    pub is: Option<FilterIs>,
}

#[derive(Debug, Default, InputObject)]
pub struct OpportunityFilterInput {
    pub id: Option<UuidFilter>,
    pub name: Option<StringFilter>,
    pub close_date: Option<DateTimeFilter>,
    pub stage: Option<OpportunityStageFilter>,
    pub company_id: Option<UuidFilter>,
    pub company: Option<CompanyFilterInput>,
    pub point_of_contact_id: Option<UuidFilter>,
    pub point_of_contact: Option<PersonFilterInput>,
    pub owner_id: Option<UuidFilter>,
    pub and: Option<Vec<OpportunityFilterInput>>,
    pub or: Option<Vec<OpportunityFilterInput>>,
    pub not: Option<Box<OpportunityFilterInput>>,
}

#[derive(Debug, SimpleObject)]
pub struct OpportunityEdge {
    pub node: Opportunity,
    pub cursor: Cursor,
}

#[derive(Debug, SimpleObject)]
pub struct OpportunityConnection {
    pub total_count: Option<i64>,
    pub edges: Vec<OpportunityEdge>,
    pub page_info: PageInfo,
}

#[derive(Debug, Default, InputObject)]
pub struct CompanyFilterInput {
    pub id: Option<UuidFilter>,
    pub created_at: Option<DateTimeFilter>,
    pub updated_at: Option<DateTimeFilter>,
    pub name: Option<StringFilter>,
    pub account_owner_id: Option<UuidFilter>,
    pub and: Option<Vec<CompanyFilterInput>>,
    pub or: Option<Vec<CompanyFilterInput>>,
    pub not: Option<Box<CompanyFilterInput>>,
}

#[derive(Debug, InputObject)]
pub struct CreateTaskInputGql {
    pub position: Option<f64>,
    pub title: Option<String>,
    pub body_v2: Option<RichText>,
    pub due_at: Option<DateTime<Utc>>,
    pub status: Option<TaskStatus>,
    pub assignee_id: Option<Uuid>,
}

impl From<CreateTaskInputGql> for CreateTaskInput {
    fn from(input: CreateTaskInputGql) -> Self {
        CreateTaskInput {
            position: input.position,
            title: input.title,
            body_v2: input.body_v2,
            due_at: input.due_at,
            status: input.status,
            assignee_id: input.assignee_id,
        }
    }
}

#[derive(Debug, InputObject)]
pub struct UpdateTaskInputGql {
    pub position: MaybeUndefined<f64>,
    pub title: MaybeUndefined<String>,
    pub body_v2: MaybeUndefined<RichText>,
    pub due_at: MaybeUndefined<DateTime<Utc>>,
    pub status: MaybeUndefined<TaskStatus>,
    pub assignee_id: MaybeUndefined<Uuid>,
}

impl From<UpdateTaskInputGql> for UpdateTaskInput {
    fn from(input: UpdateTaskInputGql) -> Self {
        UpdateTaskInput {
            position: input.position,
            title: input.title,
            body_v2: input.body_v2,
            due_at: input.due_at,
            status: input.status,
            assignee_id: input.assignee_id,
        }
    }
}

#[derive(Debug, Default, InputObject)]
pub struct TaskStatusFilter {
    pub eq: Option<TaskStatus>,
    pub neq: Option<TaskStatus>,
    pub r#in: Option<Vec<TaskStatus>>,
    pub is: Option<FilterIs>,
}

#[derive(Debug, Default, InputObject)]
pub struct TaskFilterInput {
    pub id: Option<UuidFilter>,
    pub title: Option<StringFilter>,
    pub due_at: Option<DateTimeFilter>,
    pub status: Option<TaskStatusFilter>,
    pub assignee_id: Option<UuidFilter>,
    pub and: Option<Vec<TaskFilterInput>>,
    pub or: Option<Vec<TaskFilterInput>>,
    pub not: Option<Box<TaskFilterInput>>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Task", complex)]
pub struct TaskGql {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub position: f64,
    pub title: Option<String>,
    pub body_v2: RichText,
    pub due_at: Option<DateTime<Utc>>,
    pub status: Option<TaskStatus>,
    pub created_by: squid_core::Actor,
    pub updated_by: squid_core::Actor,
    pub search_vector: Option<String>,
    pub assignee_id: Option<Uuid>,
}

impl From<Task> for TaskGql {
    fn from(task: Task) -> Self {
        Self {
            id: task.id,
            created_at: task.created_at,
            updated_at: task.updated_at,
            deleted_at: task.deleted_at,
            position: task.position,
            title: task.title,
            body_v2: task.body_v2,
            due_at: task.due_at,
            status: task.status,
            created_by: task.created_by,
            updated_by: task.updated_by,
            search_vector: task.search_vector,
            assignee_id: task.assignee_id,
        }
    }
}

#[derive(Debug, Default, InputObject)]
pub struct TaskTargetFilterInput {
    pub id: Option<UuidFilter>,
    pub task_id: Option<UuidFilter>,
    pub target_company_id: Option<UuidFilter>,
    pub target_opportunity_id: Option<UuidFilter>,
    pub target_person_id: Option<UuidFilter>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "TaskTarget", complex)]
pub struct TaskTargetGql {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub position: f64,
    pub created_by: squid_core::Actor,
    pub updated_by: squid_core::Actor,
    pub search_vector: Option<String>,
    pub task_id: Uuid,
    pub target_company_id: Option<Uuid>,
    pub target_opportunity_id: Option<Uuid>,
    pub target_person_id: Option<Uuid>,
}

impl From<TaskTarget> for TaskTargetGql {
    fn from(target: TaskTarget) -> Self {
        Self {
            id: target.id,
            created_at: target.created_at,
            updated_at: target.updated_at,
            deleted_at: target.deleted_at,
            position: target.position,
            created_by: target.created_by,
            updated_by: target.updated_by,
            search_vector: target.search_vector,
            task_id: target.task_id,
            target_company_id: target.target_company_id,
            target_opportunity_id: target.target_opportunity_id,
            target_person_id: target.target_person_id,
        }
    }
}

#[derive(Debug, SimpleObject)]
pub struct TaskTargetEdge {
    pub node: TaskTargetGql,
    pub cursor: Cursor,
}

#[derive(Debug, SimpleObject)]
pub struct TaskTargetConnection {
    pub total_count: Option<i64>,
    pub edges: Vec<TaskTargetEdge>,
    pub page_info: PageInfo,
}

#[derive(Debug, SimpleObject)]
pub struct TaskEdge {
    pub node: TaskGql,
    pub cursor: Cursor,
}

#[derive(Debug, SimpleObject)]
pub struct TaskConnection {
    pub total_count: Option<i64>,
    pub edges: Vec<TaskEdge>,
    pub page_info: PageInfo,
}

#[derive(Debug, InputObject)]
pub struct CreateNoteInputGql {
    pub position: Option<f64>,
    pub title: Option<String>,
    pub body_v2: Option<RichText>,
}

impl From<CreateNoteInputGql> for CreateNoteInput {
    fn from(input: CreateNoteInputGql) -> Self {
        CreateNoteInput {
            position: input.position,
            title: input.title,
            body_v2: input.body_v2,
        }
    }
}

#[derive(Debug, InputObject)]
pub struct UpdateNoteInputGql {
    pub position: MaybeUndefined<f64>,
    pub title: MaybeUndefined<String>,
    pub body_v2: MaybeUndefined<RichText>,
}

impl From<UpdateNoteInputGql> for UpdateNoteInput {
    fn from(input: UpdateNoteInputGql) -> Self {
        UpdateNoteInput {
            position: input.position,
            title: input.title,
            body_v2: input.body_v2,
        }
    }
}

#[derive(Debug, Default, InputObject)]
pub struct NoteFilterInput {
    pub id: Option<UuidFilter>,
    pub title: Option<StringFilter>,
    pub and: Option<Vec<NoteFilterInput>>,
    pub or: Option<Vec<NoteFilterInput>>,
    pub not: Option<Box<NoteFilterInput>>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "Note", complex)]
pub struct NoteGql {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub position: f64,
    pub title: Option<String>,
    pub body_v2: RichText,
    pub created_by: squid_core::Actor,
    pub updated_by: squid_core::Actor,
    pub search_vector: Option<String>,
}

impl From<Note> for NoteGql {
    fn from(note: Note) -> Self {
        Self {
            id: note.id,
            created_at: note.created_at,
            updated_at: note.updated_at,
            deleted_at: note.deleted_at,
            position: note.position,
            title: note.title,
            body_v2: note.body_v2,
            created_by: note.created_by,
            updated_by: note.updated_by,
            search_vector: note.search_vector,
        }
    }
}

#[derive(Debug, Default, InputObject)]
pub struct NoteTargetFilterInput {
    pub id: Option<UuidFilter>,
    pub note_id: Option<UuidFilter>,
    pub target_company_id: Option<UuidFilter>,
    pub target_opportunity_id: Option<UuidFilter>,
    pub target_person_id: Option<UuidFilter>,
}

#[derive(Debug, Clone, SimpleObject)]
#[graphql(name = "NoteTarget", complex)]
pub struct NoteTargetGql {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub position: f64,
    pub created_by: squid_core::Actor,
    pub updated_by: squid_core::Actor,
    pub search_vector: Option<String>,
    pub note_id: Uuid,
    pub target_company_id: Option<Uuid>,
    pub target_opportunity_id: Option<Uuid>,
    pub target_person_id: Option<Uuid>,
}

impl From<NoteTarget> for NoteTargetGql {
    fn from(target: NoteTarget) -> Self {
        Self {
            id: target.id,
            created_at: target.created_at,
            updated_at: target.updated_at,
            deleted_at: target.deleted_at,
            position: target.position,
            created_by: target.created_by,
            updated_by: target.updated_by,
            search_vector: target.search_vector,
            note_id: target.note_id,
            target_company_id: target.target_company_id,
            target_opportunity_id: target.target_opportunity_id,
            target_person_id: target.target_person_id,
        }
    }
}

#[derive(Debug, SimpleObject)]
pub struct NoteTargetEdge {
    pub node: NoteTargetGql,
    pub cursor: Cursor,
}

#[derive(Debug, SimpleObject)]
pub struct NoteTargetConnection {
    pub total_count: Option<i64>,
    pub edges: Vec<NoteTargetEdge>,
    pub page_info: PageInfo,
}

#[derive(Debug, SimpleObject)]
pub struct NoteEdge {
    pub node: NoteGql,
    pub cursor: Cursor,
}

#[derive(Debug, SimpleObject)]
pub struct NoteConnection {
    pub total_count: Option<i64>,
    pub edges: Vec<NoteEdge>,
    pub page_info: PageInfo,
}

#[derive(Debug, InputObject)]
pub struct CreateTaskTargetInputGql {
    pub task_id: Uuid,
    pub target_company_id: Option<Uuid>,
    pub target_opportunity_id: Option<Uuid>,
    pub target_person_id: Option<Uuid>,
}

impl From<CreateTaskTargetInputGql> for CreateTaskTargetInput {
    fn from(input: CreateTaskTargetInputGql) -> Self {
        CreateTaskTargetInput {
            task_id: input.task_id,
            target_company_id: input.target_company_id,
            target_opportunity_id: input.target_opportunity_id,
            target_person_id: input.target_person_id,
        }
    }
}

#[derive(Debug, InputObject)]
pub struct UpdateTaskTargetInputGql {
    pub position: MaybeUndefined<f64>,
    pub target_company_id: MaybeUndefined<Uuid>,
    pub target_opportunity_id: MaybeUndefined<Uuid>,
    pub target_person_id: MaybeUndefined<Uuid>,
}

impl From<UpdateTaskTargetInputGql> for squid_core::UpdateTaskTargetInput {
    fn from(input: UpdateTaskTargetInputGql) -> Self {
        squid_core::UpdateTaskTargetInput {
            position: input.position,
            target_company_id: input.target_company_id,
            target_opportunity_id: input.target_opportunity_id,
            target_person_id: input.target_person_id,
        }
    }
}

#[derive(Debug, InputObject)]
pub struct UpdateNoteTargetInputGql {
    pub position: MaybeUndefined<f64>,
    pub target_company_id: MaybeUndefined<Uuid>,
    pub target_opportunity_id: MaybeUndefined<Uuid>,
    pub target_person_id: MaybeUndefined<Uuid>,
}

impl From<UpdateNoteTargetInputGql> for squid_core::UpdateNoteTargetInput {
    fn from(input: UpdateNoteTargetInputGql) -> Self {
        squid_core::UpdateNoteTargetInput {
            position: input.position,
            target_company_id: input.target_company_id,
            target_opportunity_id: input.target_opportunity_id,
            target_person_id: input.target_person_id,
        }
    }
}

#[derive(Debug, InputObject)]
pub struct CreateNoteTargetInputGql {
    pub note_id: Uuid,
    pub target_company_id: Option<Uuid>,
    pub target_opportunity_id: Option<Uuid>,
    pub target_person_id: Option<Uuid>,
}

impl From<CreateNoteTargetInputGql> for CreateNoteTargetInput {
    fn from(input: CreateNoteTargetInputGql) -> Self {
        CreateNoteTargetInput {
            note_id: input.note_id,
            target_company_id: input.target_company_id,
            target_opportunity_id: input.target_opportunity_id,
            target_person_id: input.target_person_id,
        }
    }
}

#[derive(Debug, SimpleObject)]
pub struct CompanyEdge {
    pub node: Company,
    pub cursor: Cursor,
}

#[derive(Debug, SimpleObject)]
pub struct CompanyConnection {
    pub total_count: Option<i64>,
    pub edges: Vec<CompanyEdge>,
    pub page_info: PageInfo,
}

#[derive(Debug, InputObject)]
#[graphql(name = "WorkspaceMemberOrderByInput")]
pub struct WorkspaceMemberOrderByInput {
    pub id: Option<OrderByDirection>,
    pub created_at: Option<OrderByDirection>,
    pub updated_at: Option<OrderByDirection>,
    pub name: Option<FullNameOrderByInput>,
    pub user_email: Option<OrderByDirection>,
    pub job_title: Option<OrderByDirection>,
}

#[derive(Debug, SimpleObject)]
#[graphql(name = "WorkspaceMemberEdge")]
pub struct WorkspaceMemberEdge {
    pub node: WorkspaceMemberGql,
    pub cursor: Cursor,
}

#[derive(Debug, SimpleObject)]
#[graphql(name = "WorkspaceMemberConnection")]
pub struct WorkspaceMemberConnection {
    pub total_count: Option<i64>,
    pub edges: Vec<WorkspaceMemberEdge>,
    pub page_info: PageInfo,
}

#[derive(Debug, SimpleObject)]
pub struct PersonEdge {
    pub node: Person,
    pub cursor: Cursor,
}

#[derive(Debug, SimpleObject)]
pub struct PersonConnection {
    pub total_count: Option<i64>,
    pub edges: Vec<PersonEdge>,
    pub page_info: PageInfo,
}

fn object_matches_filter(object: &ObjectMetadata, filter: &ObjectFilterInput) -> bool {
    if let Some(comparison) = &filter.id {
        if comparison.eq.is_some_and(|value| value != object.id) {
            return false;
        }
        if comparison.neq.is_some_and(|value| value == object.id) {
            return false;
        }
    }
    if let Some(comparison) = &filter.is_active {
        if comparison.eq.is_some_and(|value| value != object.is_active) {
            return false;
        }
        if comparison
            .neq
            .is_some_and(|value| value == object.is_active)
        {
            return false;
        }
    }
    if let Some(comparison) = &filter.is_system {
        if comparison.eq.is_some_and(|value| value != object.is_system) {
            return false;
        }
        if comparison
            .neq
            .is_some_and(|value| value == object.is_system)
        {
            return false;
        }
    }
    true
}

fn field_matches_filter(field: &FieldMetadata, filter: &FieldFilterInput) -> bool {
    if let Some(comparison) = &filter.id {
        if comparison.eq.is_some_and(|value| value != field.id) {
            return false;
        }
        if comparison.neq.is_some_and(|value| value == field.id) {
            return false;
        }
    }
    if let Some(comparison) = &filter.object_metadata_id {
        if comparison
            .eq
            .is_some_and(|value| value != field.object_metadata_id)
        {
            return false;
        }
        if comparison
            .neq
            .is_some_and(|value| value == field.object_metadata_id)
        {
            return false;
        }
    }
    if let Some(comparison) = &filter.is_active {
        if comparison.eq.is_some_and(|value| value != field.is_active) {
            return false;
        }
        if comparison.neq.is_some_and(|value| value == field.is_active) {
            return false;
        }
    }
    if let Some(comparison) = &filter.is_system {
        if comparison.eq.is_some_and(|value| value != field.is_system) {
            return false;
        }
        if comparison.neq.is_some_and(|value| value == field.is_system) {
            return false;
        }
    }
    true
}

fn objects_connection(
    mut objects: Vec<ObjectMetadata>,
    paging: Option<CursorPagingInput>,
) -> ObjectConnection {
    let paging = paging.unwrap_or(CursorPagingInput {
        before: None,
        after: None,
        first: None,
        last: None,
    });
    if let Some(after) = paging.after.as_deref() {
        if let Some(position) = objects
            .iter()
            .position(|object| object.id.to_string() == after)
        {
            objects.drain(..=position);
        }
    }
    if let Some(before) = paging.before.as_deref() {
        if let Some(position) = objects
            .iter()
            .position(|object| object.id.to_string() == before)
        {
            objects.truncate(position);
        }
    }
    let has_next_page = paging
        .first
        .and_then(|first| usize::try_from(first).ok())
        .is_some_and(|first| objects.len() > first);
    if let Some(first) = paging.first.and_then(|first| usize::try_from(first).ok()) {
        objects.truncate(first);
    }
    if let Some(last) = paging.last.and_then(|last| usize::try_from(last).ok()) {
        if objects.len() > last {
            objects.drain(..objects.len() - last);
        }
    }
    let edges: Vec<ObjectEdge> = objects
        .into_iter()
        .map(|node| ObjectEdge {
            cursor: node.id.to_string(),
            node,
        })
        .collect();
    let page_info = PageInfo {
        has_next_page,
        has_previous_page: false,
        start_cursor: edges.first().map(|edge| edge.cursor.clone()),
        end_cursor: edges.last().map(|edge| edge.cursor.clone()),
    };
    ObjectConnection { page_info, edges }
}

fn direction_flags(direction: OrderByDirection) -> (bool, bool) {
    match direction {
        OrderByDirection::AscNullsFirst => (false, true),
        OrderByDirection::AscNullsLast => (false, false),
        OrderByDirection::DescNullsFirst => (true, true),
        OrderByDirection::DescNullsLast => (true, false),
    }
}

fn order_terms(input: &[PersonOrderByInput]) -> Vec<OrderTerm> {
    let mut terms = Vec::new();
    for order in input {
        let mut push = |column, direction: Option<OrderByDirection>| {
            if let Some(direction) = direction {
                let (descending, nulls_first) = direction_flags(direction);
                terms.push(OrderTerm {
                    column,
                    descending,
                    nulls_first,
                });
            }
        };
        push(PersonColumn::Id, order.id);
        push(PersonColumn::CreatedAt, order.created_at);
        push(PersonColumn::UpdatedAt, order.updated_at);
        if let Some(name) = &order.name {
            push(PersonColumn::NameFirst, name.first_name);
            push(PersonColumn::NameLast, name.last_name);
        }
        if let Some(emails) = &order.emails {
            push(PersonColumn::PrimaryEmail, emails.primary_email);
        }
        push(PersonColumn::JobTitle, order.job_title);
    }
    terms
}

fn connection_from_page(page: squid_db::PersonPage) -> PersonConnection {
    connection_with(
        page.persons,
        page.total,
        page.has_next_page,
        page.has_previous_page,
    )
}

fn connection_with(
    people: Vec<Person>,
    total: i64,
    has_next_page: bool,
    has_previous_page: bool,
) -> PersonConnection {
    let start_cursor = people
        .first()
        .map(|person| crate::rest::encode_cursor(person.id));
    let end_cursor = people
        .last()
        .map(|person| crate::rest::encode_cursor(person.id));
    let edges = people
        .into_iter()
        .map(|person| PersonEdge {
            cursor: Cursor(crate::rest::encode_cursor(person.id)),
            node: person,
        })
        .collect();
    PersonConnection {
        total_count: Some(total),
        edges,
        page_info: PageInfo {
            has_next_page,
            has_previous_page,
            start_cursor,
            end_cursor,
        },
    }
}

fn parse_expiry(raw: &str) -> AppResult<chrono::DateTime<chrono::Utc>> {
    if let Ok(value) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Ok(value.with_timezone(&chrono::Utc));
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        if let Some(datetime) = date.and_hms_opt(0, 0, 0) {
            return Ok(datetime.and_utc());
        }
    }
    Err(AppError::BadRequest(format!(
        "Invalid date value '{raw}' for expiresAt"
    )))
}

pub(crate) fn content_type_for(filename: &str) -> &'static str {
    match filename
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
}

fn schema() -> &'static SquidSchema {
    static SCHEMA: OnceLock<SquidSchema> = OnceLock::new();
    SCHEMA.get_or_init(|| Schema::build(QueryRoot, MutationRoot, EmptySubscription).finish())
}

pub struct QueryRoot;
pub struct MutationRoot;

fn ctx_data<'a>(ctx: &'a Context<'_>) -> &'a GqlCtx {
    ctx.data_unchecked::<GqlCtx>()
}

fn require_auth<'a>(ctx: &'a Context<'_>) -> async_graphql::Result<&'a AuthContext> {
    ctx_data(ctx).auth.as_ref().ok_or_else(|| {
        gql_err(AppError::Unauthorized(
            "Authentication required".to_string(),
        ))
    })
}

fn string_op_matches(ops: &[StringOp], actual: Option<&str>) -> bool {
    let actual = actual.unwrap_or_default();
    ops.iter().all(|op| match op {
        StringOp::Eq(expected) => actual == expected,
        StringOp::Neq(expected) => actual != expected,
        StringOp::Gt(expected) => actual > expected.as_str(),
        StringOp::Gte(expected) => actual >= expected.as_str(),
        StringOp::Lt(expected) => actual < expected.as_str(),
        StringOp::Lte(expected) => actual <= expected.as_str(),
        StringOp::In(values) => values.iter().any(|value| value == actual),
        StringOp::StartsWith(value) => actual.starts_with(value),
        StringOp::EndsWith(value) => actual.ends_with(value),
        StringOp::Like(pattern) => pattern.trim_matches('%') == actual,
        StringOp::ILike(pattern) => actual
            .to_lowercase()
            .contains(&pattern.trim_matches('%').to_lowercase()),
        StringOp::Regex(_) | StringOp::IRegex(_) => true,
        StringOp::IsNull(true) => false,
        StringOp::IsNull(false) => true,
    })
}

fn date_op_matches(ops: &[DateOp], actual: Option<chrono::DateTime<Utc>>) -> bool {
    ops.iter().all(|op| match op {
        DateOp::Eq(expected) => actual == Some(*expected),
        DateOp::Neq(expected) => actual != Some(*expected),
        DateOp::Gt(expected) => actual.is_some_and(|value| value > *expected),
        DateOp::Gte(expected) => actual.is_some_and(|value| value >= *expected),
        DateOp::Lt(expected) => actual.is_some_and(|value| value < *expected),
        DateOp::Lte(expected) => actual.is_some_and(|value| value <= *expected),
        DateOp::In(values) => actual.is_some_and(|value| values.contains(&value)),
        DateOp::IsNull(true) => actual.is_none(),
        DateOp::IsNull(false) => actual.is_some(),
    })
}

fn uuid_op_matches(ops: &[UuidOp], actual: Option<Uuid>) -> bool {
    ops.iter().all(|op| match op {
        UuidOp::Eq(expected) => actual == Some(*expected),
        UuidOp::Neq(expected) => actual != Some(*expected),
        UuidOp::In(values) => actual.is_some_and(|value| values.contains(&value)),
        UuidOp::IsNull(true) => actual.is_none(),
        UuidOp::IsNull(false) => actual.is_some(),
    })
}

fn company_filter_matches(company: &Company, filter: &CompanyFilterInput) -> bool {
    if let Some(and) = filter.and.as_ref() {
        if !and
            .iter()
            .all(|filter| company_filter_matches(company, filter))
        {
            return false;
        }
    }
    if let Some(or) = filter.or.as_ref() {
        if !or
            .iter()
            .any(|filter| company_filter_matches(company, filter))
        {
            return false;
        }
    }
    if let Some(not) = filter.not.as_ref() {
        if company_filter_matches(company, not) {
            return false;
        }
    }
    if let Some(id) = &filter.id {
        if !uuid_op_matches(&uuid_ops(id), Some(company.id)) {
            return false;
        }
    }
    if let Some(created_at) = &filter.created_at {
        if !date_op_matches(&date_ops(created_at), Some(company.created_at)) {
            return false;
        }
    }
    if let Some(updated_at) = &filter.updated_at {
        if !date_op_matches(&date_ops(updated_at), Some(company.updated_at)) {
            return false;
        }
    }
    if let Some(name) = &filter.name {
        if !string_op_matches(&string_ops(name), company.name.as_deref()) {
            return false;
        }
    }
    if let Some(account_owner_id) = &filter.account_owner_id {
        if !uuid_op_matches(&uuid_ops(account_owner_id), company.account_owner_id) {
            return false;
        }
    }
    true
}

fn person_filter_matches(
    person: &Person,
    filter: &PersonFilterInput,
    companies: &HashMap<Uuid, Company>,
) -> bool {
    if let Some(and) = filter.and.as_ref() {
        if !and
            .iter()
            .all(|filter| person_filter_matches(person, filter, companies))
        {
            return false;
        }
    }
    if let Some(or) = filter.or.as_ref() {
        if !or
            .iter()
            .any(|filter| person_filter_matches(person, filter, companies))
        {
            return false;
        }
    }
    if let Some(not) = filter.not.as_ref() {
        if person_filter_matches(person, not, companies) {
            return false;
        }
    }
    if let Some(id) = &filter.id {
        if !uuid_op_matches(&uuid_ops(id), Some(person.id)) {
            return false;
        }
    }
    if let Some(created_at) = &filter.created_at {
        if !date_op_matches(&date_ops(created_at), Some(person.created_at)) {
            return false;
        }
    }
    if let Some(updated_at) = &filter.updated_at {
        if !date_op_matches(&date_ops(updated_at), Some(person.updated_at)) {
            return false;
        }
    }
    if let Some(name) = &filter.name {
        if let Some(first_name) = &name.first_name {
            if !string_op_matches(&string_ops(first_name), Some(&person.name.first_name)) {
                return false;
            }
        }
        if let Some(last_name) = &name.last_name {
            if !string_op_matches(&string_ops(last_name), Some(&person.name.last_name)) {
                return false;
            }
        }
    }
    if let Some(emails) = &filter.emails {
        if let Some(primary_email) = &emails.primary_email {
            if !string_op_matches(
                &string_ops(primary_email),
                Some(&person.emails.primary_email),
            ) {
                return false;
            }
        }
    }
    if let Some(job_title) = &filter.job_title {
        if !string_op_matches(&string_ops(job_title), person.job_title.as_deref()) {
            return false;
        }
    }
    if let Some(company_id) = &filter.company_id {
        if !uuid_op_matches(&uuid_ops(company_id), person.company_id) {
            return false;
        }
    }
    if let Some(company_filter) = &filter.company {
        let Some(company_id) = person.company_id else {
            return false;
        };
        let Some(company) = companies.get(&company_id) else {
            return false;
        };
        if !company_filter_matches(company, company_filter) {
            return false;
        }
    }
    true
}

fn opportunity_needs_companies(filter: &OpportunityFilterInput) -> bool {
    filter.company.is_some()
        || filter
            .point_of_contact
            .as_ref()
            .is_some_and(|person| person.company.is_some())
        || filter
            .and
            .as_ref()
            .is_some_and(|items| items.iter().any(opportunity_needs_companies))
        || filter
            .or
            .as_ref()
            .is_some_and(|items| items.iter().any(opportunity_needs_companies))
        || filter
            .not
            .as_ref()
            .is_some_and(|item| opportunity_needs_companies(item))
}

fn opportunity_needs_people(filter: &OpportunityFilterInput) -> bool {
    filter.point_of_contact.is_some()
        || filter
            .and
            .as_ref()
            .is_some_and(|items| items.iter().any(opportunity_needs_people))
        || filter
            .or
            .as_ref()
            .is_some_and(|items| items.iter().any(opportunity_needs_people))
        || filter
            .not
            .as_ref()
            .is_some_and(|item| opportunity_needs_people(item))
}

fn opportunity_matches(
    opportunity: &Opportunity,
    filter: &OpportunityFilterInput,
    companies: &HashMap<Uuid, Company>,
    people: &HashMap<Uuid, Person>,
) -> bool {
    if let Some(and) = filter.and.as_ref() {
        if !and
            .iter()
            .all(|f| opportunity_matches(opportunity, f, companies, people))
        {
            return false;
        }
    }
    if let Some(or) = filter.or.as_ref() {
        if !or
            .iter()
            .any(|f| opportunity_matches(opportunity, f, companies, people))
        {
            return false;
        }
    }
    if let Some(not) = filter.not.as_ref() {
        if opportunity_matches(opportunity, not, companies, people) {
            return false;
        }
    }
    if let Some(id) = filter.id.as_ref() {
        if let Some(eq) = id.eq {
            if opportunity.id != eq {
                return false;
            }
        }
        if let Some(neq) = id.neq {
            if opportunity.id == neq {
                return false;
            }
        }
        if let Some(list) = id.r#in.as_ref() {
            if !list.contains(&opportunity.id) {
                return false;
            }
        }
    }
    if let Some(name) = filter.name.as_ref() {
        let value = opportunity.name.as_deref().unwrap_or_default();
        if let Some(eq) = name.eq.as_deref() {
            if value != eq {
                return false;
            }
        }
        if let Some(neq) = name.neq.as_deref() {
            if value == neq {
                return false;
            }
        }
        if let Some(like) = name.ilike.as_deref() {
            let needle = like.trim_matches('%').to_lowercase();
            if !value.to_lowercase().contains(&needle) {
                return false;
            }
        }
        if let Some(starts_with) = name.starts_with.as_deref() {
            if !value
                .to_lowercase()
                .starts_with(&starts_with.to_lowercase())
            {
                return false;
            }
        }
    }
    if let Some(stage) = filter.stage.as_ref() {
        if let Some(eq) = stage.eq {
            if opportunity.stage != eq {
                return false;
            }
        }
        if let Some(neq) = stage.neq {
            if opportunity.stage == neq {
                return false;
            }
        }
        if let Some(list) = stage.r#in.as_ref() {
            if !list.contains(&opportunity.stage) {
                return false;
            }
        }
    }
    if let Some(close_date) = filter.close_date.as_ref() {
        if let Some(eq) = close_date.eq {
            if opportunity.close_date != Some(eq) {
                return false;
            }
        }
        if let Some(gte) = close_date.gte {
            if opportunity.close_date.is_none_or(|date| date < gte) {
                return false;
            }
        }
        if let Some(lte) = close_date.lte {
            if opportunity.close_date.is_none_or(|date| date > lte) {
                return false;
            }
        }
    }
    if let Some(company_id) = filter.company_id.as_ref() {
        if let Some(eq) = company_id.eq {
            if opportunity.company_id != Some(eq) {
                return false;
            }
        }
        if let Some(neq) = company_id.neq {
            if opportunity.company_id == Some(neq) {
                return false;
            }
        }
    }
    if let Some(company_filter) = filter.company.as_ref() {
        let Some(company_id) = opportunity.company_id else {
            return false;
        };
        let Some(company) = companies.get(&company_id) else {
            return false;
        };
        if !company_filter_matches(company, company_filter) {
            return false;
        }
    }
    if let Some(contact) = filter.point_of_contact_id.as_ref() {
        if let Some(eq) = contact.eq {
            if opportunity.point_of_contact_id != Some(eq) {
                return false;
            }
        }
    }
    if let Some(person_filter) = filter.point_of_contact.as_ref() {
        let Some(person_id) = opportunity.point_of_contact_id else {
            return false;
        };
        let Some(person) = people.get(&person_id) else {
            return false;
        };
        if !person_filter_matches(person, person_filter, companies) {
            return false;
        }
    }
    if let Some(owner) = filter.owner_id.as_ref() {
        if let Some(eq) = owner.eq {
            if opportunity.owner_id != Some(eq) {
                return false;
            }
        }
    }
    true
}

fn task_matches(task: &Task, filter: &TaskFilterInput) -> bool {
    if let Some(and) = filter.and.as_ref() {
        if !and.iter().all(|f| task_matches(task, f)) {
            return false;
        }
    }
    if let Some(or) = filter.or.as_ref() {
        if !or.iter().any(|f| task_matches(task, f)) {
            return false;
        }
    }
    if let Some(not) = filter.not.as_ref() {
        if task_matches(task, not) {
            return false;
        }
    }
    if let Some(id) = filter.id.as_ref() {
        if let Some(eq) = id.eq {
            if task.id != eq {
                return false;
            }
        }
        if let Some(neq) = id.neq {
            if task.id == neq {
                return false;
            }
        }
    }
    if let Some(title) = filter.title.as_ref() {
        let value = task.title.as_deref().unwrap_or_default();
        if let Some(eq) = title.eq.as_deref() {
            if value != eq {
                return false;
            }
        }
        if let Some(neq) = title.neq.as_deref() {
            if value == neq {
                return false;
            }
        }
        if let Some(like) = title.ilike.as_deref() {
            let needle = like.trim_matches('%').to_lowercase();
            if !value.to_lowercase().contains(&needle) {
                return false;
            }
        }
    }
    if let Some(status) = filter.status.as_ref() {
        if let Some(eq) = status.eq {
            if task.status != Some(eq) {
                return false;
            }
        }
        if let Some(neq) = status.neq {
            if task.status == Some(neq) {
                return false;
            }
        }
        if let Some(list) = status.r#in.as_ref() {
            if task.status.is_none_or(|status| !list.contains(&status)) {
                return false;
            }
        }
    }
    if let Some(due_at) = filter.due_at.as_ref() {
        if let Some(eq) = due_at.eq {
            if task.due_at != Some(eq) {
                return false;
            }
        }
        if let Some(gte) = due_at.gte {
            if task.due_at.is_none_or(|date| date < gte) {
                return false;
            }
        }
        if let Some(lte) = due_at.lte {
            if task.due_at.is_none_or(|date| date > lte) {
                return false;
            }
        }
    }
    if let Some(assignee) = filter.assignee_id.as_ref() {
        if let Some(eq) = assignee.eq {
            if task.assignee_id != Some(eq) {
                return false;
            }
        }
        if let Some(neq) = assignee.neq {
            if task.assignee_id == Some(neq) {
                return false;
            }
        }
    }
    true
}

fn note_matches(note: &Note, filter: &NoteFilterInput) -> bool {
    if let Some(and) = filter.and.as_ref() {
        if !and.iter().all(|f| note_matches(note, f)) {
            return false;
        }
    }
    if let Some(or) = filter.or.as_ref() {
        if !or.iter().any(|f| note_matches(note, f)) {
            return false;
        }
    }
    if let Some(not) = filter.not.as_ref() {
        if note_matches(note, not) {
            return false;
        }
    }
    if let Some(id) = filter.id.as_ref() {
        if let Some(eq) = id.eq {
            if note.id != eq {
                return false;
            }
        }
        if let Some(neq) = id.neq {
            if note.id == neq {
                return false;
            }
        }
    }
    if let Some(title) = filter.title.as_ref() {
        let value = note.title.as_deref().unwrap_or_default();
        if let Some(eq) = title.eq.as_deref() {
            if value != eq {
                return false;
            }
        }
        if let Some(neq) = title.neq.as_deref() {
            if value == neq {
                return false;
            }
        }
        if let Some(like) = title.ilike.as_deref() {
            let needle = like.trim_matches('%').to_lowercase();
            if !value.to_lowercase().contains(&needle) {
                return false;
            }
        }
    }
    true
}

fn string_filter_matches(value: &str, filter: &StringFilter) -> bool {
    if let Some(eq) = filter.eq.as_deref() {
        if value != eq {
            return false;
        }
    }
    if let Some(neq) = filter.neq.as_deref() {
        if value == neq {
            return false;
        }
    }
    if let Some(starts_with) = filter.starts_with.as_deref() {
        if !value.starts_with(starts_with) {
            return false;
        }
    }
    if let Some(ends_with) = filter.ends_with.as_deref() {
        if !value.ends_with(ends_with) {
            return false;
        }
    }
    if let Some(like) = filter.like.as_deref() {
        if !value.contains(like.trim_matches('%')) {
            return false;
        }
    }
    if let Some(ilike) = filter.ilike.as_deref() {
        if !value
            .to_lowercase()
            .contains(&ilike.trim_matches('%').to_lowercase())
        {
            return false;
        }
    }
    if let Some(list) = filter.r#in.as_ref() {
        if !list.iter().any(|candidate| candidate == value) {
            return false;
        }
    }
    if let Some(is) = filter.is {
        match is {
            FilterIs::Null => return false,
            FilterIs::NotNull => {}
        }
    }
    true
}

fn uuid_value_matches(value: Uuid, filter: &UuidFilter) -> bool {
    if let Some(eq) = filter.eq {
        if value != eq {
            return false;
        }
    }
    if let Some(neq) = filter.neq {
        if value == neq {
            return false;
        }
    }
    if let Some(list) = filter.r#in.as_ref() {
        if !list.contains(&value) {
            return false;
        }
    }
    true
}

fn datetime_filter_matches(value: chrono::DateTime<chrono::Utc>, filter: &DateTimeFilter) -> bool {
    if let Some(eq) = filter.eq {
        if value != eq {
            return false;
        }
    }
    if let Some(neq) = filter.neq {
        if value == neq {
            return false;
        }
    }
    if let Some(gt) = filter.gt {
        if value <= gt {
            return false;
        }
    }
    if let Some(gte) = filter.gte {
        if value < gte {
            return false;
        }
    }
    if let Some(lt) = filter.lt {
        if value >= lt {
            return false;
        }
    }
    if let Some(lte) = filter.lte {
        if value > lte {
            return false;
        }
    }
    true
}

fn workspace_member_matches(
    member: &WorkspaceMemberGql,
    filter: &WorkspaceMemberFilterInput,
) -> bool {
    if let Some(and) = filter.and.as_ref() {
        if !and
            .iter()
            .all(|filter| workspace_member_matches(member, filter))
        {
            return false;
        }
    }
    if let Some(or) = filter.or.as_ref() {
        if !or
            .iter()
            .any(|filter| workspace_member_matches(member, filter))
        {
            return false;
        }
    }
    if let Some(not) = filter.not.as_ref() {
        if workspace_member_matches(member, not) {
            return false;
        }
    }
    if let Some(id) = filter.id.as_ref() {
        if !uuid_value_matches(member.id, id) {
            return false;
        }
    }
    if let Some(created_at) = filter.created_at.as_ref() {
        if !datetime_filter_matches(member.created_at, created_at) {
            return false;
        }
    }
    if let Some(updated_at) = filter.updated_at.as_ref() {
        if !datetime_filter_matches(member.updated_at, updated_at) {
            return false;
        }
    }
    if let Some(name) = filter.name.as_ref() {
        if let Some(first_name) = name.first_name.as_ref() {
            if !string_filter_matches(&member.name.first_name, first_name) {
                return false;
            }
        }
        if let Some(last_name) = name.last_name.as_ref() {
            if !string_filter_matches(&member.name.last_name, last_name) {
                return false;
            }
        }
    }
    if let Some(filter) = filter.user_email.as_ref() {
        if !string_filter_matches(&member.user_email, filter) {
            return false;
        }
    }
    if let Some(filter) = filter.job_title.as_ref() {
        let value = member.job_title.as_deref().unwrap_or_default();
        if !string_filter_matches(value, filter) {
            return false;
        }
    }
    if let Some(filter) = filter.color_scheme.as_ref() {
        if !string_filter_matches(&member.color_scheme, filter) {
            return false;
        }
    }
    if let Some(filter) = filter.locale.as_ref() {
        let value = member.locale.as_deref().unwrap_or_default();
        if !string_filter_matches(value, filter) {
            return false;
        }
    }
    if let Some(filter) = filter.time_zone.as_ref() {
        let value = member.time_zone.as_deref().unwrap_or_default();
        if !string_filter_matches(value, filter) {
            return false;
        }
    }
    if let Some(user_id) = filter.user_id.as_ref() {
        if !uuid_value_matches(member.user_id, user_id) {
            return false;
        }
    }
    true
}

fn sort_workspace_members(
    members: &mut [WorkspaceMemberGql],
    terms: &[WorkspaceMemberOrderByInput],
) {
    use std::cmp::Ordering;

    fn compare_option_string(
        left: Option<&str>,
        right: Option<&str>,
        direction: OrderByDirection,
    ) -> Ordering {
        let nulls_last = matches!(
            direction,
            OrderByDirection::AscNullsLast | OrderByDirection::DescNullsLast
        );
        match (left, right) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => {
                if nulls_last {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            (Some(_), None) => {
                if nulls_last {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            (Some(left), Some(right)) => {
                let ordering = left.cmp(right);
                if matches!(
                    direction,
                    OrderByDirection::DescNullsFirst | OrderByDirection::DescNullsLast
                ) {
                    ordering.reverse()
                } else {
                    ordering
                }
            }
        }
    }

    members.sort_by(|left, right| {
        for term in terms {
            let ordering = if let Some(direction) = term.id {
                compare_option_string(
                    Some(&left.id.to_string()),
                    Some(&right.id.to_string()),
                    direction,
                )
            } else if let Some(direction) = term.created_at {
                compare_option_string(
                    Some(&left.created_at.to_rfc3339()),
                    Some(&right.created_at.to_rfc3339()),
                    direction,
                )
            } else if let Some(direction) = term.updated_at {
                compare_option_string(
                    Some(&left.updated_at.to_rfc3339()),
                    Some(&right.updated_at.to_rfc3339()),
                    direction,
                )
            } else if let Some(direction) = term.user_email {
                compare_option_string(Some(&left.user_email), Some(&right.user_email), direction)
            } else if let Some(direction) = term.job_title {
                compare_option_string(
                    left.job_title.as_deref(),
                    right.job_title.as_deref(),
                    direction,
                )
            } else if let Some(name) = term.name.as_ref() {
                let mut ordering = Ordering::Equal;
                if let Some(direction) = name.first_name {
                    ordering = compare_option_string(
                        Some(&left.name.first_name),
                        Some(&right.name.first_name),
                        direction,
                    );
                }
                if ordering == Ordering::Equal {
                    if let Some(direction) = name.last_name {
                        ordering = compare_option_string(
                            Some(&left.name.last_name),
                            Some(&right.name.last_name),
                            direction,
                        );
                    }
                }
                ordering
            } else {
                Ordering::Equal
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        Ordering::Equal
    });
}

fn company_matches(company: &Company, filter: &CompanyFilterInput) -> bool {
    if let Some(id) = filter.id.as_ref() {
        if let Some(eq) = id.eq {
            if company.id != eq {
                return false;
            }
        }
        if let Some(neq) = id.neq {
            if company.id == neq {
                return false;
            }
        }
        if let Some(list) = id.r#in.as_ref() {
            if !list.contains(&company.id) {
                return false;
            }
        }
    }
    if let Some(name) = filter.name.as_ref() {
        let value = company.name.as_deref().unwrap_or_default();
        if let Some(eq) = name.eq.as_deref() {
            if value != eq {
                return false;
            }
        }
        if let Some(neq) = name.neq.as_deref() {
            if value == neq {
                return false;
            }
        }
        if let Some(like) = name.ilike.as_deref() {
            let needle = like.trim_matches('%').to_lowercase();
            if !value.to_lowercase().contains(&needle) {
                return false;
            }
        }
        if let Some(starts_with) = name.starts_with.as_deref() {
            if !value
                .to_lowercase()
                .starts_with(&starts_with.to_lowercase())
            {
                return false;
            }
        }
    }
    true
}

#[ComplexObject]
impl TaskGql {
    #[allow(clippy::too_many_arguments)]
    async fn task_targets(
        &self,
        ctx: &Context<'_>,
        first: Option<i64>,
        last: Option<i64>,
        offset: Option<i64>,
        before: Option<String>,
        after: Option<String>,
        filter: Option<TaskTargetFilterInput>,
        order_by: Option<Vec<serde_json::Value>>,
    ) -> async_graphql::Result<TaskTargetConnection> {
        let order = json_order_terms(order_by.as_deref().unwrap_or_default());
        let auth = require_auth(ctx)?;
        let filter = filter.unwrap_or_default();
        let mut targets = state(ctx)
            .db
            .list_task_targets(auth.workspace_id, Some(self.id), None, None, None)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        targets.retain(|target| {
            uuid_filter_matches(&filter.id, target.id)
                && uuid_filter_matches(&filter.task_id, target.task_id)
                && option_uuid_filter_matches(&filter.target_company_id, target.target_company_id)
                && option_uuid_filter_matches(
                    &filter.target_opportunity_id,
                    target.target_opportunity_id,
                )
                && option_uuid_filter_matches(&filter.target_person_id, target.target_person_id)
        });
        sort_task_targets(&mut targets, &order);
        let total = targets.len() as i64;
        let (targets, page_info) =
            paginate_targets(targets, first, last, offset, before, after, |t| t.id)?;
        let edges = targets
            .into_iter()
            .map(|node| TaskTargetEdge {
                cursor: Cursor(crate::rest::encode_cursor(node.id)),
                node: node.into(),
            })
            .collect();
        Ok(TaskTargetConnection {
            total_count: Some(total),
            edges,
            page_info,
        })
    }
}

#[ComplexObject]
impl NoteGql {
    #[allow(clippy::too_many_arguments)]
    async fn note_targets(
        &self,
        ctx: &Context<'_>,
        first: Option<i64>,
        last: Option<i64>,
        offset: Option<i64>,
        before: Option<String>,
        after: Option<String>,
        filter: Option<NoteTargetFilterInput>,
        order_by: Option<Vec<serde_json::Value>>,
    ) -> async_graphql::Result<NoteTargetConnection> {
        let order = json_order_terms(order_by.as_deref().unwrap_or_default());
        let auth = require_auth(ctx)?;
        let filter = filter.unwrap_or_default();
        let mut targets = state(ctx)
            .db
            .list_note_targets(auth.workspace_id, Some(self.id), None, None, None)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        targets.retain(|target| {
            uuid_filter_matches(&filter.id, target.id)
                && uuid_filter_matches(&filter.note_id, target.note_id)
                && option_uuid_filter_matches(&filter.target_company_id, target.target_company_id)
                && option_uuid_filter_matches(
                    &filter.target_opportunity_id,
                    target.target_opportunity_id,
                )
                && option_uuid_filter_matches(&filter.target_person_id, target.target_person_id)
        });
        sort_note_targets(&mut targets, &order);
        let total = targets.len() as i64;
        let (targets, page_info) =
            paginate_targets(targets, first, last, offset, before, after, |t| t.id)?;
        let edges = targets
            .into_iter()
            .map(|node| NoteTargetEdge {
                cursor: Cursor(crate::rest::encode_cursor(node.id)),
                node: node.into(),
            })
            .collect();
        Ok(NoteTargetConnection {
            total_count: Some(total),
            edges,
            page_info,
        })
    }
}

#[ComplexObject]
impl TaskTargetGql {
    async fn task(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<TaskGql>> {
        let auth = require_auth(ctx)?;
        Ok(state(ctx)
            .db
            .get_task(auth.workspace_id, self.task_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .map(Into::into))
    }

    async fn target_company(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<Company>> {
        let Some(id) = self.target_company_id else {
            return Ok(None);
        };
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .get_company(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn target_opportunity(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Option<Opportunity>> {
        let Some(id) = self.target_opportunity_id else {
            return Ok(None);
        };
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .get_opportunity(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn target_person(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<Person>> {
        let Some(id) = self.target_person_id else {
            return Ok(None);
        };
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .get_person(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }
}

#[ComplexObject]
impl NoteTargetGql {
    async fn note(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<NoteGql>> {
        let auth = require_auth(ctx)?;
        Ok(state(ctx)
            .db
            .get_note(auth.workspace_id, self.note_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .map(Into::into))
    }

    async fn target_company(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<Company>> {
        let Some(id) = self.target_company_id else {
            return Ok(None);
        };
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .get_company(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn target_opportunity(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Option<Opportunity>> {
        let Some(id) = self.target_opportunity_id else {
            return Ok(None);
        };
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .get_opportunity(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn target_person(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<Person>> {
        let Some(id) = self.target_person_id else {
            return Ok(None);
        };
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .get_person(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }
}

fn uuid_filter_matches(filter: &Option<UuidFilter>, value: Uuid) -> bool {
    match filter {
        Some(filter) => {
            filter.eq.is_none_or(|eq| eq == value)
                && filter.neq.is_none_or(|neq| neq != value)
                && filter
                    .r#in
                    .as_ref()
                    .is_none_or(|list| list.contains(&value))
        }
        None => true,
    }
}

fn option_uuid_filter_matches(filter: &Option<UuidFilter>, value: Option<Uuid>) -> bool {
    match filter {
        Some(filter) => {
            filter.eq.is_none_or(|eq| value == Some(eq))
                && filter.neq.is_none_or(|neq| value != Some(neq))
                && filter
                    .r#in
                    .as_ref()
                    .is_none_or(|list| value.is_some_and(|value| list.contains(&value)))
        }
        None => true,
    }
}

fn paginate_targets<T: Clone>(
    mut all: Vec<T>,
    first: Option<i64>,
    last: Option<i64>,
    offset: Option<i64>,
    before: Option<String>,
    after: Option<String>,
    id_of: impl Fn(&T) -> Uuid,
) -> async_graphql::Result<(Vec<T>, PageInfo)> {
    let limit = first.or(last).unwrap_or(20).clamp(1, 1000) as usize;
    let offset = offset.unwrap_or(0).max(0) as usize;
    let starting_after = after
        .as_deref()
        .map(crate::rest::decode_cursor)
        .transpose()
        .map_err(gql_err)?;
    let ending_before = before
        .as_deref()
        .map(crate::rest::decode_cursor)
        .transpose()
        .map_err(gql_err)?;
    if let Some(after) = starting_after {
        if let Some(index) = all.iter().position(|item| id_of(item) == after) {
            all.drain(..=index);
        }
    }
    if let Some(before) = ending_before {
        if let Some(index) = all.iter().position(|item| id_of(item) == before) {
            all.truncate(index);
        }
    }
    let start = offset.min(all.len());
    let end = (start + limit).min(all.len());
    let page_info = PageInfo {
        has_next_page: end < all.len(),
        has_previous_page: start > 0 || starting_after.is_some(),
        start_cursor: all
            .get(start)
            .map(|item| crate::rest::encode_cursor(id_of(item))),
        end_cursor: all
            .get(end.saturating_sub(1))
            .map(|item| crate::rest::encode_cursor(id_of(item))),
    };
    Ok((all[start..end].to_vec(), page_info))
}

fn gql_err(error: AppError) -> async_graphql::Error {
    let code = error.graphql_code();
    async_graphql::Error::new(error.public_message()).extend_with(|_, ext| {
        ext.set("code", code);
    })
}

fn state<'a>(ctx: &'a Context<'_>) -> &'a AppState {
    &ctx_data(ctx).state
}

#[Object]
impl QueryRoot {
    async fn current_user(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<User>> {
        let Some(auth) = ctx_data(ctx).auth.as_ref() else {
            return Ok(None);
        };
        let Some(user_id) = auth.user_id else {
            return Ok(None);
        };
        let state = state(ctx);
        let user = state
            .db
            .find_user_by_id(user_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
            .ok_or_else(|| gql_err(AppError::NotFound("User not found".to_string())))?;
        build_current_user(state, &user, auth.workspace_id, auth.role)
            .await
            .map(Some)
    }

    async fn current_workspace(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Option<Workspace>> {
        let Some(auth) = ctx_data(ctx).auth.as_ref() else {
            return Ok(None);
        };
        let state = state(ctx);
        let workspace = state
            .db
            .find_workspace(auth.workspace_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
            .ok_or_else(|| gql_err(AppError::NotFound("Workspace not found".to_string())))?;
        let count = state
            .db
            .count_workspace_members(auth.workspace_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?;
        Ok(Some(current_user::workspace(
            &workspace,
            auth.role,
            &state.config.frontend_url,
            count,
        )))
    }

    async fn get_roles(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<current_user::RoleGql>> {
        let auth = require_auth(ctx)?;
        Ok([Role::Owner, Role::Admin, Role::Member]
            .into_iter()
            .map(|role| current_user::role_gql(auth.workspace_id, role))
            .collect())
    }

    async fn get_role(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
    ) -> async_graphql::Result<current_user::RoleGql> {
        let auth = require_auth(ctx)?;
        [Role::Owner, Role::Admin, Role::Member]
            .into_iter()
            .map(|role| current_user::role_gql(auth.workspace_id, role))
            .find(|role| role.id == id)
            .ok_or_else(|| gql_err(AppError::NotFound("Role not found".to_string())))
    }

    async fn get_permission_flags(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Vec<PermissionFlagGql>> {
        let auth = require_auth(ctx)?;
        let now = Utc::now();
        Ok(PERMISSION_FLAGS
            .iter()
            .map(|(key, label)| {
                let id = Uuid::new_v5(&auth.workspace_id, key.as_bytes());
                PermissionFlagGql {
                    id,
                    universal_identifier: id,
                    key: key.to_string(),
                    label: label.to_string(),
                    description: None,
                    icon: None,
                    permission_type: "WORKSPACE".to_string(),
                    application_id: Uuid::nil(),
                    created_at: now,
                    updated_at: now,
                }
            })
            .collect())
    }

    async fn mostly_empty_field_metadata_ids(
        &self,
        ctx: &Context<'_>,
        object_metadata_id: Uuid,
    ) -> async_graphql::Result<Vec<Uuid>> {
        let auth = require_auth(ctx)?;
        let _ = state(ctx)
            .db
            .get_object(auth.workspace_id, object_metadata_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Object not found".to_string())))?;
        Ok(Vec::new())
    }

    async fn objects(
        &self,
        ctx: &Context<'_>,
        paging: Option<CursorPagingInput>,
        filter: Option<ObjectFilterInput>,
    ) -> async_graphql::Result<ObjectConnection> {
        let auth = require_auth(ctx)?;
        let filter = filter.unwrap_or_default();
        let objects = state(ctx)
            .db
            .list_objects(auth.workspace_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let objects = objects
            .into_iter()
            .filter(|object| object_matches_filter(object, &filter))
            .collect();
        Ok(objects_connection(objects, paging))
    }

    async fn object(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<ObjectMetadata> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .get_object(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Object not found".to_string())))
    }

    async fn fields(
        &self,
        ctx: &Context<'_>,
        paging: Option<CursorPagingInput>,
        filter: Option<FieldFilterInput>,
    ) -> async_graphql::Result<FieldConnection> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        state
            .db
            .ensure_standard_metadata(auth.workspace_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let filter = filter.unwrap_or_default();
        let fields = state
            .db
            .list_fields(auth.workspace_id, None)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .into_iter()
            .filter(|field| field_matches_filter(field, &filter))
            .collect::<Vec<_>>();
        let connection = squid_core::object_fields_connection(&fields, paging);
        Ok(FieldConnection {
            page_info: connection.page_info,
            edges: connection.edges,
        })
    }

    async fn get_view_fields(
        &self,
        ctx: &Context<'_>,
        view_id: String,
    ) -> async_graphql::Result<Vec<ViewField>> {
        let auth = require_auth(ctx)?;
        let view_id = Uuid::parse_str(&view_id)
            .map_err(|_| gql_err(AppError::BadRequest("Invalid view id".to_string())))?;
        state(ctx)
            .db
            .list_view_fields_public(auth.workspace_id, view_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn get_view_filters(
        &self,
        ctx: &Context<'_>,
        view_id: Option<String>,
    ) -> async_graphql::Result<Vec<ViewFilter>> {
        let auth = require_auth(ctx)?;
        let view_id = view_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|_| gql_err(AppError::BadRequest("Invalid view id".to_string())))?;
        match view_id {
            Some(view_id) => state(ctx)
                .db
                .list_view_filters(auth.workspace_id, view_id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e))),
            None => Ok(Vec::new()),
        }
    }

    async fn get_view_sorts(
        &self,
        ctx: &Context<'_>,
        view_id: Option<String>,
    ) -> async_graphql::Result<Vec<ViewSort>> {
        let auth = require_auth(ctx)?;
        let view_id = view_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|_| gql_err(AppError::BadRequest("Invalid view id".to_string())))?;
        match view_id {
            Some(view_id) => state(ctx)
                .db
                .list_view_sorts(auth.workspace_id, view_id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e))),
            None => Ok(Vec::new()),
        }
    }

    async fn get_view_field_groups(
        &self,
        ctx: &Context<'_>,
        view_id: String,
    ) -> async_graphql::Result<Vec<ViewFieldGroup>> {
        let auth = require_auth(ctx)?;
        let view_id = Uuid::parse_str(&view_id)
            .map_err(|_| gql_err(AppError::BadRequest("Invalid view id".to_string())))?;
        state(ctx)
            .db
            .list_view_field_groups(auth.workspace_id, view_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn get_view_filter_groups(
        &self,
        ctx: &Context<'_>,
        view_id: Option<String>,
    ) -> async_graphql::Result<Vec<ViewFilterGroup>> {
        let auth = require_auth(ctx)?;
        let view_id = view_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|_| gql_err(AppError::BadRequest("Invalid view id".to_string())))?;
        match view_id {
            Some(view_id) => state(ctx)
                .db
                .list_view_filter_groups(auth.workspace_id, view_id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e))),
            None => Ok(Vec::new()),
        }
    }

    async fn get_view_groups(
        &self,
        ctx: &Context<'_>,
        view_id: Option<String>,
    ) -> async_graphql::Result<Vec<ViewGroup>> {
        let auth = require_auth(ctx)?;
        let view_id = view_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|_| gql_err(AppError::BadRequest("Invalid view id".to_string())))?;
        match view_id {
            Some(view_id) => state(ctx)
                .db
                .list_view_groups(auth.workspace_id, view_id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e))),
            None => Ok(Vec::new()),
        }
    }

    async fn get_views(
        &self,
        ctx: &Context<'_>,
        object_metadata_id: Option<String>,
        view_types: Option<Vec<ViewType>>,
    ) -> async_graphql::Result<Vec<View>> {
        let auth = require_auth(ctx)?;
        let object_id = object_metadata_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|_| gql_err(AppError::BadRequest("Invalid objectMetadataId".to_string())))?;
        let types: Option<Vec<String>> = view_types.map(|types| {
            types
                .into_iter()
                .map(|view_type| view_type.as_str().to_string())
                .collect()
        });
        state(ctx)
            .db
            .list_views(auth.workspace_id, object_id, types.as_deref())
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn get_view(&self, ctx: &Context<'_>, id: String) -> async_graphql::Result<Option<View>> {
        let auth = require_auth(ctx)?;
        let id = Uuid::parse_str(&id)
            .map_err(|_| gql_err(AppError::BadRequest("Invalid view id".to_string())))?;
        state(ctx)
            .db
            .get_view(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn field(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<FieldMetadata> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .get_field(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Field not found".to_string())))
    }

    #[allow(clippy::too_many_arguments)]
    async fn people(
        &self,
        ctx: &Context<'_>,
        first: Option<i64>,
        last: Option<i64>,
        offset: Option<i64>,
        before: Option<String>,
        after: Option<String>,
        filter: Option<PersonFilterInput>,
        order_by: Option<Vec<PersonOrderByInput>>,
    ) -> async_graphql::Result<PersonConnection> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let limit = first
            .or(last)
            .unwrap_or(20)
            .clamp(1, state.config.max_page_size);
        let offset = offset.unwrap_or(0).max(0);
        let starting_after = after
            .as_deref()
            .map(crate::rest::decode_cursor)
            .transpose()
            .map_err(gql_err)?;
        let ending_before = before
            .as_deref()
            .map(crate::rest::decode_cursor)
            .transpose()
            .map_err(gql_err)?;
        let order = order_terms(order_by.as_deref().unwrap_or_default());

        let filter = filter.unwrap_or_default();
        let filter = filter.to_expr();

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
            .map_err(|e| gql_err(AppError::Internal(e)))?;

        Ok(connection_from_page(page))
    }

    async fn api_keys(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<ApiKey>> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .list_api_keys(auth.workspace_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))
    }

    async fn api_key(
        &self,
        ctx: &Context<'_>,
        input: GetApiKeyInput,
    ) -> async_graphql::Result<Option<ApiKey>> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .get_api_key(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))
    }

    #[allow(clippy::too_many_arguments)]
    async fn workspace_members(
        &self,
        ctx: &Context<'_>,
        first: Option<i64>,
        last: Option<i64>,
        offset: Option<i64>,
        before: Option<String>,
        after: Option<String>,
        filter: Option<WorkspaceMemberFilterInput>,
        order_by: Option<Vec<WorkspaceMemberOrderByInput>>,
    ) -> async_graphql::Result<WorkspaceMemberConnection> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let filter = filter.unwrap_or_default();
        let mut all = state
            .db
            .list_workspace_members_with_users(auth.workspace_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
            .into_iter()
            .map(|(membership, user)| {
                let role = membership.role;
                current_user::workspace_member(&membership, &user, role)
            })
            .filter(|member| workspace_member_matches(member, &filter))
            .collect::<Vec<_>>();
        if let Some(order_by) = order_by.as_deref() {
            sort_workspace_members(&mut all, order_by);
        }

        let total = all.len() as i64;
        let limit = first
            .or(last)
            .unwrap_or(20)
            .clamp(1, state.config.max_page_size) as usize;
        let offset = offset.unwrap_or(0).max(0) as usize;

        let starting_after = after
            .as_deref()
            .map(crate::rest::decode_cursor)
            .transpose()
            .map_err(gql_err)?;
        let ending_before = before
            .as_deref()
            .map(crate::rest::decode_cursor)
            .transpose()
            .map_err(gql_err)?;

        if let Some(after) = starting_after {
            if let Some(index) = all.iter().position(|member| member.id == after) {
                all.drain(..=index);
            }
        }
        if let Some(before) = ending_before {
            if let Some(index) = all.iter().position(|member| member.id == before) {
                all.truncate(index);
            }
        }

        let start = offset.min(all.len());
        let end = (start + limit).min(all.len());
        let has_next_page = end < all.len();
        let has_previous_page = start > 0 || starting_after.is_some();
        let edges = all[start..end]
            .iter()
            .cloned()
            .map(|member| WorkspaceMemberEdge {
                cursor: Cursor(crate::rest::encode_cursor(member.id)),
                node: member,
            })
            .collect();

        Ok(WorkspaceMemberConnection {
            total_count: Some(total),
            edges,
            page_info: PageInfo {
                has_next_page,
                has_previous_page,
                start_cursor: (end > start).then(|| crate::rest::encode_cursor(all[start].id)),
                end_cursor: (end > start).then(|| crate::rest::encode_cursor(all[end - 1].id)),
            },
        })
    }

    async fn workspace_member(
        &self,
        ctx: &Context<'_>,
        filter: WorkspaceMemberFilterInput,
    ) -> async_graphql::Result<Option<WorkspaceMemberGql>> {
        let auth = require_auth(ctx)?;
        Ok(state(ctx)
            .db
            .list_workspace_members_with_users(auth.workspace_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
            .into_iter()
            .find_map(|(membership, user)| {
                let role = membership.role;
                let member = current_user::workspace_member(&membership, &user, role);
                workspace_member_matches(&member, &filter).then_some(member)
            }))
    }

    #[allow(clippy::too_many_arguments)]
    async fn companies(
        &self,
        ctx: &Context<'_>,
        first: Option<i64>,
        last: Option<i64>,
        offset: Option<i64>,
        before: Option<String>,
        after: Option<String>,
        filter: Option<CompanyFilterInput>,
        order_by: Option<Vec<serde_json::Value>>,
    ) -> async_graphql::Result<CompanyConnection> {
        let auth = require_auth(ctx)?;
        let order = json_order_terms(order_by.as_deref().unwrap_or_default());
        let (mut all, _) = state(ctx)
            .db
            .list_companies(auth.workspace_id, 10_000, 0)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let filter = filter.unwrap_or_default();
        all.retain(|company| company_matches(company, &filter));
        sort_companies(&mut all, &order);

        let total = all.len() as i64;
        let limit = first
            .or(last)
            .unwrap_or(20)
            .clamp(1, state(ctx).config.max_page_size) as usize;
        let offset = offset.unwrap_or(0).max(0) as usize;

        let starting_after = after
            .as_deref()
            .map(crate::rest::decode_cursor)
            .transpose()
            .map_err(gql_err)?;
        let ending_before = before
            .as_deref()
            .map(crate::rest::decode_cursor)
            .transpose()
            .map_err(gql_err)?;

        if let Some(after) = starting_after {
            if let Some(index) = all.iter().position(|company| company.id == after) {
                all.drain(..=index);
            }
        }
        if let Some(before) = ending_before {
            if let Some(index) = all.iter().position(|company| company.id == before) {
                all.truncate(index);
            }
        }

        let start = offset.min(all.len());
        let end = (start + limit).min(all.len());
        let has_next_page = end < all.len();
        let has_previous_page = start > 0 || starting_after.is_some();
        let edges = all[start..end]
            .iter()
            .cloned()
            .map(|company| CompanyEdge {
                cursor: Cursor(crate::rest::encode_cursor(company.id)),
                node: company,
            })
            .collect();

        Ok(CompanyConnection {
            total_count: Some(total),
            edges,
            page_info: PageInfo {
                has_next_page,
                has_previous_page,
                start_cursor: all
                    .get(start)
                    .map(|company| crate::rest::encode_cursor(company.id)),
                end_cursor: all
                    .get(end.saturating_sub(1))
                    .map(|company| crate::rest::encode_cursor(company.id)),
            },
        })
    }

    async fn company(
        &self,
        ctx: &Context<'_>,
        filter: CompanyFilterInput,
    ) -> async_graphql::Result<Option<Company>> {
        let auth = require_auth(ctx)?;
        if filter.id.is_none() && filter.name.is_none() {
            return Err(gql_err(AppError::BadRequest(
                "company requires a non-empty filter".to_string(),
            )));
        }
        let (companies, _) = state(ctx)
            .db
            .list_companies(auth.workspace_id, 10_000, 0)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(companies
            .into_iter()
            .find(|company| company_matches(company, &filter)))
    }

    #[allow(clippy::too_many_arguments)]
    async fn opportunities(
        &self,
        ctx: &Context<'_>,
        first: Option<i64>,
        last: Option<i64>,
        offset: Option<i64>,
        before: Option<String>,
        after: Option<String>,
        filter: Option<OpportunityFilterInput>,
        order_by: Option<Vec<serde_json::Value>>,
    ) -> async_graphql::Result<OpportunityConnection> {
        let auth = require_auth(ctx)?;
        let order = json_order_terms(order_by.as_deref().unwrap_or_default());
        let (mut all, _) = state(ctx)
            .db
            .list_opportunities(auth.workspace_id, 10_000, 0)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let filter = filter.unwrap_or_default();
        let companies = if opportunity_needs_companies(&filter) {
            let (companies, _) = state(ctx)
                .db
                .list_companies(auth.workspace_id, 10_000, 0)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?;
            companies
                .into_iter()
                .map(|company| (company.id, company))
                .collect()
        } else {
            HashMap::new()
        };
        let people = if opportunity_needs_people(&filter) {
            state(ctx)
                .db
                .list_persons_page(auth.workspace_id, 10_000, 0, None, None, &[], None)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?
                .persons
                .into_iter()
                .map(|person| (person.id, person))
                .collect()
        } else {
            HashMap::new()
        };
        all.retain(|opportunity| opportunity_matches(opportunity, &filter, &companies, &people));
        sort_opportunities(&mut all, &order);

        let total = all.len() as i64;
        let limit = first
            .or(last)
            .unwrap_or(20)
            .clamp(1, state(ctx).config.max_page_size) as usize;
        let offset = offset.unwrap_or(0).max(0) as usize;

        let starting_after = after
            .as_deref()
            .map(crate::rest::decode_cursor)
            .transpose()
            .map_err(gql_err)?;
        let ending_before = before
            .as_deref()
            .map(crate::rest::decode_cursor)
            .transpose()
            .map_err(gql_err)?;

        if let Some(after) = starting_after {
            if let Some(index) = all.iter().position(|opportunity| opportunity.id == after) {
                all.drain(..=index);
            }
        }
        if let Some(before) = ending_before {
            if let Some(index) = all.iter().position(|opportunity| opportunity.id == before) {
                all.truncate(index);
            }
        }

        let start = offset.min(all.len());
        let end = (start + limit).min(all.len());
        let has_next_page = end < all.len();
        let has_previous_page = start > 0 || starting_after.is_some();
        let edges = all[start..end]
            .iter()
            .cloned()
            .map(|opportunity| OpportunityEdge {
                cursor: Cursor(crate::rest::encode_cursor(opportunity.id)),
                node: opportunity,
            })
            .collect();

        Ok(OpportunityConnection {
            total_count: Some(total),
            edges,
            page_info: PageInfo {
                has_next_page,
                has_previous_page,
                start_cursor: all
                    .get(start)
                    .map(|opportunity| crate::rest::encode_cursor(opportunity.id)),
                end_cursor: all
                    .get(end.saturating_sub(1))
                    .map(|opportunity| crate::rest::encode_cursor(opportunity.id)),
            },
        })
    }

    async fn opportunity(
        &self,
        ctx: &Context<'_>,
        filter: OpportunityFilterInput,
    ) -> async_graphql::Result<Option<Opportunity>> {
        let auth = require_auth(ctx)?;
        let (opportunities, _) = state(ctx)
            .db
            .list_opportunities(auth.workspace_id, 10_000, 0)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let companies = if opportunity_needs_companies(&filter) {
            let (companies, _) = state(ctx)
                .db
                .list_companies(auth.workspace_id, 10_000, 0)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?;
            companies
                .into_iter()
                .map(|company| (company.id, company))
                .collect()
        } else {
            HashMap::new()
        };
        let people = if opportunity_needs_people(&filter) {
            state(ctx)
                .db
                .list_persons_page(auth.workspace_id, 10_000, 0, None, None, &[], None)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?
                .persons
                .into_iter()
                .map(|person| (person.id, person))
                .collect()
        } else {
            HashMap::new()
        };
        Ok(opportunities
            .into_iter()
            .find(|opportunity| opportunity_matches(opportunity, &filter, &companies, &people)))
    }

    #[allow(clippy::too_many_arguments)]
    async fn tasks(
        &self,
        ctx: &Context<'_>,
        first: Option<i64>,
        last: Option<i64>,
        offset: Option<i64>,
        before: Option<String>,
        after: Option<String>,
        filter: Option<TaskFilterInput>,
        order_by: Option<Vec<serde_json::Value>>,
    ) -> async_graphql::Result<TaskConnection> {
        let auth = require_auth(ctx)?;
        let order = json_order_terms(order_by.as_deref().unwrap_or_default());
        let (mut all, _) = state(ctx)
            .db
            .list_tasks(auth.workspace_id, 10_000, 0)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let filter = filter.unwrap_or_default();
        all.retain(|task| task_matches(task, &filter));
        sort_tasks(&mut all, &order);

        let total = all.len() as i64;
        let limit = first
            .or(last)
            .unwrap_or(20)
            .clamp(1, state(ctx).config.max_page_size) as usize;
        let offset = offset.unwrap_or(0).max(0) as usize;
        let starting_after = after
            .as_deref()
            .map(crate::rest::decode_cursor)
            .transpose()
            .map_err(gql_err)?;
        let ending_before = before
            .as_deref()
            .map(crate::rest::decode_cursor)
            .transpose()
            .map_err(gql_err)?;
        if let Some(after) = starting_after {
            if let Some(index) = all.iter().position(|task| task.id == after) {
                all.drain(..=index);
            }
        }
        if let Some(before) = ending_before {
            if let Some(index) = all.iter().position(|task| task.id == before) {
                all.truncate(index);
            }
        }

        let start = offset.min(all.len());
        let end = (start + limit).min(all.len());
        let has_next_page = end < all.len();
        let has_previous_page = start > 0 || starting_after.is_some();
        let edges = all[start..end]
            .iter()
            .cloned()
            .map(|task| TaskEdge {
                cursor: Cursor(crate::rest::encode_cursor(task.id)),
                node: task.into(),
            })
            .collect();

        Ok(TaskConnection {
            total_count: Some(total),
            edges,
            page_info: PageInfo {
                has_next_page,
                has_previous_page,
                start_cursor: all
                    .get(start)
                    .map(|task| crate::rest::encode_cursor(task.id)),
                end_cursor: all
                    .get(end.saturating_sub(1))
                    .map(|task| crate::rest::encode_cursor(task.id)),
            },
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn task_targets(
        &self,
        ctx: &Context<'_>,
        first: Option<i64>,
        last: Option<i64>,
        offset: Option<i64>,
        before: Option<String>,
        after: Option<String>,
        filter: Option<TaskTargetFilterInput>,
        order_by: Option<Vec<serde_json::Value>>,
    ) -> async_graphql::Result<TaskTargetConnection> {
        let order = json_order_terms(order_by.as_deref().unwrap_or_default());
        let auth = require_auth(ctx)?;
        let filter = filter.unwrap_or_default();
        let mut targets = state(ctx)
            .db
            .list_task_targets(auth.workspace_id, None, None, None, None)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        targets.retain(|target| {
            uuid_filter_matches(&filter.id, target.id)
                && uuid_filter_matches(&filter.task_id, target.task_id)
                && option_uuid_filter_matches(&filter.target_company_id, target.target_company_id)
                && option_uuid_filter_matches(
                    &filter.target_opportunity_id,
                    target.target_opportunity_id,
                )
                && option_uuid_filter_matches(&filter.target_person_id, target.target_person_id)
        });
        sort_task_targets(&mut targets, &order);
        let total = targets.len() as i64;
        let (targets, page_info) =
            paginate_targets(targets, first, last, offset, before, after, |t| t.id)?;
        let edges = targets
            .into_iter()
            .map(|node| TaskTargetEdge {
                cursor: Cursor(crate::rest::encode_cursor(node.id)),
                node: node.into(),
            })
            .collect();
        Ok(TaskTargetConnection {
            total_count: Some(total),
            edges,
            page_info,
        })
    }

    async fn task_target(
        &self,
        ctx: &Context<'_>,
        filter: TaskTargetFilterInput,
    ) -> async_graphql::Result<Option<TaskTargetGql>> {
        let auth = require_auth(ctx)?;
        let targets = state(ctx)
            .db
            .list_task_targets(
                auth.workspace_id,
                filter.task_id.as_ref().and_then(|f| f.eq),
                filter.target_company_id.as_ref().and_then(|f| f.eq),
                filter.target_opportunity_id.as_ref().and_then(|f| f.eq),
                filter.target_person_id.as_ref().and_then(|f| f.eq),
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(targets
            .into_iter()
            .find(|target| {
                uuid_filter_matches(&filter.id, target.id)
                    && uuid_filter_matches(&filter.task_id, target.task_id)
                    && option_uuid_filter_matches(
                        &filter.target_company_id,
                        target.target_company_id,
                    )
                    && option_uuid_filter_matches(
                        &filter.target_opportunity_id,
                        target.target_opportunity_id,
                    )
                    && option_uuid_filter_matches(&filter.target_person_id, target.target_person_id)
            })
            .map(Into::into))
    }

    #[allow(clippy::too_many_arguments)]
    async fn note_targets(
        &self,
        ctx: &Context<'_>,
        first: Option<i64>,
        last: Option<i64>,
        offset: Option<i64>,
        before: Option<String>,
        after: Option<String>,
        filter: Option<NoteTargetFilterInput>,
        order_by: Option<Vec<serde_json::Value>>,
    ) -> async_graphql::Result<NoteTargetConnection> {
        let order = json_order_terms(order_by.as_deref().unwrap_or_default());
        let auth = require_auth(ctx)?;
        let filter = filter.unwrap_or_default();
        let mut targets = state(ctx)
            .db
            .list_note_targets(auth.workspace_id, None, None, None, None)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        targets.retain(|target| {
            uuid_filter_matches(&filter.id, target.id)
                && uuid_filter_matches(&filter.note_id, target.note_id)
                && option_uuid_filter_matches(&filter.target_company_id, target.target_company_id)
                && option_uuid_filter_matches(
                    &filter.target_opportunity_id,
                    target.target_opportunity_id,
                )
                && option_uuid_filter_matches(&filter.target_person_id, target.target_person_id)
        });
        sort_note_targets(&mut targets, &order);
        let total = targets.len() as i64;
        let (targets, page_info) =
            paginate_targets(targets, first, last, offset, before, after, |t| t.id)?;
        let edges = targets
            .into_iter()
            .map(|node| NoteTargetEdge {
                cursor: Cursor(crate::rest::encode_cursor(node.id)),
                node: node.into(),
            })
            .collect();
        Ok(NoteTargetConnection {
            total_count: Some(total),
            edges,
            page_info,
        })
    }

    async fn note_target(
        &self,
        ctx: &Context<'_>,
        filter: NoteTargetFilterInput,
    ) -> async_graphql::Result<Option<NoteTargetGql>> {
        let auth = require_auth(ctx)?;
        let targets = state(ctx)
            .db
            .list_note_targets(
                auth.workspace_id,
                filter.note_id.as_ref().and_then(|f| f.eq),
                filter.target_company_id.as_ref().and_then(|f| f.eq),
                filter.target_opportunity_id.as_ref().and_then(|f| f.eq),
                filter.target_person_id.as_ref().and_then(|f| f.eq),
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(targets
            .into_iter()
            .find(|target| {
                uuid_filter_matches(&filter.id, target.id)
                    && uuid_filter_matches(&filter.note_id, target.note_id)
                    && option_uuid_filter_matches(
                        &filter.target_company_id,
                        target.target_company_id,
                    )
                    && option_uuid_filter_matches(
                        &filter.target_opportunity_id,
                        target.target_opportunity_id,
                    )
                    && option_uuid_filter_matches(&filter.target_person_id, target.target_person_id)
            })
            .map(Into::into))
    }

    async fn task(
        &self,
        ctx: &Context<'_>,
        filter: TaskFilterInput,
    ) -> async_graphql::Result<Option<TaskGql>> {
        let auth = require_auth(ctx)?;
        let (tasks, _) = state(ctx)
            .db
            .list_tasks(auth.workspace_id, 10_000, 0)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(tasks
            .into_iter()
            .find(|task| task_matches(task, &filter))
            .map(Into::into))
    }

    #[allow(clippy::too_many_arguments)]
    async fn notes(
        &self,
        ctx: &Context<'_>,
        first: Option<i64>,
        last: Option<i64>,
        offset: Option<i64>,
        before: Option<String>,
        after: Option<String>,
        filter: Option<NoteFilterInput>,
        order_by: Option<Vec<serde_json::Value>>,
    ) -> async_graphql::Result<NoteConnection> {
        let auth = require_auth(ctx)?;
        let order = json_order_terms(order_by.as_deref().unwrap_or_default());
        let (mut all, _) = state(ctx)
            .db
            .list_notes(auth.workspace_id, 10_000, 0)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let filter = filter.unwrap_or_default();
        all.retain(|note| note_matches(note, &filter));
        sort_notes(&mut all, &order);

        let total = all.len() as i64;
        let limit = first
            .or(last)
            .unwrap_or(20)
            .clamp(1, state(ctx).config.max_page_size) as usize;
        let offset = offset.unwrap_or(0).max(0) as usize;
        let starting_after = after
            .as_deref()
            .map(crate::rest::decode_cursor)
            .transpose()
            .map_err(gql_err)?;
        let ending_before = before
            .as_deref()
            .map(crate::rest::decode_cursor)
            .transpose()
            .map_err(gql_err)?;
        if let Some(after) = starting_after {
            if let Some(index) = all.iter().position(|note| note.id == after) {
                all.drain(..=index);
            }
        }
        if let Some(before) = ending_before {
            if let Some(index) = all.iter().position(|note| note.id == before) {
                all.truncate(index);
            }
        }

        let start = offset.min(all.len());
        let end = (start + limit).min(all.len());
        let has_next_page = end < all.len();
        let has_previous_page = start > 0 || starting_after.is_some();
        let edges = all[start..end]
            .iter()
            .cloned()
            .map(|note| NoteEdge {
                cursor: Cursor(crate::rest::encode_cursor(note.id)),
                node: note.into(),
            })
            .collect();

        Ok(NoteConnection {
            total_count: Some(total),
            edges,
            page_info: PageInfo {
                has_next_page,
                has_previous_page,
                start_cursor: all
                    .get(start)
                    .map(|note| crate::rest::encode_cursor(note.id)),
                end_cursor: all
                    .get(end.saturating_sub(1))
                    .map(|note| crate::rest::encode_cursor(note.id)),
            },
        })
    }

    async fn note(
        &self,
        ctx: &Context<'_>,
        filter: NoteFilterInput,
    ) -> async_graphql::Result<Option<NoteGql>> {
        let auth = require_auth(ctx)?;
        let (notes, _) = state(ctx)
            .db
            .list_notes(auth.workspace_id, 10_000, 0)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(notes
            .into_iter()
            .find(|note| note_matches(note, &filter))
            .map(Into::into))
    }

    async fn person(
        &self,
        ctx: &Context<'_>,
        filter: PersonFilterInput,
    ) -> async_graphql::Result<Option<Person>> {
        let auth = require_auth(ctx)?;
        let filter = filter.to_expr().ok_or_else(|| {
            gql_err(AppError::BadRequest(
                "person requires a non-empty filter".to_string(),
            ))
        })?;
        let page = state(ctx)
            .db
            .list_persons_page(auth.workspace_id, 1, 0, None, None, &[], Some(&filter))
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(page.persons.into_iter().next())
    }
}

#[derive(Debug, InputObject)]
pub struct CreateOneObjectInput {
    pub object: CreateObjectMetadataInput,
}

#[derive(Debug, InputObject)]
pub struct UpdateOneObjectInput {
    pub id: Uuid,
    pub update: UpdateObjectMetadataInput,
}

#[derive(Debug, InputObject)]
pub struct DeleteOneObjectInput {
    pub id: Uuid,
}

#[derive(Debug, InputObject)]
pub struct CreateOneFieldMetadataInput {
    pub field: CreateFieldMetadataInput,
}

#[derive(Debug, InputObject)]
pub struct UpdateOneFieldMetadataInput {
    pub id: Uuid,
    pub update: UpdateFieldMetadataInput,
}

#[derive(Debug, InputObject)]
pub struct DeleteOneFieldInput {
    pub id: Uuid,
}

#[derive(Debug, InputObject)]
#[graphql(name = "UpdateViewFieldInput")]
pub struct UpdateViewFieldMutationInput {
    pub id: Uuid,
    pub update: UpdateViewFieldInput,
}

#[derive(Debug, InputObject)]
pub struct DeleteViewFieldInput {
    pub id: Uuid,
}

#[derive(Debug, InputObject)]
pub struct DestroyViewFieldInput {
    pub id: Uuid,
}

#[derive(Debug, InputObject)]
#[graphql(name = "UpdateViewFilterInput")]
pub struct UpdateViewFilterMutationInput {
    pub id: Uuid,
    pub update: UpdateViewFilterInput,
}

#[derive(Debug, InputObject)]
pub struct DeleteViewFilterInput {
    pub id: Uuid,
}

#[derive(Debug, InputObject)]
pub struct DestroyViewFilterInput {
    pub id: Uuid,
}

#[derive(Debug, InputObject)]
#[graphql(name = "UpdateViewSortInput")]
pub struct UpdateViewSortMutationInput {
    pub id: Uuid,
    pub update: UpdateViewSortInput,
}

#[derive(Debug, InputObject)]
pub struct DeleteViewSortInput {
    pub id: Uuid,
}

#[derive(Debug, InputObject)]
pub struct DestroyViewSortInput {
    pub id: Uuid,
}

#[derive(Debug, InputObject)]
#[graphql(name = "UpdateViewGroupInput")]
pub struct UpdateViewGroupMutationInput {
    pub id: Uuid,
    pub update: UpdateViewGroupInput,
}

#[derive(Debug, InputObject)]
pub struct DeleteViewGroupInput {
    pub id: Uuid,
}

#[derive(Debug, InputObject)]
pub struct DestroyViewGroupInput {
    pub id: Uuid,
}

#[derive(Debug, InputObject)]
#[graphql(name = "UpdateViewFieldGroupInput")]
pub struct UpdateViewFieldGroupMutationInput {
    pub id: Uuid,
    pub update: UpdateViewFieldGroupInput,
}

#[derive(Debug, InputObject)]
pub struct DeleteViewFieldGroupInput {
    pub id: Uuid,
}

#[derive(Debug, InputObject)]
pub struct DestroyViewFieldGroupInput {
    pub id: Uuid,
}

#[Object]
impl MutationRoot {
    async fn create_file_upload(
        &self,
        ctx: &Context<'_>,
        filename: String,
        _size: f64,
        file_folder: FileFolder,
        field_metadata_id: Option<String>,
        _field_metadata_universal_identifier: Option<String>,
    ) -> async_graphql::Result<FileUploadTarget> {
        let auth = require_auth(ctx)?;
        let field_metadata_id = field_metadata_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|_| {
                gql_err(AppError::BadRequest(
                    "Invalid field metadata id".to_string(),
                ))
            })?;
        let content_type = content_type_for(&filename);
        let file = state(ctx)
            .db
            .create_file(
                auth.workspace_id,
                &filename,
                0,
                content_type,
                file_folder,
                field_metadata_id,
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(FileUploadTarget {
            file_id: file.id,
            upload_url: format!("/files/upload/{}", file.id),
            content_type: content_type.to_string(),
            expires_at: Utc::now() + chrono::Duration::hours(1),
        })
    }

    async fn complete_file_upload(
        &self,
        ctx: &Context<'_>,
        file_id: String,
    ) -> async_graphql::Result<FileWithSignedUrl> {
        let auth = require_auth(ctx)?;
        let file_id = Uuid::parse_str(&file_id)
            .map_err(|_| gql_err(AppError::BadRequest("Invalid file id".to_string())))?;
        let file = state(ctx)
            .db
            .complete_file(auth.workspace_id, file_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("File not found".to_string())))?;
        let url = file.url();
        Ok(FileWithSignedUrl {
            id: file.id,
            path: file.path,
            size: file.size as f64,
            created_at: file.created_at,
            url,
        })
    }

    async fn create_one_index(
        &self,
        ctx: &Context<'_>,
        input: CreateOneIndexInput,
    ) -> async_graphql::Result<squid_core::IndexMetadata> {
        let auth = require_auth(ctx)?;
        if input.index.fields.is_empty() {
            return Err(gql_err(AppError::BadRequest(
                "At least one field is required".to_string(),
            )));
        }
        state(ctx)
            .db
            .create_index(auth.workspace_id, &input.index)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn delete_one_index(
        &self,
        ctx: &Context<'_>,
        input: DeleteOneIndexInput,
    ) -> async_graphql::Result<squid_core::IndexMetadata> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .delete_index(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Index not found".to_string())))
    }

    async fn create_view(
        &self,
        ctx: &Context<'_>,
        input: CreateViewInput,
    ) -> async_graphql::Result<View> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        state
            .db
            .get_object(auth.workspace_id, input.object_metadata_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Object not found".to_string())))?;
        state
            .db
            .create_view(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_view(
        &self,
        ctx: &Context<'_>,
        id: String,
        input: UpdateViewInput,
    ) -> async_graphql::Result<View> {
        let auth = require_auth(ctx)?;
        let id = Uuid::parse_str(&id)
            .map_err(|_| gql_err(AppError::BadRequest("Invalid view id".to_string())))?;
        state(ctx)
            .db
            .update_view(auth.workspace_id, id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View not found".to_string())))
    }

    async fn delete_view(&self, ctx: &Context<'_>, id: String) -> async_graphql::Result<bool> {
        let auth = require_auth(ctx)?;
        let id = Uuid::parse_str(&id)
            .map_err(|_| gql_err(AppError::BadRequest("Invalid view id".to_string())))?;
        state(ctx)
            .db
            .delete_view(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn destroy_view(&self, ctx: &Context<'_>, id: String) -> async_graphql::Result<bool> {
        let auth = require_auth(ctx)?;
        let id = Uuid::parse_str(&id)
            .map_err(|_| gql_err(AppError::BadRequest("Invalid view id".to_string())))?;
        state(ctx)
            .db
            .destroy_view(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn create_view_field(
        &self,
        ctx: &Context<'_>,
        input: CreateViewFieldInput,
    ) -> async_graphql::Result<ViewField> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .create_view_field(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_view_field(
        &self,
        ctx: &Context<'_>,
        input: UpdateViewFieldMutationInput,
    ) -> async_graphql::Result<ViewField> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .update_view_field(auth.workspace_id, input.id, &input.update)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View field not found".to_string())))
    }

    async fn delete_view_field(
        &self,
        ctx: &Context<'_>,
        input: DeleteViewFieldInput,
    ) -> async_graphql::Result<ViewField> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .delete_view_field(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View field not found".to_string())))
    }

    async fn destroy_view_field(
        &self,
        ctx: &Context<'_>,
        input: DestroyViewFieldInput,
    ) -> async_graphql::Result<ViewField> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .destroy_view_field(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View field not found".to_string())))
    }

    async fn create_view_filter(
        &self,
        ctx: &Context<'_>,
        input: CreateViewFilterInput,
    ) -> async_graphql::Result<ViewFilter> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .create_view_filter(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_view_filter(
        &self,
        ctx: &Context<'_>,
        input: UpdateViewFilterMutationInput,
    ) -> async_graphql::Result<ViewFilter> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .update_view_filter(auth.workspace_id, input.id, &input.update)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View filter not found".to_string())))
    }

    async fn delete_view_filter(
        &self,
        ctx: &Context<'_>,
        input: DeleteViewFilterInput,
    ) -> async_graphql::Result<ViewFilter> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .delete_view_filter(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View filter not found".to_string())))
    }

    async fn destroy_view_filter(
        &self,
        ctx: &Context<'_>,
        input: DestroyViewFilterInput,
    ) -> async_graphql::Result<ViewFilter> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .destroy_view_filter(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View filter not found".to_string())))
    }

    async fn create_view_sort(
        &self,
        ctx: &Context<'_>,
        input: CreateViewSortInput,
    ) -> async_graphql::Result<ViewSort> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .create_view_sort(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_view_sort(
        &self,
        ctx: &Context<'_>,
        input: UpdateViewSortMutationInput,
    ) -> async_graphql::Result<ViewSort> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .update_view_sort(auth.workspace_id, input.id, &input.update)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View sort not found".to_string())))
    }

    async fn delete_view_sort(
        &self,
        ctx: &Context<'_>,
        input: DeleteViewSortInput,
    ) -> async_graphql::Result<bool> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .delete_view_sort(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn destroy_view_sort(
        &self,
        ctx: &Context<'_>,
        input: DestroyViewSortInput,
    ) -> async_graphql::Result<bool> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .destroy_view_sort(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn create_view_group(
        &self,
        ctx: &Context<'_>,
        input: CreateViewGroupInput,
    ) -> async_graphql::Result<ViewGroup> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .create_view_group(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_view_group(
        &self,
        ctx: &Context<'_>,
        input: UpdateViewGroupMutationInput,
    ) -> async_graphql::Result<ViewGroup> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .update_view_group(auth.workspace_id, input.id, &input.update)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View group not found".to_string())))
    }

    async fn delete_view_group(
        &self,
        ctx: &Context<'_>,
        input: DeleteViewGroupInput,
    ) -> async_graphql::Result<ViewGroup> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .delete_view_group(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View group not found".to_string())))
    }

    async fn destroy_view_group(
        &self,
        ctx: &Context<'_>,
        input: DestroyViewGroupInput,
    ) -> async_graphql::Result<ViewGroup> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .destroy_view_group(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View group not found".to_string())))
    }

    async fn create_many_view_groups(
        &self,
        ctx: &Context<'_>,
        inputs: Vec<CreateViewGroupInput>,
    ) -> async_graphql::Result<Vec<ViewGroup>> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .create_many_view_groups(auth.workspace_id, &inputs)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_many_view_groups(
        &self,
        ctx: &Context<'_>,
        inputs: Vec<UpdateViewGroupMutationInput>,
    ) -> async_graphql::Result<Vec<ViewGroup>> {
        let auth = require_auth(ctx)?;
        let updates: Vec<(Uuid, UpdateViewGroupInput)> = inputs
            .into_iter()
            .map(|input| (input.id, input.update))
            .collect();
        state(ctx)
            .db
            .update_many_view_groups(auth.workspace_id, &updates)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn create_many_view_fields(
        &self,
        ctx: &Context<'_>,
        inputs: Vec<CreateViewFieldInput>,
    ) -> async_graphql::Result<Vec<ViewField>> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .create_many_view_fields(auth.workspace_id, &inputs)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn create_view_field_group(
        &self,
        ctx: &Context<'_>,
        input: CreateViewFieldGroupInput,
    ) -> async_graphql::Result<ViewFieldGroup> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .create_view_field_group(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn create_many_view_field_groups(
        &self,
        ctx: &Context<'_>,
        inputs: Vec<CreateViewFieldGroupInput>,
    ) -> async_graphql::Result<Vec<ViewFieldGroup>> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .create_many_view_field_groups(auth.workspace_id, &inputs)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_view_field_group(
        &self,
        ctx: &Context<'_>,
        input: UpdateViewFieldGroupMutationInput,
    ) -> async_graphql::Result<ViewFieldGroup> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .update_view_field_group(auth.workspace_id, input.id, &input.update)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View field group not found".to_string())))
    }

    async fn delete_view_field_group(
        &self,
        ctx: &Context<'_>,
        input: DeleteViewFieldGroupInput,
    ) -> async_graphql::Result<ViewFieldGroup> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .delete_view_field_group(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View field group not found".to_string())))
    }

    async fn destroy_view_field_group(
        &self,
        ctx: &Context<'_>,
        input: DestroyViewFieldGroupInput,
    ) -> async_graphql::Result<ViewFieldGroup> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .destroy_view_field_group(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("View field group not found".to_string())))
    }

    async fn create_view_filter_group(
        &self,
        ctx: &Context<'_>,
        input: CreateViewFilterGroupInput,
    ) -> async_graphql::Result<ViewFilterGroup> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .create_view_filter_group(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_view_filter_group(
        &self,
        ctx: &Context<'_>,
        id: String,
        input: UpdateViewFilterGroupInput,
    ) -> async_graphql::Result<ViewFilterGroup> {
        let auth = require_auth(ctx)?;
        let id = Uuid::parse_str(&id).map_err(|_| {
            gql_err(AppError::BadRequest(
                "Invalid view filter group id".to_string(),
            ))
        })?;
        state(ctx)
            .db
            .update_view_filter_group(auth.workspace_id, id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| {
                gql_err(AppError::NotFound(
                    "View filter group not found".to_string(),
                ))
            })
    }

    async fn delete_view_filter_group(
        &self,
        ctx: &Context<'_>,
        id: String,
    ) -> async_graphql::Result<bool> {
        let auth = require_auth(ctx)?;
        let id = Uuid::parse_str(&id).map_err(|_| {
            gql_err(AppError::BadRequest(
                "Invalid view filter group id".to_string(),
            ))
        })?;
        state(ctx)
            .db
            .delete_view_filter_group(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn destroy_view_filter_group(
        &self,
        ctx: &Context<'_>,
        id: String,
    ) -> async_graphql::Result<bool> {
        let auth = require_auth(ctx)?;
        let id = Uuid::parse_str(&id).map_err(|_| {
            gql_err(AppError::BadRequest(
                "Invalid view filter group id".to_string(),
            ))
        })?;
        state(ctx)
            .db
            .destroy_view_filter_group(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn create_one_object(
        &self,
        ctx: &Context<'_>,
        input: CreateOneObjectInput,
    ) -> async_graphql::Result<ObjectMetadata> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let object = input.object;
        if object.name_singular.trim().is_empty() || object.name_plural.trim().is_empty() {
            return Err(gql_err(AppError::BadRequest(
                "Object names cannot be empty".to_string(),
            )));
        }
        let existing = state
            .db
            .list_objects(auth.workspace_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        if existing.iter().any(|candidate| {
            candidate.name_singular == object.name_singular
                || candidate.name_plural == object.name_plural
        }) {
            return Err(gql_err(AppError::BadRequest(
                "An object with this name already exists".to_string(),
            )));
        }
        state
            .db
            .create_metadata_object(auth.workspace_id, &object)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_one_object(
        &self,
        ctx: &Context<'_>,
        input: UpdateOneObjectInput,
    ) -> async_graphql::Result<ObjectMetadata> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .update_metadata_object(auth.workspace_id, input.id, &input.update)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Object not found".to_string())))
    }

    async fn delete_one_object(
        &self,
        ctx: &Context<'_>,
        input: DeleteOneObjectInput,
    ) -> async_graphql::Result<ObjectMetadata> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let object = state
            .db
            .get_object(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Object not found".to_string())))?;
        if object.is_system {
            return Err(gql_err(AppError::BadRequest(
                "Standard objects cannot be deleted".to_string(),
            )));
        }
        state
            .db
            .delete_metadata_object(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(object)
    }

    async fn create_one_field(
        &self,
        ctx: &Context<'_>,
        input: CreateOneFieldMetadataInput,
    ) -> async_graphql::Result<FieldMetadata> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let field = input.field;
        if field.name.trim().is_empty() {
            return Err(gql_err(AppError::BadRequest(
                "Field name cannot be empty".to_string(),
            )));
        }
        if matches!(
            field.field_type,
            squid_core::FieldMetadataType::Relation | squid_core::FieldMetadataType::MorphRelation
        ) {
            return Err(gql_err(AppError::BadRequest(
                "Relation field creation is not supported yet".to_string(),
            )));
        }
        state
            .db
            .get_object(auth.workspace_id, field.object_metadata_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Object not found".to_string())))?;
        let existing = state
            .db
            .list_fields(auth.workspace_id, Some(field.object_metadata_id))
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        if existing
            .iter()
            .any(|candidate| candidate.name == field.name)
        {
            return Err(gql_err(AppError::BadRequest(
                "A field with this name already exists".to_string(),
            )));
        }
        state
            .db
            .create_metadata_field(auth.workspace_id, &field)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_one_field(
        &self,
        ctx: &Context<'_>,
        input: UpdateOneFieldMetadataInput,
    ) -> async_graphql::Result<FieldMetadata> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .update_metadata_field(auth.workspace_id, input.id, &input.update)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Field not found".to_string())))
    }

    async fn delete_one_field(
        &self,
        ctx: &Context<'_>,
        input: DeleteOneFieldInput,
    ) -> async_graphql::Result<FieldMetadata> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let field = state
            .db
            .get_field(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Field not found".to_string())))?;
        if field.is_system {
            return Err(gql_err(AppError::BadRequest(
                "System fields cannot be deleted".to_string(),
            )));
        }
        state
            .db
            .delete_metadata_field(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(field)
    }

    #[allow(clippy::too_many_arguments)]
    async fn sign_up(
        &self,
        ctx: &Context<'_>,
        email: String,
        password: String,
        workspace_name: Option<String>,
    ) -> async_graphql::Result<AuthPayload> {
        let state = state(ctx);
        let email = normalize_email(&email).map_err(gql_err)?;
        state
            .auth_limiter
            .check(&format!("signup:{email}"))
            .map_err(gql_err)?;
        let password_hash = hash_password(&password).map_err(gql_err)?;
        let display_name = workspace_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| format!("{}'s workspace", email.split('@').next().unwrap_or("My")));
        let subdomain = unique_subdomain(&display_name);

        let (user, workspace) = state
            .db
            .bootstrap_user_workspace(&email, &password_hash, &display_name, &subdomain)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?;

        if let Err(error) = state.db.create_workspace_schema(workspace.id).await {
            let _ = state.db.delete_workspace(workspace.id).await;
            let _ = state.db.delete_user(user.id).await;
            return Err(gql_err(AppError::Internal(error)));
        }

        let (tokens, raw_token, expires_at) = state
            .issue_session(user.id, workspace.id)
            .await
            .map_err(gql_err)?;
        set_cookie(ctx, raw_token, expires_at);

        let user = build_current_user(state, &user, workspace.id, Role::Owner).await?;
        let workspace = user
            .current_workspace
            .clone()
            .expect("current workspace is built with the user");
        Ok(AuthPayload {
            tokens,
            user,
            workspace,
        })
    }

    async fn sign_in(
        &self,
        ctx: &Context<'_>,
        email: String,
        password: String,
        _captcha_token: Option<String>,
    ) -> async_graphql::Result<AuthPayload> {
        sign_in_impl(ctx, &email, &password).await
    }

    async fn get_login_token_from_credentials(
        &self,
        ctx: &Context<'_>,
        email: String,
        password: String,
        _captcha_token: Option<String>,
        _origin: String,
    ) -> async_graphql::Result<LoginTokenPayload> {
        let state = state(ctx);
        let email = normalize_email(&email).map_err(gql_err)?;
        let (user, password_hash) = state
            .db
            .find_user_by_email(&email)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
            .ok_or_else(|| {
                gql_err(AppError::Unauthorized(
                    "Invalid email or password".to_string(),
                ))
            })?;

        verify_password(&password, &password_hash).map_err(gql_err)?;

        let login_token = state.tokens.issue_login(user.id).map_err(gql_err)?;
        Ok(LoginTokenPayload { login_token })
    }

    async fn get_auth_tokens_from_login_token(
        &self,
        ctx: &Context<'_>,
        login_token: String,
        _origin: String,
    ) -> async_graphql::Result<AuthPayload> {
        let state = state(ctx);
        let verified = state
            .tokens
            .verify(&login_token, squid_auth::TokenKind::Login)
            .map_err(gql_err)?;
        let user = state
            .db
            .find_user_by_id(verified.user_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
            .ok_or_else(|| {
                gql_err(AppError::Unauthorized(
                    "Invalid email or password".to_string(),
                ))
            })?;

        let (workspace, role) = state
            .db
            .find_workspace_for_user(user.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
            .ok_or_else(|| {
                gql_err(AppError::Forbidden(
                    "No workspace is available for this user".to_string(),
                ))
            })?;

        let (tokens, raw_token, expires_at) = state
            .issue_session(user.id, workspace.id)
            .await
            .map_err(gql_err)?;
        set_cookie(ctx, raw_token, expires_at);

        let user = build_current_user(state, &user, workspace.id, role).await?;
        let workspace = user
            .current_workspace
            .clone()
            .expect("current workspace is built with the user");
        Ok(AuthPayload {
            tokens,
            user,
            workspace,
        })
    }

    async fn create_api_key(
        &self,
        ctx: &Context<'_>,
        input: CreateApiKeyInput,
    ) -> async_graphql::Result<ApiKey> {
        let auth = require_auth(ctx)?;
        let Some(user_id) = auth.user_id else {
            return Err(gql_err(AppError::Forbidden(
                "API keys cannot create API keys".to_string(),
            )));
        };
        if input.name.trim().is_empty() {
            return Err(gql_err(AppError::BadRequest(
                "API key name is required".to_string(),
            )));
        }
        let expires_at = parse_expiry(&input.expires_at).map_err(gql_err)?;

        state(ctx)
            .db
            .create_api_key(
                auth.workspace_id,
                Some(user_id),
                input.name.trim(),
                auth.role,
                expires_at,
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))
    }

    async fn generate_api_key_token(
        &self,
        ctx: &Context<'_>,
        api_key_id: uuid::Uuid,
        expires_at: String,
    ) -> async_graphql::Result<ApiKeyToken> {
        let auth = require_auth(ctx)?;
        let expires_at = parse_expiry(&expires_at).map_err(gql_err)?;
        let state = state(ctx);

        state
            .db
            .get_api_key(auth.workspace_id, api_key_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;

        let token = format!(
            "api_{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        state
            .db
            .set_api_key_token(
                auth.workspace_id,
                api_key_id,
                &hash_token(&token),
                expires_at,
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;

        Ok(ApiKeyToken { token })
    }

    async fn revoke_api_key(
        &self,
        ctx: &Context<'_>,
        input: RevokeApiKeyInput,
    ) -> async_graphql::Result<Option<ApiKey>> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .revoke_api_key(auth.workspace_id, input.id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))
    }

    async fn create_people(
        &self,
        ctx: &Context<'_>,
        data: Vec<PersonCreateInputGql>,
        #[graphql(default)] _upsert: bool,
        _share_with: Option<Vec<ShareWithInput>>,
    ) -> async_graphql::Result<Vec<Person>> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let mut people = Vec::with_capacity(data.len());
        for data in data {
            let input: CreatePersonInput = data.into();
            if input.name.first_name.trim().is_empty() && input.name.last_name.trim().is_empty() {
                return Err(gql_err(AppError::BadRequest(
                    "A first or last name is required".to_string(),
                )));
            }
            people.push(
                state
                    .db
                    .create_person(auth.workspace_id, &input)
                    .await
                    .map_err(|e| gql_err(AppError::Internal(e)))?,
            );
        }
        Ok(people)
    }

    async fn update_people(
        &self,
        ctx: &Context<'_>,
        data: PersonUpdateInputGql,
        filter: PersonFilterInput,
    ) -> async_graphql::Result<Vec<Person>> {
        let auth = require_auth(ctx)?;
        let filter = filter.to_expr().ok_or_else(|| {
            gql_err(AppError::BadRequest(
                "updatePeople requires a non-empty filter".to_string(),
            ))
        })?;
        let state = state(ctx);
        let ids = state
            .db
            .live_person_ids(auth.workspace_id, Some(&filter))
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let input: UpdatePersonInput = data.into();
        let mut people = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(person) = state
                .db
                .update_person(auth.workspace_id, id, &input)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?
            {
                people.push(person);
            }
        }
        Ok(people)
    }

    async fn delete_people(
        &self,
        ctx: &Context<'_>,
        filter: PersonFilterInput,
    ) -> async_graphql::Result<Vec<Person>> {
        let auth = require_auth(ctx)?;
        let filter = filter.to_expr().ok_or_else(|| {
            gql_err(AppError::BadRequest(
                "deletePeople requires a non-empty filter".to_string(),
            ))
        })?;
        let state = state(ctx);
        let ids = state
            .db
            .live_person_ids(auth.workspace_id, Some(&filter))
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let mut people = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(person) = state
                .db
                .get_person(auth.workspace_id, id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?
            {
                state
                    .db
                    .soft_delete_person(auth.workspace_id, id)
                    .await
                    .map_err(|e| gql_err(AppError::Internal(e)))?;
                people.push(person);
            }
        }
        Ok(people)
    }

    async fn destroy_people(
        &self,
        ctx: &Context<'_>,
        filter: PersonFilterInput,
    ) -> async_graphql::Result<Vec<Person>> {
        let auth = require_auth(ctx)?;
        let filter = filter.to_expr().ok_or_else(|| {
            gql_err(AppError::BadRequest(
                "destroyPeople requires a non-empty filter".to_string(),
            ))
        })?;
        let state = state(ctx);
        let ids = state
            .db
            .live_person_ids(auth.workspace_id, Some(&filter))
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let mut people = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(person) = state
                .db
                .get_person(auth.workspace_id, id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?
            {
                state
                    .db
                    .destroy_person(auth.workspace_id, id)
                    .await
                    .map_err(|e| gql_err(AppError::Internal(e)))?;
                people.push(person);
            }
        }
        Ok(people)
    }

    async fn restore_people(
        &self,
        ctx: &Context<'_>,
        filter: PersonFilterInput,
    ) -> async_graphql::Result<Vec<Person>> {
        let auth = require_auth(ctx)?;
        let filter = filter.to_expr().ok_or_else(|| {
            gql_err(AppError::BadRequest(
                "restorePeople requires a non-empty filter".to_string(),
            ))
        })?;
        let state = state(ctx);
        let ids = state
            .db
            .deleted_person_ids(auth.workspace_id, Some(&filter))
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let mut people = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(person) = state
                .db
                .restore_person(auth.workspace_id, id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?
            {
                people.push(person);
            }
        }
        Ok(people)
    }

    async fn merge_people(
        &self,
        ctx: &Context<'_>,
        ids: Vec<Uuid>,
        conflict_priority_index: i32,
        dry_run: Option<bool>,
    ) -> async_graphql::Result<Person> {
        let auth = require_auth(ctx)?;
        let Ok(conflict_priority_index) = usize::try_from(conflict_priority_index) else {
            return Err(gql_err(AppError::BadRequest(
                "conflictPriorityIndex is out of range".to_string(),
            )));
        };
        if ids.is_empty() || conflict_priority_index >= ids.len() {
            return Err(gql_err(AppError::BadRequest(
                "conflictPriorityIndex is out of range".to_string(),
            )));
        }
        state(ctx)
            .db
            .merge_people(
                auth.workspace_id,
                &ids,
                conflict_priority_index,
                dry_run.unwrap_or(false),
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Person not found".to_string())))
    }

    async fn create_person(
        &self,
        ctx: &Context<'_>,
        data: PersonCreateInputGql,
        #[graphql(default)] _upsert: bool,
        _share_with: Option<Vec<ShareWithInput>>,
    ) -> async_graphql::Result<Person> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let input: CreatePersonInput = data.into();

        if input.name.first_name.trim().is_empty() && input.name.last_name.trim().is_empty() {
            return Err(gql_err(AppError::BadRequest(
                "A first or last name is required".to_string(),
            )));
        }

        state
            .db
            .create_person(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_person(
        &self,
        ctx: &Context<'_>,
        id: uuid::Uuid,
        data: PersonUpdateInputGql,
    ) -> async_graphql::Result<Person> {
        let auth = require_auth(ctx)?;
        let input: UpdatePersonInput = data.into();
        state(ctx)
            .db
            .update_person(auth.workspace_id, id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn destroy_person(
        &self,
        ctx: &Context<'_>,
        id: uuid::Uuid,
    ) -> async_graphql::Result<Person> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let person = state
            .db
            .get_person(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;

        state
            .db
            .destroy_person(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;

        Ok(person)
    }

    async fn restore_person(
        &self,
        ctx: &Context<'_>,
        id: uuid::Uuid,
    ) -> async_graphql::Result<Person> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .restore_person(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn delete_person(
        &self,
        ctx: &Context<'_>,
        id: uuid::Uuid,
    ) -> async_graphql::Result<Person> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let person = state
            .db
            .get_person(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;

        state
            .db
            .soft_delete_person(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;

        Ok(person)
    }

    async fn create_company(
        &self,
        ctx: &Context<'_>,
        data: CreateCompanyInputGql,
        #[graphql(default)] _upsert: bool,
        _share_with: Option<Vec<ShareWithInput>>,
    ) -> async_graphql::Result<Company> {
        let auth = require_auth(ctx)?;
        let input: CreateCompanyInput = data.into();
        state(ctx)
            .db
            .create_company(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_company(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        data: UpdateCompanyInputGql,
    ) -> async_graphql::Result<Company> {
        let auth = require_auth(ctx)?;
        let input: UpdateCompanyInput = data.into();
        state(ctx)
            .db
            .update_company(auth.workspace_id, id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn delete_company(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<Company> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let company = state
            .db
            .get_company(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;
        state
            .db
            .soft_delete_company(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(company)
    }

    async fn destroy_company(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<Company> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let company = state
            .db
            .get_company(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;
        state
            .db
            .destroy_company(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(company)
    }

    async fn restore_company(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<Company> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .restore_company(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn create_opportunity(
        &self,
        ctx: &Context<'_>,
        data: CreateOpportunityInputGql,
        #[graphql(default)] _upsert: bool,
        _share_with: Option<Vec<ShareWithInput>>,
    ) -> async_graphql::Result<Opportunity> {
        let auth = require_auth(ctx)?;
        let input: CreateOpportunityInput = data.into();
        state(ctx)
            .db
            .create_opportunity(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }

    async fn update_opportunity(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        data: UpdateOpportunityInputGql,
    ) -> async_graphql::Result<Opportunity> {
        let auth = require_auth(ctx)?;
        let input: UpdateOpportunityInput = data.into();
        state(ctx)
            .db
            .update_opportunity(auth.workspace_id, id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn delete_opportunity(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
    ) -> async_graphql::Result<Opportunity> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let opportunity = state
            .db
            .get_opportunity(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;
        state
            .db
            .soft_delete_opportunity(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(opportunity)
    }

    async fn destroy_opportunity(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
    ) -> async_graphql::Result<Opportunity> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let opportunity = state
            .db
            .get_opportunity(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;
        state
            .db
            .destroy_opportunity(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(opportunity)
    }

    async fn restore_opportunity(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
    ) -> async_graphql::Result<Opportunity> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .restore_opportunity(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn create_task(
        &self,
        ctx: &Context<'_>,
        data: CreateTaskInputGql,
        #[graphql(default)] _upsert: bool,
        _share_with: Option<Vec<ShareWithInput>>,
    ) -> async_graphql::Result<TaskGql> {
        let auth = require_auth(ctx)?;
        let input: CreateTaskInput = data.into();
        state(ctx)
            .db
            .create_task(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
            .map(Into::into)
    }

    async fn update_task(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        data: UpdateTaskInputGql,
    ) -> async_graphql::Result<TaskGql> {
        let auth = require_auth(ctx)?;
        let input: UpdateTaskInput = data.into();
        state(ctx)
            .db
            .update_task(auth.workspace_id, id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .map(Into::into)
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn delete_task(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<TaskGql> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let task = state
            .db
            .get_task(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;
        state
            .db
            .soft_delete_task(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(task.into())
    }

    async fn destroy_task(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<TaskGql> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let task = state
            .db
            .get_task(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;
        state
            .db
            .destroy_task(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(task.into())
    }

    async fn restore_task(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<TaskGql> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .restore_task(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .map(Into::into)
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn create_note(
        &self,
        ctx: &Context<'_>,
        data: CreateNoteInputGql,
        #[graphql(default)] _upsert: bool,
        _share_with: Option<Vec<ShareWithInput>>,
    ) -> async_graphql::Result<NoteGql> {
        let auth = require_auth(ctx)?;
        let input: CreateNoteInput = data.into();
        state(ctx)
            .db
            .create_note(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
            .map(Into::into)
    }

    async fn update_note(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        data: UpdateNoteInputGql,
    ) -> async_graphql::Result<NoteGql> {
        let auth = require_auth(ctx)?;
        let input: UpdateNoteInput = data.into();
        state(ctx)
            .db
            .update_note(auth.workspace_id, id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .map(Into::into)
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn delete_note(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<NoteGql> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let note = state
            .db
            .get_note(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;
        state
            .db
            .soft_delete_note(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(note.into())
    }

    async fn destroy_note(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<NoteGql> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let note = state
            .db
            .get_note(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;
        state
            .db
            .destroy_note(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(note.into())
    }

    async fn restore_note(&self, ctx: &Context<'_>, id: Uuid) -> async_graphql::Result<NoteGql> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .restore_note(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .map(Into::into)
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn create_task_target(
        &self,
        ctx: &Context<'_>,
        data: CreateTaskTargetInputGql,
    ) -> async_graphql::Result<TaskTargetGql> {
        let auth = require_auth(ctx)?;
        let input: CreateTaskTargetInput = data.into();
        if input.target_company_id.is_none()
            && input.target_opportunity_id.is_none()
            && input.target_person_id.is_none()
        {
            return Err(gql_err(AppError::BadRequest(
                "A task target must reference a company, opportunity or person".to_string(),
            )));
        }
        state(ctx)
            .db
            .create_task_target(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
            .map(Into::into)
    }

    async fn delete_task_target(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
    ) -> async_graphql::Result<TaskTargetGql> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let target = state
            .db
            .get_task_target(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;
        state
            .db
            .delete_task_target(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(target.into())
    }

    async fn create_note_target(
        &self,
        ctx: &Context<'_>,
        data: CreateNoteTargetInputGql,
    ) -> async_graphql::Result<NoteTargetGql> {
        let auth = require_auth(ctx)?;
        let input: CreateNoteTargetInput = data.into();
        if input.target_company_id.is_none()
            && input.target_opportunity_id.is_none()
            && input.target_person_id.is_none()
        {
            return Err(gql_err(AppError::BadRequest(
                "A note target must reference a company, opportunity or person".to_string(),
            )));
        }
        state(ctx)
            .db
            .create_note_target(auth.workspace_id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
            .map(Into::into)
    }

    async fn delete_note_target(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
    ) -> async_graphql::Result<NoteTargetGql> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let target = state
            .db
            .get_note_target(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;
        state
            .db
            .delete_note_target(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(target.into())
    }

    async fn update_task_target(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        data: UpdateTaskTargetInputGql,
    ) -> async_graphql::Result<TaskTargetGql> {
        let auth = require_auth(ctx)?;
        let input: squid_core::UpdateTaskTargetInput = data.into();
        state(ctx)
            .db
            .update_task_target(auth.workspace_id, id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .map(Into::into)
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn destroy_task_target(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
    ) -> async_graphql::Result<TaskTargetGql> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let target = state
            .db
            .get_task_target(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;
        state
            .db
            .destroy_task_target(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(target.into())
    }

    async fn restore_task_target(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
    ) -> async_graphql::Result<TaskTargetGql> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .restore_task_target(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .map(Into::into)
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn create_task_targets(
        &self,
        ctx: &Context<'_>,
        data: Vec<CreateTaskTargetInputGql>,
        #[graphql(default)] _upsert: bool,
        _share_with: Option<Vec<ShareWithInput>>,
    ) -> async_graphql::Result<Vec<TaskTargetGql>> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let mut created = Vec::with_capacity(data.len());
        for item in data {
            let input: CreateTaskTargetInput = item.into();
            if input.target_company_id.is_none()
                && input.target_opportunity_id.is_none()
                && input.target_person_id.is_none()
            {
                return Err(gql_err(AppError::BadRequest(
                    "A task target must reference a company, opportunity or person".to_string(),
                )));
            }
            created.push(
                state
                    .db
                    .create_task_target(auth.workspace_id, &input)
                    .await
                    .map_err(|e| gql_err(AppError::Internal(e)))?
                    .into(),
            );
        }
        Ok(created)
    }

    async fn update_task_targets(
        &self,
        ctx: &Context<'_>,
        data: UpdateTaskTargetInputGql,
        filter: TaskTargetFilterInput,
    ) -> async_graphql::Result<Vec<TaskTargetGql>> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let input: squid_core::UpdateTaskTargetInput = data.into();
        let targets = state
            .db
            .list_task_targets(
                auth.workspace_id,
                filter.task_id.as_ref().and_then(|f| f.eq),
                filter.target_company_id.as_ref().and_then(|f| f.eq),
                filter.target_opportunity_id.as_ref().and_then(|f| f.eq),
                filter.target_person_id.as_ref().and_then(|f| f.eq),
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let mut updated = Vec::new();
        for target in targets {
            if !uuid_filter_matches(&filter.id, target.id) {
                continue;
            }
            if let Some(target) = state
                .db
                .update_task_target(auth.workspace_id, target.id, &input)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?
            {
                updated.push(target.into());
            }
        }
        Ok(updated)
    }

    async fn delete_task_targets(
        &self,
        ctx: &Context<'_>,
        filter: TaskTargetFilterInput,
    ) -> async_graphql::Result<Vec<TaskTargetGql>> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let targets = state
            .db
            .list_task_targets(
                auth.workspace_id,
                filter.task_id.as_ref().and_then(|f| f.eq),
                filter.target_company_id.as_ref().and_then(|f| f.eq),
                filter.target_opportunity_id.as_ref().and_then(|f| f.eq),
                filter.target_person_id.as_ref().and_then(|f| f.eq),
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let mut deleted = Vec::new();
        for target in targets {
            if !uuid_filter_matches(&filter.id, target.id) {
                continue;
            }
            state
                .db
                .delete_task_target(auth.workspace_id, target.id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?;
            deleted.push(target.into());
        }
        Ok(deleted)
    }

    async fn restore_task_targets(
        &self,
        ctx: &Context<'_>,
        filter: TaskTargetFilterInput,
    ) -> async_graphql::Result<Vec<TaskTargetGql>> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let targets = state
            .db
            .list_task_targets(
                auth.workspace_id,
                filter.task_id.as_ref().and_then(|f| f.eq),
                filter.target_company_id.as_ref().and_then(|f| f.eq),
                filter.target_opportunity_id.as_ref().and_then(|f| f.eq),
                filter.target_person_id.as_ref().and_then(|f| f.eq),
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let mut restored = Vec::new();
        for target in targets {
            if !uuid_filter_matches(&filter.id, target.id) {
                continue;
            }
            if let Some(target) = state
                .db
                .restore_task_target(auth.workspace_id, target.id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?
            {
                restored.push(target.into());
            }
        }
        Ok(restored)
    }

    async fn destroy_task_targets(
        &self,
        ctx: &Context<'_>,
        filter: TaskTargetFilterInput,
    ) -> async_graphql::Result<Vec<TaskTargetGql>> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let targets = state
            .db
            .list_task_targets(
                auth.workspace_id,
                filter.task_id.as_ref().and_then(|f| f.eq),
                filter.target_company_id.as_ref().and_then(|f| f.eq),
                filter.target_opportunity_id.as_ref().and_then(|f| f.eq),
                filter.target_person_id.as_ref().and_then(|f| f.eq),
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let mut destroyed = Vec::new();
        for target in targets {
            if !uuid_filter_matches(&filter.id, target.id) {
                continue;
            }
            state
                .db
                .destroy_task_target(auth.workspace_id, target.id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?;
            destroyed.push(target.into());
        }
        Ok(destroyed)
    }

    async fn update_note_target(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
        data: UpdateNoteTargetInputGql,
    ) -> async_graphql::Result<NoteTargetGql> {
        let auth = require_auth(ctx)?;
        let input: squid_core::UpdateNoteTargetInput = data.into();
        state(ctx)
            .db
            .update_note_target(auth.workspace_id, id, &input)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .map(Into::into)
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn destroy_note_target(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
    ) -> async_graphql::Result<NoteTargetGql> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let target = state
            .db
            .get_note_target(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))?;
        state
            .db
            .destroy_note_target(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        Ok(target.into())
    }

    async fn restore_note_target(
        &self,
        ctx: &Context<'_>,
        id: Uuid,
    ) -> async_graphql::Result<NoteTargetGql> {
        let auth = require_auth(ctx)?;
        state(ctx)
            .db
            .restore_note_target(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?
            .map(Into::into)
            .ok_or_else(|| gql_err(AppError::NotFound("Record not found".to_string())))
    }

    async fn create_note_targets(
        &self,
        ctx: &Context<'_>,
        data: Vec<CreateNoteTargetInputGql>,
        #[graphql(default)] _upsert: bool,
        _share_with: Option<Vec<ShareWithInput>>,
    ) -> async_graphql::Result<Vec<NoteTargetGql>> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let mut created = Vec::with_capacity(data.len());
        for item in data {
            let input: CreateNoteTargetInput = item.into();
            if input.target_company_id.is_none()
                && input.target_opportunity_id.is_none()
                && input.target_person_id.is_none()
            {
                return Err(gql_err(AppError::BadRequest(
                    "A note target must reference a company, opportunity or person".to_string(),
                )));
            }
            created.push(
                state
                    .db
                    .create_note_target(auth.workspace_id, &input)
                    .await
                    .map_err(|e| gql_err(AppError::Internal(e)))?
                    .into(),
            );
        }
        Ok(created)
    }

    async fn update_note_targets(
        &self,
        ctx: &Context<'_>,
        data: UpdateNoteTargetInputGql,
        filter: NoteTargetFilterInput,
    ) -> async_graphql::Result<Vec<NoteTargetGql>> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let input: squid_core::UpdateNoteTargetInput = data.into();
        let targets = state
            .db
            .list_note_targets(
                auth.workspace_id,
                filter.note_id.as_ref().and_then(|f| f.eq),
                filter.target_company_id.as_ref().and_then(|f| f.eq),
                filter.target_opportunity_id.as_ref().and_then(|f| f.eq),
                filter.target_person_id.as_ref().and_then(|f| f.eq),
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let mut updated = Vec::new();
        for target in targets {
            if !uuid_filter_matches(&filter.id, target.id) {
                continue;
            }
            if let Some(target) = state
                .db
                .update_note_target(auth.workspace_id, target.id, &input)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?
            {
                updated.push(target.into());
            }
        }
        Ok(updated)
    }

    async fn delete_note_targets(
        &self,
        ctx: &Context<'_>,
        filter: NoteTargetFilterInput,
    ) -> async_graphql::Result<Vec<NoteTargetGql>> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let targets = state
            .db
            .list_note_targets(
                auth.workspace_id,
                filter.note_id.as_ref().and_then(|f| f.eq),
                filter.target_company_id.as_ref().and_then(|f| f.eq),
                filter.target_opportunity_id.as_ref().and_then(|f| f.eq),
                filter.target_person_id.as_ref().and_then(|f| f.eq),
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let mut deleted = Vec::new();
        for target in targets {
            if !uuid_filter_matches(&filter.id, target.id) {
                continue;
            }
            state
                .db
                .delete_note_target(auth.workspace_id, target.id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?;
            deleted.push(target.into());
        }
        Ok(deleted)
    }

    async fn destroy_note_targets(
        &self,
        ctx: &Context<'_>,
        filter: NoteTargetFilterInput,
    ) -> async_graphql::Result<Vec<NoteTargetGql>> {
        let auth = require_auth(ctx)?;
        let state = state(ctx);
        let targets = state
            .db
            .list_note_targets(
                auth.workspace_id,
                filter.note_id.as_ref().and_then(|f| f.eq),
                filter.target_company_id.as_ref().and_then(|f| f.eq),
                filter.target_opportunity_id.as_ref().and_then(|f| f.eq),
                filter.target_person_id.as_ref().and_then(|f| f.eq),
            )
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))?;
        let mut destroyed = Vec::new();
        for target in targets {
            if !uuid_filter_matches(&filter.id, target.id) {
                continue;
            }
            state
                .db
                .destroy_note_target(auth.workspace_id, target.id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?;
            destroyed.push(target.into());
        }
        Ok(destroyed)
    }

    async fn renew_token(
        &self,
        ctx: &Context<'_>,
        app_token: String,
    ) -> async_graphql::Result<AuthTokensPayload> {
        let tokens = state(ctx)
            .renew_session(&app_token)
            .await
            .map_err(gql_err)?;
        Ok(AuthTokensPayload { tokens })
    }

    async fn sign_out(
        &self,
        ctx: &Context<'_>,
        refresh_token: Option<String>,
    ) -> async_graphql::Result<bool> {
        let state = state(ctx);
        if let Some(refresh_token) = refresh_token {
            let _ = state.revoke_refresh_token(&refresh_token).await;
        } else if let Some(auth) = ctx_data(ctx).auth.as_ref() {
            if let Some(session_id) = auth.session_id {
                let _ = state.db.delete_session(session_id).await;
            }
        }
        if let Ok(mut slot) = ctx_data(ctx).cookie.lock() {
            *slot = Some(CookieAction::Clear);
        }
        Ok(true)
    }
}

async fn sign_in_impl(
    ctx: &Context<'_>,
    email: &str,
    password: &str,
) -> async_graphql::Result<AuthPayload> {
    let state = state(ctx);
    let email = normalize_email(email).map_err(gql_err)?;
    state
        .auth_limiter
        .check(&format!("signin:{email}"))
        .map_err(gql_err)?;

    let (user, password_hash) = state
        .db
        .find_user_by_email(&email)
        .await
        .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
        .ok_or_else(|| {
            gql_err(AppError::Unauthorized(
                "Invalid email or password".to_string(),
            ))
        })?;

    verify_password(password, &password_hash).map_err(gql_err)?;

    let (workspace, role) = state
        .db
        .find_workspace_for_user(user.id)
        .await
        .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
        .ok_or_else(|| {
            gql_err(AppError::Forbidden(
                "No workspace is available for this user".to_string(),
            ))
        })?;

    let (tokens, raw_token, expires_at) = state
        .issue_session(user.id, workspace.id)
        .await
        .map_err(gql_err)?;

    set_cookie(ctx, raw_token, expires_at);

    let user = build_current_user(state, &user, workspace.id, role).await?;
    let workspace = user
        .current_workspace
        .clone()
        .expect("current workspace is built with the user");
    Ok(AuthPayload {
        tokens,
        user,
        workspace,
    })
}

async fn build_current_user(
    state: &AppState,
    user: &CoreUser,
    workspace_id: Uuid,
    role: Role,
) -> async_graphql::Result<User> {
    let workspace = state
        .db
        .find_workspace(workspace_id)
        .await
        .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
        .ok_or_else(|| gql_err(AppError::NotFound("Workspace not found".to_string())))?;
    let member = state
        .db
        .find_workspace_member(workspace_id, user.id)
        .await
        .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
        .ok_or_else(|| gql_err(AppError::NotFound("Workspace member not found".to_string())))?;
    let member_count = state
        .db
        .count_workspace_members(workspace_id)
        .await
        .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?;

    let workspace =
        current_user::workspace(&workspace, role, &state.config.frontend_url, member_count);
    let member = current_user::workspace_member(&member, user, role);
    let all_members = state
        .db
        .list_workspace_members_with_users(workspace_id)
        .await
        .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))?
        .into_iter()
        .map(|(membership, member_user)| {
            let member_role = membership.role;
            current_user::workspace_member(&membership, &member_user, member_role)
        })
        .collect::<Vec<_>>();
    let user_workspace = current_user::UserWorkspace {
        id: member.id,
        user: None,
        user_id: user.id,
        locale: "en".to_string(),
        created_at: user.created_at,
        updated_at: user.created_at,
        deleted_at: None,
        permission_flags: Some(Vec::new()),
        object_permissions: Some(Vec::new()),
        objects_permissions: Some(Vec::new()),
        two_factor_authentication_method_summary: Some(Vec::new()),
        is_impersonating: Some(false),
    };
    let privileged = matches!(role, Role::Owner | Role::Admin);

    Ok(User {
        id: user.id,
        first_name: member.name.first_name.clone(),
        last_name: member.name.last_name.clone(),
        email: user.email.clone(),
        is_email_verified: true,
        disabled: Some(false),
        can_impersonate: false,
        can_access_full_admin_panel: privileged,
        created_at: user.created_at,
        updated_at: user.created_at,
        deleted_at: None,
        locale: "en".to_string(),
        workspace_member: Some(member.clone()),
        user_workspaces: Vec::new(),
        onboarding_status: Some("COMPLETED".to_string()),
        previous_onboarding_status: None,
        current_workspace: Some(workspace),
        current_user_workspace: Some(user_workspace),
        user_vars: Some(GqlJson(serde_json::json!({}))),
        workspace_members: Some(all_members),
        deleted_workspace_members: Some(Vec::new()),
        has_password: true,
        support_user_hash: None,
        is_workspace_creator: Some(role == Role::Owner),
        workspaces: Vec::new(),
        available_workspaces: current_user::AvailableWorkspaces {
            available_workspaces_for_sign_in: Vec::new(),
            available_workspaces_for_sign_up: Vec::new(),
        },
    })
}

fn set_cookie(ctx: &Context<'_>, token: String, expires_at: DateTime<Utc>) {
    if let Ok(mut slot) = ctx_data(ctx).cookie.lock() {
        *slot = Some(CookieAction::Set { token, expires_at });
    }
}

pub async fn graphql_handler(
    State(state): State<AppState>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    request: GraphQLRequest,
) -> Response {
    // User access tokens are accepted by the metadata GraphQL API only. The
    // data API requires an API key or the browser session cookie.
    let allow_user_access = uri.path() == "/metadata";
    let auth = resolve_auth_with_policy(&state, &headers, allow_user_access).await;
    let cookie_slot = Arc::new(Mutex::new(None));

    let mut request = request.into_inner();
    request = request.data(GqlCtx {
        state: state.clone(),
        auth,
        cookie: cookie_slot.clone(),
    });

    let response = schema().execute(request).await;
    let mut response = GraphQLResponse::from(response).into_response();

    let action = cookie_slot.lock().ok().and_then(|mut slot| slot.take());
    match action {
        Some(CookieAction::Set { token, expires_at }) => {
            let cookie = session_cookie(&token, expires_at, state.config.cookie_secure);
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                response.headers_mut().append(header::SET_COOKIE, value);
            }
        }
        Some(CookieAction::Clear) => {
            let cookie = clear_session_cookie(state.config.cookie_secure);
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                response.headers_mut().append(header::SET_COOKIE, value);
            }
        }
        None => {}
    }

    response
}

pub async fn graphql_playground() -> Html<&'static str> {
    Html(PLAYGROUND_HTML)
}

const PLAYGROUND_HTML: &str = r#"<!DOCTYPE html>
<html><head><title>Squid-CRM GraphQL</title></head>
<body>
<p>POST GraphQL queries to <code>/graphql</code>.</p>
<p>Example: <code>{"query":"{ people { id name { firstName lastName } } }"}</code></p>
</body></html>"#;

/// Compile-time checks that the resolvers keep the expected public surface.
#[allow(dead_code)]
fn _assert_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SquidSchema>();
    assert_send_sync::<GqlCtx>();
}
