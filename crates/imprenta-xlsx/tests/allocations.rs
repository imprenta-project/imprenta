//! What it costs to read a declared workbook.
//!
//! Its own test binary, because the counter is a global allocator and anything
//! else running alongside would be counted too. Within the binary the tests
//! take `MEASURING` in turn, for the same reason — see the note on it.
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
//! character of data. Boxing it took a cell to 48 and a row to **794 bytes**.
//!
//! That last figure read 1,560 here for a while, and was wrong: it was taken
//! while the styled test ran alongside and counted into the same total. Only
//! numbers measured under `MEASURING` mean anything.
//!
//! Which is the whole reason to measure rather than reason: the fix that was
//! obvious was worthless, and the one that worked was not on the list.

use imprenta_xlsx::ir;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
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

/// Only one test may be measuring at a time.
///
/// The counter is a global allocator: it counts every thread, so two tests
/// reading it at once count each other. This file's header used to say "the
/// only test in it" for that reason, and then a second test was added and
/// nobody re-read the sentence. The plain row measured 1,670 bytes instead of
/// 794 and moved between runs — until a Linux runner happened to land on
/// 2,005 and tripped a budget that had itself been written down from a
/// contaminated number.
///
/// Take this before touching `ALLOCATIONS` or `BYTES`, and hold it until the
/// delta has been read. Poisoning is ignored on purpose: if one test panics,
/// the other should report its own result rather than a lock error.
static MEASURING: Mutex<()> = Mutex::new(());

const ROWS: usize = 10_000;

/// Four cells a row, which is what an export looks like: a reference, a
/// description, a date and an amount.
const CELLS: usize = 4;

#[test]
fn reading_a_long_sheet_costs_about_what_its_cells_weigh() {
    let _measuring = MEASURING.lock().unwrap_or_else(|held| held.into_inner());
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

    // Measured at exactly 3.0 a row, run after run. The limit leaves room for a
    // field or two and stays far under what a return to buffering would cost —
    // that shows up as an order of magnitude, not a few percent.
    assert!(
        allocations < ROWS * 4,
        "{allocations} allocations for {ROWS} rows of {CELLS} cells ({:.1} per row)",
        allocations as f64 / ROWS as f64
    );

    // Handed out rather than held: both vectors double as they grow, and that
    // is paid once and freed as it goes. Measured at 794 a row; it was 2,380
    // when a cell carried its style inline, and that is the number this guards
    // against creeping back.
    assert!(
        handed_out < ROWS * 1_000,
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
    let _measuring = MEASURING.lock().unwrap_or_else(|held| held.into_inner());
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
    // format's string, on top of the plain case: 5.0 a row against 3.0. That is
    // the trade, and it is the right way round — most cells have no style, and
    // the ones that do are a handful of formats shared by every row.
    assert!(
        allocations < ROWS * 6,
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
