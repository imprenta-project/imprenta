//! What the writer produces, read back out of the bytes.
//!
//! These assert on the file rather than on the API, because the file is the
//! product. A writer whose every method behaved and whose output no reader
//! could open would pass a unit test of each method in turn.

use imprenta_core::color::Color;
use imprenta_pdf_write::{Glyph, ImageFormat, PathOp, Region, Settings, Writer};

const ROBOTO: &[u8] = include_bytes!("../../imprenta-pdf/tests/fonts/Roboto-Regular.ttf");

fn readable() -> Settings {
    Settings { compress: false }
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

/// Glyphs for `text`, shaped the crude way: one glyph per character through
/// the font's own `cmap`.
///
/// Enough for these tests and deliberately not a shaper — the engine's own
/// shaping is tested where it lives, and a second one here would only be a
/// second thing to be wrong.
fn glyphs(text: &str, size: f32) -> Vec<Glyph> {
    use skrifa::MetadataProvider;
    let font = skrifa::FontRef::new(ROBOTO).unwrap();
    let charmap = font.charmap();
    let upem = f32::from(
        skrifa::raw::TableProvider::head(&font)
            .unwrap()
            .units_per_em(),
    );
    let metrics = font.glyph_metrics(
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::default(),
    );

    let mut out = Vec::new();
    let mut at = 0usize;
    for c in text.chars() {
        let id = charmap.map(c).unwrap_or_default();
        out.push(Glyph {
            id: id.to_u32() as u16,
            x_advance: metrics.advance_width(id).unwrap_or(0.0) / upem * size,
            text: at..at + c.len_utf8(),
        });
        at += c.len_utf8();
    }
    out
}

/// A one-page document with `text` on it.
fn one_page(text: &str, settings: Settings) -> imprenta_pdf_write::Pdf {
    let mut writer = Writer::new(settings);
    let face = writer.add_face(ROBOTO.to_vec()).unwrap();
    let mut page = writer.page(595.0, 842.0);
    page.glyphs(
        face,
        10.0,
        40.0,
        60.0,
        &glyphs(text, 10.0),
        text,
        Color::BLACK,
    );
    page.finish();
    writer.finish().unwrap()
}

#[test]
fn the_output_is_a_pdf_a_reader_can_find_its_way_around() {
    let pdf = one_page("Prestación", Settings::default());

    assert_eq!(&pdf[..5], b"%PDF-");
    assert!(pdf.ends_with(b"%%EOF\n") || pdf.ends_with(b"%%EOF"));
    assert!(count(&pdf, b"\nxref\n") == 1, "no cross-reference table");
    assert!(
        count(&pdf, b"/Root ") == 1,
        "the trailer names no catalogue"
    );
}

#[test]
fn every_object_the_xref_points_at_really_starts_there() {
    // The one thing a streaming writer can get wrong that nothing else can:
    // an offset recorded before the bytes moved. A reader follows the table
    // rather than scanning, so a table that is off by one byte is a file that
    // opens as blank.
    let mut writer = Writer::new(readable());
    let face = writer.add_face(ROBOTO.to_vec()).unwrap();
    for n in 0..12 {
        let text = format!("Pagina {n}");
        let mut page = writer.page(595.0, 842.0);
        page.glyphs(
            face,
            10.0,
            40.0,
            60.0,
            &glyphs(&text, 10.0),
            &text,
            Color::BLACK,
        );
        page.finish();
    }
    let pdf = writer.finish().unwrap();

    for (number, offset) in xref(&pdf) {
        if offset == 0 {
            continue;
        }
        let head = format!("{number} 0 obj");
        assert!(
            pdf[offset..].starts_with(head.as_bytes()),
            "the table says object {number} starts at {offset}, and it does not:\n{:?}",
            String::from_utf8_lossy(&pdf[offset..(offset + 24).min(pdf.len())]),
        );
    }
}

/// `(object number, byte offset)` for every entry in the cross-reference
/// table, read the way a PDF reader reads it: twenty bytes an entry.
fn xref(pdf: &[u8]) -> Vec<(usize, usize)> {
    let text = String::from_utf8_lossy(pdf);
    let start = text.rfind("\nxref\n").expect("no xref") + 6;
    let rest = &text[start..];
    let header = rest.lines().next().unwrap();
    let count: usize = header.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = &rest[header.len() + 1..];

    (0..count)
        .filter_map(|i| {
            let entry = body.get(i * 20..i * 20 + 20)?;
            let offset: usize = entry[..10].parse().ok()?;
            Some((i, offset))
        })
        .collect()
}

#[test]
fn one_page_object_is_written_per_page() {
    let mut writer = Writer::new(readable());
    for _ in 0..5 {
        writer.page(595.0, 842.0).finish();
    }
    let pdf = writer.finish().unwrap();

    assert_eq!(count(&pdf, b"/Type /Page\n"), 5);
    assert_eq!(count(&pdf, b"/Count 5"), 1);
}

#[test]
fn a_document_with_no_pages_is_refused_rather_than_written_broken() {
    let writer = Writer::new(readable());

    assert!(writer.finish().is_err());
}

#[test]
fn the_font_is_embedded_and_says_what_its_glyphs_mean() {
    let pdf = one_page("Prestación", readable());

    assert!(count(&pdf, b"/FontFile2") > 0, "no embedded font programme");
    assert!(count(&pdf, b"/ToUnicode") > 0, "no ToUnicode map");
    assert!(count(&pdf, b"beginbfchar") > 0, "the map is empty");

    let mut mapped: Vec<String> = tounicode(&pdf).into_iter().map(|(_, s)| s).collect();
    mapped.sort();
    mapped.dedup();
    let mut expected: Vec<String> = "Prestación".chars().map(String::from).collect();
    expected.sort();
    expected.dedup();

    assert_eq!(mapped, expected, "the map does not match the page's text");
}

/// `(cid, text)` out of the `ToUnicode` CMap.
fn tounicode(pdf: &[u8]) -> Vec<(u32, String)> {
    let text = String::from_utf8_lossy(pdf);
    let mut out = Vec::new();
    for block in text.split("beginbfchar").skip(1) {
        let Some(block) = block.split("endbfchar").next() else {
            continue;
        };
        for line in block.lines() {
            let hex: Vec<&str> = line
                .split(['<', '>'])
                .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit()))
                .collect();
            if hex.len() != 2 {
                continue;
            }
            let Ok(cid) = u32::from_str_radix(hex[0], 16) else {
                continue;
            };
            let units: Vec<u16> = hex[1]
                .as_bytes()
                .chunks(4)
                .filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
                .collect();
            if let Ok(s) = String::from_utf16(&units) {
                out.push((cid, s));
            }
        }
    }
    out
}

