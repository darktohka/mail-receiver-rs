use std::sync::Arc;

use anyhow::Result;
use tokio::signal;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use mail_receiver_rs::config::Config;
use mail_receiver_rs::smtp::run_smtp_server;
use mail_receiver_rs::storage;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();
    let is_rescan = args.get(1).map(|s| s.as_str()) == Some("rescan");

    let config = Config::from_env().await?;
    storage::init_db(&config.db).await?;

    if is_rescan {
        storage::rescan_database(&config.db, &config.mail_dir).await?;
        return Ok(());
    }

    let config = Arc::new(config);

    info!("Starting mail-receiver-rs SMTP server");

    let smtp_config = Arc::clone(&config);
    let mut smtp_handle = tokio::spawn(async move {
        if let Err(e) = run_smtp_server(smtp_config).await {
            error!("SMTP server error: {e}");
        }
    });

    if let Some(port) = config.admin_app_port {
        let admin_config = Arc::clone(&config);
        let admin_router = mail_receiver_rs::admin::build_router(admin_config);
        let addr = format!("0.0.0.0:{port}");
        tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
            info!("Admin API listening on {addr}");
            axum::serve(listener, admin_router).await.unwrap();
        });
    } else {
        info!("Admin API not enabled. Set ADMIN_APP_PORT and API_KEY to enable.");
    }

    tokio::select! {
        result = &mut smtp_handle => {
            if let Err(e) = result {
                error!("SMTP task failed: {e}");
            }
        }
        _ = wait_for_shutdown_signal() => {
            info!("Received shutdown signal, shutting down...");
            smtp_handle.abort();
        }
    }

    info!("Shutdown complete");
    Ok(())
}

async fn wait_for_shutdown_signal() {
    let ctrl_c = signal::ctrl_c();

    #[cfg(unix)]
    let mut stream = signal::unix::signal(signal::unix::SignalKind::terminate())
        .expect("failed to install SIGTERM handler");

    #[cfg(unix)]
    let term = stream.recv();

    #[cfg(unix)]
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }

    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}
