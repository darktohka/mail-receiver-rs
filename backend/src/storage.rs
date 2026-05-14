use std::path::Path;

use chrono::{Datelike, Utc};
use tokio::fs;
use tracing::info;

use crate::types::{RecipientInfo, TransactionSummary, WeeklyIndex};

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

pub async fn find_transactions_by_recipient(
    mail_dir: &Path,
    username: &str,
    domain: &str,
) -> Vec<TransactionSummary> {
    let mut result = Vec::new();
    let entries = weekly_index_files(mail_dir).await;
    for path in entries {
        let index = load_weekly_index(&path).await;
        for msg in index.messages {
            if let Some((u, d)) = parse_recipient_from_path(&msg.recipient_folder_path) {
                if u == username && d == domain {
                    result.push(msg);
                }
            }
        }
    }
    result
}

pub async fn find_transaction_by_id(
    mail_dir: &Path,
    message_id: &str,
) -> Option<TransactionSummary> {
    let entries = weekly_index_files(mail_dir).await;
    for path in entries {
        let index = load_weekly_index(&path).await;
        for msg in index.messages {
            if msg.message_id == message_id {
                return Some(msg);
            }
        }
    }
    None
}

pub fn parse_recipient_from_path(recipient_folder_path: &str) -> Option<(String, String)> {
    let path = recipient_folder_path
        .strip_suffix('/')
        .unwrap_or(recipient_folder_path);
    let at_pos = path.rfind('@')?;
    let slash_pos = path[..at_pos].rfind('/')?;
    let username = path[slash_pos + 1..at_pos].to_string();
    let domain = path[at_pos + 1..].to_string();
    Some((username, domain))
}

pub async fn load_raw_message(
    mail_dir: &Path,
    domain: &str,
    username: &str,
    filename: &str,
) -> Option<Vec<u8>> {
    let raw_filename = filename.replace(".json", ".raw");
    let folder = sanitize(&format!("{username}@{domain}"));
    let path = mail_dir.join(folder).join(raw_filename);
    fs::read(&path).await.ok()
}

async fn weekly_index_files(mail_dir: &Path) -> Vec<std::path::PathBuf> {
    let mut entries = match fs::read_dir(mail_dir).await {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('w') && name.ends_with(".json") {
            files.push(entry.path());
        }
    }
    files
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

pub async fn load_attachment_bytes(
    mail_dir: &Path,
    domain: &str,
    username: &str,
    filename: &str,
    attachment_index: u32,
) -> Option<Vec<u8>> {
    use mail_parser::MessageParser;
    let raw_filename = filename.replace(".json", ".raw");
    let folder = sanitize(&format!("{username}@{domain}"));
    let path = mail_dir.join(folder).join(raw_filename);
    let raw_bytes = fs::read(&path).await.ok()?;
    let msg = MessageParser::default().parse(&raw_bytes)?;
    let part = msg.attachment(attachment_index)?;
    Some(part.contents().to_vec())
}

pub async fn find_all_recipients(mail_dir: &Path) -> Vec<RecipientInfo> {
    let mut entries = match fs::read_dir(mail_dir).await {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut recipients = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains('@') && entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
            let at_pos = name.find('@').unwrap();
            let username = name[..at_pos].to_string();
            let domain = name[at_pos + 1..].to_string();
            let count = count_json_files(entry.path()).await;
            recipients.push(RecipientInfo {
                domain,
                name: username,
                email: name,
                message_count: count as u32,
            });
        }
    }
    recipients.sort_by(|a, b| a.email.cmp(&b.email));
    recipients
}

async fn count_json_files(dir: std::path::PathBuf) -> usize {
    let mut entries = match fs::read_dir(&dir).await {
        Ok(d) => d,
        Err(_) => return 0,
    };
    let mut count = 0;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "json") {
            count += 1;
        }
    }
    count
}

pub async fn list_weekly_index_names(mail_dir: &Path) -> Vec<String> {
    let mut files = weekly_index_files(mail_dir).await;
    files.sort();
    let mut names = Vec::new();
    for path in files {
        if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
            names.push(name.to_string());
        }
    }
    names
}
