use std::sync::Arc;
use std::time::Duration;

use chrono::Datelike;
use mail_receiver_rs::config::{Config, ScopedApiKey};
use mail_receiver_rs::smtp;
use mail_receiver_rs::storage;
use sqlx::SqlitePool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn test_config(mail_dir: &std::path::Path) -> Arc<Config> {
    let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
    storage::init_db(&db).await.unwrap();
    Arc::new(Config {
        api_keys: vec![ScopedApiKey {
            key: "test-api-key-123456789012345678".into(),
            scope: "*".into(),
        }],
        email_domains: vec!["test.example.com".into()],
        email_account_prefix: "test-".into(),
        admin_app_port: None,
        smtp_port: 0,
        mail_dir: mail_dir.to_path_buf(),
        db,
    })
}

async fn write_smtp(w: &mut (impl tokio::io::AsyncWrite + Unpin), line: &str) {
    w.write_all(line.as_bytes()).await.unwrap();
    w.write_all(b"\r\n").await.unwrap();
}

async fn read_smtp_line(r: &mut (impl tokio::io::AsyncBufRead + Unpin)) -> String {
    let mut line = String::new();
    r.read_line(&mut line).await.unwrap();
    line.trim_end().to_string()
}

async fn start_smtp_server(config: Arc<Config>) -> (tokio::task::JoinHandle<()>, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        smtp::serve_smtp(listener, config).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    (handle, port)
}

async fn start_admin_server(config: Arc<Config>) -> (tokio::task::JoinHandle<()>, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = mail_receiver_rs::admin::build_router(config);
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    (handle, port)
}

async fn send_test_email(
    smtp_port: u16,
    from: &str,
    to: &str,
    subject: &str,
    body: &str,
) -> Vec<String> {
    let stream = TcpStream::connect(format!("127.0.0.1:{smtp_port}"))
        .await
        .unwrap();
    let (r, mut w) = tokio::io::split(stream);
    let mut r = tokio::io::BufReader::new(r);
    let mut responses = Vec::new();

    // Greeting
    responses.push(read_smtp_line(&mut r).await);

    // EHLO
    write_smtp(&mut w, "EHLO test").await;
    loop {
        let line = read_smtp_line(&mut r).await;
        let has_more = line.starts_with("250-");
        responses.push(line);
        if !has_more {
            break;
        }
    }

    // MAIL FROM
    write_smtp(&mut w, &format!("MAIL FROM:<{from}>")).await;
    responses.push(read_smtp_line(&mut r).await);

    // RCPT TO
    write_smtp(&mut w, &format!("RCPT TO:<{to}>")).await;
    responses.push(read_smtp_line(&mut r).await);

    // DATA
    write_smtp(&mut w, "DATA").await;
    responses.push(read_smtp_line(&mut r).await);

    // Headers + body (include From: header so the parser can extract it)
    write_smtp(&mut w, &format!("From: {from}")).await;
    write_smtp(&mut w, &format!("Subject: {subject}")).await;
    write_smtp(&mut w, "Content-Type: text/plain; charset=utf-8").await;
    write_smtp(&mut w, "").await;
    write_smtp(&mut w, body).await;

    // End of data marker
    w.write_all(b".\r\n").await.unwrap();

    // Response
    responses.push(read_smtp_line(&mut r).await);

    // QUIT
    write_smtp(&mut w, "QUIT").await;
    responses.push(read_smtp_line(&mut r).await);

    responses
}

#[tokio::test]
async fn test_smtp_accepts_valid_recipient() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_handle, port) = start_smtp_server(config).await;

    let resp = send_test_email(
        port,
        "sender@other.com",
        "test-user@test.example.com",
        "Test",
        "Hello world",
    )
    .await;
    eprintln!("SMTP responses: {resp:?}");

    assert!(
        resp.iter().any(|l| l == "250 OK"),
        "expected 250 OK for RCPT, got: {resp:?}"
    );

    let recipient_dir = dir.path().join("test-user@test.example.com");
    assert!(recipient_dir.exists(), "recipient directory should exist");
    let entries: Vec<_> = std::fs::read_dir(&recipient_dir).unwrap().collect();
    assert!(!entries.is_empty(), "should have at least one file");
    assert!(entries.iter().any(|e| {
        e.as_ref()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".raw")
    }));
    assert!(entries.iter().any(|e| {
        e.as_ref()
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".json")
    }));
}

