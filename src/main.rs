use anyhow::{Context, Result}; // For easier error handling in main
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Extension,
        Path,
        Query,
        State, // To access shared state in ws handler
    },
    http::{header, HeaderMap, HeaderValue, Request, StatusCode}, // Import header and HeaderValue
    response::{IntoResponse, Response},
    routing::get,
    Json,
    Router,
};
use badge::{render_badge_svg, BadgeStyle, RenderBadgeParams};
use dotenv::dotenv;
use futures_util::{stream::StreamExt, SinkExt}; // WebSocket stream utilities
use sqlx::postgres::PgPool;
use std::{env, net::SocketAddr, sync::Arc}; // Arc for sharing the broadcast sender
use thiserror::Error; // For creating the custom error type
use tokio::sync::broadcast; // Import broadcast channel
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::{error, info, info_span, warn, Span}; // Import warn

// --- OpenAPI Imports ---
use serde::{Deserialize, Serialize};
use utoipa::OpenApi;
use utoipa_rapidoc::RapiDoc;

// --- Font Metrics Module ---
mod badge;
mod font_metrics;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// --- Type alias for the broadcast sender ---
type Broadcaster = broadcast::Sender<String>; // Sends the 'key' string

// --- Shields.io Badge Structure ---
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")] // Use camelCase for JSON field names (shields.io standard)
struct ShieldsIoBadge {
    schema_version: u8, // Should be 1
    label: String,      // The left side of the badge
    message: String,    // The right side of the badge (the count)
    color: String,      // e.g., "blue", "green", hex codes like "ff69b4"
                        // Optional: Add fields like `labelColor`, `isError`, `namedLogo`, `logoSvg`, `logoColor`, `logoWidth`, `logoPosition`, `style`, `cacheSeconds` if needed
}

// --- API Documentation Structure ---
#[derive(OpenApi)]
#[openapi(
    components(
        // Add ShieldsIoBadge to schemas
        schemas(ApiError, AppInfo, ShieldsIoBadge)
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
        version = VERSION,
        description = r#"A simple API to increment and retrieve counts based on a key.
Includes a WebSocket endpoint `/ws` that broadcasts the `key` whenever a count is incremented.
Provides a `/badge/{key}` endpoint compatible with shields.io."#, // Updated description
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
struct ApiDoc;

// Define a more structured error response for the API
#[derive(Serialize, utoipa::ToSchema)]
struct ApiError {
    #[schema(example = "Internal Server Error")]
    message: String,
}

// Define a custom application error type
#[derive(Debug, Error)]
enum AppError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
}

// Implement IntoResponse for AppError
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        error!("Error processing request: {}", self);
        let (status, error_message) = match self {
            AppError::DatabaseError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "An unexpected database error occurred.".to_string(),
            ),
        };
        let api_error = ApiError {
            message: error_message,
        };
        // Add Cache-Control header to error responses for badges to prevent caching
        let mut response = (status, Json(api_error)).into_response();
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
        response
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt::init();

    // --- Configuration ---
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

    // --- Database Pool Setup ---
    info!("Connecting to database...");
    let pool = PgPool::connect(&database_url)
        .await
        .context("Failed to create PostgreSQL connection pool")?;
    info!("Database connection pool established.");

    // --- Broadcast Channel Setup ---
    let (tx, _) = broadcast::channel::<String>(100);
    let broadcaster = Arc::new(tx);

    // --- Axum Router Setup ---
    let app = Router::new()
        // --- API Documentation ---
        .merge(RapiDoc::with_openapi("/api-docs/openapi.json", ApiDoc::openapi()).path("/rapidoc"))
        // --- API Routes ---
        .route("/hits/{key}", get(count_increment_route))
        .route("/", get(app_info_route))
        // --- Add Badge Route ---
        .route("/badge/{key}", get(shields_badge_route))
        .route("/svg/{key}", get(direct_svg_badge_route))
        // --- WebSocket Route ---
        .route("/ws", get(ws_handler))
        .layer(
            ServiceBuilder::new()
                .layer(Extension(pool)) // Share the database pool
                .layer(Extension(broadcaster.clone())) // Share the broadcast sender
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
                                info!(
                                    status_code = response.status().as_u16(),
                                    latency = ?latency,
                                    "HTTP Response"
                                );
                            },
                        ),
                ),
        )
        .with_state(broadcaster); // Make broadcaster available via State extractor in ws_handler

    // --- Start Server ---
    info!("Starting server, listening on http://{}", addr);
    info!("Access Rapidoc UI at http://{}/rapidoc", addr);
    info!("WebSocket endpoint available at ws://{}/ws", addr);
    info!("Badge endpoint example: http://{}/badge/your-key", addr); // Log Badge URL

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to address {}", addr))?;

    axum::serve(listener, app.into_make_service())
        .await
        .context("Web server failed")?;

    Ok(())
}

