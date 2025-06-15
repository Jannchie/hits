//! API 相关类型定义

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

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

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BadgeStyle {
    Flat,
    FlatSquare,
    Plastic,
    Social,
    ForTheBadge,
}

pub fn default_label() -> String {
    "Hits".to_string()
}

pub fn default_badge_style() -> BadgeStyle {
    BadgeStyle::Flat
}
pub fn default_label_color() -> String {
    "#555".to_string()
}
pub fn default_message_color() -> String {
    "#007ec6".to_string()
}

/// 用于生成 Hit Badge 的参数
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct HitBadgeParams {
    /// The style of the badge
    #[serde(default = "default_badge_style")]
    pub style: BadgeStyle,

    /// The label text on the left side of the badge
    #[serde(default = "default_label")]
    pub label: String,

    /// The color of the label text
    #[serde(default = "default_label_color")]
    pub label_color: String,

    /// The message text on the right side of the badge
    #[serde(default = "default_message_color")]
    pub message_color: String,

    /// The link to the badge (optional)
    pub link: Option<String>,

    pub extra_link: Option<String>,

    /// The logo to display on the badge
    pub logo: Option<String>,

    /// The width of the logo in pixels
    pub logo_color: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct AppInfo {
    pub project_name: String,
    pub version: String,
    pub docs_path: String,
}
