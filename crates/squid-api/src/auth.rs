//! REST auth endpoints used by the slice. The GraphQL equivalents live in
//! `graphql.rs` and share the same session/token flow.

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use squid_auth::{hash_password, verify_password};
use squid_core::{AppError, AppResult, User, Workspace};

use crate::{clear_session_cookie, resolve_auth, session_cookie, AppState, AuthTokenPair};

#[derive(Debug, Deserialize)]
pub struct SignupInput {
    pub email: String,
    pub password: String,
    #[serde(rename = "workspaceName")]
    pub workspace_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SigninInput {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub tokens: AuthTokenPair,
    pub user: User,
    pub workspace: Workspace,
}

pub async fn signup(
    State(state): State<AppState>,
    Json(input): Json<SignupInput>,
) -> AppResult<Response> {
    let email = normalize_email(&input.email)?;
    let password_hash = hash_password(&input.password)?;

    let display_name = input
        .workspace_name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("{}'s workspace", email.split('@').next().unwrap_or("My")));

    let subdomain = unique_subdomain(&display_name);

    let (user, workspace) = state
        .db
        .bootstrap_user_workspace(&email, &password_hash, &display_name, &subdomain)
        .await
        .map_err(map_signup_error)?;

    if let Err(error) = state.db.create_workspace_schema(workspace.id).await {
        tracing::error!(workspace_id = %workspace.id, %error, "workspace schema creation failed; rolling back");
        let _ = state.db.delete_workspace(workspace.id).await;
        let _ = state.db.delete_user(user.id).await;
        return Err(AppError::Internal(error));
    }

    let (tokens, raw_token, expires_at) = state.issue_session(user.id, workspace.id).await?;
    let cookie = session_cookie(&raw_token, expires_at, state.config.cookie_secure);

    Ok((
        StatusCode::CREATED,
        [(header::SET_COOKIE, cookie)],
        Json(AuthResponse {
            tokens,
            user,
            workspace,
        }),
    )
        .into_response())
}

pub async fn signin(
    State(state): State<AppState>,
    Json(input): Json<SigninInput>,
) -> AppResult<Response> {
    let email = normalize_email(&input.email)?;
    let (user, password_hash) = state
        .db
        .find_user_by_email(&email)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?
        .ok_or_else(|| AppError::Unauthorized("Invalid email or password".to_string()))?;

    verify_password(&input.password, &password_hash)?;

    let (workspace, _role) = state
        .db
        .find_workspace_for_user(user.id)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?
        .ok_or_else(|| {
            AppError::Forbidden("No workspace is available for this user".to_string())
        })?;

    let (tokens, raw_token, expires_at) = state.issue_session(user.id, workspace.id).await?;
    let cookie = session_cookie(&raw_token, expires_at, state.config.cookie_secure);

    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(AuthResponse {
            tokens,
            user,
            workspace,
        }),
    )
        .into_response())
}

pub async fn signout(State(state): State<AppState>, headers: HeaderMap) -> AppResult<Response> {
    if let Some(context) = resolve_auth(&state, &headers).await {
        if let Some(session_id) = context.session_id {
            let _ = state.db.delete_session(session_id).await;
        }
    }

    let cookie = clear_session_cookie(state.config.cookie_secure);
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(json!({ "success": true })),
    )
        .into_response())
}

pub(crate) fn normalize_email(email: &str) -> AppResult<String> {
    let email = email.trim().to_lowercase();
    let valid = email.len() <= 254
        && email
            .split_once('@')
            .map(|(local, domain)| !local.is_empty() && domain.contains('.'))
            .unwrap_or(false);
    if !valid {
        return Err(AppError::BadRequest(
            "A valid email is required".to_string(),
        ));
    }
    Ok(email)
}

pub(crate) fn unique_subdomain(name: &str) -> String {
    let slug = slugify(name);
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    format!("{slug}-{}", &suffix[..8])
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "workspace".to_string()
    } else {
        out.chars().take(40).collect()
    }
}

fn map_signup_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .map(|db| db.is_unique_violation())
        .unwrap_or(false)
    {
        AppError::Conflict("A user with this email already exists".to_string())
    } else {
        AppError::Internal(anyhow::anyhow!(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_validates_email() {
        assert_eq!(
            normalize_email("  Alice@Example.COM ").unwrap(),
            "alice@example.com"
        );
        assert!(normalize_email("not-an-email").is_err());
        assert!(normalize_email("a@b").is_err());
    }

    #[test]
    fn subdomains_are_slugged_and_unique() {
        let a = unique_subdomain("My Company");
        let b = unique_subdomain("My Company");
        assert!(a.starts_with("my-company-"));
        assert_ne!(a, b);
        assert!(a.len() <= 48);
    }
}
