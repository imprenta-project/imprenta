//! A commercial invoice: letterhead with a logo, a two-column address block,
//! a line-item table, and a totals panel.
//!
//! Everything visual is chosen here, by the caller. The engine supplied no
//! colour, no rule, no padding and no alignment of its own.
//!
//! Run: `cargo run -p imprenta-pdf --release --example invoice -- preview/`

use imprenta_core::color::Color;
use imprenta_core::units::{Edges, Length, Pt};
use imprenta_pdf::atom::Atom;
use imprenta_pdf::content::{BoxContent, Content, ImageContent, ImageFormat};
use imprenta_pdf::decoration::{BorderSide, Decoration};
use imprenta_pdf::measure::{TextStyle, measure_text_in};
use imprenta_pdf::pack::{Flow, Group, Repeat, pack};
use imprenta_pdf::render::Fonts;
use imprenta_pdf::render::{Geometry, Options, render_faces};
use imprenta_pdf::shape::{Face, Shaper};
use imprenta_pdf::table::{Align, Cell, Column, Layout};

const ROBOTO: &[u8] = include_bytes!("../tests/fonts/Roboto-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../tests/fonts/Roboto-Bold.ttf");
const LOGO: &[u8] = include_bytes!("../tests/images/logo.png");
const MARK: &[u8] = include_bytes!("../tests/images/mark.png");
const ITEMS: usize = 34;

fn hex(s: &str) -> Color {
    Color::parse_hex(s).expect("hex")
}

struct Doc {
    shaper: Shaper,
    atoms: Vec<Atom>,
    contents: Vec<Content>,
}

impl Doc {
    fn push(&mut self, content: Content) {
        self.atoms.push(Atom::new(content.height()));
        self.contents.push(content);
    }

    fn gap(&mut self, height: f32) {
        self.atoms.push(Atom::new(Pt(height)));
        self.contents.push(Content::Empty);
    }

    fn text(&mut self, s: &str, size: f32, color: Color, width: Pt) -> Content {
        self.styled(s, size, color, width, Face::REGULAR)
    }

    fn bold(&mut self, s: &str, size: f32, color: Color, width: Pt) -> Content {
        self.styled(s, size, color, width, Face::BOLD)
    }

    fn styled(&mut self, s: &str, size: f32, color: Color, width: Pt, face: Face) -> Content {
        let m = measure_text_in(&mut self.shaper, s, TextStyle::new(Pt(size)), width, face);
        let mut boxed = BoxContent::default();
        for line in m.lines {
            boxed = boxed.stack(Content::Text(line.with_color(color)));
        }
        Content::Box(boxed)
    }
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or("preview".into());
    std::fs::create_dir_all(&out).expect("mkdir");

    let geometry = Geometry {
        width: Pt::mm(210.0),
        height: Pt::mm(297.0),
        margin: Edges::symmetric(Pt::mm(16.0), Pt::mm(18.0)),
        bands: Default::default(),
    };
    let full = geometry.width - geometry.margin.horizontal();

    let navy = hex("#1F4E79");
    let ink = hex("#2A2A2A");
    let muted = hex("#7A7A7A");
    let white = hex("#FFFFFF");
    let hairline = BorderSide {
        width: Pt(0.4),
        color: hex("#DCE6F5"),
    };

    let mut d = Doc {
        shaper: Shaper::with_faces([
            (Face::REGULAR, ROBOTO.to_vec()),
            (Face::BOLD, BOLD.to_vec()),
        ]),
        atoms: Vec::new(),
        contents: Vec::new(),
    };

