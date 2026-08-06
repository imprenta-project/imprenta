//! What a large export costs, streamed against declared whole.
//!
//! Run with `--release`. A number from a debug build is not a number.
//!
//!     cargo run -p imprenta-xlsx --example bench_memory_rows --release -- 200000

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use imprenta_xlsx::Session;
use imprenta_xlsx::ir::{Cell, Column, Row, Sheet, Value, Workbook};
use imprenta_xlsx::style::{Font, Style};

/// Counts what is alive, not what was ever asked for.
///
/// Peak live bytes is the number that decides whether a process survives an
/// export; total allocated says only how busy it was.
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
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn reset() {
    PEAK.store(LIVE.load(Relaxed), Relaxed);
}

fn peak_mb() -> f64 {
    PEAK.load(Relaxed) as f64 / (1024.0 * 1024.0)
}

fn money() -> Style {
    Style {
        format: Some("#,##0.00 €".into()),
        ..Style::default()
    }
}

fn heading() -> Style {
    Style {
        font: Font {
            bold: true,
            ..Font::default()
        },
        ..Style::default()
    }
}

fn line(n: usize) -> Row {
    Row::new(vec![
        Cell::text(format!("FV-2026-{n:06}")),
        Cell::text("Prestación de servicios profesionales"),
        Cell::date(46_000.0 + (n % 365) as f64),
        Cell {
            value: Value::Number(100.0 + ((n * 7) % 9000) as f64 / 3.0),
            style: Some(Box::new(money())),
        },
    ])
}

fn setup() -> Sheet {
    Sheet {
        name: "Libro mayor".into(),
        columns: vec![
            Column {
                width: Some(16.0),
                style: None,
            },
            Column {
                width: Some(40.0),
                style: None,
            },
            Column {
                width: Some(14.0),
                style: None,
            },
            Column {
                width: Some(16.0),
                style: None,
            },
        ],
        rows: vec![
            Row::new(vec![
                Cell::text("Referencia"),
                Cell::text("Concepto"),
                Cell::text("Fecha"),
                Cell::text("Importe"),
            ])
            .styled(heading()),
        ],
        ..Sheet::default()
    }
}

fn main() {
    let rows: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(200_000);
    let batch: usize = std::env::args()
        .nth(2)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1_000);

    println!("{rows} rows, batches of {batch}\n");

    // Declared whole: every row exists before a byte is written.
    reset();
    let started = Instant::now();
    let mut sheet = setup();
    sheet.rows.extend((0..rows).map(line));
    let book = Workbook::new(vec![sheet]);
    let declared_bytes = imprenta_xlsx::write(&book, &[])
        .expect("it should write")
        .len();
    let declared_time = started.elapsed();
    let declared_peak = peak_mb();
    drop(book);

    // Streamed: a batch at a time, and nothing else held.
    reset();
    let started = Instant::now();
    let mut session = Session::open(Cursor::new(Vec::new()), vec![setup()], vec![]).expect("open");
    let mut at = 0;
    while at < rows {
        let upto = (at + batch).min(rows);
        let chunk: Vec<Row> = (at..upto).map(line).collect();
        session.rows(&chunk).expect("rows");
        at = upto;
    }
    let streamed_bytes = session.finish().expect("finish").into_inner().len();
    let streamed_time = started.elapsed();
    let streamed_peak = peak_mb();

    println!("{:<12} {:>12} {:>12}", "", "declared", "streamed");
    println!(
        "{:<12} {:>11.2}s {:>11.2}s",
        "time",
        declared_time.as_secs_f64(),
        streamed_time.as_secs_f64()
    );
    println!(
        "{:<12} {:>10.1} MB {:>10.1} MB",
        "peak live", declared_peak, streamed_peak
    );
    println!(
        "{:<12} {:>10.1} MB {:>10.1} MB",
        "output",
        declared_bytes as f64 / 1e6,
        streamed_bytes as f64 / 1e6
    );
    println!(
        "\nsame bytes: {}",
        if declared_bytes == streamed_bytes {
            "yes"
        } else {
            "NO — the two paths have diverged"
        }
    );
    println!(
        "per row:    {:.0} B declared, {:.0} B streamed",
        declared_peak * 1e6 / rows as f64,
        streamed_peak * 1e6 / rows as f64
    );
}
