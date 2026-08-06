//! What a footer saying "de 4 849" costs.
//!
//! Its own test binary, and for the same reason as `allocations.rs`: the
//! counter is a global allocator, so anything else running alongside would be
//! counted into the answer.
//!
//! `{{pages}}` is the one token nothing can answer while pages are being
//! released, and the engine used to buy the answer by holding every painted
//! page until the last one was packed. Measured on a five-column ledger that
//! was **twenty-three times** the memory of the same document without it —
//! 244 MB against 26 MB at 2 670 pages — and the largest single reason a
//! ledger trapped. This is the test that says it may not do that any more.
//!
//! The assertion is a ratio rather than a number of megabytes. What is being
//! guarded is not a figure somebody measured on one laptop, it is the shape:
//! a document that prints its own length must cost what the same document
//! costs without printing it, give or take the pass that counts.

use imprenta_core::units::{Edges, Pt};
use imprenta_pdf::build::{Assets, build};
use imprenta_pdf::ir;
use imprenta_pdf::render::Options;
use imprenta_pdf::session::{Bands, Chunk, Session};
use imprenta_pdf::shape::Face;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let live = LIVE.fetch_add(layout.size(), Relaxed) + layout.size();
        PEAK.fetch_max(live, Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > layout.size() {
            let grown = new_size - layout.size();
            PEAK.fetch_max(LIVE.fetch_add(grown, Relaxed) + grown, Relaxed);
        } else {
            LIVE.fetch_sub(layout.size() - new_size, Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const ROBOTO: &[u8] = include_bytes!("fonts/Roboto-Regular.ttf");

const ROWS: usize = 6_000;

fn ledger(total: bool) -> ir::Document {
    ir::Document {
        page: ir::PageSetup::default(),
        accumulators: Vec::new(),
        header: None,
        footer: Some(ir::Band {
            height: Pt(20.0),
            children: vec![ir::Node::Text(ir::Text {
                runs: vec![ir::Run::new(if total {
                    "Pagina {{page}} de {{pages}}"
                } else {
                    "Pagina {{page}}"
                })],
                style: ir::TextStyle {
                    size: Pt(8.0),
                    ..Default::default()
                },
            })],
        }),
        children: vec![ir::Node::Table(ir::Table {
            columns: vec![ir::ColumnSpec::default(); 3],
            header: vec![ir::Row {
                cells: vec![
                    ir::Cell::new("Fecha"),
                    ir::Cell::new("Concepto"),
                    ir::Cell::new("Importe"),
                ],
                ..Default::default()
            }],
            repeat_header: true,
            padding: Edges::all(Pt(2.0)),
            rows: (0..ROWS)
                .map(|i| ir::Row {
                    cells: vec![
                        ir::Cell::new(format!("{:02}/{:02}/2024", i % 28 + 1, i % 12 + 1)),
                        ir::Cell::new(format!(
                            "FV-2026-{i:06} prestacion de servicios profesionales al cliente {}",
                            i % 400
                        )),
                        ir::Cell::new(format!("{:.2}", 100.0 + (i % 9000) as f64 / 3.0)),
                    ],
                    ..Default::default()
                })
                .collect(),
            ..ir::Table::empty()
        })],
    }
}

/// Peak live bytes while `f` runs, over what was live when it started.
///
/// One at a time, because the counter is the whole process: two of these
/// running at once — which is what `cargo test` does by default — each read
/// the other's allocations as their own, and the answer comes out somewhere
/// between "too small to fail" and "large enough to fail on a good day".
fn peak_of<T>(f: impl FnOnce() -> T) -> (T, usize) {
    static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = GATE.lock().unwrap_or_else(|e| e.into_inner());

    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    let out = f();
    let peak = PEAK.load(Relaxed).saturating_sub(base);
    (out, peak)
}

#[test]
fn printing_the_page_count_does_not_cost_the_whole_document() {
    let assets = Assets::new().with_font(Face::REGULAR, ROBOTO.to_vec());

    // The declaration is built before either measurement and dropped after
    // both, so what is being compared is the render and nothing else.
    let plain = ledger(false);
    let counted = ledger(true);

    let (without, cheap) = peak_of(|| build(&plain, &assets, Options::default()).unwrap());
    let (with, dear) = peak_of(|| build(&counted, &assets, Options::default()).unwrap());

    assert!(without.pages > 100, "the sample must paginate properly");
    assert_eq!(
        with.pages, without.pages,
        "printing the total changed the pagination"
    );

    // Two, not one: the pass that counts the pages allocates too, and the
    // document that prints a total is a few glyphs wider in the footer. Ten
    // is what holding costs, and the gap between the two is wide enough that
    // this cannot pass by accident.
    assert!(
        dear < cheap * 2,
        "a footer saying \"de {}\" cost {:.1} MB against {:.1} MB without it — \
         that is the whole document being held",
        with.pages,
        dear as f64 / 1e6,
        cheap as f64 / 1e6,
    );
}

#[test]
fn a_fed_document_that_prints_its_length_keeps_the_rows_and_not_the_pages() {
    // The same claim on the streaming side, where it matters most: ContaPro's
    // ledger arrives from a database in batches and is never declared whole.
    //
    // A fed document cannot be walked twice by itself — the rows are gone
    // once they have been read — so a session that must answer `{{pages}}`
    // keeps what it was *fed* and walks that again. The ratio is the point:
    // a row weighs a few hundred bytes and the painted page it lands on
    // weighs six kilobytes, so keeping the input is an order of magnitude
    // cheaper than keeping the output, and it is what the engine used to do.
    let assets = Assets::new().with_font(Face::REGULAR, ROBOTO.to_vec());

    fn feed(assets: &Assets, total: bool) -> (imprenta_pdf::build::Built, usize) {
        let document = ledger(total);
        let ir::Node::Table(table) = &document.children[0] else {
            unreachable!()
        };
        let head = ir::TableHead {
            columns: table.columns.clone(),
            header: table.header.clone(),
            repeat_header: table.repeat_header,
            padding: table.padding,
            space_after: table.space_after,
        };
        let rows = table.rows.clone();
        let bands = Bands {
            header: document.header.clone(),
            footer: document.footer.clone(),
        };

        peak_of(move || {
            let mut session = Session::open_with(
                document.page,
                bands,
                document.accumulators.len(),
                assets.clone(),
                Options::default(),
            )
            .unwrap();
            session.feed(&Chunk::OpenTable(head)).unwrap();
            for batch in rows.chunks(500) {
                session.feed(&Chunk::Rows(batch.to_vec())).unwrap();
            }
            session.feed(&Chunk::CloseTable).unwrap();
            session.finish().unwrap()
        })
    }

    let (without, cheap) = feed(&assets, false);
    let (with, dear) = feed(&assets, true);

    assert!(without.pages > 100, "the sample must paginate properly");
    assert_eq!(
        with.pages, without.pages,
        "printing the total changed the pagination"
    );
    assert!(
        dear < cheap * 2,
        "a fed footer saying \"de {}\" cost {:.1} MB against {:.1} MB without it",
        with.pages,
        dear as f64 / 1e6,
        cheap as f64 / 1e6,
    );
}
