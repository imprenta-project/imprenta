//! What the writer holds while it writes.
//!
//! Its own test binary, because the counter is a global allocator and
//! anything else running alongside would be counted into the answer.
//!
//! The claim this crate exists to make is that a finished page is *gone*: its
//! bytes are in the output and its offset is in a list, and nothing else
//! survives it. The writer it replaced kept every page until the document
//! closed and then built the whole file beside them — 5.25 KB retained per
//! page against 2.22 KB of output, measured on a ten thousand page ledger.
//!
//! So the assertion is about the shape and not about a number: what is live
//! at the end, minus the file itself, must not grow with the page count.

use imprenta_core::color::Color;
use imprenta_pdf_write::{Glyph, Settings, Writer};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
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

const ROBOTO: &[u8] = include_bytes!("../../imprenta-pdf/tests/fonts/Roboto-Regular.ttf");

/// A dense page of text, rendered `pages` times, and what it cost.
///
/// Returns the peak live bytes over the run and the size of the file.
fn ledger(pages: usize) -> (usize, usize) {
    let line = "430000  12/03/2024  Prestación de servicios profesionales  1.284,55";
    let shaped: Vec<Glyph> = line
        .char_indices()
        .map(|(at, c)| Glyph {
            // Any plausible glyph id: what is being measured is what the
            // writer keeps, not what the letters look like.
            id: ((c as u32) % 300) as u16 + 1,
            x_advance: 4.6,
            text: at..at + c.len_utf8(),
        })
        .collect();

    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);

    let mut writer = Writer::new(Settings::default());
    let face = writer.add_face(ROBOTO.to_vec()).unwrap();
    for _ in 0..pages {
        let mut page = writer.page(595.0, 842.0);
        for row in 0..48 {
            page.glyphs(
                face,
                9.0,
                34.0,
                60.0 + 14.0 * row as f32,
                &shaped,
                line,
                Color::BLACK,
            );
        }
        page.finish();
    }
    let pdf = writer.finish().unwrap();

    let peak = PEAK.load(Relaxed).saturating_sub(base);
    let size = pdf.len();
    drop(pdf);
    (peak, size)
}

// One at a time: the counter is the whole process.
static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn a_finished_page_costs_its_bytes_and_nothing_else() {
    let _held = GATE.lock().unwrap_or_else(|e| e.into_inner());

    let (small_peak, small_size) = ledger(200);
    let (large_peak, large_size) = ledger(2_000);

    // The file grew by however much it grew; what is being asked is whether
    // anything *else* did. `Vec` doubles, so the buffer holding the output
    // can be up to twice the file — hence three rather than one.
    let overhead = |peak: usize, size: usize| peak as f64 / size as f64;

    assert!(
        overhead(large_peak, large_size) < 3.0,
        "2 000 pages held {:.1} MB for a {:.1} MB file",
        large_peak as f64 / 1e6,
        large_size as f64 / 1e6,
    );
    assert!(
        overhead(large_peak, large_size) < overhead(small_peak, small_size) + 0.5,
        "ten times the pages cost {:.2}× the file where two hundred cost {:.2}×",
        overhead(large_peak, large_size),
        overhead(small_peak, small_size),
    );
}

#[test]
fn finishing_holds_one_copy_of_the_file_and_not_two() {
    let _held = GATE.lock().unwrap_or_else(|e| e.into_inner());

    // The join that used to close `finish` was the last place the writer held
    // a document twice, and it is gone: the file comes back still in its
    // blocks (issue #7). The peak over a large ledger is therefore the file
    // plus the working set, and must never approach the two-copies line the
    // old `into_vec` sat on. Large deliberately: blocks double up to their
    // ceiling, so a small file's *capacity* can near twice its content and
    // would hide the join behind the same ratio — at six thousand pages the
    // overshoot is one part-filled block and the claim is measurable.
    let (peak, size) = ledger(6_000);

    assert!(
        (peak as f64) < size as f64 * 1.5,
        "6 000 pages peaked at {:.1} MB for a {:.1} MB file, which is two copies, not one",
        peak as f64 / 1e6,
        size as f64 / 1e6,
    );
}