    // ── Letterhead: logo left, invoice reference right ───────────────────
    let logo = ImageContent::scaled_to_width(LOGO, ImageFormat::Png, (240, 80), Pt(108.0));
    let ref_block = {
        let title = d.bold("FACTURA", 20.0, navy, Pt(180.0));
        let number = d.bold("FV-2026-00418", 11.0, ink, Pt(180.0));
        let date = d.text("Emitida el 2 de agosto de 2026", 8.5, muted, Pt(180.0));
        let mut b = BoxContent::default();
        for part in [title, number, date] {
            b = b.stack(part);
        }
        b
    };
    let ref_width = ref_block.height();
    let _ = ref_width;
    d.push(Content::Box(
        BoxContent::default()
            .place(Pt(0.0), Content::Image(logo))
            .place(Pt(300.0), Content::Box(ref_block)),
    ));

    d.gap(18.0);

    // ── Two address panels side by side ──────────────────────────────────
    let panel_width = Pt((full.get() - 14.0) / 2.0);
    let panel = |d: &mut Doc, heading: &str, lines: &[&str], fill: Option<Color>| {
        let inner = panel_width - Pt(20.0);
        let mut b = BoxContent::new(Decoration {
            background: fill,
            border: Edges::all(Some(hairline)),
            ..Default::default()
        })
        .with_width(panel_width)
        .with_padding(Edges::all(Pt(10.0)));
        b = b.stack(d.text(heading, 7.5, muted, inner));
        b = b.stack(Content::Empty);
        for (i, line) in lines.iter().enumerate() {
            b = if i == 0 {
                b.stack(d.bold(line, 9.5, ink, inner))
            } else {
                b.stack(d.text(line, 9.0, ink, inner))
            };
        }
        b
    };
    let emisor = panel(
        &mut d,
        "EMISOR",
        &[
            "CONTAPRO SOFTWARE, S.L.",
            "B-87654321",
            "Calle Mayor 14, 3º",
            "35001 Las Palmas de Gran Canaria",
        ],
        None,
    );
    let receptor = panel(
        &mut d,
        "DESTINATARIO",
        &[
            "Distribuciones Atlántico, S.A.",
            "A-12345678",
            "Avenida del Puerto 221",
            "46023 Valencia",
        ],
        Some(hex("#F5F8FD")),
    );

    let tallest = emisor.height().get().max(receptor.height().get());
    d.push(Content::Box(
        BoxContent::default()
            .place(Pt(0.0), Content::Box(emisor))
            .place(panel_width + Pt(14.0), Content::Box(receptor)),
    ));
    let _ = tallest;

    d.gap(20.0);

    // ── Line items ───────────────────────────────────────────────────────
    let pad = Edges::symmetric(Pt(4.0), Pt(6.0));
    let table = Layout::new(
        vec![
            Column::new(Length::Pt(Pt(38.0))),
            Column::new(Length::Auto),
            Column::new(Length::Pt(Pt(46.0))).aligned(Align::End),
            Column::new(Length::Pt(Pt(62.0))).aligned(Align::End),
            Column::new(Length::Pt(Pt(70.0))).aligned(Align::End),
        ],
        full - pad.horizontal(),
    );

    let table_start = d.atoms.len();
    let header = table.row(
        &mut d.shaper,
        &["Ref.", "Concepto", "Uds.", "Precio", "Importe"]
            .map(|t| Cell::new(t, Pt(8.0)).inked(white).bold()),
        Decoration {
            background: Some(navy),
            ..Default::default()
        },
        pad,
    );
    let header_height = header.height();
    d.atoms.push(Atom::new(header_height).keep_with_next());
    d.contents.push(Content::Box(header));

