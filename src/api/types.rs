//! API 相关类型定义

use serde::{Serialize, Deserialize};
use utoipa::ToSchema;

/// API 错误响应结构体
#[derive(Serialize, ToSchema)]
pub struct ApiError {
    #[schema(example = "Internal Server Error")]
    pub message: String,
}

/// Shields.io Badge 结构体
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")] // Use camelCase for JSON field names (shields.io standard)
pub struct ShieldsIoBadge {
    pub schema_version: u8, // Should be 1
    pub label: String,      // The left side of the badge
    pub message: String,    // The right side of the badge (the count)
    pub color: String,      // e.g., "blue", "green", hex codes like "ff69b4"
    // Optional: Add fields like `labelColor`, `isError`, `namedLogo`, `logoSvg`, `logoColor`, `logoWidth`, `logoPosition`, `style`, `cacheSeconds` if needed
}

// --- HitBadgeParams 及默认值函数 ---
use crate::badge::BadgeStyle;

/// serde 默认值函数
pub fn default_label() -> String {
    "Hits".to_string()
}
pub fn default_label_color() -> String {
    "#555".to_string()
}
pub fn default_message_color() -> String {
    "#007ec6".to_string()
}

/// SVG 徽章参数
#[derive(Deserialize, Debug)]
pub struct HitBadgeParams {
    #[serde(default)]
    pub style: BadgeStyle,
    #[serde(default = "default_label")]
    pub label: String,
    #[serde(default = "default_label_color")]
    pub label_color: String,
    #[serde(default = "default_message_color")]
    pub message_color: String,
}

/// 应用信息结构体
#[derive(Serialize, utoipa::ToSchema)]
pub struct AppInfo {
    pub project_name: String,
    pub version: String,
    pub docs_path: String,
}
