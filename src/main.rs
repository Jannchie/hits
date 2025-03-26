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

// --- Constants for badge styling ---
const BADGE_HEIGHT: u32 = 20;
const HORIZONTAL_PADDING: u32 = 6; // Padding left/right of text
const FONT_FAMILY: &str = "Verdana,Geneva,DejaVu Sans,sans-serif";
const FONT_SIZE_SCALED: u32 = 110; // Corresponds to font-size="11" with transform="scale(.1)"
const LABEL_COLOR: &str = "#555"; // Left side background
const MESSAGE_COLOR: &str = "#007ec6"; // Right side background (blue)
                                       // Rough approximation for character width in pixels for the chosen font style at font-size 11.
                                       // This is the most inaccurate part and might need tuning or a proper font metrics library.
const LABEL_PX_PER_CHAR: f32 = 6.0;
const MESSAGE_PX_PER_CHAR: f32 = 6.0;

/// Estimates the rendered width of a text string based on a simple character count.
/// This is a basic approximation.
fn estimate_text_width(text: &str, px_per_char: f32) -> u32 {
    // Ensure minimum width to avoid overly squashed text for very short strings or single chars
    const MIN_WIDTH: u32 = 5; // Example minimum width, adjust as needed
    let estimated = (text.chars().count() as f32 * px_per_char).ceil() as u32;
    estimated.max(MIN_WIDTH)
}

// --- Struct for Query Parameters ---
#[derive(Deserialize, Debug)]
pub struct BadgeParams {
    #[serde(default = "default_style")] // Provide default if missing
    style: String,
    // Add other potential query params here, e.g., label, labelColor, color, logo, etc.
    // label: Option<String>,
    // color: Option<String>,
    // labelColor: Option<String>,
}

// Function to provide the default style value for serde
fn default_style() -> String {
    "flat".to_string()
}

