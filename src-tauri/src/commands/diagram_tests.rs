#![allow(clippy::unwrap_used)]
use super::*;

fn drawing(width: u32, height: u32, body: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
<rect width="{width}" height="{height}" fill="#ffffff"/>
<rect x="10" y="10" width="280" height="80" rx="4" fill="#f6f8fa" stroke="#d3dce3"/>
<text x="20" y="30" font-family="Atkinson Hyperlegible" font-weight="700" font-size="14" fill="#15212c">orders</text>
<text x="20" y="50" font-family="JetBrains Mono" font-size="12" fill="#4f6070">id integer</text>
{body}
</svg>"##
    )
}

#[test]
fn png_is_a_png_at_twice_the_size() {
    let bytes = render(&drawing(300, 100, ""), DiagramFormat::Png).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    // IHDR width and height, big-endian.
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 600);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 200);
}

#[test]
fn jpeg_is_a_jpeg() {
    let bytes = render(&drawing(300, 100, ""), DiagramFormat::Jpeg).unwrap();
    assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF]);
    assert_eq!(&bytes[bytes.len() - 2..], &[0xFF, 0xD9]);
}

#[test]
fn pdf_is_a_pdf_with_its_fonts() {
    let bytes = render(&drawing(300, 100, ""), DiagramFormat::Pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("FontFile2"), "text is drawn with embedded fonts");
}

#[test]
fn the_bundled_fonts_are_found_by_name() {
    let db = fonts();
    let families: Vec<String> = db
        .faces()
        .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
        .collect();
    assert!(
        families.iter().any(|f| f == "Atkinson Hyperlegible"),
        "{families:?}"
    );
    assert!(families.iter().any(|f| f == "JetBrains Mono"), "{families:?}");
    // Text in the drawing must come out as glyphs, not be dropped for a missing font.
    let tree = parse(&drawing(300, 100, "")).unwrap();
    let mut texts = 0;
    for node in tree.root().children() {
        if let usvg::Node::Text(text) = node {
            texts += 1;
            assert!(
                !text.flattened().children().is_empty(),
                "a text element drew nothing"
            );
        }
    }
    assert_eq!(texts, 2);
}

#[test]
fn outside_references_are_refused() {
    for body in [
        r#"<image href="file:///C:/Windows/win.ini" width="10" height="10"/>"#,
        r#"<image xlink:href="secret.png" width="10" height="10"/>"#,
        r#"<image href="https://example.com/a.png" width="10" height="10"/>"#,
        r#"<image href="data:image/png;base64,iVBORw0KGgo=" width="10" height="10"/>"#,
    ] {
        for format in [DiagramFormat::Png, DiagramFormat::Jpeg, DiagramFormat::Pdf] {
            let result = render(&drawing(300, 100, body), format);
            assert!(result.is_err(), "{format:?} accepted {body}");
        }
    }
}

#[test]
fn bad_input_is_refused() {
    assert!(render("not svg", DiagramFormat::Png).is_err());
    let huge = drawing(300, 100, &"<!-- padding -->".repeat(MAX_SVG_BYTES / 16 + 1));
    assert!(render(&huge, DiagramFormat::Png).is_err());
}

#[test]
fn a_huge_diagram_is_scaled_down() {
    let (scale, width, height) = raster_size(20_000.0, 500.0);
    assert!(width <= MAX_SIDE && height <= MAX_SIDE);
    assert!(scale < 1.0);
    let (_, width, height) = raster_size(12_000.0, 12_000.0);
    assert!((width as f32) * (height as f32) <= MAX_PIXELS);
    let (scale, width, height) = raster_size(300.0, 100.0);
    assert_eq!((scale, width, height), (2.0, 600, 200));
    // A real render of a wide diagram stays inside the limits.
    let bytes = render(&drawing(10_000, 200, ""), DiagramFormat::Png).unwrap();
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), MAX_SIDE);
}

#[test]
fn a_tall_diagram_fits_one_pdf_page() {
    let bytes = render(&drawing(300, 40_000, ""), DiagramFormat::Pdf).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    let start = text.find("/MediaBox [").unwrap() + "/MediaBox [".len();
    let numbers: Vec<f32> = text[start..]
        .split(']')
        .next()
        .unwrap()
        .split_whitespace()
        .map(|n| n.parse().unwrap())
        .collect();
    assert!(numbers[3] <= 14_400.0, "{numbers:?}");
}

#[test]
fn file_names_are_cleaned() {
    assert_eq!(file_stem("public"), "public");
    assert_eq!(file_stem("../../etc/passwd"), "_.._etc_passwd");
    assert_eq!(file_stem("a:b*c?"), "a_b_c_");
    assert_eq!(file_stem("..."), "diagram");
    assert_eq!(file_stem(""), "diagram");
}
