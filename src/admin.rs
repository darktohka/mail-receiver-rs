use std::sync::Arc;

use axum::extract::{Path, Query, State};
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

fn check_key(params: &ApiKeyParams, config: &Config) -> ApiResult<()> {
    match &params.api_key {
        Some(key) if key == &config.api_key => Ok(()),
        _ => Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"message": "Access denied"})),
        )),
    }
}

#[derive(Deserialize)]
pub struct YearWeek {
    year: u32,
    week: u32,
}

#[derive(Deserialize)]
pub struct MessagePath {
    domain: String,
    username: String,
    message_filename: String,
}

type ApiError = (StatusCode, Json<serde_json::Value>);
type ApiResult<T> = Result<T, ApiError>;

async fn redirect_current_week(
    State(config): State<Arc<Config>>,
    Query(params): Query<ApiKeyParams>,
) -> ApiResult<Redirect> {
    check_key(&params, &config)?;
    let now = Utc::now();
    let week = now.iso_week().week();
    let year = now.year();
    Ok(Redirect::temporary(&format!("/api/mail/{year}/{week}")))
}

async fn list_week_mail(
    State(config): State<Arc<Config>>,
    Query(params): Query<ApiKeyParams>,
    Path(YearWeek { year, week }): Path<YearWeek>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    check_key(&params, &config)?;
    let index_name = format!("w{week}-{year}");
    let index = storage::load_weekly_index_by_name(&config.mail_dir, &index_name).await;

    let messages: Vec<serde_json::Value> = index
        .messages
        .iter()
        .map(|msg| {
            let recipient = msg
                .recipient_folder_path
                .strip_prefix("mail/")
                .unwrap_or(&msg.recipient_folder_path)
                .strip_suffix('/')
                .unwrap_or(&msg.recipient_folder_path);
            let (username, domain) = match recipient.split_once('@') {
                Some((u, d)) => (u, d),
                None => (recipient, ""),
            };

            serde_json::json!({
                "messageId": msg.message_id,
                "processedAt": msg.processed_at,
                "from": msg.from,
                "subject": msg.subject,
                "filename": msg.filename,
                "recipient": recipient,
                "message": {
                    "href": format!("/api/mail/{}/{}",
                        percent_encode(domain),
                        percent_encode(username),
                    )
                }
            })
        })
        .collect();

    Ok(Json(messages))
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
    check_key(&params, &config)?;

    match storage::load_message(&config.mail_dir, &domain, &username, &message_filename).await {
        Some(msg) => Ok(Json(msg)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"message": "Not Found"})),
        )),
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

pub fn build_router(config: Arc<Config>) -> Router {
    let port = config.admin_app_port.unwrap_or(2255);
    info!("Admin API listening on port {port}");

    Router::new()
        .route("/api/mail", get(redirect_current_week))
        .route("/api/mail/{year}/{week}", get(list_week_mail))
        .route(
            "/api/mail/{domain}/{username}/{message_filename}",
            get(get_message),
        )
        .layer(CorsLayer::permissive())
        .with_state(config)
}
