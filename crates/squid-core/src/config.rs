use std::env;

/// Runtime configuration, loaded from the environment.
///
/// Secrets are never logged. `jwt_secret` must be supplied by the operator.
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub frontend_url: String,
    pub jwt_secret: String,
    pub login_token_ttl_secs: i64,
    pub access_token_ttl_secs: i64,
    pub refresh_token_ttl_secs: i64,
    pub max_page_size: i64,
    pub cookie_secure: bool,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let database_url = env::var("DATABASE_URL")
            .or_else(|_| env::var("PG_DATABASE_URL"))
            .map_err(|_| "DATABASE_URL (or PG_DATABASE_URL) must be set".to_string())?;

        let jwt_secret = env::var("APP_SECRET")
            .or_else(|_| env::var("JWT_SECRET"))
            .map_err(|_| "APP_SECRET (or JWT_SECRET) must be set".to_string())?;

        if jwt_secret.len() < 32 {
            return Err("APP_SECRET must be at least 32 characters".to_string());
        }

        Ok(Self {
            database_url,
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string()),
            frontend_url: env::var("FRONTEND_URL")
                .unwrap_or_else(|_| "http://localhost:3001".to_string()),
            jwt_secret,
            login_token_ttl_secs: parse_env("LOGIN_TOKEN_TTL_SECS", 300),
            access_token_ttl_secs: parse_env("ACCESS_TOKEN_TTL_SECS", 3600),
            refresh_token_ttl_secs: parse_env("REFRESH_TOKEN_TTL_SECS", 60 * 60 * 24 * 30),
            max_page_size: parse_env("MAX_PAGE_SIZE", 100),
            cookie_secure: env::var("COOKIE_SECURE")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
        })
    }
}

fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
