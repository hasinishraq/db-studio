//! Saves the schema diagram as PNG, JPEG or PDF. The webview sends the drawing as SVG text, which
//! is untrusted: it is size-checked, may not refer to any file or URL, and is drawn with fonts
//! bundled in the app. Rust opens the save dialog, so the webview never names a path.

use super::schema_io::file_error;
use resvg::tiny_skia::{Color, Pixmap, Transform};
use resvg::usvg::{self, fontdb, ImageHrefResolver};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use studio_types::{AppError, DiagramFormat, ErrorCode};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

/// The largest drawing accepted.
pub const MAX_SVG_BYTES: usize = 8 * 1024 * 1024;
/// Raster images are drawn at this multiple of the diagram's size, for sharp text.
const SCALE: f32 = 2.0;
/// The longest side of a raster image, in pixels.
pub const MAX_SIDE: u32 = 16_384;
/// The most pixels in a raster image, so a huge diagram cannot take gigabytes of memory.
pub const MAX_PIXELS: f32 = 40_000_000.0;
/// PDF readers refuse pages longer than 200 inches (14,400 points); this leaves a margin.
const PDF_MAX_POINTS: f32 = 14_000.0;
const JPEG_QUALITY: u8 = 90;

const SANS: &str = "Atkinson Hyperlegible";
const FONTS: [&[u8]; 4] = [
    include_bytes!("../../fonts/AtkinsonHyperlegible-Regular.ttf"),
    include_bytes!("../../fonts/AtkinsonHyperlegible-Bold.ttf"),
    include_bytes!("../../fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../fonts/JetBrainsMono-Bold.ttf"),
];

/// The bundled fonts, loaded once. No system font is used, so every machine draws the same.
fn fonts() -> Arc<fontdb::Database> {
    static DB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        for font in FONTS {
            db.load_font_data(font.to_vec());
        }
        db.set_sans_serif_family(SANS);
        db.set_serif_family(SANS);
        db.set_monospace_family("JetBrains Mono");
        Arc::new(db)
    })
    .clone()
}

/// Reads the drawing. Any reference to an image, file or URL is refused.
fn parse(svg: &str) -> Result<usvg::Tree, AppError> {
    if svg.len() > MAX_SVG_BYTES {
        return Err(AppError::invalid("the diagram is larger than 8 MB"));
    }
    let reached_out = AtomicBool::new(false);
    let options = usvg::Options {
        resources_dir: None,
        font_family: SANS.to_owned(),
        fontdb: fonts(),
        image_href_resolver: ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| {
                reached_out.store(true, Ordering::SeqCst);
                None
            }),
            resolve_string: Box::new(|_, _| {
                reached_out.store(true, Ordering::SeqCst);
                None
            }),
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(svg, &options)
        .map_err(|e| AppError::invalid(format!("the diagram could not be read: {e}")))?;
    if reached_out.load(Ordering::SeqCst) {
        return Err(AppError::invalid(
            "the diagram refers to an outside image or file",
        ));
    }
    Ok(tree)
}

/// The scale and pixel size of a raster image, kept within the side and area limits.
pub fn raster_size(width: f32, height: f32) -> (f32, u32, u32) {
    let mut scale = SCALE;
    let longest = width.max(height).max(1.0);
    if longest * scale > MAX_SIDE as f32 {
        scale = MAX_SIDE as f32 / longest;
    }
    let area = width * scale * height * scale;
    if area > MAX_PIXELS {
        scale *= (MAX_PIXELS / area).sqrt();
    }
    let side = |length: f32| ((length * scale).floor() as u32).clamp(1, MAX_SIDE);
    (scale, side(width), side(height))
}

fn raster(tree: &usvg::Tree) -> Result<Pixmap, AppError> {
    let size = tree.size();
    let (scale, width, height) = raster_size(size.width(), size.height());
    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "the image could not be allocated"))?;
    // Opaque white under everything, so JPEG needs no alpha and PNG matches it.
    pixmap.fill(Color::WHITE);
    resvg::render(tree, Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(pixmap)
}

/// Draws the SVG in the chosen format.
pub fn render(svg: &str, format: DiagramFormat) -> Result<Vec<u8>, AppError> {
    let tree = parse(svg)?;
    match format {
        DiagramFormat::Png => raster(&tree)?
            .encode_png()
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("PNG encoding failed: {e}"))),
        DiagramFormat::Jpeg => {
            let pixmap = raster(&tree)?;
            // The background is opaque, so premultiplied RGBA is plain RGBA here.
            let rgb: Vec<u8> = pixmap
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|[r, g, b, _]| [*r, *g, *b])
                .collect();
            let mut out = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
                .encode(
                    &rgb,
                    pixmap.width(),
                    pixmap.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|e| AppError::new(ErrorCode::Internal, format!("JPEG encoding failed: {e}")))?;
            Ok(out)
        }
        DiagramFormat::Pdf => {
            let size = tree.size();
            // At 96 DPI one pixel is 0.75 points; a very large diagram is shrunk to fit a page.
            let longest = size.width().max(size.height());
            let dpi = (longest * 72.0 / PDF_MAX_POINTS).max(96.0);
            svg2pdf::to_pdf(
                &tree,
                svg2pdf::ConversionOptions::default(),
                svg2pdf::PageOptions { dpi },
            )
            .map_err(|e| AppError::new(ErrorCode::Internal, format!("PDF conversion failed: {e}")))
        }
    }
}

/// A file name without path separators or other characters a file system may refuse.
fn file_stem(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(100)
        .collect();
    let clean = clean
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_owned();
    if clean.is_empty() {
        "diagram".to_owned()
    } else {
        clean
    }
}

/// Saves the diagram to a file the user chooses.
#[tauri::command]
#[specta::specta]
pub async fn export_diagram(
    app: AppHandle,
    svg: String,
    format: DiagramFormat,
    name: String,
) -> Result<(), AppError> {
    // Draw first, so a bad drawing fails before the user picks a file.
    let bytes = tokio::task::spawn_blocking(move || render(&svg, format))
        .await
        .map_err(|e| AppError::new(ErrorCode::Internal, e.to_string()))??;
    let (label, extension) = match format {
        DiagramFormat::Png => ("PNG image", "png"),
        DiagramFormat::Jpeg => ("JPEG image", "jpg"),
        DiagramFormat::Pdf => ("PDF document", "pdf"),
    };
    let chosen = app
        .dialog()
        .file()
        .add_filter(label, &[extension])
        .set_file_name(format!("{}.{extension}", file_stem(&name)))
        .blocking_save_file()
        .ok_or_else(|| AppError::new(ErrorCode::Cancelled, "no file was chosen"))?;
    let path = chosen.into_path().map_err(file_error)?;
    tokio::fs::write(path, bytes).await.map_err(file_error)
}

#[cfg(test)]
#[path = "diagram_tests.rs"]
mod diagram_tests;
