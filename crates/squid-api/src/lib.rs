//! HTTP surface for Squid-CRM: REST (`/rest`), GraphQL (`/graphql`), auth
//! (`/auth`) and health (`/healthz`), matching the audited upstream prefixes.

pub mod auth;
pub mod current_user;
pub mod filtering;
pub mod graphql;
pub mod group_by;
pub mod rate_limit;
pub mod rest;
pub use rest::run_webhook_replay_worker;
pub mod sorting;

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use squid_auth::{hash_token, AuthContext, TokenKind, TokenService, SESSION_COOKIE_NAME};
use squid_core::{AppError, Config};
use squid_db::Db;
use squid_db::IdempotencyClaim;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub config: Arc<Config>,
    pub tokens: TokenService,
    pub auth_limiter: Arc<crate::rate_limit::AuthRateLimiter>,
}

impl AppState {
    pub fn new(db: Db, config: Config) -> Self {
        let tokens = TokenService::new(
            config.jwt_secret.clone(),
            config.login_token_ttl_secs,
            config.access_token_ttl_secs,
        );
        Self {
            db,
            config: Arc::new(config),
            tokens,
            auth_limiter: Arc::new(crate::rate_limit::AuthRateLimiter::default()),
        }
    }

    pub async fn issue_session(
        &self,
        user_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
    ) -> Result<(AuthTokenPair, String, chrono::DateTime<chrono::Utc>), AppError> {
        use chrono::{Duration, Utc};

        let raw_token = squid_auth::generate_session_token();
        let token_hash = hash_token(&raw_token);
        let expires_at = Utc::now() + Duration::seconds(self.config.refresh_token_ttl_secs);

        let session_id = self
            .db
            .create_session(user_id, workspace_id, &token_hash, expires_at)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;

        let refresh_token = match self
            .issue_refresh_token(user_id, workspace_id, session_id)
            .await
        {
            Ok(token) => token,
            Err(error) => {
                let _ = self.db.delete_session(session_id).await;
                return Err(error);
            }
        };

        let pair = AuthTokenPair {
            access_or_workspace_agnostic_token: self.tokens.issue_access(
                user_id,
                workspace_id,
                session_id,
            )?,
            refresh_token,
        };

        Ok((pair, raw_token, expires_at))
    }

    async fn issue_refresh_token(
        &self,
        user_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
        session_id: uuid::Uuid,
    ) -> Result<squid_auth::AuthToken, AppError> {
        use chrono::{Duration, Utc};

        let raw_token = squid_auth::generate_session_token();
        let expires_at = Utc::now() + Duration::seconds(self.config.refresh_token_ttl_secs);
        self.db
            .create_refresh_token(
                user_id,
                workspace_id,
                session_id,
                &hash_token(&raw_token),
                expires_at,
            )
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;

        Ok(squid_auth::AuthToken {
            token: raw_token,
            expires_at,
        })
    }

    /// Rotate a refresh token. The old token is consumed atomically and a new
    /// access/refresh pair is issued for the same session.
    pub async fn renew_session(&self, refresh_token: &str) -> Result<AuthTokenPair, AppError> {
        use chrono::{Duration, Utc};

        let raw_refresh = squid_auth::generate_session_token();
        let expires_at = Utc::now() + Duration::seconds(self.config.refresh_token_ttl_secs);
        let rotated = self
            .db
            .rotate_refresh_token(
                &hash_token(refresh_token),
                &hash_token(&raw_refresh),
                expires_at,
            )
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?
            .ok_or_else(|| {
                AppError::Unauthorized("Invalid or expired refresh token".to_string())
            })?;

        Ok(AuthTokenPair {
            access_or_workspace_agnostic_token: self.tokens.issue_access(
                rotated.user_id,
                rotated.workspace_id,
                rotated.session_id,
            )?,
            refresh_token: squid_auth::AuthToken {
                token: raw_refresh,
                expires_at,
            },
        })
    }

    pub async fn revoke_refresh_token(&self, refresh_token: &str) -> Result<(), AppError> {
        let token = self
            .db
            .find_refresh_token_by_hash(&hash_token(refresh_token))
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;

        if let Some(token) = token {
            self.db
                .delete_session(token.session_id)
                .await
                .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
        }
        Ok(())
    }
}

/// Token pair shape consumed by the existing frontend (`AuthTokenPairFragment`).
#[derive(Debug, Clone, Serialize, Deserialize, async_graphql::SimpleObject)]
#[serde(rename_all = "camelCase")]
pub struct AuthTokenPair {
    pub access_or_workspace_agnostic_token: squid_auth::AuthToken,
    pub refresh_token: squid_auth::AuthToken,
}

