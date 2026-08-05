//! Where the memory goes when a document arrives declared rather than fed.
//!
//! The engine paints and releases pages as it goes, so page content is no
//! longer the cost. What is left is the declaration itself: the JSON text, and
//! the tree serde builds out of it. This measures each in turn.

use imprenta_core::units::Pt;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// Counts what is live and what has ever been handed out, so "we are holding
/// three hundred megabytes" can be told apart from "the allocator asked the
/// kernel for three hundred megabytes and never gave it back".
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK_LIVE: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);
static COUNT: AtomicUsize = AtomicUsize::new(0);
static BIGGEST: AtomicUsize = AtomicUsize::new(0);
/// Allocations by size, in powers of two: BUCKETS[n] counts those under 2^n.
static BUCKETS: [AtomicUsize; 32] = [const { AtomicUsize::new(0) }; 32];
static BUCKET_BYTES: [AtomicUsize; 32] = [const { AtomicUsize::new(0) }; 32];

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let live = LIVE.fetch_add(layout.size(), Relaxed) + layout.size();
        PEAK_LIVE.fetch_max(live, Relaxed);
        TOTAL.fetch_add(layout.size(), Relaxed);
        COUNT.fetch_add(1, Relaxed);
        BIGGEST.fetch_max(layout.size(), Relaxed);
        let bucket = (usize::BITS - layout.size().leading_zeros()) as usize;
        BUCKETS[bucket.min(31)].fetch_add(1, Relaxed);
        BUCKET_BYTES[bucket.min(31)].fetch_add(layout.size(), Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn live_mb() -> f64 {
    LIVE.load(Relaxed) as f64 / 1e6
}

fn report(stage: &str) {
    println!(
        "  {stage:<16} live {:>7.1} MB   peak live {:>7.1} MB   \
         handed out {:>8.1} MB in {} allocations",
        live_mb(),
        PEAK_LIVE.load(Relaxed) as f64 / 1e6,
        TOTAL.load(Relaxed) as f64 / 1e6,
        COUNT.load(Relaxed)
    );
}

fn histogram() {
    println!(
        "  largest single allocation: {:.1} MB",
        BIGGEST.load(Relaxed) as f64 / 1e6
    );
    println!(
        "  {:>12}  {:>12}  {:>10}",
        "up to", "allocations", "total MB"
    );
    for (n, bucket) in BUCKETS.iter().enumerate() {
        let count = bucket.load(Relaxed);
        if count == 0 {
            continue;
        }
        let bytes = BUCKET_BYTES[n].load(Relaxed) as f64 / 1e6;
        if bytes < 1.0 && count < 10_000 {
            continue;
        }
        println!("  {:>12}  {count:>12}  {bytes:>10.1}", 1usize << n);
    }
}
use imprenta_pdf::build::{Assets, build};
use imprenta_pdf::ir;
use imprenta_pdf::render::Options;
use serde::Deserialize;

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

fn rss_mb() -> f64 {
    // maxrss is in bytes on macOS, kilobytes on Linux.
    let mut usage: libc_rusage::Rusage = Default::default();
    usage.max_rss_mb()
}

mod libc_rusage {
    /// Peak resident set, read from the process rather than guessed.
    #[derive(Default)]
    pub struct Rusage;

    impl Rusage {
        pub fn max_rss_mb(&mut self) -> f64 {
            let path = "/proc/self/status";
            if let Ok(status) = std::fs::read_to_string(path) {
                for line in status.lines() {
                    if let Some(kb) = line.strip_prefix("VmHWM:") {
                        let kb: f64 = kb.trim().trim_end_matches(" kB").parse().unwrap_or(0.0);
                        return kb / 1024.0;
                    }
                }
            }
            // macOS: ask the kernel directly.
            #[cfg(target_os = "macos")]
            {
                let out = std::process::Command::new("ps")
                    .args(["-o", "rss=", "-p", &std::process::id().to_string()])
                    .output()
                    .ok();
                if let Some(out) = out {
                    let kb: f64 = String::from_utf8_lossy(&out.stdout)
                        .trim()
                        .parse()
                        .unwrap_or(0.0);
                    return kb / 1024.0;
                }
            }
            0.0
        }
    }
}

fn main() {
    // Two modes, so the measurement is never confused by the cost of
    // building the sample: `write <rows> <path>` produces the declaration,
    // and a fresh process then renders it.
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("write") => write(args[2].parse().unwrap(), &args[3]),
        Some("tree") => tree(args[2].parse().unwrap()),
        Some("scan") => scan(&args[2]),
        Some("stream") => stream(args[2].parse().unwrap()),
        Some(path) => render(path),
        None => eprintln!("usage: mem_ir write <rows> <path> | mem_ir <path>"),
    }
}

fn write(rows: usize, path: &str) {
    let document = ir::Document {
        page: ir::PageSetup::default(),
        accumulators: Vec::new(),
        children: vec![ir::Node::Table(ir::Table {
            columns: vec![ir::ColumnSpec::default(), ir::ColumnSpec::default()],
            rows: (0..rows)
                .map(|i| ir::Row {
                    cells: vec![
                        ir::Cell::new(format!("Asiento contable numero {i}")),
                        ir::Cell::new("1.200,00 EUR"),
                    ],
                    ..Default::default()
                })
                .collect(),
            ..ir::Table::empty()
        })],
        header: None,
        footer: None,
    };
    std::fs::write(path, serde_json::to_string(&document).unwrap()).unwrap();
}

