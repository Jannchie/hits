use anyhow::{Context, Result}; // For easier error handling in main
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade}, // WebSocket imports
        Extension,
        Path,
        State, // To access shared state in ws handler
    },
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
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
use serde::Serialize;
use utoipa::OpenApi;
use utoipa_rapidoc::RapiDoc;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// --- Type alias for the broadcast sender ---
type Broadcaster = broadcast::Sender<String>; // Sends the 'key' string

// --- API Documentation Structure ---
#[derive(OpenApi)]
#[openapi(
    components(
        schemas(ApiError, AppInfo) // Include ApiError and AppInfo
    ),
    tags(
        (name = "Meta", description = "Meta API Endpoints"),
        (name = "Main", description = "Main API Endpoints"),
        (name = "WebSocket", description = "WebSocket Endpoints") // Add WebSocket tag
    ),
    paths(
        count_increment_route,
        app_info_route,
        // ws_handler cannot be directly documented with `#[utoipa::path]`
        // as it involves protocol upgrade, but we can describe it.
    ),
    info(
        title = "Hits API",
        version = VERSION,
        description = r#"A simple API to increment and retrieve counts based on a key.
Includes a WebSocket endpoint `/ws` that broadcasts the `key` whenever a count is incremented."#, // Updated description
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
        (status, Json(api_error)).into_response()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt::init();

    // --- Configuration ---
    let database_url =
        env::var("DATABASE_URL").context("DATABASE_URL environment variable must be set")?;
    let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
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
    // Create a channel for broadcasting keys. Capacity determines buffer size.
    let (tx, _) = broadcast::channel::<String>(100); // tx is the Sender
    let broadcaster = Arc::new(tx); // Wrap Sender in Arc for sharing

    // --- Axum Router Setup ---
    let app = Router::new()
        // --- API Documentation ---
        .merge(RapiDoc::with_openapi("/api-docs/openapi.json", ApiDoc::openapi()).path("/rapidoc"))
        // --- API Routes ---
        .route("/hits/{key}", get(count_increment_route)) // Use ':' for path params
        .route("/", get(app_info_route))
        // --- WebSocket Route ---
        .route("/ws", get(ws_handler)) // Add the WebSocket route
        .layer(
            ServiceBuilder::new()
                .layer(Extension(pool)) // Share the database pool
                .layer(Extension(broadcaster.clone())) // Share the broadcast sender <--- ADDED
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
        // Add state for the ws_handler to access the broadcaster directly
        // Alternatively, ws_handler can use Extension too, but State is clearer if only used there
        .with_state(broadcaster); // Make broadcaster available via State extractor in ws_handler

    // --- Start Server ---
    info!("Starting server, listening on http://{}", addr);
    info!("Access Rapidoc UI at http://{}/rapidoc", addr);
    info!("WebSocket endpoint available at ws://{}/ws", addr); // Log WS URL

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to address {}", addr))?;

    axum::serve(listener, app.into_make_service())
        .await
        .context("Web server failed")?;

    Ok(())
}

// --- WebSocket Handler ---

// This handler manages the WebSocket connection upgrade request.
// Tag = "WebSocket" (Note: utoipa::path doesn't directly support WebSocket upgrades)
async fn ws_handler(
    ws: WebSocketUpgrade,                        // Extractor to handle the upgrade
    State(broadcaster): State<Arc<Broadcaster>>, // Get the broadcaster sender from shared state
) -> impl IntoResponse {
    info!("WebSocket connection request received");
    // Finalize the upgrade process and provide a callback to handle the actual socket
    ws.on_upgrade(move |socket| handle_socket(socket, broadcaster))
}

// This function runs once a WebSocket connection is established.
async fn handle_socket(socket: WebSocket, broadcaster: Arc<Broadcaster>) {
    info!("WebSocket connection established");

    // Split the socket into a sender and receiver.
    // `split()` consumes the original socket.
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Subscribe to the broadcast channel *before* starting the main loop
    let mut rx = broadcaster.subscribe();

    // --- Send Task ---
    // This task listens for broadcast messages and sends them to the client.
    // It takes ownership of `ws_sender` and `rx`.
    let send_task = tokio::spawn(async move {
        loop {
            // Wait for a new message from the broadcast channel
            match rx.recv().await {
                Ok(key) => {
                    // Send the received key as a text message to the client using the ws_sender
                    if ws_sender.send(Message::Text(key.into())).await.is_err() {
                        // If sending fails, the client likely disconnected
                        warn!("WebSocket send failed, client disconnected?");
                        break; // Exit the loop
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WebSocket receiver lagged behind by {} messages.", n);
                    // Continue listening, but messages were missed
                }
                Err(broadcast::error::RecvError::Closed) => {
                    // The broadcast channel was closed, no more messages will be sent
                    warn!("Broadcast channel closed.");
                    break; // Exit the loop
                }
            }
        }
        info!("WebSocket send task finished.");
        // ws_sender is dropped here
    });

    // --- Receive Task ---
    // This task listens for messages incoming from the client.
    // It takes ownership of `ws_receiver`.
    let recv_task = tokio::spawn(async move {
        // Loop while the client is sending messages
        while let Some(msg_result) = ws_receiver.next().await {
            match msg_result {
                Ok(msg) => {
                    // Process the received message
                    match msg {
                        Message::Text(t) => {
                            info!("Received text from WebSocket client: {}", t);
                            // Handle client messages if needed (e.g., echo, commands)
                        }
                        Message::Binary(_) => {
                            info!("Received binary data from WebSocket client.");
                        }
                        Message::Ping(_) => {
                            info!("Received WebSocket ping.");
                            // Axum/Tungstenite handles pong responses automatically via the Sink
                        }
                        Message::Pong(_) => {
                            info!("Received WebSocket pong.");
                        }
                        Message::Close(_) => {
                            info!("WebSocket client initiated close.");
                            break; // Exit loop on close message
                        }
                    }
                }
                Err(e) => {
                    // An error occurred receiving a message (e.g., connection dropped)
                    warn!("Error receiving WebSocket message: {}", e);
                    break; // Exit loop on error
                }
            }
        }
        info!("WebSocket receive task finished.");
        // ws_receiver is dropped here
    });

    // Wait for either task to complete (e.g., disconnection or channel close)
    tokio::select! {
        _ = send_task => { /* Send task finished */ }
        _ = recv_task => { /* Receive task finished */ }
    }

    info!("WebSocket connection closed.");
    // The socket and associated tasks will be dropped here.
}

// --- Existing HTTP Handlers (Modified count_increment_route) ---

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
        (status = 200, description = "Successfully incremented and returned total count.", body = i64, example = json!(15)), // Changed body to i64
        (status = 500, description = "Database error", body = ApiError) // Document error response
    )
)]
async fn count_increment_route(
    Path(key): Path<String>,
    Extension(pool): Extension<PgPool>,
    Extension(broadcaster): Extension<Arc<Broadcaster>>, // Inject the broadcaster sender
) -> Result<Json<i64>, AppError> {
    // Result type includes AppError
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
    .await?; // Propagates sqlx::Error as AppError::DatabaseError

    // Safely get the total count
    let total_count_i64 = record.total_count.unwrap_or(0);

    // Broadcast the key if it was successfully inserted/updated
    // `upserted_key` will be Some(key) on success, None only if WHERE failed (unlikely here)
    if let Some(upserted_key) = record.upserted_key {
        // Send the key to the broadcast channel.
        // Ignore the error if there are no subscribers (it's okay if no one is listening).
        if let Err(e) = broadcaster.send(upserted_key.clone()) {
            // Log if needed, but don't fail the request
            warn!("Failed to broadcast key '{}': {}", upserted_key, e);
        } else {
            info!("Broadcasted key: {}", upserted_key);
        }
    } else {
        // This case should theoretically not happen with the current query structure
        warn!("Could not retrieve upserted key for broadcasting: {}", key);
    }

    Ok(Json(total_count_i64))
}

#[derive(Serialize, utoipa::ToSchema)]
struct AppInfo {
    project_name: String,
    version: String,
    docs_path: String,
    #[schema(example = "ws://localhost:3030/ws")] // Add example WS URL
    websocket_url: String, // Add field for WebSocket URL info
}

#[utoipa::path(
    get,
    summary = "App Info",
    description = "Returns information about the application, including API docs and WebSocket endpoint.",
    path = "/",
    responses(
        (status = 200, description = "Returns information about the application.", body = AppInfo, example = json!({ "project_name": "Hits", "version": "0.1.0", "docs_path": "/rapidoc", "websocket_url": "ws://<host>:<port>/ws" }))
    ),
    tag = "Meta"
)]
async fn app_info_route() -> impl IntoResponse {
    // Ideally, get host/port dynamically if needed, but hardcoding is simple for example
    let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("PORT").unwrap_or_else(|_| "3030".to_string());
    let ws_url = format!("ws://{}:{}/ws", host, port);

    let info = AppInfo {
        project_name: "Hits".to_string(),
        version: VERSION.to_string(),
        docs_path: "/rapidoc".to_string(),
        websocket_url: ws_url, // Include the WS URL
    };
    Json(info)
}
