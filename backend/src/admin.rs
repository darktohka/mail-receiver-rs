use std::sync::Arc;

use axum::extract::{Path, Query, RawQuery, State};
use axum::response::Redirect;
use axum::routing::get;
use axum::{Json, Router, http::StatusCode};
use chrono::{Datelike, Utc};
use serde::Deserialize;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::config::Config;
use crate::storage;

#[derive(Deserialize)]
pub struct ApiKeyParams {
    pub api_key: Option<String>,
}

#[derive(Clone, Debug)]
pub enum KeyScope {
    All,
    Domain(String),
}

impl KeyScope {
    fn matches_domain(&self, domain: &str) -> bool {
        match self {
            KeyScope::All => true,
            KeyScope::Domain(d) => d == domain,
        }
    }
}

fn check_key(params: &ApiKeyParams, config: &Config) -> ApiResult<KeyScope> {
    match &params.api_key {
        Some(key) => {
            for ak in &config.api_keys {
                if key == &ak.key {
                    return Ok(match ak.scope.as_str() {
                        "*" => KeyScope::All,
                        domain => KeyScope::Domain(domain.to_string()),
                    });
                }
            }
            Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"message": "Access denied"})),
            ))
        }
        None => Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"message": "Access denied"})),
        )),
    }
}

fn forbidden() -> ApiError {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"message": "Forbidden"})),
    )
}

fn not_found() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"message": "Not Found"})),
    )
}

#[derive(Deserialize)]
pub struct YearWeek {
    year: u32,
    week: u32,
}

#[derive(Deserialize)]
pub struct DomainName {
    domain: String,
    name: String,
}

#[derive(Deserialize)]
pub struct MessagePath {
    domain: String,
    username: String,
    message_filename: String,
}

#[derive(Deserialize)]
pub struct MessageIdPath {
    message_id: String,
}

type ApiError = (StatusCode, Json<serde_json::Value>);
type ApiResult<T> = Result<T, ApiError>;

async fn redirect_current_week(
    State(config): State<Arc<Config>>,
    Query(params): Query<ApiKeyParams>,
    RawQuery(query): RawQuery,
) -> ApiResult<Redirect> {
    check_key(&params, &config)?;
    let now = Utc::now();
    let week = now.iso_week().week();
    let year = now.year();
    let mut url = format!("/api/week/{year}/{week}");
    if let Some(query) = query {
        url.push('?');
        url.push_str(&query);
    }
    Ok(Redirect::temporary(&url))
}

async fn list_week_mail(
    State(config): State<Arc<Config>>,
    Query(params): Query<ApiKeyParams>,
    Path(YearWeek { year, week }): Path<YearWeek>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    let scope = check_key(&params, &config)?;
    let index_name = format!("w{week}-{year}");
    let index = storage::load_weekly_index_by_name(&config.mail_dir, &index_name).await;

    let messages: Vec<serde_json::Value> = index
        .messages
        .iter()
        .filter(|msg| {
            storage::parse_recipient_from_path(&msg.recipient_folder_path)
                .map_or(false, |(_, domain)| scope.matches_domain(&domain))
        })
        .map(|msg| {
            let (username, domain) =
                storage::parse_recipient_from_path(&msg.recipient_folder_path).unwrap_or_default();

            serde_json::json!({
                "messageId": msg.message_id,
                "processedAt": msg.processed_at,
                "from": msg.from,
                "subject": msg.subject,
                "filename": msg.filename,
                "recipient": format!("{username}@{domain}"),
                "message": {
                "href": format!("/api/domain/{}/{}",
                    percent_encode(&domain),
                    percent_encode(&username),
                )
                }
            })
        })
        .collect();

    Ok(Json(messages))
}

async fn list_domain_mail(
    State(config): State<Arc<Config>>,
    Query(params): Query<ApiKeyParams>,
    Path(DomainName { domain, name }): Path<DomainName>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    let scope = check_key(&params, &config)?;
    if !scope.matches_domain(&domain) {
        return Err(forbidden());
    }

    let messages = storage::find_transactions_by_recipient(&config.mail_dir, &name, &domain).await;

    let result: Vec<serde_json::Value> = messages
        .iter()
        .map(|msg| {
            let (username, domain) =
                storage::parse_recipient_from_path(&msg.recipient_folder_path).unwrap_or_default();

            serde_json::json!({
                "messageId": msg.message_id,
                "processedAt": msg.processed_at,
                "from": msg.from,
                "subject": msg.subject,
                "filename": msg.filename,
                "recipient": format!("{username}@{domain}"),
            })
        })
        .collect();

    Ok(Json(result))
}

