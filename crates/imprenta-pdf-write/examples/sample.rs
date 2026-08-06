//! Writes a sample document for a real reader to open.
//!
//! Some defects cannot be asserted on — a glyph drawn upside down, a page
//! that opens blank — and this is how somebody looks.

use imprenta_core::color::Color;
use imprenta_pdf_write::{Glyph, ImageFormat, PathOp, Region, Settings, Writer};
use skrifa::MetadataProvider;

const ROBOTO: &[u8] = include_bytes!("../../imprenta-pdf/tests/fonts/Roboto-Regular.ttf");

fn glyphs(text: &str, size: f32) -> Vec<Glyph> {
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

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/pdfcheck/sample.pdf".into());
    let mut writer = Writer::new(Settings::default());
    let face = writer.add_face(ROBOTO.to_vec()).unwrap();
    let logo = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../imprenta-pdf/tests/images/logo.png"
    ))
    .unwrap();
    let logo: std::sync::Arc<[u8]> = logo.into();
    let image = writer.add_image(&logo, ImageFormat::Png);

    for page_number in 1..=3 {
        let mut page = writer.page(595.2756, 841.8898);

        page.fill(
            &[
                PathOp::MoveTo(34.0, 100.0),
                PathOp::LineTo(561.0, 100.0),
                PathOp::LineTo(561.0, 118.0),
                PathOp::LineTo(34.0, 118.0),
                PathOp::Close,
            ],
            Color::rgb(240, 244, 250),
        );
        page.stroke(
            &[PathOp::MoveTo(34.0, 118.0), PathOp::LineTo(561.0, 118.0)],
            Color::rgb(150, 150, 150),
            0.5,
        );

        let title = "Libro mayor · ejercicio 2024";
        page.glyphs(
            face,
            14.0,
            34.0,
            60.0,
            &glyphs(title, 14.0),
            title,
            Color::rgb(31, 78, 121),
        );

        for row in 0..30 {
            let line = format!(
                "430000  {:02}/03/2024  Prestación de servicios profesionales  1.284,55",
                row + 1
            );
            page.glyphs(
                face,
                9.0,
                34.0,
                140.0 + 14.0 * row as f32,
                &glyphs(&line, 9.0),
                &line,
                Color::BLACK,
            );
        }

        let footer = format!("Página {page_number} de 3");
        page.glyphs(
            face,
            8.0,
            34.0,
            800.0,
            &glyphs(&footer, 8.0),
            &footer,
            Color::BLACK,
        );

        if let Some(image) = image {
            page.image(image, 460.0, 40.0, 100.0, 40.0);
        }
        page.link(
            Region {
                x: 34.0,
                y: 792.0,
                width: 90.0,
                height: 10.0,
            },
            "https://example.test/libro-mayor",
        );
        page.finish();
    }

    let pdf = writer.finish().unwrap();
    std::fs::write(&path, &pdf).unwrap();
    println!("wrote {path} ({} bytes)", pdf.len());
}