pub fn app(state: AppState) -> Router {
    let origin = HeaderValue::from_str(&state.config.frontend_url)
        .unwrap_or_else(|_| HeaderValue::from_static("http://localhost:3001"));

    // Credentialed CORS cannot use wildcards. Allow only the frontend origin
    // and the headers/methods the client actually sends.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(origin))
        .allow_credentials(true)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PATCH,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::header::ACCEPT,
            axum::http::header::COOKIE,
            axum::http::HeaderName::from_static("x-requested-with"),
            axum::http::HeaderName::from_static("idempotency-key"),
            axum::http::HeaderName::from_static("x-correlation-id"),
            axum::http::HeaderName::from_static("x-request-id"),
        ])
        .expose_headers([
            axum::http::HeaderName::from_static("idempotency-replayed"),
            axum::http::HeaderName::from_static("x-correlation-id"),
        ]);

    Router::new()
        .route("/healthz", get(rest::healthz))
        .route("/client-config", get(rest::client_config))
        .route(
            "/rest/people",
            get(rest::list_people).post(rest::create_person),
        )
        .route(
            "/rest/people/{id}",
            get(rest::get_person)
                .patch(rest::update_person)
                .delete(rest::delete_person),
        )
        .route("/rest/people/{id}/restore", patch(rest::restore_person))
        .route("/rest/people/export", get(rest::export_people))
        .route("/rest/people/import", post(rest::import_people))
        .route("/rest/people/groupBy", get(rest::group_by_people))
        .route(
            "/rest/companies",
            get(rest::list_companies).post(rest::create_company),
        )
        .route(
            "/rest/companies/{id}",
            get(rest::get_company)
                .patch(rest::update_company)
                .delete(rest::delete_company),
        )
        .route("/rest/companies/{id}/restore", patch(rest::restore_company))
        .route("/rest/companies/export", get(rest::export_companies))
        .route("/rest/companies/import", post(rest::import_companies))
        .route("/rest/companies/groupBy", get(rest::group_by_companies))
        .route(
            "/rest/opportunities",
            get(rest::list_opportunities).post(rest::create_opportunity),
        )
        .route(
            "/rest/opportunities/{id}",
            get(rest::get_opportunity)
                .patch(rest::update_opportunity)
                .delete(rest::delete_opportunity),
        )
        .route(
            "/rest/opportunities/{id}/restore",
            patch(rest::restore_opportunity),
        )
        .route(
            "/rest/opportunities/export",
            get(rest::export_opportunities),
        )
        .route(
            "/rest/opportunities/import",
            post(rest::import_opportunities),
        )
        .route(
            "/rest/opportunities/groupBy",
            get(rest::group_by_opportunities),
        )
        .route("/rest/tasks", get(rest::list_tasks).post(rest::create_task))
        .route(
            "/rest/tasks/{id}",
            get(rest::get_task)
                .patch(rest::update_task)
                .delete(rest::delete_task),
        )
        .route("/rest/tasks/{id}/restore", patch(rest::restore_task))
        .route("/rest/tasks/export", get(rest::export_tasks))
        .route("/rest/tasks/import", post(rest::import_tasks))
        .route("/rest/tasks/groupBy", get(rest::group_by_tasks))
        .route("/rest/notes", get(rest::list_notes).post(rest::create_note))
        .route(
            "/rest/notes/{id}",
            get(rest::get_note)
                .patch(rest::update_note)
                .delete(rest::delete_note),
        )
        .route("/rest/notes/{id}/restore", patch(rest::restore_note))
        .route("/rest/notes/export", get(rest::export_notes))
        .route("/rest/notes/import", post(rest::import_notes))
        .route("/rest/notes/groupBy", get(rest::group_by_notes))
        .route(
            "/rest/taskTargets",
            get(rest::list_task_targets).post(rest::create_task_target),
        )
        .route(
            "/rest/taskTargets/{id}",
            get(rest::get_task_target)
                .patch(rest::update_task_target)
                .delete(rest::delete_task_target),
        )
        .route(
            "/rest/taskTargets/{id}/restore",
            patch(rest::restore_task_target),
        )
        .route(
            "/rest/taskTargets/groupBy",
            get(rest::group_by_task_targets),
        )
        .route(
            "/rest/noteTargets",
            get(rest::list_note_targets).post(rest::create_note_target),
        )
        .route(
            "/rest/noteTargets/{id}",
            get(rest::get_note_target)
                .patch(rest::update_note_target)
                .delete(rest::delete_note_target),
        )
        .route(
            "/rest/noteTargets/{id}/restore",
            patch(rest::restore_note_target),
        )
        .route(
            "/rest/noteTargets/groupBy",
            get(rest::group_by_note_targets),
        )
        .route("/files/upload/{id}", axum::routing::put(rest::upload_file))
        .route("/files/{id}", get(rest::download_file))
        .route("/rest/me", get(rest::me))
        .route("/auth/signup", post(auth::signup))
        .route("/auth/signin", post(auth::signin))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/signout", post(auth::signout))
        .route(
            "/graphql",
            post(graphql::graphql_handler).get(graphql::graphql_playground),
        )
        .route(
            "/metadata",
            post(graphql::graphql_handler).get(graphql::graphql_playground),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            idempotency_guard,
        ))
        .layer(middleware::from_fn_with_state(state.clone(), csrf_guard))
        .layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .layer(middleware::from_fn(correlation_id))
        .fallback(rest::custom_rest_fallback)
        .with_state(state)
}

