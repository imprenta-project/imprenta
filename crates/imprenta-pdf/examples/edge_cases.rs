//! Renders the content most likely to expose a rendering defect.
//!
//! Exercises the cases most likely to go wrong: a paragraph straddling a
//! page break, a line that exactly fills the content box, text wider than
//! the column, a forced parity break, mixed sizes, and accented text.
//!
//! Also writes each page as its own PDF (`page-01.pdf`, …) so every page can
//! be rasterised, not just the first.
//!
//! Run: `cargo run -p imprenta-pdf --example edge_cases -- preview/`

use imprenta_core::units::Pt;
use imprenta_pdf::atom::{Atom, Break};
use imprenta_pdf::measure::{TextStyle, measure_text};
use imprenta_pdf::pack::{Flow, pack};
use imprenta_pdf::render::{Geometry, render};
use imprenta_pdf::shape::{Line, Shaper};

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");

fn main() {
    let out = std::env::args().nth(1).unwrap_or("out".into());
    std::fs::create_dir_all(&out).expect("mkdir");

    let geometry = Geometry::a4();
    let column = geometry.width - geometry.margin.horizontal();

    let mut shaper = Shaper::new(ROBOTO.to_vec());
    let mut atoms: Vec<Atom> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();

    let add = |shaper: &mut Shaper,
               atoms: &mut Vec<Atom>,
               lines: &mut Vec<Line>,
               text: &str,
               size: f32,
               width: Pt,
               heading: bool,
               brk: Break| {
        let m = measure_text(shaper, text, TextStyle::new(Pt(size)), width);
        let start = atoms.len();
        atoms.extend(m.atoms);
        lines.extend(m.lines);
        if heading {
            for a in &mut atoms[start..] {
                a.keep_with_next = true;
            }
        }
        if brk != Break::Auto && start < atoms.len() {
            atoms[start].break_before = brk;
        }
        if let Some(last) = atoms.last_mut() {
            last.height = last.height + Pt(size * 0.5);
        }
    };

    const LOREM: &str = "El empaquetador recorre el documento en orden y decide en \
        qué página cae cada átomo. Ninguna línea suelta encabeza una página, ningún \
        encabezado se queda solo al pie, y los saltos forzados respetan la paridad. \
        Nada de esto se decide contando filas: todas las alturas están medidas.";

    add(
        &mut shaper,
        &mut atoms,
        &mut lines,
        "Prueba de artefactos",
        22.0,
        column,
        true,
        Break::Auto,
    );
    add(
        &mut shaper,
        &mut atoms,
        &mut lines,
        "Cada bloque de esta página busca un defecto concreto. Si algo se solapa, se \
         sale del margen, se recorta o sale con el glifo equivocado, se ve aquí.",
        10.0,
        column,
        false,
        Break::Auto,
    );

    add(
        &mut shaper,
        &mut atoms,
        &mut lines,
        "1 · Tildes, puntuación y símbolos",
        13.0,
        column,
        true,
        Break::Auto,
    );
    add(
        &mut shaper,
        &mut atoms,
        &mut lines,
        "áéíóú ÁÉÍÓÚ ñÑ üÜ ¿? ¡! «» “” ‘’ — – · € £ ¢ ½ ¼ ° ± × ÷ ‰ † ‡ § ¶ © ® ™ \
         Prestación · Contabilización · Añadió · Güell · ¿Cuánto? ¡Ojo!",
        10.0,
        column,
        false,
        Break::Auto,
    );

    add(
        &mut shaper,
        &mut atoms,
        &mut lines,
        "2 · Tamaños mezclados en secuencia",
        13.0,
        column,
        true,
        Break::Auto,
    );
    for size in [7.0f32, 9.0, 11.0, 14.0, 18.0] {
        add(
            &mut shaper,
            &mut atoms,
            &mut lines,
            &format!(
                "{size:.0} pt — las líneas no deben tocarse ni solaparse jamás, ni siquiera con descendentes: pgqjy Áàâ"
            ),
            size,
            column,
            false,
            Break::Auto,
        );
    }

    add(
        &mut shaper,
        &mut atoms,
        &mut lines,
        "3 · Columna estrecha y desbordamiento",
        13.0,
        column,
        true,
        Break::Auto,
    );
    add(
        &mut shaper,
        &mut atoms,
        &mut lines,
        "Esta columna mide 90 pt y contiene la palabra Contabilización, que no cabe \
         entera. Debe desbordar, no partirse a mitad de letra.",
        10.0,
        Pt(90.0),
        false,
        Break::Auto,
    );

    add(
        &mut shaper,
        &mut atoms,
        &mut lines,
        "4 · Párrafo que cruza el corte de página",
        13.0,
        column,
        true,
        Break::Auto,
    );
    for _ in 0..6 {
        add(
            &mut shaper,
            &mut atoms,
            &mut lines,
            LOREM,
            10.0,
            column,
            false,
            Break::Auto,
        );
    }

    add(
        &mut shaper,
        &mut atoms,
        &mut lines,
        "5 · Salto forzado a página impar",
        13.0,
        column,
        true,
        Break::Odd,
    );
    add(
        &mut shaper,
        &mut atoms,
        &mut lines,
        "Este bloque abre en recto. Si la página anterior era impar, la que precede a \
         ésta debe salir en blanco — y en blanco significa vacía, no con restos.",
        10.0,
        column,
        false,
        Break::Auto,
    );

    add(
        &mut shaper,
        &mut atoms,
        &mut lines,
        "6 · Última línea contra el margen inferior",
        13.0,
        column,
        true,
        Break::Auto,
    );
    for i in 1..=40 {
        add(
            &mut shaper,
            &mut atoms,
            &mut lines,
            &format!(
                "Línea {i:02} — el bloque de texto no puede rebasar el margen inferior ni quedarse corto sin motivo."
            ),
            9.0,
            column,
            false,
            Break::Auto,
        );
    }

    let pages = pack(&Flow::new(&atoms), geometry.content_height());
    let pdf = render(&pages, &lines, ROBOTO, geometry).expect("render");
    std::fs::write(format!("{out}/all.pdf"), &pdf).expect("write");

    // One PDF per page, so every page can be rasterised.
    for (i, page) in pages.iter().enumerate() {
        let single = render(std::slice::from_ref(page), &lines, ROBOTO, geometry).expect("render");
        std::fs::write(format!("{out}/page-{:02}.pdf", i + 1), single).expect("write");
    }

    // What the eye cannot check: no line box may cross the content box, and
    // no two lines on a page may overlap.
    let mut problems = 0;
    for (i, page) in pages.iter().enumerate() {
        let mut prev_bottom = -1.0f32;
        for pl in &page.placements {
            let h = lines[pl.atom].height.get();
            let top = pl.y.get();
            let bottom = top + h;
            if bottom > geometry.content_height().get() + 0.01 {
                println!(
                    "  ! page {} line at y={top:.1} ends at {bottom:.1}, past the {:.1} budget",
                    i + 1,
                    geometry.content_height().get()
                );
                problems += 1;
            }
            if top < prev_bottom - 0.01 {
                println!(
                    "  ! page {} line at y={top:.1} overlaps the one ending at {prev_bottom:.1}",
                    i + 1
                );
                problems += 1;
            }
            prev_bottom = bottom;
        }
    }
    println!(
        "geometry check: {}",
        if problems == 0 {
            "clean".to_string()
        } else {
            format!("{problems} PROBLEMS")
        }
    );

    println!("{} pages -> {out}/", pages.len());
    for (i, p) in pages.iter().enumerate() {
        let last = p.placements.last().map(|pl| pl.y.get()).unwrap_or(0.0);
        println!(
            "  page {:2}: {:3} lines, lowest y = {:6.1} pt (budget {:.1})",
            i + 1,
            p.placements.len(),
            last,
            geometry.content_height().get()
        );
    }
}
