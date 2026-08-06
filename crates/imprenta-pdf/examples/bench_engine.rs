//! What one render costs, in the units that decide whether it traps.
//!
//! RSS cannot answer this. It mixes the allocator's own high-water mark in
//! with what is actually held, it counts the worker pool on the Node side,
//! and it never comes down. The number that predicts a WebAssembly module's
//! linear memory is **peak live bytes** — what was allocated and not yet
//! freed at the worst moment — so that is what this counts, with an allocator
//! that keeps the running total itself.
//!
//! Two figures come out of it and they answer different questions. Peak live
//! is what has to fit; live at the end is whether the engine gave it back.
//!
//! ```text
//! cargo run -p imprenta-pdf --release --example bench_engine -- 40000 ledger
//! ```

use imprenta_core::color::Color;
use imprenta_core::units::{Edges, Pt};
use imprenta_pdf::build::{Assets, build};
use imprenta_pdf::ir;
use imprenta_pdf::render::Options;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);
static COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let live = LIVE.fetch_add(layout.size(), Relaxed) + layout.size();
        PEAK.fetch_max(live, Relaxed);
        TOTAL.fetch_add(layout.size(), Relaxed);
        COUNT.fetch_add(1, Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > layout.size() {
            let grown = new_size - layout.size();
            let live = LIVE.fetch_add(grown, Relaxed) + grown;
            PEAK.fetch_max(live, Relaxed);
            TOTAL.fetch_add(grown, Relaxed);
        } else {
            LIVE.fetch_sub(layout.size() - new_size, Relaxed);
        }
        COUNT.fetch_add(1, Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");
const ROBOTO_BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");

fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1_048_576.0
}

/// A five-column ledger with a two-row repeated header, zebra rows, a header
/// band and a footer band — the document that started all of this.
fn ledger(rows: usize, total: bool) -> ir::Document {
    let head = |text: &str| ir::Cell {
        text: text.into(),
        weight: ir::Weight::Bold,
        ..ir::Cell::new("")
    };
    let zebra = ir::BoxStyle {
        background: Some(Color::rgb(249, 250, 251)),
        ..Default::default()
    };

    ir::Document {
        page: ir::PageSetup::default(),
        accumulators: vec!["debe".into()],
        header: Some(ir::Band {
            height: Pt(28.0),
            children: vec![ir::Node::Text(ir::Text {
                runs: vec![ir::Run::new("Libro mayor · ejercicio 2024").bold()],
                style: ir::TextStyle {
                    size: Pt(10.0),
                    ..Default::default()
                },
            })],
        }),
        footer: Some(ir::Band {
            height: Pt(20.0),
            children: vec![ir::Node::Text(ir::Text {
                runs: vec![ir::Run::new(if total {
                    "Pagina {{page}} de {{pages}} · suma y sigue {{debe}}"
                } else {
                    "Pagina {{page}} · suma y sigue {{debe}}"
                })],
                style: ir::TextStyle {
                    size: Pt(8.0),
                    ..Default::default()
                },
            })],
        }),
        children: vec![ir::Node::Table(ir::Table {
            columns: vec![
                ir::ColumnSpec::default(),
                ir::ColumnSpec::default(),
                ir::ColumnSpec::default(),
                ir::ColumnSpec::default(),
                ir::ColumnSpec::default(),
            ],
            header: vec![
                ir::Row {
                    cells: vec![
                        head("430000 · Clientes"),
                        head(""),
                        head(""),
                        head(""),
                        head(""),
                    ],
                    ..Default::default()
                },
                ir::Row {
                    cells: vec![
                        head("Fecha"),
                        head("Concepto"),
                        head("Debe"),
                        head("Haber"),
                        head("Saldo"),
                    ],
                    ..Default::default()
                },
            ],
            repeat_header: true,
            padding: Edges::all(Pt(2.0)),
            rows: (0..rows)
                .map(|i| ir::Row {
                    cells: vec![
                        ir::Cell::new(format!("{:02}/{:02}/2024", i % 28 + 1, i % 12 + 1)),
                        ir::Cell::new(format!(
                            "FV-2026-{i:06} Prestacion de servicios profesionales a cliente {}",
                            i % 400
                        )),
                        ir::Cell::new(format!("{:.2}", 100.0 + (i % 9000) as f64 / 3.0)),
                        ir::Cell::new("0,00"),
                        ir::Cell::new(format!("{:.2}", 1000.0 + (i % 7000) as f64 / 7.0)),
                    ],
                    style: if i % 2 == 0 {
                        zebra
                    } else {
                        Default::default()
                    },
                    totals: vec![ir::TotalContribution {
                        accumulator: 0,
                        value: 1.0,
                    }],
                })
                .collect(),
            ..ir::Table::empty()
        })],
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // `write` exists so the very same document can be measured through the
    // WebAssembly module, which is the target that actually traps. A ledger
    // declared twice in two languages would be two documents.
    if args.get(1).map(String::as_str) == Some("write") {
        let rows: usize = args[2].parse().unwrap();
        let total = args.get(3).is_some_and(|m| m.contains("total"));
        let path = args.last().unwrap();
        std::fs::write(path, serde_json::to_string(&ledger(rows, total)).unwrap()).unwrap();
        return;
    }

    let rows: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(40_000);
    let mode = std::env::args().nth(2).unwrap_or_else(|| "ledger".into());
    let total = mode.contains("total");

    let assets = Assets::new()
        .with_font(imprenta_pdf::shape::Face::REGULAR, ROBOTO.to_vec())
        .with_font(imprenta_pdf::shape::Face::BOLD, ROBOTO_BOLD.to_vec());

    let document = ledger(rows, total);

    // The declaration itself is not what is being measured, so the counters
    // are zeroed once it exists and the peak is re-based on what is live now.
    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    TOTAL.store(0, Relaxed);
    COUNT.store(0, Relaxed);

    let t = Instant::now();
    let built = build(&document, &assets, Options::default()).expect("render");
    let ms = t.elapsed().as_secs_f64() * 1000.0;

    let peak = PEAK.load(Relaxed) - base;
    let after = LIVE.load(Relaxed) - base;
    let pdf = built.pdf.len();
    drop(built.pdf);
    let released = LIVE.load(Relaxed).saturating_sub(base);

    println!(
        "{rows} rows{} → {} pages · {ms:.0} ms\n  \
         peak live {:.1} MB ({:.2} KB/page) · held after {:.1} MB · once the bytes go {:.1} MB\n  \
         handed out {:.1} MB in {} allocations · pdf {:.2} MB ({:.2} KB/page)",
        if total { " + {{pages}}" } else { "" },
        built.pages,
        mb(peak),
        peak as f64 / 1024.0 / built.pages as f64,
        mb(after),
        mb(released),
        mb(TOTAL.load(Relaxed)),
        COUNT.load(Relaxed),
        pdf as f64 / 1e6,
        pdf as f64 / 1024.0 / built.pages as f64,
    );
}
