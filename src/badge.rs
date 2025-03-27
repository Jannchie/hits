use serde::Deserialize;

use crate::font_metrics;

// --- Constants for badge styling ---
const BADGE_HEIGHT: u32 = 20;
const HORIZONTAL_PADDING: u32 = 6; // Padding left/right of text
const FONT_FAMILY: &str = "Verdana,Geneva,DejaVu Sans,sans-serif";
const FONT_SIZE_SCALED: u32 = 110; // Corresponds to font-size="11" with transform="scale(.1)"

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum BadgeStyle {
    Flat,
    Social,
    FlatSquare,
    Plastic,
}

impl Default for BadgeStyle {
    fn default() -> Self {
        BadgeStyle::Flat
    }
}

pub fn default_label_color() -> &'static str {
    "#555"
}

pub fn default_message_color() -> &'static str {
    "#007ec6"
}

// 假设 BadgeStyle 也实现了 Default
impl<'a> Default for RenderBadgeParams<'a> {
    fn default() -> Self {
        // 注意：这里的 &'a str 处理可能比较棘手
        // 通常 Default 实现会使用 &'static str 或 String/Cow
        // 如果必须是 &'a str，Default 可能不适用，或者默认值需要特殊处理
        Self {
            style: BadgeStyle::default(),
            label: "", // 需要一个 &'a str 类型的默认值，这通常很难提供
            // 除非你改成 &'static str 或者 String
            message: "",                            // 同上
            label_color: default_label_color(),     // 假设返回 &'static str
            message_color: default_message_color(), // 假设返回 &'static str
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct RenderBadgeParams<'a> {
    #[serde(default)]
    pub style: BadgeStyle,
    pub label: &'a str,
    pub message: &'a str,
    #[serde(default = "default_label_color")]
    pub label_color: &'a str,
    #[serde(default = "default_message_color")]
    pub message_color: &'a str,
}

pub fn render_badge_svg(params: RenderBadgeParams) -> String {
    match params.style {
        BadgeStyle::Flat => render_flat_badge_svg(
            params.label,
            params.message,
            params.label_color,
            params.message_color,
        ),
        BadgeStyle::Social => render_social_badge_svg(params.label, params.message),
        BadgeStyle::FlatSquare => generate_flat_square_style_svg(
            params.label,
            params.message,
            params.label_color,
            params.message_color,
        ),
        BadgeStyle::Plastic => render_plastic_style_svg(
            params.label,
            params.message,
            params.label_color,
            params.message_color,
        ),
    }
}

// --- SVG Generation Function for "Flat" Style ---
// (Extracted from your original code)
fn render_flat_badge_svg(
    label: &str,
    message: &str,
    label_color: &str,
    message_color: &str,
) -> String {
    // Calculate SVG dimensions based on text using the font metrics module
    let label_text_render_width = font_metrics::get_text_width_px(label, FONT_FAMILY);
    let message_text_render_width = font_metrics::get_text_width_px(message, FONT_FAMILY);

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

fn render_social_badge_svg(label: &str, message: &str) -> String {
    // Note: _label_color and _message_color are ignored for social style, using fixed colors.
    let badge_height: u32 = BADGE_HEIGHT; // 20
    let rect_height: u32 = badge_height - 1; // 19 (for 0.5px offset)
    let corner_radius: u32 = 2; // Social style uses slightly rounded corners

    // Calculate text widths using the font metrics module
    let label_text_render_width = font_metrics::get_text_width_px(label, SOCIAL_FONT_FAMILY);
    let message_text_render_width = font_metrics::get_text_width_px(message, SOCIAL_FONT_FAMILY);

    // Calculate dimensions of the two main parts
    let label_part_width = label_text_render_width + 2 * SOCIAL_HORIZONTAL_PADDING;
    let message_part_width = message_text_render_width + 2 * SOCIAL_HORIZONTAL_PADDING;

    // Calculate overall width and positioning
    // total_width = label_width + gap + message_width (using dimensions for positioning)
    let message_rect_start_x = label_part_width + SOCIAL_GAP;
    // Final SVG width needs to encompass everything including the 0.5 offsets
    let total_width = (message_rect_start_x + message_part_width) as f32 + 0.5f32; // Add 0.5 for the right edge offset
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

    // Calculate SVG dimensions based on text using the font metrics module
    let label_text_render_width = font_metrics::get_text_width_px(label, FONT_FAMILY);
    let message_text_render_width = font_metrics::get_text_width_px(message, FONT_FAMILY);

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
fn render_plastic_style_svg(
    label: &str,
    message: &str,
    label_color: &str,
    message_color: &str,
) -> String {
    let badge_height = BADGE_HEIGHT;
    let corner_radius = 3; // Standard rounded corner for plastic

    // Calculate SVG dimensions based on text using the font metrics module
    let label_text_render_width = font_metrics::get_text_width_px(label, FONT_FAMILY);
    let message_text_render_width = font_metrics::get_text_width_px(message, FONT_FAMILY);

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
