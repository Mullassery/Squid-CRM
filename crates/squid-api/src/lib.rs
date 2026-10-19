//! HTTP surface for Squid-CRM: REST (`/rest`), GraphQL (`/graphql`), auth
//! (`/auth`) and health (`/healthz`), matching the audited upstream prefixes.

pub mod auth;
pub mod graphql;
pub mod rest;

use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue};
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use squid_auth::{hash_token, AuthContext, TokenKind, TokenService, SESSION_COOKIE_NAME};
use squid_core::{AppError, Config};
use squid_db::Db;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub config: Arc<Config>,
    pub tokens: TokenService,
}

impl AppState {
    pub fn new(db: Db, config: Config) -> Self {
        let tokens = TokenService::new(
            config.jwt_secret.clone(),
            config.login_token_ttl_secs,
            config.access_token_ttl_secs,
            config.refresh_token_ttl_secs,
        );
        Self {
            db,
            config: Arc::new(config),
            tokens,
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

        self.db
            .create_session(user_id, workspace_id, &token_hash, expires_at)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;

        let pair = AuthTokenPair {
            access_or_workspace_agnostic_token: self.tokens.issue_access(user_id, workspace_id)?,
            refresh_token: self.tokens.issue_refresh(user_id)?,
        };

        Ok((pair, raw_token, expires_at))
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
            get(rest::get_person).delete(rest::delete_person),
        )
        .route("/rest/me", get(rest::me))
        .route("/auth/signup", post(auth::signup))
        .route("/auth/signin", post(auth::signin))
        .route("/auth/signout", post(auth::signout))
        .route(
            "/graphql",
            post(graphql::graphql_handler).get(graphql::graphql_playground),
        )
        .route(
            "/metadata",
            post(graphql::graphql_handler).get(graphql::graphql_playground),
        )
        .layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}

/// Resolve the caller from `Authorization: Bearer` or the session cookie.
/// Returns `None` when no valid credential is present.
pub async fn resolve_auth(state: &AppState, headers: &HeaderMap) -> Option<AuthContext> {
    if let Some(value) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(raw) = value.to_str() {
            if let Some(token) = raw.strip_prefix("Bearer ") {
                if let Ok(verified) = state.tokens.verify(token, TokenKind::Access) {
                    let workspace_id = verified.workspace_id?;
                    let role = state
                        .db
                        .find_membership_role(workspace_id, verified.user_id)
                        .await
                        .ok()
                        .flatten()?;
                    return Some(AuthContext {
                        user_id: Some(verified.user_id),
                        workspace_id,
                        role,
                        session_id: None,
                        api_key_id: None,
                    });
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

    Some(AuthContext {
        user_id: Some(session.user_id),
        workspace_id: session.workspace_id,
        role,
        session_id: Some(session.id),
        api_key_id: None,
    })
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