// --- Updated UTOIPA Documentation ---
#[utoipa::path(
    get,
    path = "/svg/{key}",
    tag = "Badge",
    summary = "Get Total Hits as an SVG Badge with Style Options",
    description = "Retrieves the total count for the given key, increments it, and returns it as an SVG badge. Supports different visual styles via the `style` query parameter (e.g., 'flat', 'social'). Includes Cache-Control headers.",
    params(
        ("key" = String, Path, description = "The unique key for the counter."),
        ("style" = Option<String>, Query, description = "Optional badge style (e.g., 'flat', 'social'). Defaults to 'flat'.") // Document the style query param
        // Add docs for other query params if you implement them
    ),
    responses(
        (status = 200, description = "Successfully generated and returned the SVG badge.", content_type = "image/svg+xml", body = String),
        (status = 400, description = "Invalid parameters (e.g., unsupported style, although current implementation falls back)", body = ApiError), // Optional: Add if you validate style strictly
        (status = 500, description = "Database error or other internal error", body = ApiError)
    )
)]
// --- Updated Route Handler ---
async fn direct_svg_badge_route(
    Path(key): Path<String>,
    Query(params): Query<BadgeParams>, // Use Query extractor
    Extension(pool): Extension<PgPool>,
    Extension(broadcaster): Extension<Arc<Broadcaster>>,
) -> Result<Response, AppError> {
    // Return Response directly for easier header manipulation

    // --- 1. Get the count (and increment it) ---
    let total_count = increase_and_get_count(pool, key.clone(), broadcaster).await;

    // --- 2. Determine Style and Prepare Badge Data ---
    let style = params.style.to_lowercase(); // Use lowercase for matching
    let label_text = "Hits"; // Could also make this a query parameter: params.label.unwrap_or("hits".to_string())
    let message_text = total_count.to_string();
    // Colors could also be query parameters: params.color.unwrap_or(MESSAGE_COLOR.to_string())
    let label_color = LABEL_COLOR;
    let message_color = MESSAGE_COLOR;

    // --- 3. Generate SVG based on Style ---
    let svg_string = match style.as_str() {
        "social" => {
            // Placeholder: Implement social style SVG generation
            // You would likely need different constants and SVG structure
            generate_social_style_svg(label_text, &message_text, label_color, message_color)
            // Call a different generator
        } // Add other styles like "flat-square", "plastic", etc.
        "flat-square" => {
            generate_flat_square_style_svg(label_text, &message_text, label_color, message_color)
        }
        "plastic" => {
            generate_plastic_style_svg(label_text, &message_text, label_color, message_color)
        }
        _default => generate_flat_style_svg(label_text, &message_text, label_color, message_color),
    };

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

// --- SVG Generation Function for "Flat" Style ---
// (Extracted from your original code)
fn generate_flat_style_svg(
    label: &str,
    message: &str,
    label_color: &str,
    message_color: &str,
) -> String {
    // Calculate SVG dimensions based on text
    let label_text_render_width = estimate_text_width(label, LABEL_PX_PER_CHAR);
    let message_text_render_width = estimate_text_width(message, MESSAGE_PX_PER_CHAR);

    let label_rect_width = label_text_render_width + 2 * HORIZONTAL_PADDING;
    let message_rect_width = message_text_render_width + 2 * HORIZONTAL_PADDING;
    let total_width = label_rect_width + message_rect_width;

    // Calculate text positioning
    let label_x_scaled = (label_rect_width / 2) * 10;
    let message_x_scaled = (label_rect_width + message_rect_width / 2) * 10;
    let label_text_length_scaled = label_text_render_width * 10;
    let message_text_length_scaled = message_text_render_width * 10;

    // Generate the SVG string
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{total_width}" height="{badge_height}" role="img" aria-label="{label}: {message}">
            <title>{label}: {message}</title>
            <linearGradient id="s" x2="0" y2="100%">
                <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
                <stop offset="1" stop-opacity=".1"/>
            </linearGradient>
            <clipPath id="r">
                <rect width="{total_width}" height="{badge_height}" rx="3" fill="#fff"/>
            </clipPath>
            <g clip-path="url(#r)">
                <rect width="{label_rect_width}" height="{badge_height}" fill="{label_color}"/>
                <rect x="{label_rect_width}" width="{message_rect_width}" height="{badge_height}" fill="{message_color}"/>
                <rect width="{total_width}" height="{badge_height}" fill="url(#s)"/>
            </g>
            <g fill="#fff" text-anchor="middle" font-family="{font_family}" text-rendering="geometricPrecision" font-size="{font_size_scaled}">
                <text aria-hidden="true" x="{label_x_scaled}" y="150" fill="#010101" fill-opacity=".3" transform="scale(.1)" textLength="{label_text_length_scaled}">{label}</text>
                <text x="{label_x_scaled}" y="140" transform="scale(.1)" fill="#fff" textLength="{label_text_length_scaled}">{label}</text>
                <text aria-hidden="true" x="{message_x_scaled}" y="150" fill="#010101" fill-opacity=".3" transform="scale(.1)" textLength="{message_text_length_scaled}">{message}</text>
                <text x="{message_x_scaled}" y="140" transform="scale(.1)" fill="#fff" textLength="{message_text_length_scaled}">{message}</text>
            </g>
        </svg>"##,
        total_width = total_width,
        badge_height = BADGE_HEIGHT,
        label = label,     // Use function args
        message = message, // Use function args
        label_rect_width = label_rect_width,
        message_rect_width = message_rect_width,
        label_color = label_color,     // Use function args
        message_color = message_color, // Use function args
        font_family = FONT_FAMILY,
        font_size_scaled = FONT_SIZE_SCALED,
        label_x_scaled = label_x_scaled,
        message_x_scaled = message_x_scaled,
        label_text_length_scaled = label_text_length_scaled,
        message_text_length_scaled = message_text_length_scaled,
    )
}

// Social Style Specific Constants
const SOCIAL_FONT_FAMILY: &str = "Helvetica Neue,Helvetica,Arial,sans-serif";
const SOCIAL_FONT_WEIGHT: u32 = 700;
const SOCIAL_FONT_SIZE_SCALED: u32 = 110; // 11px
const SOCIAL_STROKE_COLOR: &str = "#d5d5d5";
const SOCIAL_LABEL_BG_COLOR: &str = "#fcfcfc";
const SOCIAL_MESSAGE_BG_COLOR: &str = "#fafafa";
const SOCIAL_TEXT_COLOR: &str = "#333";
const SOCIAL_HORIZONTAL_PADDING: u32 = 6; // Padding within each part
const SOCIAL_GAP: u32 = 6; // Gap between label and message parts for the arrow
const SOCIAL_LABEL_PX_PER_CHAR: f32 = 6.0;
const SOCIAL_MESSAGE_PX_PER_CHAR: f32 = 6.0;

