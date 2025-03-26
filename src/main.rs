use anyhow::{Context, Result}; // For easier error handling in main
use axum::{
    extract::{Extension, Path},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use dotenv::dotenv;
use sqlx::postgres::PgPool;
use std::{env, net::SocketAddr};
use thiserror::Error; // For creating the custom error type
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::{error, info, info_span, Span}; // Import info_span

// --- OpenAPI Imports ---
use serde::Serialize;
use utoipa::OpenApi;
use utoipa_rapidoc::RapiDoc;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// --- API Documentation Structure ---
#[derive(OpenApi)]
#[openapi(
    components(
        schemas(ApiError) // Include error response schema if defined
    ),
    tags(
        (name = "Meta", description = "Meta API Endpoints"),
        (name = "Main", description = "Main API Endpoints")
    ),
    paths(
        count_increment_route, // Include the count_increment_route in the API documentation
        app_info_route,
    ),
    info(
        title = "Hits API",
        version = VERSION,
        description = "A simple API to increment and retrieve counts based on a key.",
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

// Define a more structured error response for the API (Optional but recommended)
#[derive(Serialize, utoipa::ToSchema)] // Derive Serialize and ToSchema
struct ApiError {
    #[schema(example = "Internal Server Error")]
    message: String,
    // You could add more fields like error_code, details, etc.
}

// Define a custom application error type
#[derive(Debug, Error)]
enum AppError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
}

// Implement IntoResponse for AppError to convert errors into HTTP responses
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // Log the full error details internally for debugging
        error!("Error processing request: {}", self);

        let (status, error_message) = match self {
            AppError::DatabaseError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "An unexpected database error occurred.".to_string(),
            ),
        };

        // Create the structured error response
        let api_error = ApiError {
            message: error_message,
        };

        // Return a JSON error response
        (status, Json(api_error)).into_response()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Use anyhow::Result for main
    // Load environment variables from .env file, ignore errors if .env is missing
    dotenv().ok();

    // Initialize tracing subscriber for logging
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

    // --- Axum Router Setup ---
    let app = Router::new()
        // --- Add Swagger UI Route ---
        // This route serves the Swagger UI interface
        .merge(RapiDoc::with_openapi("/api-docs/openapi.json", ApiDoc::openapi()).path("/rapidoc"))
        // --- Your API Routes ---
        .route("/count/{key}", get(count_increment_route))
        .route("/", get(app_info_route))
        .layer(
            ServiceBuilder::new()
                .layer(Extension(pool)) // Share the database pool via Extension layer
                .layer(
                    // Add tracing layer for HTTP request logging
                    TraceLayer::new_for_http()
                        .make_span_with(|request: &Request<axum::body::Body>| {
                            // Create a span for each request
                            info_span!( // Use info_span! macro
                                "HTTP Request",
                                method = %request.method(),
                                uri = %request.uri(),
                                // Add other fields like request_id from headers if available
                                // status_code will be added by on_response
                            )
                        })
                        .on_response(
                            |response: &Response, latency: std::time::Duration, span: &Span| {
                                span.record("status_code", response.status().as_u16());
                                // Explicitly log the status code here
                                info!(
                                    status_code = response.status().as_u16(),
                                    latency = ?latency,
                                    "HTTP Response"
                                );
                            },
                        ),
                ),
        );

    // --- Start Server ---
    info!("Starting server, listening on http://{}", addr);
    info!("Access Rapidoc UI at http://{}/rapidoc", addr); // Log Swagger UI URL
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to address {}", addr))?;

    // Run the server
    axum::serve(listener, app.into_make_service()) // Use into_make_service for potential graceful shutdown features
        .await
        .context("Web server failed")?;

    Ok(())
}

#[utoipa::path(
    get, // HTTP Method
    summary = "Increment and Get Total Count", // Short description
    description = "Increments a counter for the given key within the current minute window and returns the total count for that key across all time windows.", // Detailed description
    path = "/count/{key}", // Route path, matching Axum's route
    tag = "Main", // Group endpoints in the API documentation
    params(
        // Describe path parameters
        ("key" = String, Path, description = "The unique key for the counter to increment.")
    ),
    responses(
        // Describe possible responses
        (status = 200, description = "Successfully incremented and returned total count.", body = i32, example = json!(15)),
    )
)]
async fn count_increment_route(
    Path(key): Path<String>,            // Extract the 'key' path parameter
    Extension(pool): Extension<PgPool>, // Extract the database pool
) -> Result<Json<i64>, AppError> {
    // Return type indicates success (Json<i32>) or AppError
    // Execute the combined INSERT/UPDATE and SELECT SUM query
    // Use '?' which implicitly uses 'From<sqlx::Error>' for AppError
    let record = sqlx::query!(
        r#"
        WITH updated AS (
            INSERT INTO counters (key, count, minute_window)
            VALUES ($1, 1, DATE_TRUNC('minute', NOW() AT TIME ZONE 'UTC')) -- Use UTC for consistency
            ON CONFLICT (key, minute_window)
            DO UPDATE SET count = counters.count + 1
        )
        SELECT SUM(c.count) AS total_count -- Sum across all time windows for the key
        FROM counters c
        WHERE c.key = $1; 
        "#,
        key
    )
    .fetch_one(&pool) // Expect exactly one row (the sum)
    .await?; // Use '?' to propagate sqlx::Error, which converts to AppError::DatabaseError

    // `SUM` returns Option<i64> in sqlx for integer types.
    // If no rows exist for the key (shouldn't happen after the INSERT), SUM is NULL.
    let total_count_i64 = record.total_count.unwrap_or(0) + 1;
    Ok(Json(total_count_i64)) 
}

#[derive(Serialize, utoipa::ToSchema)]
struct AppInfo {
    project_name: String,
    version: String,
    docs_path: String, // Changed from docs_url to docs_path based on refinement
}

#[utoipa::path(
    get,
    summary = "App Info",
    description = "Returns information about the application.",
    path = "/",
    responses( 
        (status = 200, description = "Returns information about the application.", body = AppInfo, example = json!({ "project_name": "Hits", "version": "0.1.0", "docs_path": "/rapidoc" }))
    ),
    tag = "Meta"
)]
async fn app_info_route() -> impl IntoResponse {
    // Use impl IntoResponse for flexibility
    let info = AppInfo {
        project_name: "Hits".to_string(),  // From #[openapi]
        version: VERSION.to_string(),      // From the const
        docs_path: "/rapidoc".to_string(), // The path configured in main
    };
    Json(info) // Return as JSON
}
