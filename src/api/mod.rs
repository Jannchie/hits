//! API 路由与文档集成模块

pub mod handlers;
pub mod types;
pub mod ws;

use handlers::ApiDoc;
use utoipa::OpenApi;
pub use ws::ws_handler;

use axum::{http::Request, response::Response, routing::get, Extension, Router};
use sqlx::postgres::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::{info_span, Span};
use utoipa_scalar::{Scalar, Servable};

/// 构建 API 路由与中间件
pub fn create_router(pool: PgPool, broadcaster: Arc<broadcast::Sender<String>>) -> Router {
    use handlers::{
        app_info_route, count_increment_route, direct_svg_badge_route, shields_badge_route,
    };
    Router::new()
        // API 文档
        .merge(Scalar::with_url("/scalar", ApiDoc::openapi()))
        // API 路由
        .route("/hits/{key}", get(count_increment_route))
        .route("/", get(app_info_route))
        .route("/badge/{key}", get(shields_badge_route))
        .route("/svg/{key}", get(direct_svg_badge_route))
        .route("/ws", get(ws_handler))
        .layer(
            ServiceBuilder::new()
                .layer(Extension(pool))
                .layer(Extension(broadcaster.clone()))
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(|request: &Request<axum::body::Body>| {
                            info_span!(
                                "HTTP Request",
                                method = %request.method(),
                                uri = %request.uri(),
                            )
                        })
                        .on_response(
                            |response: &Response, latency: std::time::Duration, span: &Span| {
                                span.record("status_code", response.status().as_u16());
                                tracing::info!(
                                    status_code = response.status().as_u16(),
                                    latency = ?latency,
                                    "HTTP Response"
                                );
                            },
                        ),
                ),
        )
        .with_state(broadcaster)
}