fn generate_social_style_svg(
    label: &str,
    message: &str,
    _label_color: &str,
    _message_color: &str,
) -> String {
    // Note: _label_color and _message_color are ignored for social style, using fixed colors.
    let badge_height: u32 = BADGE_HEIGHT; // 20
    let rect_height: u32 = badge_height - 1; // 19 (for 0.5px offset)
    let corner_radius: u32 = 2; // Social style uses slightly rounded corners

    // Calculate text widths using specific constants for social style font
    let label_text_render_width = estimate_text_width(label, SOCIAL_LABEL_PX_PER_CHAR);
    let message_text_render_width = estimate_text_width(message, SOCIAL_MESSAGE_PX_PER_CHAR);

    // Calculate dimensions of the two main parts
    let label_part_width = label_text_render_width + 2 * SOCIAL_HORIZONTAL_PADDING;
    let message_part_width = message_text_render_width + 2 * SOCIAL_HORIZONTAL_PADDING;

    // Calculate overall width and positioning
    // total_width = label_width + gap + message_width (using dimensions for positioning)
    let message_rect_start_x = label_part_width + SOCIAL_GAP;
    // Final SVG width needs to encompass everything including the 0.5 offsets
    let total_width = (message_rect_start_x + message_part_width) as f32 + 0.5; // Add 0.5 for the right edge offset
    let total_width_rounded = total_width.ceil() as u32; // Round up for SVG width attribute

    // --- Calculate Text Positioning (Scaled * 10) ---
    // Label text X: Center of the label part
    let label_text_x_scaled = (label_part_width as f32 / 2.0 * 10.0).round() as u32;
    // Message text X: Center of the message part (relative to SVG start)
    let message_text_x_scaled =
        ((message_rect_start_x as f32 + message_part_width as f32 / 2.0) * 10.0).round() as u32;
    // Text Y positions (scaled * 10)
    let text_y_main_scaled = 140; // 14px from top in 20px height
    let text_y_shadow_scaled = text_y_main_scaled + 10; // 15px from top
                                                        // Scaled text lengths
    let label_text_length_scaled = label_text_render_width * 10;
    let message_text_length_scaled = message_text_render_width * 10;

    // Generate the SVG string based on the provided example structure
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{total_width_rounded}" height="{badge_height}" role="img" aria-label="{label}: {message}">
            <title>{label}: {message}</title>
            <style>a:hover #llink{{fill:url(#b);stroke:#ccc}}a:hover #rlink{{fill:#4183c4}}</style>
            <linearGradient id="a" x2="0" y2="100%">
                <stop offset="0" stop-color="#fcfcfc" stop-opacity="0"/>
                <stop offset="1" stop-opacity=".1"/>
            </linearGradient>
            <linearGradient id="b" x2="0" y2="100%">
                <stop offset="0" stop-color="#ccc" stop-opacity=".1"/>
                <stop offset="1" stop-opacity=".1"/>
            </linearGradient>
            <g stroke="{stroke_color}">
                <rect stroke="none" fill="{label_bg_color}" x="0.5" y="0.5" width="{label_part_width}" height="{rect_height}" rx="{corner_radius}"/>
                <rect x="{message_part_start_x_pos}" y="0.5" width="{message_part_width}" height="{rect_height}" rx="{corner_radius}" fill="{message_bg_color}"/>
                <rect x="{divider_x}" y="7.5" width="0.5" height="5" stroke="{message_bg_color}"/>
                <path d="M{arrow_start_x} 6.5 l-3 3v1 l3 3" stroke="{stroke_color}" fill="{message_bg_color}"/> 
            </g>
            <g aria-hidden="true" fill="{text_color}" text-anchor="middle" font-family="{font_family}" text-rendering="geometricPrecision" font-weight="{font_weight}" font-size="{font_size_scaled}px" line-height="14px">
                <rect id="llink" stroke="{stroke_color}" fill="url(#a)" x=".5" y=".5" width="{label_part_width}" height="{rect_height}" rx="{corner_radius}"/>
                <text aria-hidden="true" x="{label_text_x_scaled}" y="{text_y_shadow_scaled}" fill="#fff" transform="scale(.1)" textLength="{label_text_length_scaled}">{label}</text>
                <text x="{label_text_x_scaled}" y="{text_y_main_scaled}" transform="scale(.1)" textLength="{label_text_length_scaled}">{label}</text>
                <text aria-hidden="true" x="{message_text_x_scaled}" y="{text_y_shadow_scaled}" fill="#fff" transform="scale(.1)" textLength="{message_text_length_scaled}">{message}</text>
                <text id="rlink" x="{message_text_x_scaled}" y="{text_y_main_scaled}" transform="scale(.1)" textLength="{message_text_length_scaled}">{message}</text>
            </g>
        </svg>"##,
        // Dimensions & Positions
        total_width_rounded = total_width_rounded,
        badge_height = badge_height,
        rect_height = rect_height,
        label_part_width = label_part_width,
        message_part_width = message_part_width,
        message_part_start_x_pos = message_rect_start_x as f32 - 0.2, // For rect x attribute
        divider_x = message_rect_start_x,                             // For divider rect
        arrow_start_x = message_rect_start_x,                         // For path M command
        corner_radius = corner_radius,
        // Colors
        stroke_color = SOCIAL_STROKE_COLOR,
        label_bg_color = SOCIAL_LABEL_BG_COLOR,
        message_bg_color = SOCIAL_MESSAGE_BG_COLOR,
        text_color = SOCIAL_TEXT_COLOR,
        // Font & Text Attributes
        font_family = SOCIAL_FONT_FAMILY,
        font_weight = SOCIAL_FONT_WEIGHT,
        font_size_scaled = SOCIAL_FONT_SIZE_SCALED,
        label = label,
        message = message,
        label_text_x_scaled = label_text_x_scaled,
        message_text_x_scaled = message_text_x_scaled,
        text_y_main_scaled = text_y_main_scaled,
        text_y_shadow_scaled = text_y_shadow_scaled,
        label_text_length_scaled = label_text_length_scaled,
        message_text_length_scaled = message_text_length_scaled,
    )
}

