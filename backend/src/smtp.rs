use std::sync::Arc;

use chrono::Utc;
use mail_parser::MessageParser;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::storage;
use crate::types::TransactionSummary;

pub async fn run_smtp_server(config: Arc<Config>) -> anyhow::Result<()> {
    let addr = format!("0.0.0.0:{}", config.smtp_port);
    let listener = TcpListener::bind(&addr).await?;
    info!("SMTP server listening on {addr}");
    serve_smtp(listener, config).await
}

pub async fn serve_smtp(listener: TcpListener, config: Arc<Config>) -> anyhow::Result<()> {
    loop {
        let (stream, peer) = listener.accept().await?;
        let config = Arc::clone(&config);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, config).await {
                error!("Connection from {peer} failed: {e}");
            }
        });
    }
}

async fn handle_connection(stream: tokio::net::TcpStream, config: Arc<Config>) -> anyhow::Result<()> {
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);

    write_half.write_all(b"220 localhost ESMTP mail-receiver-rs\r\n").await?;

    let mut line = String::with_capacity(512);
    let mut mail_from = String::new();
    let mut rcpt_to: Vec<String> = Vec::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }

        let trimmed = line.trim();
        let upper = trimmed.to_uppercase();

        if upper.starts_with("EHLO ") || upper.starts_with("HELO ") {
            write_half.write_all(b"250-localhost\r\n").await?;
            write_half.write_all(b"250-8BITMIME\r\n").await?;
            write_half.write_all(b"250-SMTPUTF8\r\n").await?;
            write_half.write_all(b"250 PIPELINING\r\n").await?;
        } else if upper.starts_with("MAIL FROM:") {
            mail_from = extract_address_upper(trimmed, &upper, "MAIL FROM:").unwrap_or_default();
            if mail_from.is_empty() {
                write_half.write_all(b"501 Syntax error in parameters\r\n").await?;
            } else {
                write_half.write_all(b"250 OK\r\n").await?;
            }
        } else if upper.starts_with("RCPT TO:") {
            let addr = extract_address_upper(trimmed, &upper, "RCPT TO:").unwrap_or_default();
            if addr.is_empty() {
                write_half.write_all(b"501 Syntax error in parameters\r\n").await?;
            } else if config.is_valid_recipient(&addr) {
                info!("Email to \"{addr}\" accepted");
                rcpt_to.push(addr);
                write_half.write_all(b"250 OK\r\n").await?;
            } else {
                info!("Email to \"{addr}\" refused");
                write_half.write_all(b"550 No thank you\r\n").await?;
            }
        } else if upper == "DATA" {
            if mail_from.is_empty() || rcpt_to.is_empty() {
                write_half.write_all(b"503 Bad sequence of commands\r\n").await?;
            } else {
                let result = handle_data(&mut reader, &mut write_half, &config, &rcpt_to).await;
                mail_from.clear();
                rcpt_to.clear();

                match result {
                    Ok(()) => write_half.write_all(b"250 OK: Message accepted\r\n").await?,
                    Err(e) => {
                        error!("DATA processing failed: {e}");
                        let _ = write_half.write_all(b"451 Requested action aborted: local error\r\n").await;
                    }
                }
            }
        } else if upper == "QUIT" {
            write_half.write_all(b"221 Bye\r\n").await?;
            break;
        } else if upper == "RSET" {
            mail_from.clear();
            rcpt_to.clear();
            write_half.write_all(b"250 OK\r\n").await?;
        } else if upper == "NOOP" {
            write_half.write_all(b"250 OK\r\n").await?;
        } else {
            write_half.write_all(b"500 Command not recognized\r\n").await?;
        }
    }

    Ok(())
}

fn extract_address_upper<'a>(line: &'a str, upper: &'a str, prefix: &str) -> Option<String> {
    let rest = upper.strip_prefix(prefix)?.trim();
    // Use the same index range from the original case line
    let original_rest = &line[line.len() - rest.len()..].trim();
    if original_rest.starts_with('<') {
        let end = original_rest.find('>')?;
        Some(original_rest[1..end].to_string())
    } else {
        Some(original_rest.to_string())
    }
}

async fn handle_data<R, W>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    config: &Config,
    rcpt_to: &[String],
) -> anyhow::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    writer.write_all(b"354 End data with <CRLF>.<CRLF>\r\n").await?;

    let raw = read_raw_email(reader).await?;

    let now = Utc::now();
    let message_id = uuid::Uuid::new_v4().to_string();
    let processed_at = now.to_rfc3339();
    let label = format!("{processed_at}-{message_id}");

    let recipient = &rcpt_to[0];
    let dir = storage::recipient_dir(&config.mail_dir, recipient);
    storage::ensure_recipient_dir(&dir).await?;

    let raw_path = dir.join(format!("{label}.raw"));
    storage::write_file(&raw_path, &raw).await?;

    let (recipient_name, recipient_domain) = recipient
        .split_once('@')
        .unwrap_or((recipient, ""));
    let mut summary = TransactionSummary {
        message_id,
        processed_at,
        from: None,
        subject: None,
        filename: String::new(),
        recipient_name: recipient_name.to_string(),
        recipient_domain: recipient_domain.to_string(),
    };

    match MessageParser::default().parse(&raw) {
        Some(parsed) => {
            summary.from = parsed.from()
                .and_then(|a| a.first())
                .and_then(|a| a.address())
                .map(|s| s.to_string());
            summary.subject = parsed.subject().map(|s| s.to_string());
            summary.filename = format!("{label}.json");

            let json_path = dir.join(&summary.filename);
            let value = serde_json::to_value(&crate::types::ParsedMail::from(&parsed)).unwrap_or_default();
            storage::write_json(&json_path, &value).await?;

            info!("Email parsed and saved. Inserting into database...");
            storage::insert_message(&config.db, &summary).await?;
        }
        None => {
            let err_path = dir.join(format!("{label}.err"));
            let err_val = serde_json::json!({"error": "Failed to parse email"});
            storage::write_json(&err_path, &err_val).await?;
            warn!("Failed to parse email");
        }
    }

    info!("Done processing email");
    Ok(())
}

async fn read_raw_email<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> std::io::Result<Vec<u8>> {
    let mut raw = Vec::with_capacity(8192);
    let mut buf = Vec::with_capacity(1024);

    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf).await?;
        if n == 0 {
            break;
        }

        // Check for DATA terminator: ".\r\n" or ".\n"
        if (buf.len() == 2 && buf[0] == b'.' && buf[1] == b'\n')
            || (buf.len() == 3 && buf[0] == b'.' && buf[1] == b'\r' && buf[2] == b'\n')
        {
            break;
        }

        // Remove SMTP dot-stuffing: any line starting with '.' has the leading dot removed.
        if !buf.is_empty() && buf[0] == b'.' {
            raw.extend_from_slice(&buf[1..]);
        } else {
            raw.extend_from_slice(&buf);
        }
    }

    Ok(raw)
}
