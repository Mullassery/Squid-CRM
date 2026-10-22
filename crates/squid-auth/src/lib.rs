//! Authentication primitives: password hashing, JWT tokens, opaque session
//! tokens and the authenticated request context.
//!
//! Tokens carry a `typ` claim so a login token can never be replayed as an
//! access token. Session tokens are stored only as SHA-256 hashes.

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use squid_core::{AppError, AppResult, Role};
use uuid::Uuid;

pub const SESSION_COOKIE_NAME: &str = "twenty-session";
const TOKEN_ISSUER: &str = "squid-crm";

#[derive(Clone)]
pub struct TokenService {
    secret: String,
    login_ttl: Duration,
    access_ttl: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Login,
    Access,
    Refresh,
}

impl TokenKind {
    fn as_str(self) -> &'static str {
        match self {
            TokenKind::Login => "login",
            TokenKind::Access => "access",
            TokenKind::Refresh => "refresh",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sid: Option<Uuid>,
    jti: Uuid,
    typ: String,
    iss: String,
    iat: i64,
    exp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, async_graphql::SimpleObject)]
pub struct AuthToken {
    pub token: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct VerifiedToken {
    pub user_id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
}

/// The authenticated identity for a request.
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// `None` for workspace API keys, which are not tied to a user.
    pub user_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub role: Role,
    pub role_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub api_key_id: Option<Uuid>,
}

impl AuthContext {
    pub fn require_admin(&self) -> AppResult<()> {
        if self.role.can_administer() {
            Ok(())
        } else {
            Err(AppError::Forbidden(
                "You do not have permission to perform this action".to_string(),
            ))
        }
    }
}

impl TokenService {
    pub fn new(secret: impl Into<String>, login_ttl_secs: i64, access_ttl_secs: i64) -> Self {
        Self {
            secret: secret.into(),
            login_ttl: Duration::seconds(login_ttl_secs),
            access_ttl: Duration::seconds(access_ttl_secs),
        }
    }

    fn issue(
        &self,
        user_id: Uuid,
        workspace_id: Option<Uuid>,
        session_id: Option<Uuid>,
        kind: TokenKind,
        ttl: Duration,
    ) -> AppResult<AuthToken> {
        let now = Utc::now();
        let expires_at = now + ttl;
        let claims = Claims {
            sub: user_id,
            workspace_id,
            sid: session_id,
            jti: Uuid::new_v4(),
            typ: kind.as_str().to_string(),
            iss: TOKEN_ISSUER.to_string(),
            iat: now.timestamp(),
            exp: expires_at.timestamp(),
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .map_err(|e| AppError::Internal(anyhow::anyhow!("token encode failed: {e}")))?;
        Ok(AuthToken { token, expires_at })
    }

    pub fn issue_login(&self, user_id: Uuid) -> AppResult<AuthToken> {
        self.issue(user_id, None, None, TokenKind::Login, self.login_ttl)
    }

    pub fn issue_access(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
        session_id: Uuid,
    ) -> AppResult<AuthToken> {
        self.issue(
            user_id,
            Some(workspace_id),
            Some(session_id),
            TokenKind::Access,
            self.access_ttl,
        )
    }

    pub fn verify(&self, token: &str, expected: TokenKind) -> AppResult<VerifiedToken> {
        let mut validation = Validation::default();
        validation.set_issuer(&[TOKEN_ISSUER]);
        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &validation,
        )
        .map_err(|_| AppError::Unauthorized("Invalid or expired token".to_string()))?;

        if data.claims.typ != expected.as_str() {
            return Err(AppError::Unauthorized("Invalid token type".to_string()));
        }

        Ok(VerifiedToken {
            user_id: data.claims.sub,
            workspace_id: data.claims.workspace_id,
            session_id: data.claims.sid,
        })
    }
}

pub fn hash_password(password: &str) -> AppResult<String> {
    if password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(anyhow::anyhow!("password hash failed: {e}")))
}

pub fn verify_password(password: &str, hash: &str) -> AppResult<()> {
    let parsed = PasswordHash::new(hash)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("stored password hash is invalid")))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AppError::Unauthorized("Invalid email or password".to_string()))
}

/// Opaque session token: 32 random bytes, hex encoded.
pub fn generate_session_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    hex::encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> TokenService {
        TokenService::new("test-secret-that-is-long-enough-123456", 300, 3600)
    }

    #[test]
    fn login_token_cannot_be_used_as_access_token() {
        let svc = service();
        let login = svc.issue_login(Uuid::new_v4()).unwrap();
        let err = svc.verify(&login.token, TokenKind::Access).unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
    }

    #[test]
    fn access_token_roundtrip() {
        let svc = service();
        let user = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let session = Uuid::new_v4();
        let token = svc.issue_access(user, workspace, session).unwrap();
        let verified = svc.verify(&token.token, TokenKind::Access).unwrap();
        assert_eq!(verified.user_id, user);
        assert_eq!(verified.workspace_id, Some(workspace));
        assert_eq!(verified.session_id, Some(session));
    }

    #[test]
    fn password_hash_roundtrip_and_rejects_wrong_password() {
        let hash = hash_password("correct-horse-battery").unwrap();
        assert!(verify_password("correct-horse-battery", &hash).is_ok());
        assert!(verify_password("wrong-password", &hash).is_err());
    }

    #[test]
    fn short_passwords_are_rejected() {
        assert!(hash_password("short").is_err());
    }

    #[test]
    fn session_tokens_are_random_and_hashed() {
        let a = generate_session_token();
        let b = generate_session_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(hash_token(&a), a);
        assert_eq!(hash_token(&a), hash_token(&a));
    }
}
