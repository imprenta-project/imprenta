//! What it costs to read a declared workbook.
//!
//! Its own test binary, and the only test in it, because the counter is a
//! global allocator: anything else running alongside would be counted too.
//!
//! # What it found
//!
//! Not what was expected. `ir.rs` carried a note predicting the PDF crate's
//! problem — serde buffering a tagged enum — and a hand-written `Deserialize`
//! written on that prediction measured 7.5 allocations a row against the
//! derive's 7.3. No difference at all: **adjacent** tagging does not buffer,
//! only internal tagging does, and the note was wrong. It was taken out again.
//!
//! The cost was the size of `Cell`. A `Style` is 128 bytes of mostly-absent
//! options and it sat inline, so every cell was 168 bytes whether or not
//! anything had been said about how it looks — 672 bytes of row before a
//! character of data. Boxing it took a cell to 48 and a row from 2,380 bytes
//! to about 1,560.
//!
//! Which is the whole reason to measure rather than reason: the fix that was
//! obvious was worthless, and the one that worked was not on the list.

use imprenta_xlsx::ir;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Relaxed);
        BYTES.fetch_add(layout.size(), Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const ROWS: usize = 10_000;

/// Four cells a row, which is what an export looks like: a reference, a
/// description, a date and an amount.
const CELLS: usize = 4;

#[test]
fn reading_a_long_sheet_costs_about_what_its_cells_weigh() {
    let json = ledger(ROWS);

    let before = ALLOCATIONS.load(Relaxed);
    let bytes_before = BYTES.load(Relaxed);

    let book: ir::Workbook = serde_json::from_str(&json).unwrap();

    let allocations = ALLOCATIONS.load(Relaxed) - before;
    let handed_out = BYTES.load(Relaxed) - bytes_before;

    assert_eq!(
        book.sheets[0].rows.len(),
        ROWS,
        "the sample must have parsed"
    );

    println!(
        "{allocations} allocations ({:.1}/row, {:.1}/cell), {handed_out} bytes ({:.0}/row)",
        allocations as f64 / ROWS as f64,
        allocations as f64 / (ROWS * CELLS) as f64,
        handed_out as f64 / ROWS as f64
    );

    // Measured at about 7.2. The limit is set with room for a field or two, and far
    // under what a return to buffering would cost — that shows up as an order
    // of magnitude, not a few percent.
    assert!(
        allocations < ROWS * 11,
        "{allocations} allocations for {ROWS} rows of {CELLS} cells ({:.1} per row)",
        allocations as f64 / ROWS as f64
    );

    // Handed out rather than held: both vectors double as they grow, and that
    // is paid once and freed as it goes. Measured at about 1,560 a row; it was 2,380
    // when a cell carried its style inline, and that is the number this guards
    // against creeping back.
    assert!(
        handed_out < ROWS * 2_000,
        "{handed_out} bytes for {ROWS} rows ({:.0} per row) — a cell has grown",
        handed_out as f64 / ROWS as f64
    );
}

#[test]
fn a_cell_with_a_style_on_it_does_not_change_the_order_of_things() {
    // A styled cell carries a whole `Style` — a font, a fill, four borders, an
    // alignment — and every one of those is `Option` and mostly absent. If a
    // format is ever read by buffering, this is where it would show, because a
    // style is the deepest thing in the tree.
    let json = styled(ROWS);

    let before = ALLOCATIONS.load(Relaxed);
    let book: ir::Workbook = serde_json::from_str(&json).unwrap();
    let allocations = ALLOCATIONS.load(Relaxed) - before;

    assert_eq!(book.sheets[0].rows.len(), ROWS);
    println!(
        "styled: {allocations} allocations ({:.1}/row)",
        allocations as f64 / ROWS as f64
    );

    // A styled cell pays one allocation for its boxed style and one for the
    // format's string, on top of the plain case. That is the trade, and it is
    // the right way round: most cells have no style, and the ones that do are
    // a handful of formats shared by every row.
    assert!(
        allocations < ROWS * 8,
        "{allocations} allocations for {ROWS} styled rows ({:.1} per row)",
        allocations as f64 / ROWS as f64
    );
}

fn ledger(rows: usize) -> String {
    let mut json = String::from(r#"{"sheets":[{"name":"Libro","rows":["#);
    for n in 0..rows {
        if n > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"cells":[
                {{"value":{{"t":"text","v":"FV-2026-{n:06}"}}}},
                {{"value":{{"t":"text","v":"Prestación de servicios profesionales"}}}},
                {{"value":{{"t":"date","v":{}}}}},
                {{"value":{{"t":"number","v":{}.25}}}}
            ]}}"#,
            46_000 + n % 365,
            100 + n % 9000
        ));
    }
    json.push_str("]}]}");
    json
}

fn styled(rows: usize) -> String {
    // Extra hashes on the raw strings: a `"#` inside one closes it, and both a
    // hex colour and a number format begin with the character.
    let mut json = String::from(r##"{"sheets":[{"name":"Libro","rows":["##);
    for n in 0..rows {
        if n > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r##"{{"cells":[{{"value":{{"t":"number","v":{n}}},"style":{{"font":{{"bold":true}},"fill":"#f1f5f9","align":{{"horizontal":"right"}},"format":"#,##0.00"}}}}]}}"##
        ));
    }
    json.push_str("]}]}");
    json
}
