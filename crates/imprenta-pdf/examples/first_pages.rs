//! Writes `first-pages.pdf` — the whole pipeline end to end.
//!
//! Only text is painted so far: no boxes, no rules, no tables, no headers.
//! What this shows is the part that is done — shaping, line breaking, widow
//! and orphan control, and pagination.
//!
//! Run with: `cargo run -p imprenta-pdf --example first_pages`

use imprenta_core::units::Pt;
use imprenta_pdf::atom::Atom;
use imprenta_pdf::measure::{TextStyle, measure_text};
use imprenta_pdf::pack::{Flow, pack};
use imprenta_pdf::render::{Geometry, render};
use imprenta_pdf::shape::{Line, Shaper};

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

const PARAGRAPHS: &[(&str, f32)] = &[
    ("Imprenta", 24.0),
    (
        "Motor de documentos en Rust con autoría en React. Esta página la ha \
         compuesto el motor de principio a fin: el texto se ha moldeado con \
         Parley contra una Roboto empotrada en el repositorio, se ha partido \
         en líneas por oportunidades de salto Unicode, cada línea se ha \
         convertido en un átomo con su altura medida, y el empaquetador ha \
         decidido en qué página cae cada uno.",
        11.0,
    ),
    ("Lo que ya decide el motor", 15.0),
    (
        "Ningún encabezado se queda solo al pie de una página. Ninguna línea \
         suelta encabeza la siguiente. Los saltos forzados respetan la \
         paridad, de modo que un capítulo puede abrir siempre en recto. Un \
         grupo que se parte entre páginas puede repetir una cabecera, y \
         puede no hacerlo: la política la elige la primitiva, no el motor.",
        11.0,
    ),
    (
        "Nada de esto se decide contando filas. Todas las alturas están \
         medidas, y el empaquetador ve el documento entero en orden, que es \
         la única forma de llevar un saldo acumulado de una página a la \
         siguiente sin romperlo.",
        11.0,
    ),
    ("Lo que todavía no hay", 15.0),
    (
        "Este PDF solo tiene texto. No hay cajas, ni reglas, ni tablas, ni \
         cabeceras de página, ni colores, ni imágenes. El empaquetador ya \
         calcula las continuaciones y los totales por página, pero el \
         pintor aún no los dibuja. Tampoco hay paralelismo: todo esto lo ha \
         hecho un solo hilo.",
        11.0,
    ),
    (
        "El texto de esta página se puede seleccionar, copiar y buscar. La \
         fuente va empotrada y subseteada, y el mapa ToUnicode se comprueba \
         en los tests contra los caracteres realmente dibujados, tildes \
         incluidas: ñ, ó, ü, ¿?, «».",
        11.0,
    ),
    (
        "Prestación de servicios profesionales, según contrato marco vigente. \
         Cliente comercial número 1042. Total asiento 4.318,75 €.",
        11.0,
    ),
];

fn main() {
    let geometry = Geometry::a4();
    let column = geometry.width - geometry.margin.horizontal();

    let mut shaper = Shaper::new(ROBOTO.to_vec());
    let mut atoms: Vec<Atom> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();

    for (text, size) in PARAGRAPHS {
        // A heading keeps with the paragraph that follows it.
        let is_heading = *size > 12.0;

        let measured = measure_text(&mut shaper, text, TextStyle::new(Pt(*size)), column);
        let last = measured.atoms.len().saturating_sub(1);

        for (i, mut atom) in measured.atoms.into_iter().enumerate() {
            if is_heading {
                atom = atom.keep_with_next();
            }
            // Space after the paragraph, folded into its last line.
            if i == last && !is_heading {
                atom.height = atom.height + Pt(*size * 0.6);
            }
            atoms.push(atom);
        }
        lines.extend(measured.lines);
    }

    let pages = pack(&Flow::new(&atoms), geometry.content_height());
    let pdf = render(&pages, &lines, ROBOTO, geometry).expect("render");

    let path = std::env::args().nth(1).unwrap_or("first-pages.pdf".into());
    std::fs::write(&path, &pdf).expect("write");

    println!(
        "{path}: {} pages, {} lines, {:.1} KB — shaping cache {} hits / {} misses",
        pages.len(),
        lines.len(),
        pdf.len() as f64 / 1024.0,
        shaper.hits(),
        shaper.misses(),
    );
}
