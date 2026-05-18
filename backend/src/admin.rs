use std::sync::Arc;

use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::Redirect;
use axum::routing::{get, get_service};
use axum::{Json, Router, http::StatusCode};
use chrono::{Datelike, Utc};
use serde::Deserialize;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
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

#[derive(Deserialize)]
pub struct AttachmentPath {
    message_id: String,
    index: u32,
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
    let messages = storage::find_messages_by_week(&config.db, year as i32, week as i32).await;

    let result: Vec<serde_json::Value> = messages
        .into_iter()
        .filter(|msg| scope.matches_domain(&msg.recipient_domain))
        .map(|msg| {
            serde_json::json!({
                "messageId": msg.message_id,
                "processedAt": msg.processed_at,
                "from": msg.from,
                "subject": msg.subject,
                "filename": msg.filename,
                "recipient": format!("{}@{}", msg.recipient_name, msg.recipient_domain),
                "message": {
                "href": format!("/api/domain/{}/{}",
                    percent_encode(&msg.recipient_domain),
                    percent_encode(&msg.recipient_name),
                )
                }
            })
        })
        .collect();

    Ok(Json(result))
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

    let messages = storage::find_transactions_by_recipient(&config.db, &name, &domain).await;

    let result: Vec<serde_json::Value> = messages
        .into_iter()
        .map(|msg| {
            serde_json::json!({
                "messageId": msg.message_id,
                "processedAt": msg.processed_at,
                "from": msg.from,
                "subject": msg.subject,
                "filename": msg.filename,
                "recipient": format!("{}@{}", msg.recipient_name, msg.recipient_domain),
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

    match storage::find_transaction_by_id(&config.db, &message_id).await {
        Some(summary) => {
            if !scope.matches_domain(&summary.recipient_domain) {
                return Err(forbidden());
            }

            match storage::load_message(
                &config.mail_dir,
                &summary.recipient_domain,
                &summary.recipient_name,
                &summary.filename,
            )
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

    match storage::find_transaction_by_id(&config.db, &message_id).await {
        Some(summary) => {
            if !scope.matches_domain(&summary.recipient_domain) {
                return Err(forbidden());
            }

            match storage::load_raw_message(
                &config.mail_dir,
                &summary.recipient_domain,
                &summary.recipient_name,
                &summary.filename,
            )
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

    let recipients = storage::find_all_recipients(&config.db).await;

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

    let weeks = storage::list_weeks(&config.db).await;
    Ok(Json(weeks))
}

#[derive(Deserialize)]
pub struct AttachmentQuery {
    pub api_key: Option<String>,
    pub view: Option<String>,
}

async fn get_attachment(
    State(config): State<Arc<Config>>,
    Query(params): Query<AttachmentQuery>,
    Path(AttachmentPath { message_id, index }): Path<AttachmentPath>,
) -> Result<(HeaderMap, Vec<u8>), ApiError> {
    let scope = check_key(&ApiKeyParams { api_key: params.api_key }, &config)?;

    match storage::find_transaction_by_id(&config.db, &message_id).await {
        Some(summary) => {
            if !scope.matches_domain(&summary.recipient_domain) {
                return Err(forbidden());
            }

            match storage::load_attachment_bytes(
                &config.mail_dir,
                &summary.recipient_domain,
                &summary.recipient_name,
                &summary.filename,
                index,
            )
            .await
            {
                Some((bytes, real_type)) => {
                    let is_view = params.view.is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
                    let disposition = if is_view { "inline" } else { "attachment" };
                    let content_type = if is_view { real_type } else { "application/octet-stream".to_string() };

                    let mut headers = HeaderMap::new();
                    headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(&content_type).unwrap());
                    headers.insert(header::CONTENT_DISPOSITION, HeaderValue::from_str(disposition).unwrap());
                    Ok((headers, bytes))
                }
                None => Err(not_found()),
            }
        }
        None => Err(not_found()),
    }
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

async fn api_catch_all() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"message": "Not Found"})))
}

pub fn build_router(config: Arc<Config>) -> Router {
    let port = config.admin_app_port.unwrap_or(2255);
    info!("Admin API listening on port {port}");

    let static_service = get_service(
        ServeDir::new("dist").fallback(ServeFile::new("dist/index.html")),
    )
    .handle_error(|_| async {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"message": "Not Found"})))
    });

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
        .route(
            "/api/message/{message_id}/attachment/{index}",
            get(get_attachment),
        )
        .route("/api/{*path}", get(api_catch_all))
        .layer(CorsLayer::permissive())
        .with_state(config)
        .fallback_service(static_service)
}