#[test]
fn a_face_that_was_registered_and_never_drawn_with_is_not_embedded() {
    // A document that declares bold and never uses it should not carry a
    // second copy of the family — and a `/Font` entry pointing at an object
    // that was never written is a file some readers refuse outright.
    let mut writer = Writer::new(readable());
    let _regular = writer.add_face(ROBOTO.to_vec()).unwrap();
    let _bold = writer.add_face(ROBOTO.to_vec()).unwrap();
    writer.page(595.0, 842.0).finish();
    let pdf = writer.finish().unwrap();

    assert_eq!(count(&pdf, b"/FontFile2"), 0);
}

#[test]
fn a_filled_path_comes_out_as_a_path_and_a_fill() {
    let mut writer = Writer::new(readable());
    {
        let mut page = writer.page(200.0, 200.0);
        page.fill(
            &[
                PathOp::MoveTo(10.0, 10.0),
                PathOp::LineTo(90.0, 10.0),
                PathOp::LineTo(90.0, 50.0),
                PathOp::LineTo(10.0, 50.0),
                PathOp::Close,
            ],
            Color::rgb(31, 78, 121),
        );
        page.finish();
    }
    let pdf = writer.finish().unwrap();
    let stream = String::from_utf8_lossy(&pdf).to_string();

    assert!(stream.contains(" m\n"), "no subpath was started");
    assert!(stream.contains("\nh\n"), "the path was not closed");
    assert!(stream.contains("\nf\n"), "the path was never filled");
    assert!(
        stream.contains("0.121"),
        "the colour is not in the operators"
    );
}

