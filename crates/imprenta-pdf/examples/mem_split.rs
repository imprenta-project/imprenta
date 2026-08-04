//! Where the memory goes: our structures, or krilla's retained pages?
//!
//! krilla keeps every page until `finish()` — an annotation may point at a
//! page not written yet — so its retention is a floor we cannot lower from
//! outside. Ours is not.

use imprenta_core::units::Pt;
use imprenta_pdf::atom::Atom;
use imprenta_pdf::content::Content;
use imprenta_pdf::measure::{TextStyle, measure_text};
use imprenta_pdf::pack::{Flow, pack};
use imprenta_pdf::render::{Geometry, Options, render_with};
use imprenta_pdf::shape::Shaper;

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

#[cfg(target_os = "macos")]
fn rss_mb() -> f64 {
    // Resident size via `ps`, which is enough for an order-of-magnitude split.
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .unwrap_or(0.0)
        / 1024.0
}

fn main() {
    let entries: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5000);
    let geometry = Geometry::a4();
    let column = geometry.width - geometry.margin.horizontal();
    let style = TextStyle::new(Pt(7.0)).with_widows_orphans(1, 1);

    let base = rss_mb();
    let mut shaper = Shaper::new(ROBOTO.to_vec());
    let mut atoms: Vec<Atom> = Vec::new();
    let mut contents: Vec<Content> = Vec::new();

    for n in 0..entries {
        for l in 0..10 {
            let text = format!(
                "{}   Cliente comercial número {}   Factura venta   FV-2026-{n:06}-{l}   \
                 Prestación de servicios profesionales periodo {}   {:.2}",
                430000 + l % 40,
                (n * 3 + l) % 2000,
                (n % 12) + 1,
                100.0 + ((n * 7 + l * 13) % 9000) as f32 / 3.0
            );
            let m = measure_text(&mut shaper, &text, style, column);
            atoms.extend(m.atoms);
            contents.extend(m.lines.into_iter().map(Content::Text));
        }
    }
    let after_measure = rss_mb();

    let pages = pack(&Flow::new(&atoms), geometry.content_height());
    let after_pack = rss_mb();
    let page_count = pages.len();

    let pdf = render_with(&pages, &contents, ROBOTO, geometry, Options::default()).expect("render");
    let after_render = rss_mb();

    println!("{page_count} pages, {} atoms", atoms.len());
    println!("  baseline           {base:8.1} MB");
    println!(
        "  after measure      {after_measure:8.1} MB   (+{:.1} — ours)",
        after_measure - base
    );
    println!(
        "  after pack         {after_pack:8.1} MB   (+{:.1} — ours)",
        after_pack - after_measure
    );
    println!(
        "  after render       {after_render:8.1} MB   (+{:.1} — krilla)",
        after_render - after_pack
    );
    println!();
    println!(
        "  ours   {:6.1} KB/page",
        (after_pack - base) * 1024.0 / page_count as f64
    );
    println!(
        "  krilla {:6.1} KB/page",
        (after_render - after_pack) * 1024.0 / page_count as f64
    );
    println!(
        "  pdf    {:6.1} KB/page",
        pdf.len() as f64 / 1024.0 / page_count as f64
    );
}