/// The same document built in Rust, with no JSON anywhere near it.
fn tree(rows: usize) {
    let before = rss_mb();
    let document = ir::Document {
        page: ir::PageSetup::default(),
        accumulators: Vec::new(),
        children: vec![ir::Node::Table(ir::Table {
            columns: vec![ir::ColumnSpec::default(), ir::ColumnSpec::default()],
            rows: (0..rows)
                .map(|i| ir::Row {
                    cells: vec![
                        ir::Cell::new(format!("Asiento contable numero {i}")),
                        ir::Cell::new("1.200,00 EUR"),
                    ],
                    ..Default::default()
                })
                .collect(),
            ..ir::Table::empty()
        })],
        header: None,
        footer: None,
    };
    let after = rss_mb();
    println!(
        "tree of {rows} rows: {:.0} MB ({:.0} bytes/row)",
        after - before,
        (after - before) * 1e6 / rows as f64
    );
    std::hint::black_box(&document);
}

/// The same ledger fed in pieces, so nothing but the tail is ever held.
fn stream(rows: usize) {
    use imprenta_pdf::session::{Chunk, Session};

    let assets = Assets::new().with_font(Default::default(), ROBOTO.to_vec());
    let mut session =
        Session::open(ir::PageSetup::default(), 0, assets, Options::default()).unwrap();
    report("opened");

    session
        .feed(&Chunk::OpenTable(ir::TableHead {
            columns: vec![ir::ColumnSpec::default(), ir::ColumnSpec::default()],
            header: Vec::new(),
            repeat_header: true,
            padding: Default::default(),
            space_after: Pt(0.0),
        }))
        .unwrap();

    const BATCH: usize = 1_000;
    let mut sent = 0;
    while sent < rows {
        let end = (sent + BATCH).min(rows);
        session
            .feed(&Chunk::Rows(
                (sent..end)
                    .map(|i| ir::Row {
                        cells: vec![
                            ir::Cell::new(format!("Asiento contable numero {i}")),
                            ir::Cell::new("1.200,00 EUR"),
                        ],
                        ..Default::default()
                    })
                    .collect(),
            ))
            .unwrap();
        sent = end;
    }
    session.feed(&Chunk::CloseTable).unwrap();
    report("fed");

    let built = session.finish().unwrap();
    report("finished");
    println!("pages           {}", built.pages);
    println!("RSS at the end  {:.0} MB", rss_mb());
    println!("output          {:.1} MB", built.pdf.len() as f64 / 1e6);
}

/// Parses and throws everything away, so the parser's own appetite is
/// visible with no tree to blame it on.
fn scan(path: &str) {
    let json = std::fs::read_to_string(path).unwrap();
    report("read");

    let mut de = serde_json::Deserializer::from_str(&json);
    serde::de::IgnoredAny::deserialize(&mut de).unwrap();
    report("scanned");

    // The same rows, but reached without going through the tagged `Node`
    // enum. If the difference is large, the tag is the cost.
    let table_json = {
        let start = json.find(r#"{"t":"table""#).unwrap();
        let end = json.rfind("}]}").unwrap() + 1;
        json[start..end].to_string()
    };
    report("sliced");
    let bare: serde_json::Value = serde_json::from_str("null").unwrap();
    std::hint::black_box(bare);

    #[derive(serde::Deserialize)]
    struct Untagged {
        rows: Vec<ir::Row>,
    }
    let untagged: Untagged = serde_json::from_str(&table_json).unwrap();
    report("untagged");
    println!("  untagged rows: {}", untagged.rows.len());
    drop(untagged);

    let doc: ir::Document = serde_json::from_str(&json).unwrap();
    report("tagged");
    histogram();
    println!(
        "rows: {}",
        match &doc.children[0] {
            ir::Node::Table(t) => t.rows.len(),
            _ => 0,
        }
    );
}

fn render(path: &str) {
    let json = std::fs::read_to_string(path).unwrap();
    let text_mb = json.len() as f64 / 1e6;
    let after_read = rss_mb();
    report("read");

    let document: ir::Document = serde_json::from_str(&json).unwrap();
    let after_parse = rss_mb();
    report("parsed");

    let assets = Assets::new().with_font(Default::default(), ROBOTO.to_vec());
    let built = build(&document, &assets, Options::default()).unwrap();
    let peak = rss_mb();
    report("built");

    let pages = built.pages as f64;
    println!("pages           {}", built.pages);
    println!("JSON text       {text_mb:.1} MB");
    println!("after reading   {after_read:.0} MB");
    println!("after parsing   {after_parse:.0} MB  (the declaration, in full)");
    println!(
        "peak            {peak:.0} MB  ({:.1} KB/page)",
        peak / pages * 1000.0
    );
    println!(
        "  of which rendering {:.0} MB  ({:.1} KB/page)",
        peak - after_parse,
        (peak - after_parse) / pages * 1000.0
    );
    println!("output          {:.1} MB", built.pdf.len() as f64 / 1e6);
    let _ = Pt(0.0);
}