// --- SVG Generation Function for "Flat Square" Style ---
fn generate_flat_square_style_svg(
    label: &str,
    message: &str,
    label_color: &str,
    message_color: &str,
) -> String {
    // Uses default BADGE_HEIGHT = 20
    let badge_height = BADGE_HEIGHT;

    // Calculate SVG dimensions based on text
    let label_text_render_width = estimate_text_width(label, LABEL_PX_PER_CHAR);
    let message_text_render_width = estimate_text_width(message, MESSAGE_PX_PER_CHAR);

    let label_rect_width = label_text_render_width + 2 * HORIZONTAL_PADDING;
    let message_rect_width = message_text_render_width + 2 * HORIZONTAL_PADDING;
    let total_width = label_rect_width + message_rect_width;

    // Calculate text positioning (using scaled coordinates)
    let label_x_scaled = (label_rect_width / 2) * 10;
    let message_x_scaled = (label_rect_width + message_rect_width / 2) * 10;
    let label_text_length_scaled = label_text_render_width * 10;
    let message_text_length_scaled = message_text_render_width * 10;

    // Y position for text (scaled) - same as flat
    let text_y_scaled = 140; // Corresponds to 14px from top in a 20px badge
    let shadow_text_y_scaled = text_y_scaled + 10; // 1px lower

    // Generate the SVG string - Note rx="0" in clipPath
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{total_width}" height="{badge_height}" role="img" aria-label="{label}: {message}">
            <title>{label}: {message}</title>
            <linearGradient id="s" x2="0" y2="100%">
                <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
                <stop offset="1" stop-opacity=".1"/>
            </linearGradient>
            <clipPath id="r">
                <rect width="{total_width}" height="{badge_height}" rx="0" fill="#fff"/> 
            </clipPath>
            <g clip-path="url(#r)">
                <rect width="{label_rect_width}" height="{badge_height}" fill="{label_color}"/>
                <rect x="{label_rect_width}" width="{message_rect_width}" height="{badge_height}" fill="{message_color}"/>
                <rect width="{total_width}" height="{badge_height}" fill="url(#s)"/>
            </g>
            <g fill="#fff" text-anchor="middle" font-family="{font_family}" text-rendering="geometricPrecision" font-size="{font_size_scaled}">
                <text aria-hidden="true" x="{label_x_scaled}" y="{shadow_text_y_scaled}" fill="#010101" fill-opacity=".3" transform="scale(.1)" textLength="{label_text_length_scaled}">{label}</text>
                <text x="{label_x_scaled}" y="{text_y_scaled}" transform="scale(.1)" fill="#fff" textLength="{label_text_length_scaled}">{label}</text>
                <text aria-hidden="true" x="{message_x_scaled}" y="{shadow_text_y_scaled}" fill="#010101" fill-opacity=".3" transform="scale(.1)" textLength="{message_text_length_scaled}">{message}</text>
                <text x="{message_x_scaled}" y="{text_y_scaled}" transform="scale(.1)" fill="#fff" textLength="{message_text_length_scaled}">{message}</text>
            </g>
        </svg>"##,
        total_width = total_width,
        badge_height = badge_height,
        label = label,
        message = message,
        label_rect_width = label_rect_width,
        message_rect_width = message_rect_width,
        label_color = label_color,
        message_color = message_color,
        font_family = FONT_FAMILY,
        font_size_scaled = FONT_SIZE_SCALED,
        label_x_scaled = label_x_scaled,
        message_x_scaled = message_x_scaled,
        shadow_text_y_scaled = shadow_text_y_scaled,
        text_y_scaled = text_y_scaled,
        label_text_length_scaled = label_text_length_scaled,
        message_text_length_scaled = message_text_length_scaled,
    )
}

