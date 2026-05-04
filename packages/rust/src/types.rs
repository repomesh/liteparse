use serde::{Deserialize, Serialize};

/// Supported output formats for parsed documents.
/// - `"json"` — Structured JSON with per-page text items, bounding boxes, and metadata.
/// - `"text"` — Plain text with spatial layout preserved.
#[derive(Debug, Serialize, Deserialize)]
pub enum OutputFormat {
    Json,
    Text,
}

/// Accepted input types for input documents.
/// - `FilePath(String)` — A file path to a local PDF document.
/// - `Buffer(Vec<u8>)` — A byte buffer containing the PDF data.
#[derive(Debug, Serialize, Deserialize)]
pub enum InputType {
    FilePath(String),
    Buffer(Vec<u8>),
}

/// Represents a single text item extracted from a PDF page,
/// including its content, position, size, rotation, and font metadata.
#[derive(Debug, Clone, Serialize)]
pub struct TextItem {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub rotation: f32,
    pub font_name: Option<String>,
    pub font_size: Option<f32>,
}

/// Represents a single page in a PDF document, including its dimensions and extracted text items.
#[derive(Debug, Serialize)]
pub struct Page {
    pub page_number: usize,
    pub page_width: f32,
    pub page_height: f32,
    pub text_items: Vec<TextItem>,
}

/// Represents a fully parsed page with projected text layout.
#[derive(Debug, Serialize)]
pub struct ParsedPage {
    pub page_number: usize,
    pub page_width: f32,
    pub page_height: f32,
    pub text: String,
    pub text_items: Vec<TextItem>,
}

#[derive(Debug, Serialize)]
pub enum Snap {
    Left,
    Right,
    Center,
}

#[derive(Debug, Serialize)]
pub enum Anchor {
    Left,
    Right,
    Center,
}

/// Represents a Projected piece of text, responsible for keeping track of projection related data
#[derive(Debug, Serialize)]
pub struct ProjectedTextItem {
    pub item: TextItem,
    pub snap: Snap,
    pub anchor: Anchor,
    pub is_dup: bool,
    pub rendered: bool,
    pub num_spaces: usize,
    pub force_unsnapped: bool,
    pub is_margin_line_number: bool,
    pub rotated: bool,
    pub d: f32,
}