/// Propagate a caller-supplied request/correlation id, or generate one. The
/// id is returned on every response so logs and downstream calls can be joined.
async fn correlation_id(request: Request, next: Next) -> Response {
    let incoming = request
        .headers()
        .get("x-correlation-id")
        .or_else(|| request.headers().get("x-request-id"))
        .and_then(|value| value.to_str().ok())
        .filter(|value| valid_request_id(value))
        .map(str::to_string);
    let correlation = incoming.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&correlation) {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static("x-correlation-id"),
            value,
        );
    }
    response
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

/// Replay-safe REST POSTs. A caller may supply `Idempotency-Key`; the first
/// successful response is stored per workspace and returned verbatim on retry.
/// Reusing the same key with a different method/path/body is rejected.
async fn idempotency_guard(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() != Method::POST || !request.uri().path().starts_with("/rest/") {
        return next.run(request).await;
    }
    let Some(raw_key) = request
        .headers()
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .map(str::to_string)
    else {
        return next.run(request).await;
    };
    if !valid_idempotency_key(&raw_key) {
        return AppError::BadRequest(
            "Idempotency-Key must be 1-255 printable ASCII characters".to_string(),
        )
        .into_response();
    }

    let path = request.uri().path().to_string();
    let (parts, body) = request.into_parts();
    let body_bytes = match to_bytes(body, 2 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return AppError::BadRequest("Request body could not be read".to_string())
                .into_response()
        }
    };

    let Some(auth) = resolve_auth(&state, &parts.headers).await else {
        return next
            .run(Request::from_parts(parts, Body::from(body_bytes)))
            .await;
    };

    let request_hash = hex::encode(Sha256::digest(&body_bytes));
    let claim = match state
        .db
        .claim_idempotency(
            auth.workspace_id,
            &raw_key,
            parts.method.as_str(),
            &path,
            &request_hash,
        )
        .await
    {
        Ok(claim) => claim,
        Err(error) => {
            return AppError::Internal(anyhow::anyhow!(error)).into_response();
        }
    };

    match claim {
        IdempotencyClaim::Replay {
            status,
            content_type,
            body,
        } => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            return Response::builder()
                .status(status)
                .header(axum::http::header::CONTENT_TYPE, content_type)
                .header("idempotency-replayed", "true")
                .header("idempotency-key", raw_key)
                .body(Body::from(body))
                .unwrap_or_else(|_| {
                    AppError::Internal(anyhow::anyhow!("invalid replayed response")).into_response()
                });
        }
        IdempotencyClaim::Conflict => {
            return AppError::Conflict(
                "Idempotency-Key was already used for a different request".to_string(),
            )
            .into_response();
        }
        IdempotencyClaim::InProgress => {
            return AppError::Conflict(
                "An identical request is still in progress; retry shortly".to_string(),
            )
            .into_response();
        }
        IdempotencyClaim::Claimed => {}
    }

    let response = next
        .run(Request::from_parts(parts, Body::from(body_bytes)))
        .await;
    if !response.status().is_success() {
        let _ = state
            .db
            .release_idempotency(auth.workspace_id, &raw_key)
            .await;
        return response;
    }

    let (parts, body) = response.into_parts();
    let content_type = parts
        .headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let bytes = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = state
                .db
                .release_idempotency(auth.workspace_id, &raw_key)
                .await;
            return AppError::Internal(anyhow::anyhow!(error)).into_response();
        }
    };
    if bytes.len() > 1024 * 1024 {
        let _ = state
            .db
            .release_idempotency(auth.workspace_id, &raw_key)
            .await;
        tracing::warn!(
            path = %path,
            status = parts.status.as_u16(),
            "response too large to persist for idempotency replay"
        );
        return Response::from_parts(parts, Body::from(bytes));
    }

    if let Err(error) = state
        .db
        .complete_idempotency(
            auth.workspace_id,
            &raw_key,
            parts.status.as_u16(),
            &content_type,
            &bytes,
        )
        .await
    {
        tracing::warn!(%error, %path, "failed to persist idempotent response");
        let _ = state
            .db
            .release_idempotency(auth.workspace_id, &raw_key)
            .await;
    }

    let mut response = Response::from_parts(parts, Body::from(bytes));
    if let Ok(value) = HeaderValue::from_str(&raw_key) {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static("idempotency-key"),
            value,
        );
    }
    response
}