// --- WebSocket Handler (Unchanged) ---
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(broadcaster): State<Arc<Broadcaster>>,
) -> impl IntoResponse {
    info!("WebSocket connection request received");
    ws.on_upgrade(move |socket| handle_socket(socket, broadcaster))
}

async fn handle_socket(socket: WebSocket, broadcaster: Arc<Broadcaster>) {
    info!("WebSocket connection established");
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let mut rx = broadcaster.subscribe();

    let send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(key) => {
                    if ws_sender.send(Message::Text(key.into())).await.is_err() {
                        warn!("WebSocket send failed, client disconnected?");
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WebSocket receiver lagged behind by {} messages.", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    // The broadcaster has been dropped, no more messages to receive
                    break;
                }
            }
        }
        info!("WebSocket send task finished.");
    });

    let recv_task = tokio::spawn(async move {
        while let Some(msg_result) = ws_receiver.next().await {
            match msg_result {
                Ok(msg) => match msg {
                    Message::Text(t) => info!("Received text from WebSocket client: {}", t),
                    Message::Binary(_) => info!("Received binary data from WebSocket client."),
                    Message::Ping(_) => info!("Received WebSocket ping."),
                    Message::Pong(_) => info!("Received WebSocket pong."),
                    Message::Close(_) => {
                        info!("WebSocket client initiated close.");
                        break;
                    }
                },
                Err(e) => {
                    warn!("Error receiving WebSocket message: {}", e);
                    break;
                }
            }
        }
        info!("WebSocket receive task finished.");
    });

    tokio::select! {
        _ = send_task => { /* Send task finished */ }
        _ = recv_task => { /* Receive task finished */ }
    }
    info!("WebSocket connection closed.");
}

async fn increase_and_get_count(pool: PgPool, key: String, broadcaster: Arc<Broadcaster>) -> i64 {
    let record = sqlx::query!(
        r#"
        WITH updated AS (
            INSERT INTO counters (key, count, minute_window)
            VALUES ($1, 1, DATE_TRUNC('minute', NOW() AT TIME ZONE 'UTC'))
            ON CONFLICT (key, minute_window)
            DO UPDATE SET count = counters.count + 1
            RETURNING key -- Return the key from the insert/update
        )
        SELECT
            (SELECT key FROM updated LIMIT 1) as upserted_key, -- Get the key that was actually inserted/updated
            SUM(c.count) AS total_count
        FROM counters c
        WHERE c.key = $1;
        "#,
        key
    )
    .fetch_one(&pool)
    .await.unwrap(); // Propagate any database errors
    broadcaster.send(key.clone()).ok(); // Ignore errors
    record.total_count.unwrap_or(0) + 1
}

// --- HTTP Handlers ---

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

async fn count_increment_route(
    Path(key): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(broadcaster): Extension<Arc<Broadcaster>>,
) -> Result<Json<i64>, AppError> {
    let total_count_i64 = increase_and_get_count(pool, key.clone(), broadcaster.clone()).await;
    Ok(Json(total_count_i64))
}

// --- New Shields.io Badge Handler ---

