//! What the single-line fast path is worth on cell-shaped content.
//!
//! Both workloads go through the real `break_lines`. They differ only in
//! whether the strings repeat, which is the whole question: the shortcut
//! exists because a table cell says "Factura venta" on eight thousand rows.

use imprenta_core::units::Pt;
use imprenta_pdf::shape::Shaper;
use std::time::Instant;

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

/// A ledger's cells: labels and codes that recur, amounts that mostly do not.
fn cells(entries: u32, unique: bool) -> Vec<String> {
    let mut out = Vec::new();
    for n in 1..=entries {
        for l in 0..8u32 {
            let amount = 100.0 + ((n * 7 + l * 13) % 9000) as f32 / 3.0;
            let mut row = vec![
                format!("{}", 430000 + (l % 40)),
                format!("Cliente {}", (n * 3 + l) % 200),
                "Factura venta".to_string(),
                format!("FV-{n:06}"),
                format!("Periodo {}", (n % 12) + 1),
                format!("{amount:.2}").replace('.', ","),
                "—".to_string(),
            ];
            if unique {
                // Defeats the cache without changing the amount of shaping.
                for (i, c) in row.iter_mut().enumerate() {
                    c.push_str(&format!("{n}{l}{i}"));
                }
            }
            out.extend(row);
        }
    }
    out
}

fn run(label: &str, cells: &[String]) -> f64 {
    let mut s = Shaper::new(ROBOTO.to_vec());
    let t = Instant::now();
    for c in cells {
        std::hint::black_box(s.break_lines(c, Pt(7.0), Pt(120.0)));
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let total = s.hits() + s.misses();
    println!(
        "  {label:22} {ms:7.1} ms   {:3.0}% hit rate",
        if total == 0 {
            0.0
        } else {
            s.hits() as f64 / total as f64 * 100.0
        }
    );
    ms
}

fn main() {
    let entries: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);

    let repeating = cells(entries, false);
    let unique = cells(entries, true);
    println!("{} cells from {entries} entries", repeating.len());

    // Warm the allocator so the first run is not penalised.
    run("warmup", &repeating[..1000.min(repeating.len())]);

    let a = run("cells as they are", &repeating);
    let b = run("same, made unique", &unique);
    println!("  {:22} {:7.1}x", "speedup from reuse", b / a);
}
