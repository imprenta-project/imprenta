//! A ledger long enough to cross pages, built with the table layout.
//!
//! Everything visual here comes from the caller: the engine chose no colour,
//! no rule and no padding. Swap the decorations and it is a different table.
//!
//! Run: `cargo run -p imprenta-pdf --release --example ledger_table -- preview/`

use imprenta_core::color::Color;
use imprenta_core::units::{Edges, Length, Pt};
use imprenta_pdf::atom::Atom;
use imprenta_pdf::content::Content;
use imprenta_pdf::decoration::{BorderSide, Decoration};
use imprenta_pdf::pack::{Contribution, Flow, Group, Repeat, pack};
use imprenta_pdf::render::{Geometry, Options, render_with};
use imprenta_pdf::shape::Shaper;
use imprenta_pdf::table::{Align, Cell, Column, Layout};

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");
const ENTRIES: usize = 120;

fn hex(s: &str) -> Color {
    Color::parse_hex(s).expect("hex")
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or("preview".into());
    std::fs::create_dir_all(&out).expect("mkdir");

    let geometry = Geometry::a4();
    let pad = Edges::symmetric(Pt(2.5), Pt(4.0));
    let content_width = geometry.width - geometry.margin.horizontal() - pad.horizontal();

    let layout = Layout::new(
        vec![
            Column::new(Length::Pt(Pt(52.0))),
            Column::new(Length::Auto),
            Column::new(Length::Pt(Pt(78.0))),
            Column::new(Length::Pt(Pt(66.0))).aligned(Align::End),
            Column::new(Length::Pt(Pt(66.0))).aligned(Align::End),
        ],
        content_width,
    );

    let navy = hex("#1F4E79");
    let ink = hex("#333333");
    let hairline = BorderSide {
        width: Pt(0.4),
        color: hex("#DCE6F5"),
    };

    let mut shaper = Shaper::new(ROBOTO.to_vec());
    let mut atoms: Vec<Atom> = Vec::new();
    let mut contents: Vec<Content> = Vec::new();
    let mut contributions: Vec<Contribution> = Vec::new();

    let header = layout.row(
        &mut shaper,
        &["Cuenta", "Descripción", "Documento", "Debe", "Haber"]
            .map(|t| Cell::new(t, Pt(8.0)).inked(hex("#FFFFFF"))),
        Decoration {
            background: Some(navy),
            ..Default::default()
        },
        pad,
    );
    let header_height = header.height();
    atoms.push(Atom::new(header_height).keep_with_next());
    contents.push(Content::Box(header));

    let mut total = 0.0f64;
    for i in 0..ENTRIES {
        let amount = 100.0 + ((i * 137) % 900) as f64;
        total += amount;

        let row = layout.row(
            &mut shaper,
            &[
                Cell::new(format!("{}", 430000 + i % 40), Pt(8.0)).inked(ink),
                Cell::new(
                    format!(
                        "Prestación de servicios profesionales, cliente {}",
                        i * 7 % 500
                    ),
                    Pt(8.0),
                )
                .inked(ink),
                Cell::new(format!("FV-2026-{:05}", i + 1), Pt(8.0)).inked(ink),
                Cell::new(format!("{amount:.2}"), Pt(8.0)).inked(ink),
                Cell::new("—", Pt(8.0)).inked(hex("#AAAAAA")),
            ],
            Decoration {
                background: (i % 2 == 0).then(|| hex("#F5F8FD")),
                border: Edges {
                    bottom: Some(hairline),
                    ..Default::default()
                },
                ..Default::default()
            },
            pad,
        );

        contributions.push(Contribution {
            atom: atoms.len(),
            accumulator: 0,
            value: amount,
        });
        atoms.push(Atom::new(row.height()));
        contents.push(Content::Box(row));
    }

    let groups = vec![Group {
        atoms: 0..atoms.len(),
        repeat_prefix: Some(Repeat {
            atom: 0,
            height: header_height,
        }),
    }];

    let pages = pack(
        &Flow::new(&atoms)
            .with_groups(&groups)
            .with_accumulators(1, &contributions),
        geometry.content_height(),
    );
    let pdf = render_with(&pages, &contents, ROBOTO, geometry, Options::default()).expect("render");

    let path = format!("{out}/ledger-table.pdf");
    std::fs::write(&path, &pdf).expect("write");

    for (i, page) in pages.iter().enumerate() {
        std::fs::write(
            format!("{out}/ledger-{:02}.pdf", i + 1),
            render_with(
                std::slice::from_ref(page),
                &contents,
                ROBOTO,
                geometry,
                Options::default(),
            )
            .expect("render"),
        )
        .expect("write");
    }

    println!("{path}: {} pages, total {total:.2}", pages.len());
    for (i, p) in pages.iter().enumerate() {
        println!(
            "  page {}: {:3} rows, brought forward {:>10.2}, carried forward {:>10.2}{}",
            i + 1,
            p.placements.len(),
            p.opening[0],
            p.closing[0],
            if p.continuations.is_empty() {
                ""
            } else {
                "   (header repeated)"
            }
        );
    }
}
