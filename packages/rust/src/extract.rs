use pdfium_render::prelude::*;
use crate::types::{TextItem, Page};

// If gap is < fontSize * CHAR_X_GAP_THRESHOLD, consider it part of the same word
const CHAR_X_GAP_THRESHOLD: f32 = 0.3;

// If gap is < fontSize * NEGATIVE_X_GAP_THRESHOLD, consider it a negative gap (e.g. kerning) and start a new word
const NEGATIVE_X_GAP_THRESHOLD: f32 = -0.2;

// If gap is > fontSize * CHAR_Y_GAP_THRESHOLD, consider it a new item
const CHAR_Y_GAP_THRESHOLD: f32 = 0.25;

/// Extract pages from a PDF file and return them as structured data.
pub fn extract_pages(pdf_path: &str, page_num: Option<u32>) -> Result<Vec<Page>, Box<dyn std::error::Error>> {
    let pdfium = Pdfium::new(
        Pdfium::bind_to_statically_linked_library().unwrap()
    );

    let document = pdfium.load_pdf_from_file(pdf_path, None)?;
    let mut pages = Vec::new();

    for (page_index, page) in document.pages().iter().enumerate() {
        if let Some(target_page) = page_num {
            if page_index as u32 + 1 != target_page {
                continue;
            }
        }

        let text_items = extract_page_text_items(&page)?;

        pages.push(Page {
            page_number: page_index + 1,
            page_width: page.width().value,
            page_height: page.height().value,
            text_items,
        });
    }

    Ok(pages)
}

/// Extract raw text items and print each page as a JSON-line object to stdout.
pub fn extract(pdf_path: &str, page_num: Option<u32>) -> Result<(), Box<dyn std::error::Error>> {
    let pages = extract_pages(pdf_path, page_num)?;
    for page in &pages {
        println!("{}", serde_json::to_string(page)?);
    }
    Ok(())
}

fn extract_page_text_items(page: &PdfPage) -> Result<Vec<TextItem>, Box<dyn std::error::Error>> {
    let mut text_items = Vec::new();
    let mut cur_item: Option<TextItem> = None;

    for object in page.objects().iter() {
        if let Some(object) = object.as_text_object() {
            let bounds = object.bounds().unwrap();
            let obj_x = bounds.x1.value;
            let obj_y = bounds.y1.value;
            // Get rotation and normalize to 0-360
            let obj_rotation = object.get_rotation_counter_clockwise_degrees();
            let obj_rotation = if obj_rotation < 0.0 { obj_rotation + 360.0 } else { obj_rotation };

            // scaled_font_size() = unscaled * matrix.d (vertical scale).
            // For 90° rotated text, matrix.d ≈ 0, so scaled_font_size returns 0.
            // In that case, compute from the matrix horizontal scale: sqrt(a² + b²) * unscaled.
            let unscaled_fs = object.unscaled_font_size().value;
            let scaled_fs = object.scaled_font_size().value;
            let obj_font_size = if scaled_fs > 0.0 {
                scaled_fs
            } else {
                // Use horizontal scale factor from the transformation matrix
                let matrix = object.matrix().unwrap();
                let h_scale = (matrix.a() * matrix.a() + matrix.b() * matrix.b()).sqrt();
                unscaled_fs * h_scale
            };

            let is_vertical = (obj_rotation > 45.0 && obj_rotation < 135.0)
                || (obj_rotation > 225.0 && obj_rotation < 315.0);
            let (obj_w, obj_h) = if is_vertical {
                // 90°/270° — object.width()/height() return 0, use bounds + font size
                let bounds_h = (bounds.y2.value - bounds.y1.value).abs();
                (obj_font_size, f32::max(bounds_h, obj_font_size))
            } else {
                // 0°/180° — normal: object.width() works, font size for height
                (object.width().unwrap().value, obj_font_size)
            };

            if obj_rotation > 1.0 {
                eprintln!(
                    "ROTATED: text='{}' rot={:.1} bounds=({:.2},{:.2})-({:.2},{:.2}) scaled_fs={:.2}",
                    object.text(), obj_rotation,
                    bounds.x1.value, bounds.y1.value, bounds.x2.value, bounds.y2.value,
                    obj_font_size
                );
            }

            if let Some(ref mut item) = cur_item {
                let cur_font_size = item.font_size.unwrap_or(obj_font_size);
                let rotation_matches = (item.rotation - obj_rotation).abs() < 1.0;
                let cur_is_vertical = (item.rotation > 45.0 && item.rotation < 135.0)
                    || (item.rotation > 225.0 && item.rotation < 315.0);

                // For vertical text, "reading direction" is along y-axis, so swap gap roles:
                // - reading_gap: distance along reading direction (x for horizontal, y for vertical)
                // - cross_gap: distance across lines (y for horizontal, x for vertical)
                let (reading_gap, cross_gap) = if cur_is_vertical {
                    let y_gap = obj_y - (item.y + item.height);
                    let x_gap = (item.x - obj_x).abs();
                    (y_gap, x_gap)
                } else {
                    let x_gap = obj_x - (item.x + item.width);
                    let y_gap = (item.y - obj_y).abs();
                    (x_gap, y_gap)
                };

                // Vertical text glyphs commonly overlap along the reading axis,
                // so use a more lenient negative threshold to allow merging.
                let neg_threshold = if cur_is_vertical { -0.5 } else { NEGATIVE_X_GAP_THRESHOLD };
                if reading_gap < cur_font_size * neg_threshold || !rotation_matches {
                    // Negative gap or rotation difference — start new item
                    text_items.push(cur_item.take().unwrap());
                    cur_item = Some(TextItem {
                        text: object.text().to_string(),
                        x: obj_x,
                        y: obj_y,
                        width: obj_w,
                        height: obj_h,
                        rotation: obj_rotation,
                        font_name: Some(object.font().family().to_string()),
                        font_size: Some(obj_font_size),
                    });
                } else if reading_gap < cur_font_size * CHAR_X_GAP_THRESHOLD && cross_gap < cur_font_size * CHAR_Y_GAP_THRESHOLD {
                    // Same word — merge directly
                    item.text.push_str(object.text().trim_start());
                    if cur_is_vertical {
                        item.height = (obj_y + obj_h) - item.y;
                        item.width = f32::max(item.width, obj_w);
                    } else {
                        item.width = (obj_x + obj_w) - item.x;
                        item.height = f32::max(item.height, obj_h);
                    }
                } else {
                    // Large gap — flush current item and start new one
                    text_items.push(cur_item.take().unwrap());
                    cur_item = Some(TextItem {
                        text: object.text().to_string(),
                        x: obj_x,
                        y: obj_y,
                        width: obj_w,
                        height: obj_h,
                        rotation: obj_rotation,
                        font_name: Some(object.font().family().to_string()),
                        font_size: Some(obj_font_size),
                    });
                }
            } else {
                // First text object on this page
                cur_item = Some(TextItem {
                    text: object.text().to_string(),
                    x: obj_x,
                    y: obj_y,
                    width: obj_w,
                    height: obj_h,
                    rotation: obj_rotation,
                    font_name: Some(object.font().family().to_string()),
                    font_size: Some(obj_font_size),
                });
            }
        }
    }

    // Push the last item if it has text
    if let Some(item) = cur_item.take() {
        if !item.text.is_empty() {
            text_items.push(item);
        }
    }

    Ok(text_items)
}
