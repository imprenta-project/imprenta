//! What it costs to read a declared document.
//!
//! Its own test binary, and the only test in it, because the counter is a
//! global allocator: anything else running alongside would be counted too.
//!
//! The number this guards is not a micro-optimisation. Serde deserialises an
//! internally tagged enum — `{"t": "table", …}` — by buffering the entire map
//! into an intermediate tree before it knows which variant to build. For a
//! node holding forty thousand rows that intermediate tree is the whole
//! document, several times over, and it dominated peak memory: nine
//! allocations and three kilobytes per row, for rows whose own data is a few
//! hundred bytes. `Node` therefore reads its tag by hand.

use imprenta_pdf::ir;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Relaxed);
        BYTES.fetch_add(layout.size(), Relaxed);
        PEAK.fetch_max(
            LIVE.fetch_add(layout.size(), Relaxed) + layout.size(),
            Relaxed,
        );
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const ROWS: usize = 10_000;

#[test]
fn reading_a_long_table_costs_about_what_its_rows_weigh() {
    let json = ledger(ROWS);
    let text = json.len();

    let before = ALLOCATIONS.load(Relaxed);
    let bytes_before = BYTES.load(Relaxed);
    PEAK.store(LIVE.load(Relaxed), Relaxed);

    let document: ir::Document = serde_json::from_str(&json).unwrap();

    let allocations = ALLOCATIONS.load(Relaxed) - before;
    let handed_out = BYTES.load(Relaxed) - bytes_before;
    let peak = PEAK.load(Relaxed) - LIVE.load(Relaxed) + handed_out.min(LIVE.load(Relaxed));

    assert_eq!(rows_of(&document), ROWS, "the sample must have parsed");

    // Three allocations a row is the data itself: the text of each of the two
    // cells, and the vector holding them. Buffering the tagged node cost
    // sixteen. Six leaves room for a field or two without hiding a return to
    // buffering, which would show up as an order of magnitude.
    println!(
        "{allocations} allocations ({:.1}/row), {handed_out} bytes ({:.0}/row)",
        allocations as f64 / ROWS as f64,
        handed_out as f64 / ROWS as f64
    );
    assert!(
        allocations < ROWS * 6,
        "{allocations} allocations for {ROWS} rows ({:.1} per row)",
        allocations as f64 / ROWS as f64
    );

    // Handed out rather than held. The tree settles at about 270 bytes a
    // row; the rest is the row vector doubling as it grows, which is paid
    // once and freed as it goes. Buffering cost three kilobytes a row, so the
    // line sits well below that and well above where we are.
    assert!(
        handed_out < ROWS * 1_500,
        "handed out {:.0} bytes a row to read {:.1} MB of JSON",
        handed_out as f64 / ROWS as f64,
        text as f64 / 1e6
    );

    let _ = peak;
}

fn rows_of(document: &ir::Document) -> usize {
    match &document.children[0] {
        ir::Node::Table(table) => table.rows.len(),
        _ => 0,
    }
}

/// A ledger declared the way a producer would emit it, tag first.
fn ledger(rows: usize) -> String {
    let mut json = String::from(
        r#"{"page":{"width":595,"height":842},"children":[{"t":"table","columns":[{},{}],"rows":["#,
    );
    for i in 0..rows {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"cells":[{{"text":"Asiento contable numero {i}"}},{{"text":"1.200,00 EUR"}}]}}"#
        ));
    }
    json.push_str("]}]}");
    json
}
