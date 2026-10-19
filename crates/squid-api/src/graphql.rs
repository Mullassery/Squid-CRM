//! GraphQL surface. The Phase 4 slice exposes the auth flow the frontend uses
//! (`getLoginTokenFromCredentials` -> `getAuthTokensFromLoginToken`) plus the
//! `person` object, so the existing React client can authenticate and read
//! people against the Rust backend.

use std::sync::{Arc, Mutex, OnceLock};

use async_graphql::ErrorExtensions;
use async_graphql::{Context, EmptySubscription, InputObject, Object, Schema, SimpleObject};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{Html, IntoResponse, Response};
use chrono::{DateTime, Utc};
use squid_auth::AuthToken;
use squid_auth::{hash_password, hash_token, verify_password, AuthContext};
use squid_core::{
    ApiKey, AppError, AppResult, CreatePersonInput, GqlJson, PageInfo, Person, UpdatePersonInput,
    User, Workspace,
};
use squid_db::{OrderTerm, PersonColumn};

use crate::auth::{normalize_email, unique_subdomain};
use crate::{clear_session_cookie, resolve_auth, session_cookie, AppState, AuthTokenPair};

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
#[graphql(name = "PersonCreateInput")]
pub struct PersonCreateInputGql {
    pub name: Option<FullNameCreateInput>,
    pub emails: Option<EmailsCreateInput>,
    pub job_title: Option<String>,
    pub linkedin_link: Option<LinksCreateInput>,
    pub phones: Option<PhonesCreateInput>,
    pub avatar_url: Option<String>,
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
            position: input.position,
            company_id: input.company_id,
        }
    }
}

#[derive(Debug, InputObject)]
#[graphql(name = "PersonUpdateInput")]
pub struct PersonUpdateInputGql {
    pub name: Option<FullNameCreateInput>,
    pub emails: Option<EmailsCreateInput>,
    pub job_title: Option<String>,
    pub linkedin_link: Option<LinksCreateInput>,
    pub phones: Option<PhonesCreateInput>,
    pub avatar_url: Option<String>,
    pub position: Option<f64>,
    pub company_id: Option<uuid::Uuid>,
}

impl From<PersonUpdateInputGql> for UpdatePersonInput {
    fn from(input: PersonUpdateInputGql) -> Self {
        UpdatePersonInput {
            name: input.name.map(|name| squid_core::FullName {
                first_name: name.first_name.unwrap_or_default(),
                last_name: name.last_name.unwrap_or_default(),
            }),
            emails: input.emails.map(|emails| squid_core::Emails {
                primary_email: emails.primary_email.unwrap_or_default(),
                additional_emails: emails.additional_emails.unwrap_or_default(),
            }),
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
            position: input.position,
            company_id: input.company_id,
        }
    }
}

#[derive(Debug, InputObject)]
pub struct UuidFilter {
    pub eq: Option<uuid::Uuid>,
}

#[derive(Debug, InputObject)]
pub struct StringFilter {
    pub eq: Option<String>,
}

#[derive(Debug, InputObject)]
pub struct PersonFilterInput {
    pub id: Option<UuidFilter>,
    pub job_title: Option<StringFilter>,
    pub and: Option<Vec<PersonFilterInput>>,
    pub or: Option<Vec<PersonFilterInput>>,
    pub not: Option<Box<PersonFilterInput>>,
}

impl PersonFilterInput {
    fn id_eq(&self) -> Option<uuid::Uuid> {
        self.id.as_ref().and_then(|id| id.eq)
    }

    fn is_empty(&self) -> bool {
        self.id_eq().is_none()
            && self.job_title.is_none()
            && self.and.as_ref().is_none_or(Vec::is_empty)
            && self.or.as_ref().is_none_or(Vec::is_empty)
            && self.not.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, async_graphql::Enum)]
pub enum OrderByDirection {
    AscNullsFirst,
    AscNullsLast,
    DescNullsFirst,
    DescNullsLast,
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

fn empty_filter() -> PersonFilterInput {
    PersonFilterInput {
        id: None,
        job_title: None,
        and: None,
        or: None,
        not: None,
    }
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

fn connection(people: Vec<Person>, limit: i64) -> PersonConnection {
    let total = people.len() as i64;
    let has_next_page = total > limit;
    let people: Vec<Person> = people.into_iter().take(limit as usize).collect();
    connection_with(people, total, has_next_page, false)
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
        state(ctx)
            .db
            .find_user_by_id(user_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))
    }

    async fn current_workspace(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Option<Workspace>> {
        let Some(auth) = ctx_data(ctx).auth.as_ref() else {
            return Ok(None);
        };
        state(ctx)
            .db
            .find_workspace(auth.workspace_id)
            .await
            .map_err(|e| gql_err(AppError::Internal(anyhow::anyhow!(e))))
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

        let filter = filter.unwrap_or_else(empty_filter);
        if let Some(id) = filter.id_eq() {
            let person = state
                .db
                .get_person(auth.workspace_id, id)
                .await
                .map_err(|e| gql_err(AppError::Internal(e)))?;
            let people: Vec<Person> = person.into_iter().collect();
            return Ok(connection(people, limit));
        }
        if !filter.is_empty() {
            return Err(gql_err(AppError::BadRequest(
                "Unsupported filter: only id.eq is implemented".to_string(),
            )));
        }

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

    async fn person(
        &self,
        ctx: &Context<'_>,
        filter: PersonFilterInput,
    ) -> async_graphql::Result<Option<Person>> {
        let auth = require_auth(ctx)?;
        let id = filter.id_eq().ok_or_else(|| {
            gql_err(AppError::BadRequest(
                "person requires filter.id.eq".to_string(),
            ))
        })?;
        state(ctx)
            .db
            .get_person(auth.workspace_id, id)
            .await
            .map_err(|e| gql_err(AppError::Internal(e)))
    }
}

#[Object]
impl MutationRoot {
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

        let (workspace, _role) = state
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

    async fn sign_out(&self, ctx: &Context<'_>) -> async_graphql::Result<bool> {
        if let Some(auth) = ctx_data(ctx).auth.as_ref() {
            if let Some(session_id) = auth.session_id {
                let _ = state(ctx).db.delete_session(session_id).await;
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

    let (workspace, _role) = state
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

    Ok(AuthPayload {
        tokens,
        user,
        workspace,
    })
}

fn set_cookie(ctx: &Context<'_>, token: String, expires_at: DateTime<Utc>) {
    if let Ok(mut slot) = ctx_data(ctx).cookie.lock() {
        *slot = Some(CookieAction::Set { token, expires_at });
    }
}

pub async fn graphql_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: GraphQLRequest,
) -> Response {
    let auth = resolve_auth(&state, &headers).await;
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