#[utoipa::path(
    get,
    summary = "Get Total Hits for Shields.io Badge",
    description = "Retrieves the total count for the given key, formatted as a JSON response suitable for shields.io. This endpoint does NOT increment the counter. It includes Cache-Control headers to prevent caching.",
    path = "/badge/{key}",
    tag = "Badge", // Assign to the new "Badge" tag
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
async fn shields_badge_route(
    Path(key): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(broadcaster): Extension<Arc<Broadcaster>>,
) -> Result<impl IntoResponse, AppError> {
    // Return type needs to handle headers
    // Query *only* for the sum, do not insert or update
    let total_count = increase_and_get_count(pool, key, broadcaster).await;
    // If the key doesn't exist, SUM returns NULL, which sqlx maps to None. Default to 0.

    let badge = ShieldsIoBadge {
        schema_version: 1,
        label: "hits".to_string(), // You could make this dynamic if needed
        message: total_count.to_string(),
        color: "blue".to_string(), // Customize as desired
    };

    // Create response with appropriate headers to prevent caching, as recommended by shields.io
    let mut response = (StatusCode::OK, Json(badge)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    response.headers_mut().insert(
        header::PRAGMA,
        HeaderValue::from_static("no-cache"), // For HTTP/1.0 backward compatibility
    );
    response.headers_mut().insert(
        header::EXPIRES,
        HeaderValue::from_static("0"), // Proxies
    );

    Ok(response)
}

// Function to provide the default style value for serde
fn default_label() -> String {
    "Hits".to_string()
}

fn default_label_color() -> String {
    "#555".to_string()
}

fn default_message_color() -> String {
    "#007ec6".to_string()
}

#[derive(Deserialize, Debug)]
struct HitBadgeParams {
    #[serde(default)]
    style: BadgeStyle,
    #[serde(default = "default_label")]
    label: String,
    #[serde(default = "default_label_color")]
    label_color: String,
    #[serde(default = "default_message_color")]
    message_color: String,
}

#[utoipa::path(
    get,
    path = "/svg/{key}",
    tag = "Badge",
    summary = "Get Total Hits as an SVG Badge with Style Options",
    description = "Retrieves the total count for the given key, increments it, and returns it as an SVG badge. Supports different visual styles via the `style` query parameter (e.g., 'flat', 'social'). Includes Cache-Control headers.",
    params(
        ("key" = String, Path, description = "The unique key for the counter."),
        ("style" = Option<String>, Query, description = "Optional badge style (e.g., 'flat', 'social'). Defaults to 'flat'."),
        ("label" = Option<String>, Query, description = "Optional label text for the badge. Defaults to 'Hits'."),
        ("label_color" = Option<String>, Query, description = "Optional label color in hex format. Defaults to '#555'."),
        ("message_color" = Option<String>, Query, description = "Optional message color in hex format. Defaults to '#007ec6'."),
    ),
    responses(
        (status = 200, description = "Successfully generated and returned the SVG badge.", content_type = "image/svg+xml", body = String),
        (status = 400, description = "Invalid parameters (e.g., unsupported style, although current implementation falls back)", body = ApiError), // Optional: Add if you validate style strictly
        (status = 500, description = "Database error or other internal error", body = ApiError)
    )
)]
async fn direct_svg_badge_route(
    Path(key): Path<String>,
    Query(params): Query<HitBadgeParams>, // Use Query extractor with explicit lifetime
    Extension(pool): Extension<PgPool>,
    Extension(broadcaster): Extension<Arc<Broadcaster>>,
) -> Result<Response, AppError> {
    // Return Response directly for easier header manipulation

    // --- 1. Get the count (and increment it) ---
    let total_count = increase_and_get_count(pool, key.clone(), broadcaster).await;

    // --- 2. Determine Style and Prepare Badge Data ---
    let message_text = total_count.to_string();
    let svg_generate_params = RenderBadgeParams {
        style: params.style, // Default to flat style
        label: params.label.as_str(),
        message: message_text.as_str(),
        label_color: params.label_color.as_str(),
        message_color: params.message_color.as_str(),
    };

    // --- 3. Generate SVG based on Style ---
    let svg_string = render_badge_svg(svg_generate_params);

    // --- 4. Build the response with SVG content type and cache headers ---
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml;charset=utf-8"),
    ); // Add charset
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(header::EXPIRES, HeaderValue::from_static("0"));

    Ok((StatusCode::OK, headers, svg_string).into_response())
}

#[derive(Serialize, utoipa::ToSchema)]
struct AppInfo {
    project_name: String,
    version: String,
    docs_path: String,
}

#[utoipa::path(
    get,
    summary = "App Info",
    description = "Returns information about the application, including API docs, WebSocket endpoint, and badge endpoint example.",
    path = "/",
    responses(
        (status = 200, description = "Returns information about the application.", body = AppInfo, example = json!({ "project_name": "Hits", "version": "0.1.0", "docs_path": "/rapidoc", "websocket_url": "ws://<host>:<port>/ws", "badge_url_example": "http://<host>:<port>/badge/your-key"}))
    ),
    tag = "Meta"
)]
async fn app_info_route() -> impl IntoResponse {
    let info = AppInfo {
        project_name: "Hits".to_string(),
        version: VERSION.to_string(),
        docs_path: "/rapidoc".to_string(),
    };
    Json(info)
}
