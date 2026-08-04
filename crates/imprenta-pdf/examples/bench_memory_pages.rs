//! Memory held while composing, streamed against all-at-once.

use imprenta_core::units::Pt;
use imprenta_pdf::atom::Atom;
use imprenta_pdf::compose::Composer;
use imprenta_pdf::content::Content;
use imprenta_pdf::measure::{TextStyle, measure_text};
use imprenta_pdf::pack::{Flow, pack};
use imprenta_pdf::parallel::{Block, Faces, measure_all_in};
use imprenta_pdf::render::{Fonts, Geometry, Options, render_faces};
use imprenta_pdf::shape::Shaper;
use std::time::Instant;

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

fn rss_mb() -> f64 {
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

fn row(shaper: &mut Shaper, i: usize, width: Pt) -> (Atom, Content) {
    let m = measure_text(
        shaper,
        &format!(
            "{}   Cliente comercial número {}   FV-2026-{i:06}   Prestación de servicios   {:.2}",
            430000 + i % 40,
            (i * 3) % 2000,
            100.0 + ((i * 7) % 9000) as f32 / 3.0
        ),
        TextStyle::new(Pt(8.0)),
        width,
    );
    let line = m.lines.into_iter().next().unwrap();
    (Atom::new(line.height), Content::Text(line))
}

fn main() {
    let rows: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(50_000);
    let streaming = std::env::args()
        .nth(2)
        .map(|s| s != "batch")
        .unwrap_or(true);

    let geometry = Geometry::a4();
    let width = geometry.width - geometry.margin.horizontal();
    let shaper = Shaper::new(ROBOTO.to_vec());
    let fonts = Fonts::from_shaper(&shaper).unwrap();
    let mut shaper = shaper;

    let base = rss_mb();
    let t = Instant::now();
    let mut peak = base;

    let mode = std::env::args().nth(2).unwrap_or_else(|| "stream".into());

    if mode == "both" {
        // Streaming and parallel measuring together: blocks are measured in
        // batches across every core, then fed in order and released as they
        // are painted.
        let faces: Faces = vec![(imprenta_pdf::shape::Face::REGULAR, ROBOTO.to_vec())];
        let mut c = Composer::new(geometry, fonts.clone()).unwrap();
        const BATCH: usize = 4096;

        let mut i = 0usize;
        while i < rows {
            let n = BATCH.min(rows - i);
            let texts: Vec<String> = (i..i + n)
                .map(|k| {
                    format!(
                        "{}   Cliente comercial número {}   FV-2026-{k:06}   Prestación de servicios   {:.2}",
                        430000 + k % 40,
                        (k * 3) % 2000,
                        100.0 + ((k * 7) % 9000) as f32 / 3.0
                    )
                })
                .collect();
            let blocks: Vec<Block<'_>> = texts
                .iter()
                .map(|t| Block::new(t, TextStyle::new(Pt(8.0)), width))
                .collect();

            for measured in measure_all_in(&faces, &blocks) {
                for (atom, line) in measured.atoms.into_iter().zip(measured.lines) {
                    c.push(atom, Content::Text(line));
                }
            }
            c.flush();
            peak = peak.max(rss_mb());
            i += n;
        }

        let out = c.finish().unwrap();
        peak = peak.max(rss_mb());
        if let Ok(path) = std::env::var("IMPRENTA_OUT") {
            std::fs::write(path, &out.pdf).expect("write");
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "streaming + parallel · {rows} rows → {} pages · {ms:.0} ms · peak {:.0} MB ({:.1} KB/page) · pdf {:.1} MB",
            out.totals.len(),
            peak,
            (peak - base) * 1024.0 / out.totals.len() as f64,
            out.pdf.len() as f64 / 1e6
        );
        return;
    }

    let (pages, bytes) = if streaming {
        let mut c = Composer::new(geometry, fonts.clone()).unwrap();
        for i in 0..rows {
            let (atom, content) = row(&mut shaper, i, width);
            c.push(atom, content);
            if i % 64 == 0 {
                c.flush();
                if i % 4096 == 0 {
                    peak = peak.max(rss_mb());
                }
            }
        }
        let n = {
            c.flush();
            c.pages()
        };
        let _ = n;
        let out = c.finish().unwrap();
        peak = peak.max(rss_mb());
        (out.totals.len(), out.pdf.len())
    } else {
        let mut atoms = Vec::new();
        let mut contents = Vec::new();
        for i in 0..rows {
            let (atom, content) = row(&mut shaper, i, width);
            atoms.push(atom);
            contents.push(content);
            if i % 4096 == 0 {
                peak = peak.max(rss_mb());
            }
        }
        let packed = pack(&Flow::new(&atoms), geometry.content_height());
        peak = peak.max(rss_mb());
        let pdf = render_faces(&packed, &contents, &fonts, geometry, Options::default()).unwrap();
        peak = peak.max(rss_mb());
        (packed.len(), pdf.len())
    };

    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "{} · {rows} rows → {pages} pages · {ms:.0} ms · peak {:.0} MB ({:.1} KB/page) · pdf {:.1} MB",
        if streaming {
            "streaming"
        } else {
            "all at once"
        },
        peak,
        (peak - base) * 1024.0 / pages as f64,
        bytes as f64 / 1e6
    );
}