#[tokio::test]
async fn test_smtp_rejects_invalid_recipient() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_handle, port) = start_smtp_server(config).await;

    let resp = send_test_email(
        port,
        "sender@other.com",
        "spam@test.example.com",
        "Test",
        "Hello",
    )
    .await;

    let has_550 = resp.iter().any(|l| l.contains("550"));
    assert!(has_550, "expected 550 for invalid recipient, got: {resp:?}");

    let recipient_dir = dir.path().join("spam@test.example.com");
    assert!(
        !recipient_dir.exists(),
        "recipient directory should NOT exist for rejected email"
    );
}

#[tokio::test]
async fn test_smtp_rejects_wrong_domain() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_handle, port) = start_smtp_server(config).await;

    let resp = send_test_email(
        port,
        "sender@other.com",
        "test-user@evil.com",
        "Test",
        "Hello",
    )
    .await;

    let has_550 = resp.iter().any(|l| l.contains("550"));
    assert!(has_550, "expected 550 for wrong domain, got: {resp:?}");
}

#[tokio::test]
async fn test_smtp_wrong_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_handle, port) = start_smtp_server(config).await;

    let resp = send_test_email(
        port,
        "sender@other.com",
        "wrongprefix@test.example.com",
        "Test",
        "Hello",
    )
    .await;

    let has_550 = resp.iter().any(|l| l.contains("550"));
    assert!(has_550, "expected 550 for wrong prefix, got: {resp:?}");
}

#[tokio::test]
async fn test_smtp_rejects_data_without_recipient() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_handle, port) = start_smtp_server(config).await;

    let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let (r, mut w) = tokio::io::split(stream);
    let mut r = tokio::io::BufReader::new(r);

    read_smtp_line(&mut r).await; // greeting
    write_smtp(&mut w, "EHLO test").await;
    loop {
        let line = read_smtp_line(&mut r).await;
        if !line.starts_with("250-") {
            break;
        }
    }

    write_smtp(&mut w, "DATA").await;
    let resp = read_smtp_line(&mut r).await;
    assert!(
        resp.contains("503"),
        "expected 503 for DATA without recipient, got: {resp}"
    );

    write_smtp(&mut w, "QUIT").await;
}

#[tokio::test]
async fn test_smtp_rset_resets_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_handle, port) = start_smtp_server(config).await;

    let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let (r, mut w) = tokio::io::split(stream);
    let mut r = tokio::io::BufReader::new(r);

    read_smtp_line(&mut r).await;
    write_smtp(&mut w, "EHLO test").await;
    loop {
        let line = read_smtp_line(&mut r).await;
        if !line.starts_with("250-") {
            break;
        }
    }

    write_smtp(&mut w, "MAIL FROM:<sender@test.com>").await;
    let _ = read_smtp_line(&mut r).await;

    write_smtp(&mut w, "RSET").await;
    let resp = read_smtp_line(&mut r).await;
    assert!(resp.contains("250"), "expected 250 for RSET, got: {resp}");

    write_smtp(&mut w, "DATA").await;
    let resp = read_smtp_line(&mut r).await;
    assert!(
        resp.contains("503"),
        "expected 503 for DATA after RSET, got: {resp}"
    );

    write_smtp(&mut w, "QUIT").await;
}

#[tokio::test]
async fn test_smtp_unknown_command() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_handle, port) = start_smtp_server(config).await;

    let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let (r, mut w) = tokio::io::split(stream);
    let mut r = tokio::io::BufReader::new(r);

    read_smtp_line(&mut r).await;
    write_smtp(&mut w, "BOGUS").await;
    let resp = read_smtp_line(&mut r).await;

    assert!(
        resp.contains("500"),
        "expected 500 for unknown command, got: {resp}"
    );

    write_smtp(&mut w, "QUIT").await;
}