    let concepts = [
        "Licencia anual Imprenta Server, plan profesional",
        "Implantación y migración de plantillas existentes",
        "Soporte prioritario 24×7, bolsa de 40 horas",
        "Formación in situ para el equipo de desarrollo",
        "Auditoría de accesibilidad PDF/UA sobre 47 informes",
    ];
    let mut net = 0.0f64;
    for i in 0..ITEMS {
        let units = 1 + (i * 3) % 9;
        let price = 120.0 + ((i * 211) % 700) as f64;
        let amount = units as f64 * price;
        net += amount;

        let row = table.row(
            &mut d.shaper,
            &[
                Cell::new(format!("{:03}", i + 1), Pt(8.0)).inked(muted),
                Cell::new(concepts[i % concepts.len()], Pt(8.0)).inked(ink),
                Cell::new(units.to_string(), Pt(8.0)).inked(ink),
                Cell::new(format!("{price:.2} €"), Pt(8.0)).inked(ink),
                Cell::new(format!("{amount:.2} €"), Pt(8.0)).inked(ink),
            ],
            Decoration {
                background: (i % 2 == 1).then(|| hex("#FAFCFF")),
                border: Edges {
                    bottom: Some(hairline),
                    ..Default::default()
                },
                ..Default::default()
            },
            pad,
        );
        d.push(Content::Box(row));
    }
    let groups = vec![Group {
        atoms: table_start..d.atoms.len(),
        repeat_prefix: Some(Repeat {
            atom: table_start,
            height: header_height,
        }),
    }];

    d.gap(16.0);

    // ── Totals panel, right-aligned ──────────────────────────────────────
    let vat = net * 0.07;
    let totals_width = Pt(230.0);
    let totals_layout = Layout::new(
        vec![
            Column::new(Length::Auto),
            Column::new(Length::Pt(Pt(92.0))).aligned(Align::End),
        ],
        totals_width - Pt(20.0),
    );
    let mut totals = BoxContent::new(Decoration {
        background: Some(hex("#F5F8FD")),
        border: Edges::all(Some(hairline)),
        ..Default::default()
    })
    .with_width(totals_width)
    .with_padding(Edges::all(Pt(10.0)));

    for (label, value, size, color) in [
        ("Base imponible", format!("{net:.2} €"), 9.0, ink),
        ("IGIC 7 %", format!("{vat:.2} €"), 9.0, ink),
        ("TOTAL A PAGAR", format!("{:.2} €", net + vat), 12.0, navy),
    ] {
        let heavy = size > 10.0;
        let cell = |t: String| {
            let c = Cell::new(t, Pt(size)).inked(color);
            if heavy { c.bold() } else { c }
        };
        let row = totals_layout.row(
            &mut d.shaper,
            &[cell(label.to_string()), cell(value)],
            Decoration::default(),
            Edges::symmetric(Pt(2.0), Pt(0.0)),
        );
        totals = totals.stack(Content::Box(row));
    }
    d.push(Content::Box(
        BoxContent::default().place(full - totals_width, Content::Box(totals)),
    ));

    d.gap(14.0);

    // ── Footer note with the small mark ──────────────────────────────────
    let mark = ImageContent::scaled_to_width(MARK, ImageFormat::Png, (64, 64), Pt(22.0));
    let note = d.text(
        "Pago por transferencia a ES12 3456 7890 1234 5678 9012 en 30 días. \
         Documento generado por Imprenta; el texto es seleccionable y la fuente va empotrada.",
        7.5,
        muted,
        full - Pt(34.0),
    );
    d.push(Content::Box(
        BoxContent::default()
            .place(Pt(0.0), Content::Image(mark))
            .place(Pt(34.0), note),
    ));

    let pages = pack(
        &Flow::new(&d.atoms).with_groups(&groups),
        geometry.content_height(),
    );
    let fonts = Fonts::from_shaper(&d.shaper).expect("fonts");
    let pdf =
        render_faces(&pages, &d.contents, &fonts, geometry, Options::default()).expect("render");
    std::fs::write(format!("{out}/invoice.pdf"), &pdf).expect("write");

    for (i, page) in pages.iter().enumerate() {
        std::fs::write(
            format!("{out}/invoice-{:02}.pdf", i + 1),
            render_faces(
                std::slice::from_ref(page),
                &d.contents,
                &fonts,
                geometry,
                Options::default(),
            )
            .unwrap(),
        )
        .unwrap();
    }
    println!(
        "{out}/invoice.pdf: {} pages, {:.1} KB",
        pages.len(),
        pdf.len() as f64 / 1024.0
    );
}
