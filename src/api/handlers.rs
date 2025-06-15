use utoipa::OpenApi;

use crate::api::types::{ApiError, AppInfo, BadgeStyle, ShieldsIoBadge};
use crate::error::AppError;
use axum::{
    extract::{Extension, Path},
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
    Json,
};
use shields::render_badge_svg;
use sqlx::postgres::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::api::types::HitBadgeParams;
use axum::{extract::Query, http::HeaderMap, response::Response};

/// OpenAPI 文档结构体
#[derive(OpenApi)]
#[openapi(
    components(
        schemas(BadgeStyle)
    ),
    tags(
        (name = "Meta", description = "Meta API Endpoints"),
        (name = "Main", description = "Main API Endpoints"),
        (name = "WebSocket", description = "WebSocket Endpoints"),
        (name = "Badge", description = "Shields.io Badge Endpoint")
    ),
    paths(
        count_increment_route,
        app_info_route,
        shields_badge_route,
        direct_svg_badge_route,
    ),
    info(
        title = "Hits API",
        version = env!("CARGO_PKG_VERSION"),
        description = r#"A simple API to increment and retrieve counts based on a key.
Includes a WebSocket endpoint `/ws` that broadcasts the `key` whenever a count is incremented.
Provides a `/badge/{key}` endpoint compatible with shields.io."#,
        contact(
            name = "Jannchie",
            email = "jannchie@gmail.com"
        ),
        license(
            name = "MIT",
            url = "https://opensource.org/licenses/MIT"
        )
    )
)]
pub struct ApiDoc;

// 其余 handler 保持不变
/// 广播通道类型
pub type Broadcaster = broadcast::Sender<String>;

/// 数据库操作：自增并获取计数
pub async fn increase_and_get_count(
    pool: PgPool,
    key: String,
    broadcaster: Arc<Broadcaster>,
) -> i64 {
    let record = sqlx::query!(
        r#"
        WITH updated AS (
            INSERT INTO counters (key, count, minute_window)
            VALUES ($1, 1, DATE_TRUNC('minute', NOW() AT TIME ZONE 'UTC'))
            ON CONFLICT (key, minute_window)
            DO UPDATE SET count = counters.count + 1
            RETURNING key
        )
        SELECT
            (SELECT key FROM updated LIMIT 1) as upserted_key,
            SUM(c.count) AS total_count
        FROM counters c
        WHERE c.key = $1;
        "#,
        key
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    broadcaster.send(key.clone()).ok();
    record.total_count.unwrap_or(0) + 1
}

/// 计数自增接口
#[utoipa::path(
    get,
    summary = "Increment and Get Total Hits",
    description = "Increments a counter for the given key and returns the total count. Broadcasts the key via WebSocket.",
    path = "/hits/{key}",
    tag = "Main",
    params(
        ("key" = String, Path, description = "The unique key for the counter to increment.")
    ),
    responses(
        (status = 200, description = "Successfully incremented and returned total count.", body = i64, example = json!(15)),
        (status = 500, description = "Database error", body = ApiError)
    )
)]
pub async fn count_increment_route(
    Path(key): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(broadcaster): Extension<Arc<Broadcaster>>,
) -> Result<Json<i64>, AppError> {
    let total_count_i64 = increase_and_get_count(pool, key.clone(), broadcaster.clone()).await;
    Ok(Json(total_count_i64))
}

/// Shields.io Badge 查询接口
#[utoipa::path(
    get,
    summary = "Get Total Hits for Shields.io Badge",
    description = "Retrieves the total count for the given key, formatted as a JSON response suitable for shields.io. This endpoint does NOT increment the counter. It includes Cache-Control headers to prevent caching.",
    path = "/badge/{key}",
    tag = "Badge",
    params(
        ("key" = String, Path, description = "The unique key for the counter to retrieve.")
    ),
    responses(
        (status = 200, description = "Successfully retrieved total count for the badge.", body = ShieldsIoBadge,
         example = json!({"schemaVersion": 1, "label": "hits", "message": "1234", "color": "blue"}),
        ),
        (status = 500, description = "Database error", body = ApiError)
    )
)]
pub async fn shields_badge_route(
    Path(key): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(broadcaster): Extension<Arc<Broadcaster>>,
) -> Result<impl IntoResponse, AppError> {
    let total_count = increase_and_get_count(pool, key, broadcaster).await;
    let badge = ShieldsIoBadge {
        schema_version: 1,
        label: "hits".to_string(),
        message: total_count.to_string(),
        color: "blue".to_string(),
    };
    let mut response = (StatusCode::OK, Json(badge)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response
        .headers_mut()
        .insert(header::EXPIRES, HeaderValue::from_static("0"));
    Ok(response)
}

/// SVG Badge 查询接口
#[utoipa::path(
    get,
    path = "/svg/{key}",
    tag = "Badge",
    summary = "Get Total Hits as an SVG Badge with Style Options",
    description = "Retrieves the total count for the given key, increments it, and returns it as an SVG badge. Supports different visual styles via the `style` query parameter (e.g., 'flat', 'social'). Includes Cache-Control headers.",
    params(
        HitBadgeParams
    ),
    responses(
        (status = 200, description = "Successfully generated and returned the SVG badge.", content_type = "image/svg+xml", body = String),
        (status = 400, description = "Invalid parameters (e.g., unsupported style, although current implementation falls back)", body = ApiError),
        (status = 500, description = "Database error or other internal error", body = ApiError)
    )
)]
pub async fn direct_svg_badge_route(
    Path(key): Path<String>,
    Query(params): Query<HitBadgeParams>,
    Extension(pool): Extension<PgPool>,
    Extension(broadcaster): Extension<Arc<Broadcaster>>,
) -> Result<Response, AppError> {
    let total_count = increase_and_get_count(pool, key.clone(), broadcaster).await;
    let message_text = total_count.to_string();
    let style = match params.style {
        BadgeStyle::Flat => shields::BadgeStyle::Flat,
        BadgeStyle::FlatSquare => shields::BadgeStyle::FlatSquare,
        BadgeStyle::Plastic => shields::BadgeStyle::Plastic,
        BadgeStyle::Social => shields::BadgeStyle::Social,
        BadgeStyle::ForTheBadge => shields::BadgeStyle::ForTheBadge,
    };
    // let svg_generate_params = Builder::flat(){
    let svg_string = render_badge_svg(&shields::BadgeParams {
        style,
        label: Some(params.label.as_str()),
        message: Some(message_text.as_str()),
        label_color: Some(params.label_color.as_str()),
        message_color: Some(params.message_color.as_str()),
        link: params.link.as_deref(),
        extra_link: params.extra_link.as_deref(),
        logo: params.logo.as_deref(),
        logo_color: params.logo_color.as_deref(),
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml;charset=utf-8"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(header::EXPIRES, HeaderValue::from_static("0"));
    Ok((StatusCode::OK, headers, svg_string).into_response())
}

/// 应用信息接口
#[utoipa::path(
    get,
    summary = "App Info",
    description = "Returns information about the application, including API docs, WebSocket endpoint, and badge endpoint example.",
    path = "/",
    responses(
        (status = 200, description = "Returns information about the application.", body = AppInfo, example = json!({ "project_name": "Hits", "version": "0.1.0", "docs_path": "/scalar", "websocket_url": "ws://<host>:<port>/ws", "badge_url_example": "http://<host>:<port>/badge/your-key"}))
    ),
    tag = "Meta"
)]
pub async fn app_info_route() -> impl IntoResponse {
    let info = AppInfo {
        project_name: "Hits".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        docs_path: "/scalar".to_string(),
    };
    Json(info)
}
