use std::path::Path;

use chrono::Datelike;
use sqlx::Row;
use sqlx::SqlitePool;
use tracing::{info, warn};

use crate::types::{ParsedMail, RecipientInfo, TransactionSummary};

pub async fn init_db(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS messages (
            message_id TEXT NOT NULL PRIMARY KEY,
            processed_at TEXT NOT NULL,
            week_number INTEGER NOT NULL,
            year_number INTEGER NOT NULL,
            sender TEXT,
            subject TEXT,
            filename TEXT NOT NULL,
            recipient_name TEXT NOT NULL,
            recipient_domain TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_messages_recipient
         ON messages(recipient_name, recipient_domain)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_messages_week
         ON messages(year_number, week_number)",
    )
    .execute(pool)
    .await?;

    info!("Database initialized");
    Ok(())
}

pub async fn insert_message(pool: &SqlitePool, summary: &TransactionSummary) -> anyhow::Result<()> {
    let processed_at = chrono::DateTime::parse_from_rfc3339(&summary.processed_at)
        .unwrap_or_else(|_| chrono::Utc::now().into());
    let utc = processed_at.with_timezone(&chrono::Utc);
    let week = utc.iso_week().week() as i32;
    let year = utc.year();

    sqlx::query(
        "INSERT INTO messages (message_id, processed_at, week_number, year_number, sender, subject, filename, recipient_name, recipient_domain)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&summary.message_id)
    .bind(&summary.processed_at)
    .bind(week)
    .bind(year)
    .bind(&summary.from)
    .bind(&summary.subject)
    .bind(&summary.filename)
    .bind(&summary.recipient_name)
    .bind(&summary.recipient_domain)
    .execute(pool)
    .await?;

    info!("Message {} inserted into database", summary.message_id);
    Ok(())
}

pub async fn find_transactions_by_recipient(
    pool: &SqlitePool,
    username: &str,
    domain: &str,
) -> Vec<TransactionSummary> {
    let rows = sqlx::query(
        "SELECT message_id, processed_at, sender, subject, filename, recipient_name, recipient_domain
         FROM messages
         WHERE recipient_name = ? AND recipient_domain = ?
         ORDER BY processed_at DESC",
    )
    .bind(username)
    .bind(domain)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.into_iter()
        .map(|row| TransactionSummary {
            message_id: row.get("message_id"),
            processed_at: row.get("processed_at"),
            from: row.get("sender"),
            subject: row.get("subject"),
            filename: row.get("filename"),
            recipient_name: row.get("recipient_name"),
            recipient_domain: row.get("recipient_domain"),
        })
        .collect()
}

pub async fn find_transaction_by_id(
    pool: &SqlitePool,
    message_id: &str,
) -> Option<TransactionSummary> {
    let row = sqlx::query(
        "SELECT message_id, processed_at, sender, subject, filename, recipient_name, recipient_domain
         FROM messages
         WHERE message_id = ?",
    )
    .bind(message_id)
    .fetch_optional(pool)
    .await
    .ok()??;

    Some(TransactionSummary {
        message_id: row.get("message_id"),
        processed_at: row.get("processed_at"),
        from: row.get("sender"),
        subject: row.get("subject"),
        filename: row.get("filename"),
        recipient_name: row.get("recipient_name"),
        recipient_domain: row.get("recipient_domain"),
    })
}

pub async fn find_messages_by_week(
    pool: &SqlitePool,
    year: i32,
    week: i32,
) -> Vec<TransactionSummary> {
    let rows = sqlx::query(
        "SELECT message_id, processed_at, sender, subject, filename, recipient_name, recipient_domain
         FROM messages
         WHERE year_number = ? AND week_number = ?
         ORDER BY processed_at DESC",
    )
    .bind(year)
    .bind(week)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.into_iter()
        .map(|row| TransactionSummary {
            message_id: row.get("message_id"),
            processed_at: row.get("processed_at"),
            from: row.get("sender"),
            subject: row.get("subject"),
            filename: row.get("filename"),
            recipient_name: row.get("recipient_name"),
            recipient_domain: row.get("recipient_domain"),
        })
        .collect()
}

pub async fn find_all_recipients(pool: &SqlitePool) -> Vec<RecipientInfo> {
    let rows = sqlx::query(
        "SELECT recipient_name, recipient_domain, COUNT(*) as cnt
         FROM messages
         GROUP BY recipient_name, recipient_domain
         ORDER BY recipient_name, recipient_domain",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.into_iter()
        .map(|row| {
            let name: String = row.get("recipient_name");
            let domain: String = row.get("recipient_domain");
            let count: i64 = row.get("cnt");
            RecipientInfo {
                email: format!("{name}@{domain}"),
                name,
                domain,
                message_count: count as u32,
            }
        })
        .collect()
}

pub async fn list_weeks(pool: &SqlitePool) -> Vec<String> {
    let rows = sqlx::query(
        "SELECT DISTINCT year_number, week_number
         FROM messages
         ORDER BY year_number DESC, week_number DESC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.into_iter()
        .map(|row| {
            let year: i32 = row.get("year_number");
            let week: i32 = row.get("week_number");
            format!("w{week}-{year}")
        })
        .collect()
}

/// File-based helpers (raw message storage remains on disk)
fn sanitize(name: &str) -> String {
    name.replace(['/', '\0'], "-")
}

pub fn recipient_dir(mail_dir: &Path, email: &str) -> std::path::PathBuf {
    mail_dir.join(sanitize(email))
}

pub async fn ensure_recipient_dir(path: &Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(path).await
}

pub async fn write_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    tokio::fs::write(path, data).await
}

pub async fn write_json(path: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    let body = serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".into());
    tokio::fs::write(path, body.as_bytes()).await
}

pub async fn load_message(
    mail_dir: &Path,
    domain: &str,
    username: &str,
    filename: &str,
) -> Option<serde_json::Value> {
    let folder = sanitize(&format!("{username}@{domain}"));
    let path = mail_dir.join(folder).join(filename);
    match tokio::fs::read_to_string(&path).await {
        Ok(data) => serde_json::from_str(&data).ok(),
        Err(_) => None,
    }
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
    tokio::fs::read(&path).await.ok()
}

pub async fn load_attachment_bytes(
    mail_dir: &Path,
    domain: &str,
    username: &str,
    filename: &str,
    attachment_index: u32,
) -> Option<(Vec<u8>, String)> {
    use mail_parser::{MimeHeaders, MessageParser};
    let raw_filename = filename.replace(".json", ".raw");
    let folder = sanitize(&format!("{username}@{domain}"));
    let path = mail_dir.join(folder).join(raw_filename);
    let raw_bytes = tokio::fs::read(&path).await.ok()?;
    let msg = MessageParser::default().parse(&raw_bytes)?;
    let part = msg.attachment(attachment_index)?;
    let content_type = part
        .content_type()
        .map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")))
        .unwrap_or_else(|| "application/octet-stream".to_string());
    Some((part.contents().to_vec(), content_type))
}

/// Rebuild the database by scanning all recipient directories on disk.
/// Wipes the existing database and inserts every message in a single transaction.
pub async fn rescan_database(pool: &SqlitePool, mail_dir: &Path) -> anyhow::Result<()> {
    info!("Rescanning mail directory: {}", mail_dir.display());

    let mut entries = match tokio::fs::read_dir(mail_dir).await {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!("Mail directory does not exist, nothing to rescan");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    let mut summaries = Vec::new();

    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.contains('@')
            || !entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false)
        {
            continue;
        }

        let (recipient_name, recipient_domain) = match name.split_once('@') {
            Some(pair) => (pair.0.to_string(), pair.1.to_string()),
            None => continue,
        };

        let mut dir_entries = match tokio::fs::read_dir(entry.path()).await {
            Ok(d) => d,
            Err(_) => continue,
        };

        while let Ok(Some(file_entry)) = dir_entries.next_entry().await {
            let fname = file_entry.file_name().to_string_lossy().to_string();
            if !fname.ends_with(".json") {
                continue;
            }

            let stem = match fname.strip_suffix(".json") {
                Some(s) => s,
                None => continue,
            };

            let (processed_at, message_id) = match parse_message_filename(stem) {
                Ok(pair) => pair,
                Err(e) => {
                    warn!("Skipping {}: {e}", fname);
                    continue;
                }
            };

            let json_content = match tokio::fs::read_to_string(file_entry.path()).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let parsed: ParsedMail = match serde_json::from_str(&json_content) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let from = parsed
                .from
                .as_ref()
                .and_then(|a| a.value.first())
                .map(|a| a.address.clone())
                .filter(|s| !s.is_empty());

            summaries.push(TransactionSummary {
                message_id,
                processed_at,
                from,
                subject: parsed.subject,
                filename: fname,
                recipient_name: recipient_name.clone(),
                recipient_domain: recipient_domain.clone(),
            });
        }
    }

    // Wipe and re-insert in a single transaction
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM messages").execute(&mut *tx).await?;

    for summary in &summaries {
        let processed_at = chrono::DateTime::parse_from_rfc3339(&summary.processed_at)
            .unwrap_or_else(|_| chrono::Utc::now().into());
        let utc = processed_at.with_timezone(&chrono::Utc);
        let week = utc.iso_week().week() as i32;
        let year = utc.year();

        sqlx::query(
            "INSERT INTO messages (message_id, processed_at, week_number, year_number, sender, subject, filename, recipient_name, recipient_domain)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&summary.message_id)
        .bind(&summary.processed_at)
        .bind(week)
        .bind(year)
        .bind(&summary.from)
        .bind(&summary.subject)
        .bind(&summary.filename)
        .bind(&summary.recipient_name)
        .bind(&summary.recipient_domain)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    info!("Rescan complete: {} messages indexed", summaries.len());
    Ok(())
}

/// Parse a filename stem like `{processed_at}-{uuid}` into (processed_at, message_id).
/// The UUID is the trailing 36-character segment (8-4-4-4-12 hex digits).
fn parse_message_filename(stem: &str) -> anyhow::Result<(String, String)> {
    if stem.len() < 37 {
        anyhow::bail!("filename stem too short: {stem}");
    }
    let uuid = &stem[stem.len() - 36..];
    let chars: Vec<char> = uuid.chars().collect();
    let valid = chars.len() == 36
        && chars[8] == '-'
        && chars[13] == '-'
        && chars[18] == '-'
        && chars[23] == '-'
        && chars.iter().enumerate().all(|(i, c)| {
            matches!(i, 8 | 13 | 18 | 23) || c.is_ascii_hexdigit()
        });
    if !valid {
        anyhow::bail!("invalid UUID in filename: {stem}");
    }
    let processed_at = stem[..stem.len() - 37].to_string();
    Ok((processed_at, uuid.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_parsed_json(subject: &str, from_addr: &str) -> serde_json::Value {
        serde_json::json!({
            "attachments": [],
            "headers": {},
            "headerLines": [],
            "html": null,
            "text": null,
            "textAsHtml": null,
            "subject": subject,
            "date": null,
            "to": null,
            "from": {
                "value": [{"address": from_addr, "name": ""}],
                "text": from_addr
            },
            "cc": null,
            "bcc": null,
            "replyTo": null,
            "messageId": null,
            "inReplyTo": null,
            "references": null
        })
    }

    #[tokio::test]
    async fn test_rescan_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        init_db(&pool).await.unwrap();

        rescan_database(&pool, dir.path()).await.unwrap();

        let rows = sqlx::query("SELECT COUNT(*) as cnt FROM messages")
            .fetch_one(&pool)
            .await
            .unwrap();
        let count: i64 = rows.get("cnt");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_rescan_nonexistent_directory() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        init_db(&pool).await.unwrap();

        rescan_database(&pool, Path::new("/tmp/opencode/nonexistent-rescan-test-dir")).await.unwrap();

        let rows = sqlx::query("SELECT COUNT(*) as cnt FROM messages")
            .fetch_one(&pool)
            .await
            .unwrap();
        let count: i64 = rows.get("cnt");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_rescan_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let mail_dir = dir.path();

        // Recipient: user1@example.com – two messages
        let r1 = mail_dir.join("user1@example.com");
        tokio::fs::create_dir_all(&r1).await.unwrap();

        let json1 = make_parsed_json("Hello", "alice@other.com");
        let f1 = "2026-01-15T10:30:00Z-11111111-1111-4111-8111-111111111111.json";
        tokio::fs::write(r1.join(f1), serde_json::to_string_pretty(&json1).unwrap())
            .await
            .unwrap();
        // Raw file should exist alongside json but rescan ignores it
        tokio::fs::write(r1.join(f1.replace(".json", ".raw")), b"raw content")
            .await
            .unwrap();

        let json2 = make_parsed_json("Re: Hello", "bob@other.com");
        let f2 = "2026-01-15T11:00:00Z-22222222-2222-4222-8222-222222222222.json";
        tokio::fs::write(r1.join(f2), serde_json::to_string_pretty(&json2).unwrap())
            .await
            .unwrap();

        // Recipient: user2@example.com – one message
        let r2 = mail_dir.join("user2@example.com");
        tokio::fs::create_dir_all(&r2).await.unwrap();

        let json3 = make_parsed_json("Test", "carol@other.net");
        let f3 = "2026-01-16T12:00:00Z-33333333-3333-4333-8333-333333333333.json";
        tokio::fs::write(r2.join(f3), serde_json::to_string_pretty(&json3).unwrap())
            .await
            .unwrap();

        // Edge-case files that should be ignored
        // .err file
        tokio::fs::write(r1.join("some-file.err"), b"{}").await.unwrap();
        // Invalid UUID in filename
        tokio::fs::write(
            r1.join("2026-01-15T10:30:00Z-not-a-uuid.json"),
            b"{}",
        )
        .await
        .unwrap();
        // Directory without @ in name
        tokio::fs::create_dir_all(mail_dir.join(".hidden")).await.unwrap();
        tokio::fs::write(mail_dir.join(".hidden").join("msg.json"), b"{}")
            .await
            .unwrap();
        // File directly in mail root (no recipient dir)
        tokio::fs::write(mail_dir.join("stray.json"), b"{}").await.unwrap();

        // Pre-populate db with stale entries (rescan should wipe them)
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        init_db(&pool).await.unwrap();
        sqlx::query("INSERT INTO messages (message_id, processed_at, week_number, year_number, sender, subject, filename, recipient_name, recipient_domain) VALUES ('stale-id', 'ignored', 0, 0, NULL, NULL, 'x.json', 'ghost', 'void')")
            .execute(&pool)
            .await
            .unwrap();

        rescan_database(&pool, mail_dir).await.unwrap();

        // Verify all valid messages were indexed
        let rows = sqlx::query("SELECT message_id, recipient_name, recipient_domain, sender, subject FROM messages ORDER BY processed_at")
            .fetch_all(&pool)
            .await
            .unwrap();

        assert_eq!(rows.len(), 3, "expected 3 indexed messages");

        // Message 1
        assert_eq!(rows[0].get::<String, _>("message_id"), "11111111-1111-4111-8111-111111111111");
        assert_eq!(rows[0].get::<String, _>("recipient_name"), "user1");
        assert_eq!(rows[0].get::<String, _>("recipient_domain"), "example.com");
        assert_eq!(rows[0].get::<Option<String>, _>("sender").unwrap(), "alice@other.com");
        assert_eq!(rows[0].get::<Option<String>, _>("subject").unwrap(), "Hello");

        // Message 2
        assert_eq!(rows[1].get::<String, _>("message_id"), "22222222-2222-4222-8222-222222222222");
        assert_eq!(rows[1].get::<Option<String>, _>("sender").unwrap(), "bob@other.com");
        assert_eq!(rows[1].get::<Option<String>, _>("subject").unwrap(), "Re: Hello");

        // Message 3
        assert_eq!(rows[2].get::<String, _>("recipient_name"), "user2");
        assert_eq!(rows[2].get::<String, _>("recipient_domain"), "example.com");
        assert_eq!(rows[2].get::<Option<String>, _>("sender").unwrap(), "carol@other.net");
        assert_eq!(rows[2].get::<Option<String>, _>("subject").unwrap(), "Test");

        // Stale entry was wiped
        let found = sqlx::query("SELECT COUNT(*) as cnt FROM messages WHERE message_id = 'stale-id'")
            .fetch_one(&pool)
            .await
            .unwrap();
        let stale: i64 = found.get("cnt");
        assert_eq!(stale, 0, "stale entry should have been wiped");
    }
}