#[tokio::test]
async fn test_smtp_noop() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_handle, port) = start_smtp_server(config).await;

    let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let (r, mut w) = tokio::io::split(stream);
    let mut r = tokio::io::BufReader::new(r);

    read_smtp_line(&mut r).await;
    write_smtp(&mut w, "NOOP").await;
    let resp = read_smtp_line(&mut r).await;

    assert!(resp.contains("250"), "expected 250 for NOOP, got: {resp}");

    write_smtp(&mut w, "QUIT").await;
}

#[tokio::test]
async fn test_smtp_stores_raw_and_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_handle, port) = start_smtp_server(config).await;

    send_test_email(
        port,
        "alice@example.com",
        "test-myapp@test.example.com",
        "Hello World",
        "Email body",
    )
    .await;

    let recipient_dir = dir.path().join("test-myapp@test.example.com");
    assert!(recipient_dir.exists());

    let entries: Vec<_> = std::fs::read_dir(&recipient_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();

    let raw: Vec<_> = entries
        .iter()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".raw"))
        .collect();
    let json: Vec<_> = entries
        .iter()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
        .collect();

    assert_eq!(raw.len(), 1, "expected exactly one .raw file");
    assert_eq!(json.len(), 1, "expected exactly one .json file");

    let raw_content = std::fs::read_to_string(raw[0].path()).unwrap();
    assert!(raw_content.contains("Subject: Hello World"));
    assert!(raw_content.contains("Email body"));

    let json_content = std::fs::read_to_string(json[0].path()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_content).unwrap();
    let subject = parsed["subject"].as_str();
    assert_eq!(
        subject,
        Some("Hello World"),
        "expected subject field, parsed: {parsed}"
    );
}

#[tokio::test]
async fn test_admin_api_redirect() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_admin_handle, admin_port) = start_admin_server(config).await;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://127.0.0.1:{admin_port}/api/mail"))
        .query(&[("api_key", "test-api-key-123456789012345678")])
        .send()
        .await
        .unwrap();

    assert!(
        resp.status().is_redirection(),
        "expected redirect, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn test_admin_api_unauthorized() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_admin_handle, admin_port) = start_admin_server(config).await;

    let client = reqwest::Client::new();

    // No API key
    let resp = client
        .get(format!("http://127.0.0.1:{admin_port}/api/mail"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "expected 401 without API key");

    // Wrong API key
    let resp = client
        .get(format!("http://127.0.0.1:{admin_port}/api/mail"))
        .query(&[("api_key", "wrong-key")])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "expected 401 with wrong API key");

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["message"], "Access denied");
}

#[tokio::test]
async fn test_admin_api_list_week() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;

    // Start SMTP
    let smtp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let smtp_port = smtp_listener.local_addr().unwrap().port();
    let smtp_config = Arc::clone(&config);
    tokio::spawn(async move {
        smtp::serve_smtp(smtp_listener, smtp_config).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send email
    send_test_email(
        smtp_port,
        "bob@other.com",
        "test-user@test.example.com",
        "Admin test",
        "hello",
    )
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Start admin
    let (_admin_handle, admin_port) = start_admin_server(config).await;

    let now = chrono::Utc::now();
    let week = now.iso_week().week();
    let year = now.year();

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "http://127.0.0.1:{admin_port}/api/week/{year}/{week}"
        ))
        .query(&[("api_key", "test-api-key-123456789012345678")])
        .send()
        .await
        .unwrap();

    assert!(
        resp.status().is_success(),
        "expected 200, got {}",
        resp.status()
    );

    let messages: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(!messages.is_empty(), "expected at least one message");
    assert_eq!(messages[0]["subject"], "Admin test");
    assert_eq!(messages[0]["from"], "bob@other.com");
}

#[tokio::test]
async fn test_weekly_index_created() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_handle, port) = start_smtp_server(Arc::clone(&config)).await;

    send_test_email(
        port,
        "a@b.com",
        "test-user@test.example.com",
        "Index test",
        "Body",
    )
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let messages = storage::find_transactions_by_recipient(&config.db, "test-user", "test.example.com").await;
    assert!(!messages.is_empty(), "should have at least one message in db");
    assert_eq!(messages[0].subject.as_deref(), Some("Index test"));

    let weeks = storage::list_weeks(&config.db).await;
    assert!(!weeks.is_empty(), "should have at least one week");
}

