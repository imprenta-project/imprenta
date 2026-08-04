//! Inline style: weight, italics and colour changing mid-sentence.
//!
//! Shaped as one layout per paragraph, not one per stretch, so kerning
//! crosses a style boundary and a line break may fall inside a bold phrase.

use imprenta_core::color::Color;
use imprenta_core::units::{Edges, Pt};
use imprenta_pdf::atom::Atom;
use imprenta_pdf::compose::Composer;
use imprenta_pdf::content::{BoxContent, Content};
use imprenta_pdf::render::{Fonts, Geometry, Options};
use imprenta_pdf::shape::{Face, Shaper, TextRun};

const REGULAR: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");
const ITALIC: &[u8] = include_bytes!("../tests/fonts/Roboto-Italic.ttf");

fn hex(s: &str) -> Color {
    Color::parse_hex(s).expect("hex")
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or("preview".into());
    std::fs::create_dir_all(&out).expect("mkdir");

    let geometry = Geometry {
        width: Pt::mm(210.0),
        height: Pt::mm(297.0),
        margin: Edges::all(Pt::mm(20.0)),
        bands: Default::default(),
    };
    let width = geometry.width - geometry.margin.horizontal();

    let mut shaper = Shaper::with_faces([
        (Face::REGULAR, REGULAR.to_vec()),
        (Face::BOLD, BOLD.to_vec()),
        (Face::ITALIC, ITALIC.to_vec()),
    ]);
    let fonts = Fonts::from_shaper(&shaper).expect("fonts");
    let mut composer = Composer::with_options(geometry, fonts.clone(), Options::default()).unwrap();

    let navy = hex("#1F4E79");
    let ink = hex("#2A2A2A");
    let red = hex("#B4453C");

    let paragraphs: Vec<(f32, Vec<TextRun>)> = vec![
        (
            22.0,
            vec![TextRun::new("Texto enriquecido").bold().inked(navy)],
        ),
        (
            11.0,
            vec![
                TextRun::new("Un párrafo puede cambiar de ").inked(ink),
                TextRun::new("peso").bold().inked(ink),
                TextRun::new(", de ").inked(ink),
                TextRun::new("inclinación").italic().inked(ink),
                TextRun::new(" y de ").inked(ink),
                TextRun::new("color").inked(red),
                TextRun::new(" a mitad de frase, y sigue siendo un solo párrafo.").inked(ink),
            ],
        ),
        (
            11.0,
            vec![
                TextRun::new("Se moldea con ").inked(ink),
                TextRun::new("una sola pasada").bold().inked(navy),
                TextRun::new(
                    " sobre todo el párrafo, no una por tramo. Eso importa por tres \
                     razones concretas: el kerning cruza el límite entre estilos, las \
                     letras árabes se enlazan a través de él, y un salto de línea puede \
                     caer justo en medio de una frase en negrita — como esta, que es \
                     deliberadamente larga para que el corte le toque por dentro y se \
                     vea que ",
                )
                .inked(ink),
                TextRun::new(
                    "la continuación conserva el peso que le corresponde y no vuelve a \
                     la redonda al empezar la línea siguiente",
                )
                .bold()
                .inked(ink),
                TextRun::new(".").inked(ink),
            ],
        ),
        (
            11.0,
            vec![
                TextRun::new("Tildes y símbolos entre estilos: ").inked(ink),
                TextRun::new("ñ ó ü ¿? ¡! «» — €").bold().inked(navy),
                TextRun::new(" · ").inked(ink),
                TextRun::new("ñ ó ü ¿? ¡! «» — €").italic().inked(red),
            ],
        ),
    ];

    for (size, runs) in paragraphs {
        let lines = shaper.break_rich(&runs, Pt(size), width);
        let mut boxed = BoxContent::default().with_width(width);
        for line in lines {
            boxed = boxed.stack(Content::Text(line));
        }
        let mut atom = Atom::new(boxed.height() + Pt(size * 0.6));
        atom.keep_with_next = size > 12.0;
        composer.push(atom, Content::Box(boxed));
    }

    let composed = composer.finish().expect("render");
    let path = format!("{out}/rich-text.pdf");
    std::fs::write(&path, &composed.pdf).expect("write");
    println!(
        "{path}: {} pages, {:.1} KB",
        composed.totals.len(),
        composed.pdf.len() as f64 / 1024.0
    );
}
