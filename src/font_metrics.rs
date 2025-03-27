use fontdue::{Font, FontSettings};
use std::collections::HashMap;
use std::sync::OnceLock;

// Default font sizes used in badges
const DEFAULT_FONT_SIZE: f32 = 11.0; // Corresponds to font-size="11" with transform="scale(.1)"

// Font cache to avoid reloading fonts
static FONT_CACHE: OnceLock<FontCache> = OnceLock::new();

// Embedded font data to avoid file system dependencies
// Verdana font data (commonly used in badges)
static VERDANA_FONT_DATA: &[u8] = include_bytes!("../assets/fonts/verdana.ttf");
// Helvetica font data (used in social style badges)
static HELVETICA_FONT_DATA: &[u8] = include_bytes!("../assets/fonts/helvetica.ttf");

// Font cache structure
struct FontCache {
    fonts: HashMap<String, Font>,
}

impl FontCache {
    fn new() -> Self {
        let mut fonts = HashMap::new();
        
        // Load Verdana font
        let verdana_font = Font::from_bytes(
            VERDANA_FONT_DATA,
            FontSettings::default(),
        ).expect("Failed to load Verdana font");
        fonts.insert("verdana".to_string(), verdana_font);
        
        // Load Helvetica font
        let helvetica_font = Font::from_bytes(
            HELVETICA_FONT_DATA,
            FontSettings::default(),
        ).expect("Failed to load Helvetica font");
        fonts.insert("helvetica".to_string(), helvetica_font);
        
        Self { fonts }
    }
    
    fn get_font(&self, font_name: &str) -> Option<&Font> {
        // Normalize font name to lowercase for case-insensitive lookup
        let normalized_name = font_name.to_lowercase();
        
        // Try exact match first
        if let Some(font) = self.fonts.get(&normalized_name) {
            return Some(font);
        }
        
        // If no exact match, try to find a font that contains the requested name
        // This handles cases like "Verdana,Geneva,sans-serif" -> "verdana"
        for (name, font) in &self.fonts {
            if normalized_name.contains(name) {
                return Some(font);
            }
        }
        
        // If still no match, return the first font as fallback
        self.fonts.values().next()
    }
}

// Get the font cache, initializing it if necessary
fn get_font_cache() -> &'static FontCache {
    FONT_CACHE.get_or_init(FontCache::new)
}

/// Accurately measures text width using font metrics
pub fn measure_text_width(text: &str, font_family: &str, font_size: f32) -> f32 {
    let cache = get_font_cache();
    let font = cache.get_font(font_family).expect("Font not found");
    
    // Sum up the width of each character
    let mut total_width = 0.0;
    
    for c in text.chars() {
        let metrics = font.metrics(c, font_size);
        total_width += metrics.advance_width;
    }
    
    // Return the total width
    total_width
}

/// Convenience function that uses the default font size
pub fn measure_text_width_default(text: &str, font_family: &str) -> f32 {
    measure_text_width(text, font_family, DEFAULT_FONT_SIZE)
}

/// Converts the measured width to an integer pixel value suitable for SVG
pub fn get_text_width_px(text: &str, font_family: &str) -> u32 {
    // Ensure minimum width to avoid overly squashed text for very short strings
    const MIN_WIDTH: u32 = 5;
    
    let width = measure_text_width_default(text, font_family);
    let width_px = width.ceil() as u32;
    width_px.max(MIN_WIDTH)
}