#[tokio::test]
async fn test_smtp_concurrent_messages() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_handle, port) = start_smtp_server(config).await;

    let mut handles = Vec::new();
    for i in 0..5 {
        let subject = format!("Concurrent test {i}");
        let body = format!("Body of message {i}");
        handles.push(tokio::spawn(async move {
            send_test_email(
                port,
                "sender@test.com",
                "test-user@test.example.com",
                &subject,
                &body,
            )
            .await
        }));
    }

    for handle in handles {
        let resp = handle.await.unwrap();
        assert!(
            resp.iter().any(|l| l == "250 OK: Message accepted"),
            "expected message accepted, got: {resp:?}"
        );
    }

    let recipient_dir = dir.path().join("test-user@test.example.com");
    let entries: Vec<_> = std::fs::read_dir(&recipient_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();

    assert_eq!(
        entries.len(),
        10,
        "expected 10 files (5 raw + 5 json), got {}",
        entries.len()
    );
}

#[tokio::test]
async fn test_smtp_large_body() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;
    let (_handle, port) = start_smtp_server(config).await;

    let body = "A".repeat(100_000);
    let resp = send_test_email(
        port,
        "big@test.com",
        "test-user@test.example.com",
        "Large",
        &body,
    )
    .await;

    assert!(
        resp.iter().any(|l| l == "250 OK: Message accepted"),
        "expected acceptance, got: {resp:?}"
    );

    let recipient_dir = dir.path().join("test-user@test.example.com");
    let entries: Vec<_> = std::fs::read_dir(&recipient_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();

    let raw_file = entries
        .iter()
        .find(|e| e.file_name().to_string_lossy().ends_with(".raw"))
        .unwrap();
    let content = std::fs::read_to_string(raw_file.path()).unwrap();
    assert!(
        content.contains(&body),
        "raw file should contain the large body"
    );
}

#[tokio::test]
async fn test_admin_api_list_domain() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;

    let smtp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let smtp_port = smtp_listener.local_addr().unwrap().port();
    let smtp_config = Arc::clone(&config);
    tokio::spawn(async move {
        smtp::serve_smtp(smtp_listener, smtp_config).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    send_test_email(
        smtp_port,
        "alice@other.com",
        "test-user@test.example.com",
        "Domain test",
        "hello",
    )
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let (_admin_handle, admin_port) = start_admin_server(config).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "http://127.0.0.1:{admin_port}/api/domain/test.example.com/test-user"
        ))
        .query(&[("api_key", "test-api-key-123456789012345678")])
        .send()
        .await
        .unwrap();

    assert!(
        resp.status().is_success(),
        "expected 200, got {}",
        resp.status()
    );

    let messages: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(!messages.is_empty(), "expected at least one message");
    assert_eq!(messages[0]["subject"], "Domain test");
    assert_eq!(messages[0]["from"], "alice@other.com");
}

