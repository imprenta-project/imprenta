//! What an independent implementation makes of what we wrote.
//!
//! Asserting on our own XML proves only that we are consistent with
//! ourselves. A spreadsheet is worth nothing until Excel opens it, and the
//! nearest thing to Excel that fits in a test is another reader that had no
//! part in writing the file. calamine parses the package the way Excel does —
//! relationships, content types, the lot — so a file it cannot open is a file
//! nobody can.
//!
//! This is the automated form of "look at the PDF".

use calamine::{Data, Reader};
use imprenta_xlsx::ir::{Cell, Column, Freeze, Merge, Row, Sheet, Workbook};
use imprenta_xlsx::style::{Across, Alignment, Font, Points, Style};
use imprenta_xlsx::{serial, write};
use std::io::Cursor;

/// Writes a workbook and reads it back with calamine.
fn round_trip(book: &Workbook) -> calamine::Xlsx<Cursor<Vec<u8>>> {
    let bytes = write(book).expect("a workbook we built ourselves should write");
    calamine::open_workbook_from_rs(Cursor::new(bytes)).expect("calamine should open it")
}

#[test]
fn a_workbook_with_one_cell_opens_and_says_what_is_in_it() {
    let book = Workbook::new(vec![Sheet::new(
        "Hoja",
        vec![Row::new(vec![Cell::text("Hola")])],
    )]);

    let mut read = round_trip(&book);
    let range = read
        .worksheet_range("Hoja")
        .expect("the sheet should be there, under the name we gave it");

    assert_eq!(range.get_value((0, 0)), Some(&Data::String("Hola".into())));
}

#[test]
fn sheets_keep_their_names_and_their_order() {
    // Order is part of what an author declared: a workbook whose tabs shuffle
    // between runs is one nobody can diff.
    let book = Workbook::new(vec![
        Sheet::new("Ventas", vec![Row::new(vec![Cell::text("uno")])]),
        Sheet::new("Resumen", vec![Row::new(vec![Cell::text("dos")])]),
    ]);

    let read = round_trip(&book);

    assert_eq!(read.sheet_names(), &["Ventas", "Resumen"]);
}

#[test]
fn a_short_row_leaves_the_cells_after_it_empty() {
    // Rows are ragged in every real spreadsheet, and a missing cell is absent
    // rather than empty-stringed: Excel tells the two apart and so does COUNTA.
    let book = Workbook::new(vec![Sheet::new(
        "Hoja",
        vec![
            Row::new(vec![Cell::text("a"), Cell::text("b")]),
            Row::new(vec![Cell::text("c")]),
        ],
    )]);

    let mut read = round_trip(&book);
    let range = read.worksheet_range("Hoja").expect("sheet");

    assert_eq!(range.get_value((1, 0)), Some(&Data::String("c".into())));
    assert!(matches!(range.get_value((1, 1)), None | Some(&Data::Empty)));
}

#[test]
fn a_number_comes_back_a_number_and_not_a_string() {
    // The whole difference between this crate and the PDF one. Written as
    // text, 1200 shows the same on screen and makes SUM return zero, which is
    // a broken deliverable rather than a cosmetic defect.
    let book = Workbook::new(vec![Sheet::new(
        "Hoja",
        vec![Row::new(vec![
            Cell::text("1200"),
            Cell::number(1200.0),
            Cell::number(-125.25),
        ])],
    )]);

    let mut read = round_trip(&book);
    let range = read.worksheet_range("Hoja").expect("sheet");

    assert_eq!(range.get_value((0, 0)), Some(&Data::String("1200".into())));
    assert_eq!(range.get_value((0, 1)), Some(&Data::Float(1200.0)));
    assert_eq!(range.get_value((0, 2)), Some(&Data::Float(-125.25)));
}

