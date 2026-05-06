use std::path::Path;

use chrono::{Datelike, Utc};
use tokio::fs;
use tracing::info;

use crate::types::{TransactionSummary, WeeklyIndex};

fn sanitize(name: &str) -> String {
    name.replace(['/', '\0'], "-")
}

pub fn recipient_dir(mail_dir: &Path, email: &str) -> std::path::PathBuf {
    mail_dir.join(sanitize(email))
}

pub async fn ensure_recipient_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path).await
}

pub async fn write_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    fs::write(path, data).await
}

pub async fn write_json(path: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    let body = serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".into());
    fs::write(path, body.as_bytes()).await
}

pub async fn update_or_create_weekly_index(
    mail_dir: &Path,
    summary: &TransactionSummary,
) -> anyhow::Result<()> {
    let processed_at = chrono::DateTime::parse_from_rfc3339(&summary.processed_at)
        .unwrap_or_else(|_| Utc::now().into());
    let utc = processed_at.with_timezone(&Utc);
    let week = utc.iso_week().week();
    let year = utc.year();
    let index_name = format!("w{week}-{year}");
    let index_path = mail_dir.join(format!("{index_name}.json"));

    let mut index = load_weekly_index(&index_path).await;
    index.name.clone_from(&index_name);
    index.messages.insert(0, summary.clone());

    let content = serde_json::to_string_pretty(&index)?;
    fs::write(&index_path, content.as_bytes()).await?;
    info!("Updated weekly index: {}", index_path.display());
    Ok(())
}

async fn load_weekly_index(path: &Path) -> WeeklyIndex {
    match fs::read_to_string(path).await {
        Ok(data) => serde_json::from_str(&data).unwrap_or(WeeklyIndex {
            name: String::new(),
            messages: Vec::new(),
        }),
        Err(_) => WeeklyIndex {
            name: String::new(),
            messages: Vec::new(),
        },
    }
}

pub async fn load_weekly_index_by_name(mail_dir: &Path, index_name: &str) -> WeeklyIndex {
    load_weekly_index(&mail_dir.join(format!("{index_name}.json"))).await
}

pub async fn load_message(
    mail_dir: &Path,
    domain: &str,
    username: &str,
    filename: &str,
) -> Option<serde_json::Value> {
    let folder = sanitize(&format!("{username}@{domain}"));
    let path = mail_dir.join(folder).join(filename);
    match fs::read_to_string(&path).await {
        Ok(data) => serde_json::from_str(&data).ok(),
        Err(_) => None,
    }
}
