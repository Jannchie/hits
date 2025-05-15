use axum::{
    response::{IntoResponse, Response},
    Json,
    http::{StatusCode, header, HeaderValue},
};
use tracing::error;
use crate::api::types::ApiError;
use thiserror::Error;

/// 应用自定义错误类型
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
}

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