#[test]
fn a_boolean_comes_back_a_boolean() {
    let book = Workbook::new(vec![Sheet::new(
        "Hoja",
        vec![Row::new(vec![Cell::boolean(true), Cell::boolean(false)])],
    )]);

    let mut read = round_trip(&book);
    let range = read.worksheet_range("Hoja").expect("sheet");

    assert_eq!(range.get_value((0, 0)), Some(&Data::Bool(true)));
    assert_eq!(range.get_value((0, 1)), Some(&Data::Bool(false)));
}

#[test]
fn a_formula_survives_as_a_formula() {
    // Not as its answer: the point of writing a formula rather than a number
    // is that it recalculates when somebody edits the rows above it.
    let book = Workbook::new(vec![Sheet::new(
        "Hoja",
        vec![
            Row::new(vec![Cell::number(10.0)]),
            Row::new(vec![Cell::number(32.0)]),
            Row::new(vec![Cell::formula("SUM(A1:A2)")]),
        ],
    )]);

    let mut read = round_trip(&book);
    let formulas = read
        .worksheet_formula("Hoja")
        .expect("the sheet should have formulas");

    assert_eq!(
        formulas.get_value((2, 0)).map(String::as_str),
        Some("SUM(A1:A2)")
    );
}

#[test]
fn a_cached_answer_is_there_for_readers_that_do_not_calculate() {
    // calamine does not evaluate formulas, and neither does pandas. Without a
    // cached value they see an empty cell where the total should be.
    let book = Workbook::new(vec![Sheet::new(
        "Hoja",
        vec![Row::new(vec![Cell::formula_worth("SUM(A1:A2)", 42.0)])],
    )]);

    let mut read = round_trip(&book);
    let range = read.worksheet_range("Hoja").expect("sheet");

    assert_eq!(range.get_value((0, 0)), Some(&Data::Float(42.0)));
}

#[test]
fn a_date_comes_back_a_date_and_not_five_digits() {
    // The serial alone is not enough: what makes 46237 a date is the number
    // format on the cell, and a reader tells them apart the same way Excel
    // does. If this fails, every date in every export reads as a number.
    let book = Workbook::new(vec![Sheet::new(
        "Hoja",
        vec![Row::new(vec![Cell::date(
            serial::from_ymd(2026, 8, 3).expect("a real date"),
        )])],
    )]);

    let mut read = round_trip(&book);
    let range = read.worksheet_range("Hoja").expect("sheet");

    let value = range.get_value((0, 0)).expect("a cell");
    assert!(
        matches!(value, Data::DateTime(_)),
        "expected a date, got {value:?}"
    );
}

#[test]
fn merged_cells_survive_as_one_block() {
    let book = Workbook::new(vec![Sheet {
        name: "Hoja".into(),
        rows: vec![Row::new(vec![Cell::text("Total")])],
        merges: vec![Merge {
            from_row: 0,
            from_column: 0,
            to_row: 0,
            to_column: 2,
        }],
        ..Sheet::default()
    }]);

    let mut read = round_trip(&book);
    let merges = read
        .worksheet_merge_cells("Hoja")
        .expect("sheet")
        .expect("the merges should parse");

    assert_eq!(merges.len(), 1);
    assert_eq!(merges[0].start, (0, 0));
    assert_eq!(merges[0].end, (0, 2));
}

