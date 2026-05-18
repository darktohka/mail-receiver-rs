use std::path::PathBuf;

use anyhow::{Context, Result};
use sqlx::SqlitePool;

fn var(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("{key} must be set"))
}

#[derive(Clone, Debug)]
pub struct ScopedApiKey {
    pub key: String,
    pub scope: String,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub api_keys: Vec<ScopedApiKey>,
    pub email_domains: Vec<String>,
    pub email_account_prefix: String,
    pub admin_app_port: Option<u16>,
    pub smtp_port: u16,
    pub mail_dir: PathBuf,
    pub db: SqlitePool,
}

fn parse_api_keys(input: &str) -> Vec<ScopedApiKey> {
    input
        .split(',')
        .filter_map(|s| {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            let colon = s.rfind(':')?;
            let key = s[..colon].to_string();
            let scope = s[colon + 1..].to_string();
            if key.is_empty() || scope.is_empty() {
                return None;
            }
            Some(ScopedApiKey { key, scope })
        })
        .collect()
}

impl Config {
    pub async fn from_env() -> Result<Self> {
        let api_keys = if let Ok(keys_str) = std::env::var("API_KEYS") {
            let keys = parse_api_keys(&keys_str);
            if keys.is_empty() {
                anyhow::bail!("API_KEYS is set but no valid keys found (format: key1:*,key2:domain.com)");
            }
            keys
        } else {
            let key = var("API_KEY")?;
            if key.len() < 8 {
                anyhow::bail!("API_KEY must be at least 8 characters long");
            }
            vec![ScopedApiKey {
                key,
                scope: "*".to_string(),
            }]
        };

        let email_domains: Vec<String> = var("EMAIL_DOMAIN")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        let email_account_prefix = var("EMAIL_ACCOUNT_PREFIX").unwrap_or_default();
        let admin_app_port = var("ADMIN_APP_PORT").ok().and_then(|p| p.parse().ok());
        let smtp_port = var("SMTP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(25);

        let mail_dir = PathBuf::from("mail");
        let db_url = format!("sqlite:{}/mail.db", mail_dir.display());
        let db = SqlitePool::connect(&db_url).await?;

        Ok(Config {
            api_keys,
            email_domains,
            email_account_prefix,
            admin_app_port,
            smtp_port,
            mail_dir,
            db,
        })
    }

    pub fn is_valid_recipient(&self, address: &str) -> bool {
        let lower = address.to_lowercase();
        lower.starts_with(&self.email_account_prefix.to_lowercase())
            && (self.email_domains.is_empty()
                || self.email_domains.iter().any(|d| lower.ends_with(d)))
    }
}
