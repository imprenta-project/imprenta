//! A ledger extract with shaded rows and rules — the first output that needs
//! more than glyphs.
//!
//! Text and boxes are separate atoms stacked at the same place, which is why
//! a row is two entries here rather than one. A `Table` primitive will own
//! that pairing; until it exists, the example does it by hand.
//!
//! Run: `cargo run -p imprenta-pdf --example ruled_rows -- preview/`

use imprenta_core::color::Color;
use imprenta_core::units::{Edges, Pt};
use imprenta_pdf::atom::Atom;
use imprenta_pdf::content::{BoxContent, Content};
use imprenta_pdf::decoration::{BorderSide, Decoration};
use imprenta_pdf::pack::{Flow, pack};
use imprenta_pdf::render::{Geometry, Options, render_with};
use imprenta_pdf::shape::Shaper;

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");
const ROW: f32 = 14.0;

fn hex(s: &str) -> Color {
    Color::parse_hex(s).expect("hex")
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or("preview".into());
    std::fs::create_dir_all(&out).expect("mkdir");

    let geometry = Geometry::a4();
    let column = geometry.width - geometry.margin.horizontal();
    let mut shaper = Shaper::new(ROBOTO.to_vec());

    let mut atoms: Vec<Atom> = Vec::new();
    let mut contents: Vec<Content> = Vec::new();

    // One atom per row: the box *contains* its text, so the text is painted
    // over its own background and the next row cannot cover it.
    let row = |shaper: &mut Shaper,
               atoms: &mut Vec<Atom>,
               contents: &mut Vec<Content>,
               text: &str,
               size: f32,
               ink: Color,
               decoration: Decoration| {
        let line = shaper
            .break_lines(text, Pt(size), column)
            .remove(0)
            .with_color(ink);
        let pad = Pt((ROW - line.height.get()) / 2.0);

        let boxed = BoxContent::new(decoration)
            .with_padding(Edges::symmetric(pad, Pt(4.0)))
            .stack(Content::Text(line));

        atoms.push(Atom::new(boxed.height()));
        contents.push(Content::Box(boxed));
    };

    let navy = hex("#1F4E79");
    let header = Decoration {
        background: Some(navy),
        border: Edges::default(),
        ..Default::default()
    };
    let stripe = Decoration {
        background: Some(hex("#F2F6FC")),
        border: Edges {
            bottom: Some(BorderSide {
                width: Pt(0.4),
                color: hex("#D6E4F7"),
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let plain = Decoration {
        background: None,
        border: Edges {
            bottom: Some(BorderSide {
                width: Pt(0.4),
                color: hex("#D6E4F7"),
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let total = Decoration {
        background: Some(hex("#EEF2FF")),
        border: Edges {
            top: Some(BorderSide {
                width: Pt(1.0),
                color: navy,
            }),
            ..Default::default()
        },
        ..Default::default()
    };

    row(
        &mut shaper,
        &mut atoms,
        &mut contents,
        "   Cuenta      Nombre                       Documento         Debe        Haber",
        8.0,
        hex("#FFFFFF"),
        header,
    );

    let mut sum = 0.0f32;
    for i in 0..24u32 {
        let amount = 100.0 + ((i * 137) % 900) as f32;
        sum += amount;
        let text = format!(
            "   {}    Cliente comercial número {:<4}    FV-2026-{:04}    {:>9.2}        —",
            430000 + i % 40,
            (i * 7) % 500,
            i + 1,
            amount
        );
        let decoration = if i % 2 == 0 { stripe } else { plain };
        row(
            &mut shaper,
            &mut atoms,
            &mut contents,
            &text,
            8.0,
            hex("#333333"),
            decoration,
        );
    }

    row(
        &mut shaper,
        &mut atoms,
        &mut contents,
        &format!("   Suma y sigue{:>62.2}", sum),
        8.0,
        hex("#1F4E79"),
        total,
    );

    let pages = pack(&Flow::new(&atoms), geometry.content_height());
    let pdf = render_with(&pages, &contents, ROBOTO, geometry, Options::default()).expect("render");
    let path = format!("{out}/ruled-rows.pdf");
    std::fs::write(&path, &pdf).expect("write");
    println!(
        "{path}: {} pages, {:.1} KB",
        pages.len(),
        pdf.len() as f64 / 1024.0
    );
}