async fn get_message(
    State(config): State<Arc<Config>>,
    Query(params): Query<ApiKeyParams>,
    Path(MessagePath {
        domain,
        username,
        message_filename,
    }): Path<MessagePath>,
) -> ApiResult<Json<serde_json::Value>> {
    let scope = check_key(&params, &config)?;
    if !scope.matches_domain(&domain) {
        return Err(forbidden());
    }

    match storage::load_message(&config.mail_dir, &domain, &username, &message_filename).await {
        Some(msg) => Ok(Json(msg)),
        None => Err(not_found()),
    }
}

async fn get_message_by_id(
    State(config): State<Arc<Config>>,
    Query(params): Query<ApiKeyParams>,
    Path(MessageIdPath { message_id }): Path<MessageIdPath>,
) -> ApiResult<Json<serde_json::Value>> {
    let scope = check_key(&params, &config)?;

    match storage::find_transaction_by_id(&config.mail_dir, &message_id).await {
        Some(summary) => {
            let (username, domain) =
                storage::parse_recipient_from_path(&summary.recipient_folder_path)
                    .ok_or_else(not_found)?;

            if !scope.matches_domain(&domain) {
                return Err(forbidden());
            }

            match storage::load_message(&config.mail_dir, &domain, &username, &summary.filename)
                .await
            {
                Some(msg) => Ok(Json(msg)),
                None => Err(not_found()),
            }
        }
        None => Err(not_found()),
    }
}

async fn get_raw_message(
    State(config): State<Arc<Config>>,
    Query(params): Query<ApiKeyParams>,
    Path(MessageIdPath { message_id }): Path<MessageIdPath>,
) -> Result<([(&'static str, &'static str); 1], Vec<u8>), ApiError> {
    let scope = check_key(&params, &config)?;

    match storage::find_transaction_by_id(&config.mail_dir, &message_id).await {
        Some(summary) => {
            let (username, domain) =
                storage::parse_recipient_from_path(&summary.recipient_folder_path)
                    .ok_or_else(not_found)?;

            if !scope.matches_domain(&domain) {
                return Err(forbidden());
            }

            match storage::load_raw_message(&config.mail_dir, &domain, &username, &summary.filename)
                .await
            {
                Some(data) => Ok(([("content-type", "message/rfc822")], data)),
                None => Err(not_found()),
            }
        }
        None => Err(not_found()),
    }
}

async fn list_recipients(
    State(config): State<Arc<Config>>,
    Query(params): Query<ApiKeyParams>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    let scope = check_key(&params, &config)?;

    let recipients = storage::find_all_recipients(&config.mail_dir).await;

    let result: Vec<serde_json::Value> = recipients
        .into_iter()
        .filter(|r| scope.matches_domain(&r.domain))
        .map(|r| {
            serde_json::json!({
                "domain": r.domain,
                "name": r.name,
                "email": r.email,
                "messageCount": r.message_count,
            })
        })
        .collect();

    Ok(Json(result))
}

async fn list_weeks(
    State(config): State<Arc<Config>>,
    Query(params): Query<ApiKeyParams>,
) -> ApiResult<Json<Vec<String>>> {
    let _scope = check_key(&params, &config)?;

    let weeks = storage::list_weekly_index_names(&config.mail_dir).await;
    Ok(Json(weeks))
}

fn percent_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

pub fn build_router(config: Arc<Config>) -> Router {
    let port = config.admin_app_port.unwrap_or(2255);
    info!("Admin API listening on port {port}");

    Router::new()
        .route("/api/mail", get(redirect_current_week))
        .route("/api/week/{year}/{week}", get(list_week_mail))
        .route("/api/recipients", get(list_recipients))
        .route("/api/weeks", get(list_weeks))
        .route("/api/domain/{domain}/{name}", get(list_domain_mail))
        .route(
            "/api/domain/{domain}/{username}/{message_filename}",
            get(get_message),
        )
        .route("/api/message/{message_id}", get(get_message_by_id))
        .route("/api/message/{message_id}/raw", get(get_raw_message))
        .layer(CorsLayer::permissive())
        .with_state(config)
}
