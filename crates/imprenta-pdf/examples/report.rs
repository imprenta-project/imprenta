//! An analytical report — same engine, deliberately nothing like the invoice.
//!
//! Dark banner, stat cards on a grid, a ruled section, a compact data table
//! with a highlighted total, and build diagnostics printed at the end. The
//! engine contributed no colour, spacing or alignment to any of it.
//!
//! Run: `cargo run -p imprenta-pdf --release --example report -- preview/`

use imprenta_core::color::Color;
use imprenta_core::diagnostic::Diagnostics;
use imprenta_core::units::{Edges, Length, Pt};
use imprenta_pdf::atom::Atom;
use imprenta_pdf::content::{
    BoxContent, CanvasContent, Content, ImageContent, ImageFormat, LinkContent,
};
use imprenta_pdf::decoration::{BorderSide, Decoration};
use imprenta_pdf::measure::{TextStyle, measure_text};
use imprenta_pdf::pack::{Flow, Group, Repeat, pack};
use imprenta_pdf::render::{Geometry, Options, render_with};
use imprenta_pdf::shape::Shaper;
use imprenta_pdf::table::{Align, Cell, Column, Layout, Overflow};

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");
const MARK: &[u8] = include_bytes!("../tests/images/mark.png");

fn hex(s: &str) -> Color {
    Color::parse_hex(s).expect("hex")
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or("preview".into());
    std::fs::create_dir_all(&out).expect("mkdir");

    let geometry = Geometry {
        width: Pt::mm(210.0),
        height: Pt::mm(297.0),
        margin: Edges::all(Pt::mm(14.0)),
        bands: Default::default(),
    };
    let full = geometry.width - geometry.margin.horizontal();

    let charcoal = hex("#14212E");
    let teal = hex("#0E7C86");
    let sand = hex("#F7F4EE");
    let ink = hex("#22303C");
    let muted = hex("#8894A0");
    let white = hex("#FFFFFF");
    let rule = BorderSide {
        width: Pt(0.5),
        color: hex("#E2E6EA"),
    };

    let mut shaper = Shaper::new(ROBOTO.to_vec());
    let mut atoms: Vec<Atom> = Vec::new();
    let mut contents: Vec<Content> = Vec::new();
    let mut diagnostics = Diagnostics::default();

    let para = |shaper: &mut Shaper, text: &str, size: f32, color: Color, width: Pt| {
        let m = measure_text(shaper, text, TextStyle::new(Pt(size)), width);
        let mut b = BoxContent::default().with_width(width);
        for line in m.lines {
            b = b.stack(Content::Text(line.with_color(color)));
        }
        b
    };

    let push = |atoms: &mut Vec<Atom>, contents: &mut Vec<Content>, c: Content| {
        atoms.push(Atom::new(c.height()));
        contents.push(c);
    };
    let gap = |atoms: &mut Vec<Atom>, contents: &mut Vec<Content>, h: f32| {
        atoms.push(Atom::new(Pt(h)));
        contents.push(Content::Empty);
    };

    // ── Dark banner ──────────────────────────────────────────────────────
    let banner = BoxContent::new(Decoration {
        background: Some(charcoal),
        ..Default::default()
    })
    .with_width(full)
    .with_padding(Edges::symmetric(Pt(18.0), Pt(20.0)))
    .place(
        Pt(0.0),
        Content::Box(para(
            &mut shaper,
            "INFORME TRIMESTRAL",
            8.0,
            teal,
            Pt(260.0),
        )),
    )
    .place(
        Pt(full.get() - 60.0),
        Content::Image(ImageContent::scaled_to_width(
            MARK,
            ImageFormat::Png,
            (64, 64),
            Pt(26.0),
        )),
    )
    .stack(Content::Box(para(
        &mut shaper,
        "Actividad económica · Q3 2026",
        21.0,
        white,
        Pt(360.0),
    )));
    push(&mut atoms, &mut contents, Content::Box(banner));

    gap(&mut atoms, &mut contents, 16.0);

    // ── Stat cards ───────────────────────────────────────────────────────
    let card_w = Pt((full.get() - 24.0) / 3.0);
    let mut cards = BoxContent::default().with_width(full);
    for (i, (label, value, delta)) in [
        ("FACTURACIÓN", "1.284.930 €", "+12,4 %"),
        ("ASIENTOS", "48.216", "+3,1 %"),
        ("MOROSIDAD", "1,8 %", "−0,4 pp"),
    ]
    .iter()
    .enumerate()
    {
        let inner = card_w - Pt(28.0);
        let card = BoxContent::new(Decoration {
            background: Some(sand),
            border: Edges {
                bottom: Some(BorderSide {
                    width: Pt(2.0),
                    color: teal,
                }),
                ..Default::default()
            },
            ..Default::default()
        })
        .with_width(card_w)
        .with_padding(Edges::all(Pt(14.0)))
        .stack(Content::Box(para(&mut shaper, label, 7.0, muted, inner)))
        .stack(Content::Box(para(
            &mut shaper,
            value,
            17.0,
            charcoal,
            inner,
        )))
        .stack(Content::Box(para(&mut shaper, delta, 8.0, teal, inner)));
        cards = cards.place(Pt(i as f32 * (card_w.get() + 12.0)), Content::Box(card));
    }
    push(&mut atoms, &mut contents, Content::Box(cards));

    gap(&mut atoms, &mut contents, 18.0);

    // ── A bar chart, drawn with raw path operations ──────────────────────
    //
    // Nothing in the engine knows what a chart is. Bars are rectangles and
    // rectangles are paths, so this needed no new primitive — which is the
    // whole point of having one.
    let chart_h = 92.0f32;
    let series = [62.0f32, 48.0, 71.0, 55.0, 83.0, 44.0, 68.0, 91.0];
    let bar_w = (full.get() - 7.0 * 9.0) / 8.0;
    let mut chart = CanvasContent::new(full, Pt(chart_h)).filled(teal);
    for (i, value) in series.iter().enumerate() {
        let h = chart_h * (value / 100.0);
        chart = chart.rect(
            Pt(i as f32 * (bar_w + 9.0)),
            Pt(chart_h - h),
            Pt(bar_w),
            Pt(h),
        );
    }
    push(&mut atoms, &mut contents, Content::Canvas(chart));
    gap(&mut atoms, &mut contents, 6.0);
    push(
        &mut atoms,
        &mut contents,
        Content::Box(para(
            &mut shaper,
            "Facturación mensual, en miles de euros",
            7.0,
            muted,
            full,
        )),
    );
    gap(&mut atoms, &mut contents, 22.0);

    // ── Section heading with a rule under it ─────────────────────────────
    let heading = BoxContent::new(Decoration {
        background: None,
        border: Edges {
            bottom: Some(rule),
            ..Default::default()
        },
        ..Default::default()
    })
    .with_width(full)
    .with_padding(Edges::symmetric(Pt(0.0), Pt(0.0)))
    .stack(Content::Box(para(
        &mut shaper,
        "Desglose por delegación",
        13.0,
        charcoal,
        full,
    )))
    .stack(Content::Empty);
    let heading_atom = atoms.len();
    atoms.push(Atom::new(heading.height()).keep_with_next());
    contents.push(Content::Box(heading));
    let _ = heading_atom;

    gap(&mut atoms, &mut contents, 10.0);

    // ── Compact data table, tight columns on purpose ─────────────────────
    let pad = Edges::symmetric(Pt(3.5), Pt(8.0));
    let table = Layout::new(
        vec![
            Column::new(Length::Pt(Pt(120.0))).overflowing(Overflow::Ellipsis),
            Column::new(Length::Auto),
            Column::new(Length::Pt(Pt(70.0))).aligned(Align::End),
            Column::new(Length::Pt(Pt(58.0))).aligned(Align::End),
            Column::new(Length::Pt(Pt(58.0))).aligned(Align::End),
        ],
        full - pad.horizontal(),
    );

    let table_start = atoms.len();
    let header = table.row_reporting(
        &mut shaper,
        &["Delegación", "Responsable", "Facturación", "Peso", "Var."]
            .map(|t| Cell::new(t, Pt(7.5)).inked(muted)),
        Decoration {
            background: None,
            border: Edges {
                bottom: Some(BorderSide {
                    width: Pt(1.0),
                    color: charcoal,
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        pad,
        &mut diagnostics,
    );
    let header_height = header.height();
    atoms.push(Atom::new(header_height).keep_with_next());
    contents.push(Content::Box(header));

    let places = [
        ("Las Palmas de Gran Canaria — sede central", "M. Betancor"),
        ("Santa Cruz de Tenerife", "J. Rodríguez Peña"),
        ("Madrid — delegación peninsular norte", "A. Sáez"),
        ("Barcelona", "L. Puigdemont i Roure"),
        ("Valencia", "C. Ferrer"),
        ("Sevilla — Andalucía occidental", "R. Domínguez"),
        ("Bilbao", "I. Etxeberria Goikoetxea"),
        ("Palma de Mallorca", "N. Coll"),
    ];
    let mut total = 0.0f64;
    for (i, (place, who)) in places.iter().cycle().take(46).enumerate() {
        let amount = 18_000.0 + ((i * 4093) % 61_000) as f64;
        total += amount;
        let row = table.row_reporting(
            &mut shaper,
            &[
                Cell::new(*place, Pt(8.0)).inked(ink),
                Cell::new(*who, Pt(8.0)).inked(muted),
                Cell::new(format!("{amount:.0} €"), Pt(8.0)).inked(ink),
                Cell::new(format!("{:.1} %", amount / 1500.0), Pt(8.0)).inked(muted),
                Cell::new(if i % 3 == 0 { "+" } else { "−" }, Pt(8.0)).inked(if i % 3 == 0 {
                    teal
                } else {
                    hex("#B4453C")
                }),
            ],
            Decoration {
                background: None,
                border: Edges {
                    bottom: Some(rule),
                    ..Default::default()
                },
                ..Default::default()
            },
            pad,
            &mut diagnostics,
        );
        push(&mut atoms, &mut contents, Content::Box(row));
    }

    let total_row = table.row_reporting(
        &mut shaper,
        &[
            Cell::new("TOTAL", Pt(9.0)).inked(charcoal),
            Cell::new("", Pt(9.0)),
            Cell::new(format!("{total:.0} €"), Pt(9.0)).inked(charcoal),
            Cell::new("100,0 %", Pt(9.0)).inked(muted),
            Cell::new("", Pt(9.0)),
        ],
        Decoration {
            background: Some(sand),
            border: Edges {
                top: Some(BorderSide {
                    width: Pt(1.0),
                    color: charcoal,
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        pad,
        &mut diagnostics,
    );
    push(&mut atoms, &mut contents, Content::Box(total_row));

    let groups = vec![Group {
        atoms: table_start..atoms.len(),
        repeat_prefix: Some(Repeat {
            atom: table_start,
            height: header_height,
        }),
    }];

    // ── A clickable footer ───────────────────────────────────────────────
    gap(&mut atoms, &mut contents, 16.0);
    push(
        &mut atoms,
        &mut contents,
        Content::Link(Box::new(
            LinkContent::url(
                "https://imprenta.dev/informes",
                Content::Box(para(
                    &mut shaper,
                    "Metodología completa en imprenta.dev/informes",
                    8.0,
                    teal,
                    Pt(260.0),
                )),
            )
            .with_width(Pt(260.0)),
        )),
    );

    let pages = pack(
        &Flow::new(&atoms).with_groups(&groups),
        geometry.content_height(),
    );
    let pdf = render_with(&pages, &contents, ROBOTO, geometry, Options::default()).expect("render");
    std::fs::write(format!("{out}/report.pdf"), &pdf).expect("write");
    for (i, page) in pages.iter().enumerate() {
        std::fs::write(
            format!("{out}/report-{:02}.pdf", i + 1),
            render_with(
                std::slice::from_ref(page),
                &contents,
                ROBOTO,
                geometry,
                Options::default(),
            )
            .unwrap(),
        )
        .unwrap();
    }

    println!(
        "{out}/report.pdf: {} pages, {:.1} KB",
        pages.len(),
        pdf.len() as f64 / 1024.0
    );
    if diagnostics.is_empty() {
        println!("no diagnostics");
    } else {
        println!("{} diagnostic(s):", diagnostics.len());
        for d in diagnostics.iter() {
            println!("  {d}");
        }
    }
}
