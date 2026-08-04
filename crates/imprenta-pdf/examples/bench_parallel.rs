//! What parallel measuring is worth, and what Amdahl leaves on the table.

use imprenta_core::units::Pt;
use imprenta_pdf::measure::{TextStyle, measure_text};
use imprenta_pdf::parallel::{Block, measure_all};
use imprenta_pdf::shape::Shaper;
use std::time::Instant;

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_000);
    let texts: Vec<String> = (0..n)
        .map(|i| {
            format!(
                "{}   Cliente comercial número {}   Prestación de servicios profesionales periodo {}",
                430000 + i % 40,
                (i * 3) % 2000,
                (i % 12) + 1
            )
        })
        .collect();
    let blocks: Vec<Block<'_>> = texts
        .iter()
        .map(|t| Block::new(t, TextStyle::new(Pt(8.0)), Pt(240.0)))
        .collect();

    let t = Instant::now();
    let mut shaper = Shaper::new(ROBOTO.to_vec());
    let serial: usize = texts
        .iter()
        .map(|t| measure_text(&mut shaper, t, TextStyle::new(Pt(8.0)), Pt(240.0)).len())
        .sum();
    let serial_ms = t.elapsed().as_secs_f64() * 1000.0;

    let t = Instant::now();
    let parallel: usize = measure_all(ROBOTO, &blocks).iter().map(|m| m.len()).sum();
    let parallel_ms = t.elapsed().as_secs_f64() * 1000.0;

    assert_eq!(serial, parallel, "the two paths disagreed");
    println!("{n} blocks on {} threads", rayon::current_num_threads());
    println!("  sequential {serial_ms:8.1} ms");
    println!(
        "  parallel   {parallel_ms:8.1} ms   ({:.1}x)",
        serial_ms / parallel_ms
    );
}