// --- SVG Generation Function for "Plastic" Style ---
fn generate_plastic_style_svg(
    label: &str,
    message: &str,
    label_color: &str,
    message_color: &str,
) -> String {
    let badge_height = BADGE_HEIGHT;
    let corner_radius = 3; // Standard rounded corner for plastic

    // Calculate SVG dimensions based on text
    let label_text_render_width = estimate_text_width(label, LABEL_PX_PER_CHAR);
    let message_text_render_width = estimate_text_width(message, MESSAGE_PX_PER_CHAR);

    // Padding might be slightly different visually, but let's keep HORIZONTAL_PADDING = 6 for now
    let label_rect_width = label_text_render_width + 2 * HORIZONTAL_PADDING;
    let message_rect_width = message_text_render_width + 2 * HORIZONTAL_PADDING;
    let total_width = label_rect_width + message_rect_width;

    // Calculate text positioning (using scaled coordinates)
    let label_x_scaled = (label_rect_width / 2) * 10;
    let message_x_scaled = (label_rect_width + message_rect_width / 2) * 10;
    let label_text_length_scaled = label_text_render_width * 10;
    let message_text_length_scaled = message_text_render_width * 10;

    // Y position for text (scaled) - Adjust for 18px height if needed, 140 often still looks ok.
    // 14px from top in 18px height. Let's try 135 for slightly higher.
    let text_y_scaled = 135;
    let shadow_text_y_scaled = text_y_scaled + 10; // 1px lower

    // Generate the SVG string - Note different structure
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{total_width}" height="{badge_height}" role="img" aria-label="{label}: {message}">
            <title>{label}: {message}</title>
            <linearGradient id="a" x2="0" y2="100%">
                <stop offset="0" stop-color="#fff" stop-opacity=".7"/>
                <stop offset=".1" stop-color="#aaa" stop-opacity=".1"/>
                <stop offset=".9" stop-color="#000" stop-opacity=".3"/>
                <stop offset="1" stop-color="#000" stop-opacity=".5"/>
            </linearGradient>
            <clipPath id="r">
                <rect width="{total_width}" height="{badge_height}" rx="{corner_radius}" fill="#fff"/>
            </clipPath>
            <g clip-path="url(#r)">
                <rect width="{label_rect_width}" height="{badge_height}" fill="{label_color}"/>
                <rect x="{label_rect_width}" width="{message_rect_width}" height="{badge_height}" fill="{message_color}"/>
                <rect width="{total_width}" height="{badge_height}" fill="url(#a)"/>
            </g>
            <g fill="#fff" text-anchor="middle" font-family="{font_family}" text-rendering="geometricPrecision" font-size="{font_size_scaled}">
                <text aria-hidden="true" x="{label_x_scaled}" y="{shadow_text_y_scaled}" fill="#111" fill-opacity=".3" transform="scale(.1)" textLength="{label_text_length_scaled}">{label}</text>
                <text x="{label_x_scaled}" y="{text_y_scaled}" transform="scale(.1)" fill="#fff" textLength="{label_text_length_scaled}">{label}</text>
                <text aria-hidden="true" x="{message_x_scaled}" y="{shadow_text_y_scaled}" fill="#111" fill-opacity=".3" transform="scale(.1)" textLength="{message_text_length_scaled}">{message}</text>
                <text x="{message_x_scaled}" y="{text_y_scaled}" transform="scale(.1)" fill="#fff" textLength="{message_text_length_scaled}">{message}</text>
            </g>
        </svg>"##,
        total_width = total_width,
        badge_height = badge_height,
        corner_radius = corner_radius,
        label = label,
        message = message,
        label_rect_width = label_rect_width,
        message_rect_width = message_rect_width,
        label_color = label_color,
        message_color = message_color,
        font_family = FONT_FAMILY,
        font_size_scaled = FONT_SIZE_SCALED,
        label_x_scaled = label_x_scaled,
        message_x_scaled = message_x_scaled,
        shadow_text_y_scaled = shadow_text_y_scaled,
        text_y_scaled = text_y_scaled,
        label_text_length_scaled = label_text_length_scaled,
        message_text_length_scaled = message_text_length_scaled,
    )
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
