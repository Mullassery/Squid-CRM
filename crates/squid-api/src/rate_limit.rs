//! Small in-process limiter for credential endpoints. This complements the
//! per-account lockout with a coarse burst guard; deployments behind multiple
//! replicas should also rate limit at the edge.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use squid_core::{AppError, AppResult};

const MAX_ATTEMPTS: usize = 10;
const WINDOW: Duration = Duration::from_secs(15 * 60);
const MAX_TRACKED_KEYS: usize = 10_000;

#[derive(Default)]
pub struct AuthRateLimiter {
    attempts: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl AuthRateLimiter {
    pub fn check(&self, key: &str) -> AppResult<()> {
        let now = Instant::now();
        let mut attempts = self
            .attempts
            .lock()
            .map_err(|_| AppError::Internal(anyhow::anyhow!("auth rate limiter poisoned")))?;

        if attempts.len() >= MAX_TRACKED_KEYS && !attempts.contains_key(key) {
            attempts.retain(|_, entries| {
                entries
                    .back()
                    .is_some_and(|instant| now.duration_since(*instant) <= WINDOW)
            });
            if attempts.len() >= MAX_TRACKED_KEYS {
                return Err(AppError::TooManyRequests(
                    "Too many authentication attempts. Try again later.".to_string(),
                ));
            }
        }

        let entries = attempts.entry(key.to_string()).or_default();
        while entries
            .front()
            .is_some_and(|instant| now.duration_since(*instant) > WINDOW)
        {
            entries.pop_front();
        }
        if entries.len() >= MAX_ATTEMPTS {
            return Err(AppError::TooManyRequests(
                "Too many authentication attempts. Try again later.".to_string(),
            ));
        }
        entries.push_back(now);
        Ok(())
    }

    pub fn clear(&self, key: &str) {
        if let Ok(mut attempts) = self.attempts.lock() {
            attempts.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_the_burst_limit_and_can_be_cleared() {
        let limiter = AuthRateLimiter::default();
        for _ in 0..MAX_ATTEMPTS {
            limiter.check("signin:a@example.com").unwrap();
        }
        assert!(matches!(
            limiter.check("signin:a@example.com"),
            Err(AppError::TooManyRequests(_))
        ));
        limiter.clear("signin:a@example.com");
        assert!(limiter.check("signin:a@example.com").is_ok());
    }

    #[test]
    fn bounds_the_number_of_tracked_keys() {
        let limiter = AuthRateLimiter::default();
        for index in 0..MAX_TRACKED_KEYS {
            limiter
                .check(&format!("signin:{index}@example.com"))
                .unwrap();
        }
        assert!(matches!(
            limiter.check("signin:new@example.com"),
            Err(AppError::TooManyRequests(_))
        ));
    }
}
