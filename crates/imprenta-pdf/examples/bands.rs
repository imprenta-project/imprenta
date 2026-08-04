//! A ledger with a header and a "suma y sigue" footer on every page.

use imprenta_core::color::Color;
use imprenta_core::units::{Edges, Length, Pt};
use imprenta_pdf::build::{Assets, build};
use imprenta_pdf::ir;
use imprenta_pdf::render::Options;

const REGULAR: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");

fn text(runs: Vec<ir::Run>, size: f32, color: &str) -> ir::Node {
    ir::Node::Text(ir::Text {
        runs,
        style: ir::TextStyle {
            size: Pt(size),
            color: Color::parse_hex(color).unwrap(),
            ..Default::default()
        },
    })
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let navy = Color::parse_hex("#1b3a5c").unwrap();

    let document = ir::Document {
        page: ir::PageSetup {
            margin: Edges::all(Pt(40.0)),
            ..Default::default()
        },
        header: Some(ir::Band {
            height: Pt(38.0),
            children: vec![
                text(
                    vec![ir::Run::new("Libro mayor — ejercicio 2026")],
                    12.0,
                    "#1b3a5c",
                ),
                text(vec![ir::Run::new("Cuenta 430 · Clientes")], 8.0, "#64748b"),
            ],
        }),
        footer: Some(ir::Band {
            height: Pt(30.0),
            children: vec![ir::Node::Box(ir::Container {
                style: ir::BoxStyle {
                    border: Edges {
                        top: Some(ir::Border {
                            width: Pt(0.5),
                            color: Color::parse_hex("#cbd5e1").unwrap(),
                        }),
                        ..Default::default()
                    },
                    padding: Edges {
                        top: Pt(6.0),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                children: vec![text(
                    vec![
                        ir::Run::new("Suma anterior {{opening:saldo}}   ·   Suma y sigue "),
                        ir::Run {
                            text: "{{closing:saldo}}".into(),
                            weight: ir::Weight::Bold,
                            color: Some(navy),
                            ..ir::Run::new("")
                        },
                        ir::Run::new("        Pagina {{page}} de {{pages}}"),
                    ],
                    8.0,
                    "#475569",
                )],
            })],
        }),
        accumulators: vec!["saldo".into()],
        children: vec![ir::Node::Table(ir::Table {
            columns: vec![
                ir::ColumnSpec {
                    width: Length::Pt(Pt(60.0)),
                    ..Default::default()
                },
                ir::ColumnSpec::default(),
                ir::ColumnSpec {
                    width: Length::Pt(Pt(80.0)),
                    align: ir::Align::End,
                    ..Default::default()
                },
            ],
            header: Some(ir::Row {
                cells: vec![
                    ir::Cell::new("Fecha"),
                    ir::Cell::new("Concepto"),
                    ir::Cell::new("Importe"),
                ],
                style: ir::BoxStyle {
                    background: Some(navy),
                    ..Default::default()
                },
                ..Default::default()
            }),
            rows: (0..180)
                .map(|i| ir::Row {
                    cells: vec![
                        ir::Cell::new(format!("2026-08-{:02}", (i % 28) + 1)),
                        ir::Cell::new(format!("Asiento contable numero {i}")),
                        ir::Cell::new(format!("{:.2}", 100.0 + (i as f64 * 7.5))),
                    ],
                    totals: vec![ir::TotalContribution {
                        accumulator: 0,
                        value: 100.0 + (i as f64 * 7.5),
                    }],
                    ..Default::default()
                })
                .collect(),
            padding: Edges::all(Pt(4.0)),
            ..ir::Table::empty()
        })],
    };

    let assets = Assets::new()
        .with_font(Default::default(), REGULAR.to_vec())
        .with_font(imprenta_pdf::shape::Face::BOLD, BOLD.to_vec());

    let built = build(&document, &assets, Options::default()).unwrap();
    let path = format!("{out}/bands.pdf");
    std::fs::write(&path, &built.pdf).unwrap();
    println!(
        "{path}: {} pages, {:.1} KB",
        built.pages,
        built.pdf.len() as f64 / 1024.0
    );
    for note in &built.diagnostics {
        println!("  {note}");
    }
}
