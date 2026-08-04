//! Renders a document declared entirely as JSON.
//!
//! No Rust describes this invoice — the whole thing is in
//! `examples/data/invoice.json`. That file is what React will emit, and what
//! any other producer can emit instead.
//!
//! Run: `cargo run -p imprenta-pdf --release --example from_json -- preview/`

use imprenta_pdf::build::{Assets, build};
use imprenta_pdf::ir;
use imprenta_pdf::render::Options;
use imprenta_pdf::shape::Face;

const REGULAR: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");
const ITALIC: &[u8] = include_bytes!("../tests/fonts/Roboto-Italic.ttf");
const LOGO: &[u8] = include_bytes!("../tests/images/logo.png");
const DOCUMENT: &str = include_str!("data/invoice.json");

fn main() {
    let out = std::env::args().nth(1).unwrap_or("preview".into());
    std::fs::create_dir_all(&out).expect("mkdir");

    let document: ir::Document = serde_json::from_str(DOCUMENT).expect("the IR must parse");
    let assets = Assets::new()
        .with_font(Face::REGULAR, REGULAR.to_vec())
        .with_font(Face::BOLD, BOLD.to_vec())
        .with_font(Face::ITALIC, ITALIC.to_vec())
        .with_image("logo", LOGO.to_vec())
        .expect("the logo is a PNG");

    let built = build(&document, &assets, Options::default()).expect("build");

    let path = format!("{out}/from-json.pdf");
    std::fs::write(&path, &built.pdf).expect("write");
    println!(
        "{path}: {} page(s), {:.1} KB",
        built.pages,
        built.pdf.len() as f64 / 1024.0
    );
    for d in &built.diagnostics {
        println!("  {d}");
    }
}