fn valid_idempotency_key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 255 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

/// Cookie-authenticated state-changing requests must come from the configured
/// frontend origin when the browser supplies an `Origin` header. Bearer/API-key
/// requests are not cookie-authenticated and are therefore unaffected.
async fn csrf_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let method = request.method();
    let unsafe_method = !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    let has_bearer = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .is_some();
    let has_cookie = request.headers().get(axum::http::header::COOKIE).is_some();

    if unsafe_method && has_cookie && !has_bearer {
        if let Some(origin) = request.headers().get(axum::http::header::ORIGIN) {
            if origin.to_str().ok() != Some(state.config.frontend_url.as_str()) {
                return AppError::Forbidden("Invalid request origin".to_string()).into_response();
            }
        }
    }

    next.run(request).await
}

/// Resolve the caller from `Authorization: Bearer` or the session cookie.
/// Returns `None` when no valid credential is present. Access tokens are only
/// accepted where `allow_user_access` is true (the `/metadata` API); data APIs
/// require an API key or the session cookie, matching upstream.
pub async fn resolve_auth_with_policy(
    state: &AppState,
    headers: &HeaderMap,
    allow_user_access: bool,
) -> Option<AuthContext> {
    if let Some(value) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(raw) = value.to_str() {
            if let Some(token) = raw.strip_prefix("Bearer ") {
                if allow_user_access {
                    if let Ok(verified) = state.tokens.verify(token, TokenKind::Access) {
                        let workspace_id = verified.workspace_id?;
                        let session_id = verified.session_id?;
                        let session = state.db.find_session_by_id(session_id).await.ok()??;
                        if session.user_id != verified.user_id
                            || session.workspace_id != workspace_id
                        {
                            return None;
                        }
                        let role = state
                            .db
                            .find_membership_role(workspace_id, verified.user_id)
                            .await
                            .ok()
                            .flatten()?;
                        let role_id = state
                            .db
                            .find_membership_role_id(workspace_id, verified.user_id)
                            .await
                            .ok()
                            .flatten();
                        return Some(AuthContext {
                            user_id: Some(verified.user_id),
                            workspace_id,
                            role,
                            role_id,
                            session_id: Some(session_id),
                            api_key_id: None,
                        });
                    }
                }

                if let Ok(Some(key)) = state
                    .db
                    .find_api_key_by_token_hash(&hash_token(token))
                    .await
                {
                    return Some(AuthContext {
                        user_id: None,
                        workspace_id: key.workspace_id,
                        role: key.role,
                        role_id: key.role_id,
                        session_id: None,
                        api_key_id: Some(key.api_key_id),
                    });
                }
            }
        }
    }

    let token = extract_cookie(headers, SESSION_COOKIE_NAME)?;
    let session = state
        .db
        .find_session_by_hash(&hash_token(&token))
        .await
        .ok()
        .flatten()?;
    let role = state
        .db
        .find_membership_role(session.workspace_id, session.user_id)
        .await
        .ok()
        .flatten()?;

    let role_id = state
        .db
        .find_membership_role_id(session.workspace_id, session.user_id)
        .await
        .ok()
        .flatten();
    Some(AuthContext {
        user_id: Some(session.user_id),
        workspace_id: session.workspace_id,
        role,
        role_id,
        session_id: Some(session.id),
        api_key_id: None,
    })
}

pub async fn resolve_auth(state: &AppState, headers: &HeaderMap) -> Option<AuthContext> {
    resolve_auth_with_policy(state, headers, true).await
}

pub fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for part in header.split(';') {
        let part = part.trim();
        if let Some((key, value)) = part.split_once('=') {
            if key.trim() == name {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

pub fn session_cookie(
    token: &str,
    expires_at: chrono::DateTime<chrono::Utc>,
    secure: bool,
) -> String {
    let mut cookie = format!(
        "{SESSION_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Expires={}",
        expires_at.to_rfc2822()
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

pub fn clear_session_cookie(secure: bool) -> String {
    let mut cookie = format!(
        "{SESSION_COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn extracts_cookie_value() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_static("a=1; twenty-session=abc123; b=2"),
        );
        assert_eq!(
            extract_cookie(&headers, SESSION_COOKIE_NAME),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn missing_cookie_returns_none() {
        let headers = HeaderMap::new();
        assert_eq!(extract_cookie(&headers, SESSION_COOKIE_NAME), None);
    }
}
