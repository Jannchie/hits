use anyhow::{Context, Result};
use dotenv::dotenv;
use sqlx::postgres::PgPool;
use std::{env, net::SocketAddr, sync::Arc};
use tokio::sync::broadcast;
use tracing::info;

mod badge;
mod font_metrics;
mod api;
mod error;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt::init();

    // --- 配置 ---
    let database_url =
        env::var("DATABASE_URL").context("DATABASE_URL environment variable must be set")?;
    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port_str = env::var("PORT").unwrap_or_else(|_| "3030".to_string());
    let port: u16 = port_str
        .parse()
        .with_context(|| format!("Invalid PORT value: {}", port_str))?;
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .with_context(|| format!("Invalid HOST/PORT combination: {}:{}", host, port))?;

    // --- 数据库连接池 ---
    info!("Connecting to database...");
    let pool = PgPool::connect(&database_url)
        .await
        .context("Failed to create PostgreSQL connection pool")?;
    info!("Database connection pool established.");

    // --- 广播通道 ---
    let (tx, _) = broadcast::channel::<String>(100);
    let broadcaster = Arc::new(tx);

    // --- 路由与服务启动 ---
    let app = api::create_router(pool, broadcaster.clone());

    info!("Starting server, listening on http://{}", addr);
    info!("Access Scalar UI at http://{}/scalar", addr);
    info!("WebSocket endpoint available at ws://{}/ws", addr);
    info!("Badge endpoint example: http://{}/badge/your-key", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to address {}", addr))?;

    axum::serve(listener, app.into_make_service())
        .await
        .context("Web server failed")?;

    Ok(())
}
