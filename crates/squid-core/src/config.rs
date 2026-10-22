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
    pub s3: Option<S3Config>,
}

#[derive(Clone)]
pub struct S3Config {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
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
            s3: s3_from_env()?,
        })
    }
}

fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn s3_from_env() -> Result<Option<S3Config>, String> {
    let values = [
        ("S3_ENDPOINT", env::var("S3_ENDPOINT").ok()),
        ("S3_BUCKET", env::var("S3_BUCKET").ok()),
        ("S3_REGION", env::var("S3_REGION").ok()),
        ("S3_ACCESS_KEY_ID", env::var("S3_ACCESS_KEY_ID").ok()),
        (
            "S3_SECRET_ACCESS_KEY",
            env::var("S3_SECRET_ACCESS_KEY").ok(),
        ),
    ];
    if values.iter().all(|(_, value)| value.is_none()) {
        return Ok(None);
    }
    let mut resolved = std::collections::HashMap::new();
    for (name, value) in values {
        let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
            return Err(format!("{name} must be set when S3 storage is configured"));
        };
        resolved.insert(name, value);
    }
    Ok(Some(S3Config {
        endpoint: resolved["S3_ENDPOINT"].trim_end_matches('/').to_string(),
        bucket: resolved["S3_BUCKET"].clone(),
        region: resolved["S3_REGION"].clone(),
        access_key_id: resolved["S3_ACCESS_KEY_ID"].clone(),
        secret_access_key: resolved["S3_SECRET_ACCESS_KEY"].clone(),
    }))
}
