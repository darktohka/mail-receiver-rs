use std::path::PathBuf;

use anyhow::{Context, Result};

fn var(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("{key} must be set"))
}

#[derive(Clone, Debug)]
pub struct Config {
    pub api_key: String,
    pub email_domains: Vec<String>,
    pub email_account_prefix: String,
    pub admin_app_port: Option<u16>,
    pub smtp_port: u16,
    pub mail_dir: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let api_key = var("API_KEY")?;
        if api_key.len() < 20 {
            anyhow::bail!("API_KEY must be at least 20 characters long");
        }

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

        Ok(Config {
            api_key,
            email_domains,
            email_account_prefix,
            admin_app_port,
            smtp_port,
            mail_dir: PathBuf::from("mail"),
        })
    }

    pub fn is_valid_recipient(&self, address: &str) -> bool {
        let lower = address.to_lowercase();
        lower.starts_with(&self.email_account_prefix.to_lowercase())
            && (self.email_domains.is_empty()
                || self.email_domains.iter().any(|d| lower.ends_with(d)))
    }
}
