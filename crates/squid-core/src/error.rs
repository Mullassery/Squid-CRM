use axum::http::StatusCode;

pub type AppResult<T> = Result<T, AppError>;

/// Application error. `public_message` is safe to send to clients; internal
/// causes are logged separately by the `IntoResponse` impl in `lib.rs`.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("too many requests: {0}")]
    TooManyRequests(String),

    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn public_message(&self) -> &str {
        match self {
            AppError::BadRequest(m)
            | AppError::Unauthorized(m)
            | AppError::Forbidden(m)
            | AppError::NotFound(m)
            | AppError::Conflict(m)
            | AppError::TooManyRequests(m) => m,
            AppError::Internal(_) => "Internal server error",
        }
    }

    /// Optional `code` member on REST errors. The captured upstream bodies omit
    /// it for 400/403/404; authentication failures may include one.
    pub fn error_code(&self) -> Option<&'static str> {
        match self {
            AppError::Unauthorized(_) => Some("UNAUTHENTICATED"),
            _ => None,
        }
    }

    /// GraphQL error extension code (all variants get one).
    pub fn graphql_code(&self) -> &'static str {
        match self {
            AppError::BadRequest(_) => "BAD_USER_INPUT",
            AppError::Unauthorized(_) => "UNAUTHENTICATED",
            AppError::Forbidden(_) => "FORBIDDEN",
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::Conflict(_) => "CONFLICT",
            AppError::TooManyRequests(_) => "TOO_MANY_REQUESTS",
            AppError::Internal(_) => "INTERNAL_SERVER_ERROR",
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        AppError::Internal(anyhow::Error::new(value))
    }
}

impl From<async_graphql::Error> for AppError {
    fn from(value: async_graphql::Error) -> Self {
        AppError::BadRequest(value.message)
    }
}
