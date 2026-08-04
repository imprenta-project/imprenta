//! Times the three phases over a ledger-shaped document.
//!
//! **Not a like-for-like comparison with a real table.** A seven-column table
//! with borders, alternating backgrounds and per-cell positioning is more work
//! per row than this measures; the path below only draws lines of text, and it
//! cannot yet use the shaping cache — see the note the run prints. Treat the
//! numbers as a bound on the text path, not as a verdict on the design.
//!
//! Run with: `cargo run -p imprenta-pdf --release --example bench_ledger -- 1000`

use imprenta_core::units::Pt;
use imprenta_pdf::atom::Atom;
use imprenta_pdf::measure::{TextStyle, measure_text};
use imprenta_pdf::pack::{Flow, pack};
use imprenta_pdf::render::{Geometry, render};
use imprenta_pdf::shape::{Line, Shaper};
use std::time::Instant;

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");
const SIZE: f32 = 7.0;

fn money(v: f32) -> String {
    format!("{v:.2}").replace('.', ",")
}

/// One ledger entry: a heading, eight movement lines, a total.
fn entry(n: u32) -> Vec<String> {
    let mut out = Vec::with_capacity(10);
    out.push(format!(
        "Asiento {n}          {:02}/{:02}/2026",
        (n % 28) + 1,
        (n % 12) + 1
    ));

    let mut debit = 0.0f32;
    for l in 0..8u32 {
        let amount = 100.0 + ((n * 7 + l * 13) % 9000) as f32 / 3.0;
        debit += amount;
        out.push(format!(
            "{}   Cliente comercial número {}   Factura venta   FV-2026-{:06}-{}   \
             Prestación de servicios profesionales periodo {}   {}",
            430000 + (l % 40),
            (n * 3 + l) % 2000,
            n,
            l,
            (n % 12) + 1,
            money(amount),
        ));
    }
    out.push(format!("Total asiento          {}", money(debit)));
    out
}

fn main() {
    let entries: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);

    let geometry = Geometry::a4();
    let column = geometry.width - geometry.margin.horizontal();
    let style = TextStyle::new(Pt(SIZE)).with_widows_orphans(1, 1);

    let mut shaper = Shaper::new(ROBOTO.to_vec());
    let mut atoms: Vec<Atom> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();

    // ── Phase A ─────────────────────────────────────────────────────────
    let t0 = Instant::now();
    for n in 1..=entries {
        let rows = entry(n);
        for (i, row) in rows.iter().enumerate() {
            let m = measure_text(&mut shaper, row, style, column);
            // The heading keeps with the first movement line.
            let heading = i == 0;
            for mut atom in m.atoms {
                if heading {
                    atom = atom.keep_with_next();
                }
                atoms.push(atom);
            }
            lines.extend(m.lines);
        }
    }
    let measure_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // ── Phase B ─────────────────────────────────────────────────────────
    let t0 = Instant::now();
    let pages = pack(&Flow::new(&atoms), geometry.content_height());
    let pack_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // ── Phase C ─────────────────────────────────────────────────────────
    let t0 = Instant::now();
    let pdf = render(&pages, &lines, ROBOTO, geometry).expect("render");
    let render_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let total = measure_ms + pack_ms + render_ms;
    let n = pages.len() as f64;

    println!(
        "entries={entries}  lines={}  pages={}  pdf={:.1} MB",
        lines.len(),
        pages.len(),
        pdf.len() as f64 / 1e6
    );
    println!(
        "  measure {measure_ms:8.1} ms  ({:4.1}%)\n  \
           pack    {pack_ms:8.2} ms  ({:4.1}%)\n  \
           render  {render_ms:8.1} ms  ({:4.1}%)\n  \
           TOTAL   {total:8.1} ms   =  {:.2} ms/page",
        measure_ms / total * 100.0,
        pack_ms / total * 100.0,
        render_ms / total * 100.0,
        total / n
    );
    println!(
        "  shaping cache: {} hits / {} misses  ({:.0}% hit rate)",
        shaper.hits(),
        shaper.misses(),
        if shaper.hits() + shaper.misses() == 0 {
            0.0
        } else {
            shaper.hits() as f64 / (shaper.hits() + shaper.misses()) as f64 * 100.0
        }
    );

    if let Some(path) = std::env::args().nth(2) {
        std::fs::write(path, &pdf).expect("write");
    }
}