#[tokio::test]
async fn test_admin_api_get_message_by_id() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;

    let smtp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let smtp_port = smtp_listener.local_addr().unwrap().port();
    let smtp_config = Arc::clone(&config);
    tokio::spawn(async move {
        smtp::serve_smtp(smtp_listener, smtp_config).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    send_test_email(
        smtp_port,
        "carol@other.com",
        "test-user@test.example.com",
        "ByID test",
        "body",
    )
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Get message_id from the weekly listing
    let now = chrono::Utc::now();
    let week = now.iso_week().week();
    let year = now.year();
    let (_admin_handle, admin_port) = start_admin_server(config).await;

    let client = reqwest::Client::new();
    let list_resp = client
        .get(format!(
            "http://127.0.0.1:{admin_port}/api/week/{year}/{week}"
        ))
        .query(&[("api_key", "test-api-key-123456789012345678")])
        .send()
        .await
        .unwrap();

    let messages: Vec<serde_json::Value> = list_resp.json().await.unwrap();
    let message_id = messages[0]["messageId"].as_str().unwrap().to_string();

    // Now fetch by message_id
    let resp = client
        .get(format!(
            "http://127.0.0.1:{admin_port}/api/message/{message_id}"
        ))
        .query(&[("api_key", "test-api-key-123456789012345678")])
        .send()
        .await
        .unwrap();

    assert!(
        resp.status().is_success(),
        "expected 200, got {}",
        resp.status()
    );

    let msg: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(msg["subject"], "ByID test");
}

#[tokio::test]
async fn test_admin_api_get_raw_message() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path()).await;

    let smtp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let smtp_port = smtp_listener.local_addr().unwrap().port();
    let smtp_config = Arc::clone(&config);
    tokio::spawn(async move {
        smtp::serve_smtp(smtp_listener, smtp_config).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    send_test_email(
        smtp_port,
        "dave@other.com",
        "test-user@test.example.com",
        "Raw test",
        "raw body content",
    )
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let now = chrono::Utc::now();
    let week = now.iso_week().week();
    let year = now.year();
    let (_admin_handle, admin_port) = start_admin_server(config).await;

    let client = reqwest::Client::new();
    let list_resp = client
        .get(format!(
            "http://127.0.0.1:{admin_port}/api/week/{year}/{week}"
        ))
        .query(&[("api_key", "test-api-key-123456789012345678")])
        .send()
        .await
        .unwrap();

    let messages: Vec<serde_json::Value> = list_resp.json().await.unwrap();
    let message_id = messages[0]["messageId"].as_str().unwrap().to_string();

    // Fetch raw by message_id
    let resp = client
        .get(format!(
            "http://127.0.0.1:{admin_port}/api/message/{message_id}/raw"
        ))
        .query(&[("api_key", "test-api-key-123456789012345678")])
        .send()
        .await
        .unwrap();

    assert!(
        resp.status().is_success(),
        "expected 200, got {}",
        resp.status()
    );

    let raw_text = resp.text().await.unwrap();
    assert!(raw_text.contains("Subject: Raw test"));
    assert!(raw_text.contains("raw body content"));
}

#[tokio::test]
async fn test_admin_api_scoped_key_forbidden() {
    let dir = tempfile::tempdir().unwrap();
    let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
    storage::init_db(&db).await.unwrap();
    let config = Arc::new(Config {
        api_keys: vec![
            ScopedApiKey {
                key: "admin-key".into(),
                scope: "*".into(),
            },
            ScopedApiKey {
                key: "limited-key".into(),
                scope: "test.example.com".into(),
            },
        ],
        email_domains: vec!["test.example.com".into()],
        email_account_prefix: "test-".into(),
        admin_app_port: None,
        smtp_port: 0,
        mail_dir: dir.path().to_path_buf(),
        db,
    });

    let smtp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let smtp_port = smtp_listener.local_addr().unwrap().port();
    let smtp_config = Arc::clone(&config);
    tokio::spawn(async move {
        smtp::serve_smtp(smtp_listener, smtp_config).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    send_test_email(
        smtp_port,
        "eve@other.com",
        "test-user@test.example.com",
        "Scoped test",
        "hello",
    )
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let now = chrono::Utc::now();
    let week = now.iso_week().week();
    let year = now.year();
    let (_admin_handle, admin_port) = start_admin_server(config).await;

    let client = reqwest::Client::new();

    // Scoped key should have access to its domain
    let resp = client
        .get(format!(
            "http://127.0.0.1:{admin_port}/api/domain/test.example.com/test-user"
        ))
        .query(&[("api_key", "limited-key")])
        .send()
        .await
        .unwrap();

    assert!(
        resp.status().is_success(),
        "scoped key should access its domain, got {}",
        resp.status()
    );

    let messages: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(!messages.is_empty());

    // Scoped key should be forbidden from accessing a different domain
    let resp = client
        .get(format!(
            "http://127.0.0.1:{admin_port}/api/domain/other.com/test-user"
        ))
        .query(&[("api_key", "limited-key")])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 403, "expected 403 for wrong domain");

    // Wildcard key should access everything
    let resp = client
        .get(format!(
            "http://127.0.0.1:{admin_port}/api/week/{year}/{week}"
        ))
        .query(&[("api_key", "admin-key")])
        .send()
        .await
        .unwrap();

    assert!(
        resp.status().is_success(),
        "wildcard key should access all, got {}",
        resp.status()
    );
}