#[test]
fn a_merge_down_the_page_survives_as_well_as_one_across() {
    // rowSpan was written on the same afternoon as colSpan and only the
    // sideways one had ever been read back. A merge is two-dimensional and the
    // rows and columns are easy to write the wrong way round — which shows up
    // as a block merged the wrong way, not as an error.
    let book = Workbook::new(vec![Sheet {
        name: "Hoja".into(),
        rows: vec![
            Row::new(vec![Cell::text("Trimestre"), Cell::text("Enero")]),
            Row::new(vec![Cell::blank(), Cell::text("Febrero")]),
            Row::new(vec![Cell::blank(), Cell::text("Marzo")]),
        ],
        // Down three rows, one column wide.
        merges: vec![Merge {
            from_row: 0,
            from_column: 0,
            to_row: 2,
            to_column: 0,
        }],
        ..Sheet::default()
    }]);

    let mut read = round_trip(&book);
    let merges = read
        .worksheet_merge_cells("Hoja")
        .expect("sheet")
        .expect("the merges should parse");

    assert_eq!(merges.len(), 1);
    assert_eq!(merges[0].start, (0, 0), "it should begin at the top left");
    assert_eq!(
        merges[0].end,
        (2, 0),
        "and end three rows down, not three across"
    );

    // The cells beside it are untouched, which is the thing a transposed merge
    // would quietly break.
    let range = read.worksheet_range("Hoja").expect("sheet");
    assert_eq!(range.get_value((2, 1)), Some(&Data::String("Marzo".into())));
}

#[test]
fn a_block_merged_in_both_directions_keeps_its_corners() {
    let book = Workbook::new(vec![Sheet {
        name: "Hoja".into(),
        rows: vec![
            Row::new(vec![
                Cell::text("Bloque"),
                Cell::blank(),
                Cell::text("fuera"),
            ]),
            Row::new(vec![
                Cell::blank(),
                Cell::blank(),
                Cell::text("también fuera"),
            ]),
        ],
        merges: vec![Merge {
            from_row: 0,
            from_column: 0,
            to_row: 1,
            to_column: 1,
        }],
        ..Sheet::default()
    }]);

    let mut read = round_trip(&book);
    let merges = read
        .worksheet_merge_cells("Hoja")
        .expect("sheet")
        .expect("merges");

    assert_eq!(merges[0].start, (0, 0));
    assert_eq!(merges[0].end, (1, 1));

    let range = read.worksheet_range("Hoja").expect("sheet");
    assert_eq!(
        range.get_value((1, 2)),
        Some(&Data::String("también fuera".into()))
    );
}

#[test]
fn a_styled_workbook_still_opens_and_keeps_its_values() {
    // Formatting must never cost a value. A style index pointing at an entry
    // that is not there is the classic way to make a workbook unopenable, and
    // the failure looks nothing like a styling bug.
    let heading = Style {
        font: Font {
            bold: true,
            size: Some(Points(14.0)),
            ..Font::default()
        },
        align: Alignment {
            horizontal: Some(Across::Center),
            ..Alignment::default()
        },
        ..Style::default()
    };

    let book = Workbook::new(vec![Sheet {
        name: "Hoja".into(),
        columns: vec![Column {
            width: Some(24.0),
            style: None,
        }],
        rows: vec![
            Row::new(vec![Cell::text("Concepto"), Cell::text("Importe")]).styled(heading),
            Row::new(vec![Cell::text("Licencia"), Cell::number(1200.0)]),
        ],
        freeze: Some(Freeze {
            rows: 1,
            columns: 0,
        }),
        ..Sheet::default()
    }]);

    let mut read = round_trip(&book);
    let range = read.worksheet_range("Hoja").expect("sheet");

    assert_eq!(
        range.get_value((0, 0)),
        Some(&Data::String("Concepto".into()))
    );
    assert_eq!(range.get_value((1, 1)), Some(&Data::Float(1200.0)));
}

#[test]
fn text_that_would_break_the_xml_survives_it() {
    // A concept line with an ampersand in it is not an edge case, it is
    // Tuesday. Getting this wrong produces a file Excel refuses to open at
    // all, with a repair dialog that names nothing.
    let awkward = r#"Ampersand & <tag> "quoted" 'single' ]]>"#;
    let book = Workbook::new(vec![Sheet::new(
        "Hoja",
        vec![Row::new(vec![Cell::text(awkward)])],
    )]);

    let mut read = round_trip(&book);
    let range = read.worksheet_range("Hoja").expect("sheet");

    assert_eq!(
        range.get_value((0, 0)),
        Some(&Data::String(awkward.to_string()))
    );
}