#[test]
fn a_colour_is_set_once_and_not_before_every_run() {
    // The paint state is the stream's, not the caller's. A table of forty
    // thousand black rows that set the ink before each of them would be forty
    // thousand operators nobody needs.
    let mut writer = Writer::new(readable());
    let face = writer.add_face(ROBOTO.to_vec()).unwrap();
    {
        let mut page = writer.page(595.0, 842.0);
        for line in 0..8 {
            page.glyphs(
                face,
                10.0,
                40.0,
                60.0 + 12.0 * line as f32,
                &glyphs("uno", 10.0),
                "uno",
                Color::BLACK,
            );
        }
        page.finish();
    }
    let pdf = writer.finish().unwrap();

    assert_eq!(count(&pdf, b" rg\n"), 1, "the ink was set again and again");
}

#[test]
fn a_translucent_fill_gets_a_graphics_state_and_an_opaque_one_does_not() {
    let mut opaque = Writer::new(readable());
    {
        let mut page = opaque.page(200.0, 200.0);
        page.fill(
            &[PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(10.0, 10.0)],
            Color::BLACK,
        );
        page.finish();
    }
    let opaque = opaque.finish().unwrap();

    let mut faded = Writer::new(readable());
    {
        let mut page = faded.page(200.0, 200.0);
        page.fill(
            &[PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(10.0, 10.0)],
            Color {
                a: 128,
                ..Color::BLACK
            },
        );
        page.finish();
    }
    let faded = faded.finish().unwrap();

    assert_eq!(
        count(&opaque, b"/ca "),
        0,
        "an opaque fill asked for a state"
    );
    assert_eq!(count(&faded, b"/ca "), 1, "no fill alpha was written");
    assert_eq!(count(&faded, b"/CA "), 1, "no stroke alpha was written");
}

#[test]
fn an_image_is_written_once_however_many_pages_it_appears_on() {
    // A JPEG, which carries no alpha: a transparent PNG is two image
    // objects — itself and its mask — and this is counting how many times
    // *one* picture reaches the file.
    let logo = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../imprenta-pdf/tests/images/logo.jpg"
    ))
    .unwrap();

    let mut writer = Writer::new(readable());
    let logo: std::sync::Arc<[u8]> = logo.into();
    let image = writer.add_image(&logo, ImageFormat::Jpeg).expect("decode");
    for _ in 0..6 {
        let mut page = writer.page(595.0, 842.0);
        page.image(image, 40.0, 40.0, 80.0, 40.0);
        page.finish();
    }
    let pdf = writer.finish().unwrap();

    assert_eq!(count(&pdf, b"/Subtype /Image"), 1);
    assert_eq!(count(&pdf, b"/i0 Do"), 6, "the logo is not on every page");
}

#[test]
fn a_link_becomes_an_annotation_on_its_own_page() {
    let mut writer = Writer::new(readable());
    {
        let mut page = writer.page(595.0, 842.0);
        page.link(
            Region {
                x: 40.0,
                y: 50.0,
                width: 120.0,
                height: 12.0,
            },
            "https://example.test/factura",
        );
        page.finish();
    }
    writer.page(595.0, 842.0).finish();
    let pdf = writer.finish().unwrap();

    assert_eq!(count(&pdf, b"/Subtype /Link"), 1);
    assert!(count(&pdf, b"example.test") > 0, "the target was lost");
    assert_eq!(count(&pdf, b"/Annots"), 1, "the second page got one too");
}

#[test]
fn the_same_document_written_twice_is_the_same_bytes() {
    // Determinism is what makes a golden PDF worth diffing in CI, and a
    // subset tag pulled from a counter or a clock would quietly break it.
    assert_eq!(
        one_page("Prestación", readable()),
        one_page("Prestación", readable())
    );
}

#[test]
fn compression_changes_the_size_and_not_the_shape() {
    let small = one_page("Prestación de servicios profesionales", Settings::default());
    let plain = one_page("Prestación de servicios profesionales", readable());

    assert!(small.len() < plain.len(), "compression did nothing");
    assert_eq!(
        count(&small, b"/Type /Page\n"),
        count(&plain, b"/Type /Page\n")
    );
    assert_eq!(tounicode(&plain).len(), tounicode(&small).len());
}
