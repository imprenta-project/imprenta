//! A real spreadsheet, written to `preview/ventas.xlsx`.
//!
//! The counterpart of the PDF crate's examples: a test tells you the bytes are
//! what you expected, and opening the file tells you whether anyone can use
//! it. Run it and open the result.

use imprenta_core::color::Color;
use imprenta_core::units::Edges;
use imprenta_xlsx::ir::{Cell, Column, Freeze, Merge, Row, Sheet, Workbook};
use imprenta_xlsx::style::{Across, Alignment, Border, Font, Line, Points, Style};
use imprenta_xlsx::{serial, write_to_file};

const SLATE: Color = Color {
    r: 241,
    g: 245,
    b: 249,
    a: 255,
};
const INK: Color = Color {
    r: 27,
    g: 58,
    b: 92,
    a: 255,
};
const RED: Color = Color {
    r: 220,
    g: 38,
    b: 38,
    a: 255,
};

fn money(negative: bool) -> Style {
    Style {
        format: Some("#,##0.00 €".into()),
        font: Font {
            color: negative.then_some(RED),
            ..Font::default()
        },
        ..Style::default()
    }
}

fn main() {
    let heading = Style {
        font: Font {
            bold: true,
            color: Some(INK),
            size: Some(Points(11.0)),
            ..Font::default()
        },
        fill: Some(SLATE),
        align: Alignment {
            horizontal: Some(Across::Center),
            ..Alignment::default()
        },
        ..Style::default()
    };

    let mut rows = vec![
        Row::new(vec![
            Cell::text("Ref."),
            Cell::text("Concepto"),
            Cell::text("Fecha"),
            Cell::text("Pagado"),
            Cell::text("Importe"),
        ])
        .styled(heading),
    ];

    let lines = [
        ("Licencia anual", (2026, 1, 15), true, 1200.0),
        ("Soporte & mantenimiento", (2026, 3, 1), true, 350.5),
        ("Formación <in situ>", (2026, 6, 30), false, 900.0),
        ("Descuento", (2026, 7, 4), false, -125.25),
    ];

    for (n, (concept, (y, m, d), paid, amount)) in lines.into_iter().enumerate() {
        rows.push(Row::new(vec![
            Cell::text(format!("{:03}", n + 1)),
            Cell::text(concept),
            Cell::date(serial::from_ymd(y, m, d).expect("a real date")),
            Cell::boolean(paid),
            Cell {
                value: imprenta_xlsx::ir::Value::Number(amount),
                style: Some(Box::new(money(amount < 0.0))),
            },
        ]));
    }

    let total: f64 = lines.iter().map(|line| line.3).sum();
    let total_style = Style {
        font: Font {
            bold: true,
            ..Font::default()
        },
        border: Edges {
            top: Some(Border {
                style: Line::Thin,
                color: None,
            }),
            ..Edges::default()
        },
        format: Some("#,##0.00 €".into()),
        ..Style::default()
    };
    rows.push(Row::new(vec![
        Cell::text("Total"),
        Cell::blank(),
        Cell::blank(),
        Cell::blank(),
        // Both a formula and its answer: Excel recalculates it, and anything
        // that only reads still sees the number.
        Cell {
            value: imprenta_xlsx::ir::Value::formula_worth(
                format!("SUM(E2:E{})", lines.len() + 1),
                total,
            ),
            style: Some(Box::new(total_style)),
        },
    ]));

    let ventas = Sheet {
        name: "Ventas".into(),
        columns: vec![
            Column {
                width: Some(8.0),
                style: None,
            },
            Column {
                width: Some(30.0),
                style: None,
            },
            Column {
                width: Some(14.0),
                style: None,
            },
            Column {
                width: Some(10.0),
                style: None,
            },
            Column {
                width: Some(16.0),
                style: None,
            },
        ],
        rows,
        // "Total" spans the four columns before the number.
        merges: vec![Merge {
            from_row: (lines.len() + 1) as u32,
            from_column: 0,
            to_row: (lines.len() + 1) as u32,
            to_column: 3,
        }],
        freeze: Some(Freeze {
            rows: 1,
            columns: 0,
        }),
    };

    let book = Workbook::new(vec![
        ventas,
        Sheet::new(
            "Notas",
            vec![Row::new(vec![Cell::text(
                "Una hoja puede estar casi vacía.",
            )])],
        ),
    ]);

    std::fs::create_dir_all("preview").expect("preview/ should be creatable");
    let bytes = write_to_file(&book, "preview/ventas.xlsx").expect("it should write");
    println!("preview/ventas.xlsx — {bytes} bytes");
}
