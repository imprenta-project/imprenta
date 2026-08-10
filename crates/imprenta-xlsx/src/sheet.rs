//! One worksheet, as XML.
//!
//! Written straight into the zip entry rather than built up as a string.
//! Excel allows just over a million rows a sheet, and at ten columns each
//! that is ten million cells; the finished XML for one large sheet is
//! comfortably larger than the finished file, and holding it would put back
//! the memory the streaming design exists to avoid.
//!
//! So the unit of buffering is **one row**, in a `String` that is cleared and
//! reused. Nothing here allocates per cell.

use crate::ir::{Merge, Row, Sheet, Value};
use crate::style::{Style, Styles};
use crate::xml;

/// A date with no format of its own shows as 46237, which is a date to nobody.
///
/// ISO rather than Excel's built-in 14, which is `mm-dd-yy` and means one
/// thing in Boston and another everywhere else. An author who wants the
/// reader's local format can ask for it by name.
const DEFAULT_DATE: &str = "yyyy-mm-dd";
const DEFAULT_DATE_TIME: &str = "yyyy-mm-dd hh:mm:ss";

/// The frozen panes, if any.
pub(crate) fn views(sheet: &Sheet) -> String {
    let Some(freeze) = sheet.freeze.filter(|f| f.rows > 0 || f.columns > 0) else {
        return String::new();
    };

    // `topLeftCell` is the first cell that scrolls, which is the one just past
    // the frozen block in both directions.
    let mut corner = String::new();
    xml::cell_ref(freeze.rows, freeze.columns, &mut corner);

    // Which pane the cursor lands in. Wrong here and the sheet opens scrolled
    // into the frozen rows, which looks like the freeze did not take.
    let active = match (freeze.rows > 0, freeze.columns > 0) {
        (true, true) => "bottomRight",
        (true, false) => "bottomLeft",
        (false, true) => "topRight",
        (false, false) => unreachable!("filtered above"),
    };

    let mut xml = String::from(r#"<sheetViews><sheetView workbookViewId="0"><pane"#);
    if freeze.columns > 0 {
        xml.push_str(&format!(r#" xSplit="{}""#, freeze.columns));
    }
    if freeze.rows > 0 {
        xml.push_str(&format!(r#" ySplit="{}""#, freeze.rows));
    }
    xml.push_str(&format!(
        r#" topLeftCell="{corner}" activePane="{active}" state="frozen"/></sheetView></sheetViews>"#
    ));
    xml
}

/// Column widths and defaults.
pub(crate) fn columns(sheet: &Sheet, styles: &mut Styles) -> String {
    let described: Vec<_> = sheet
        .columns
        .iter()
        .enumerate()
        .filter(|(_, column)| column.width.is_some() || column.style.is_some())
        .collect();
    if described.is_empty() {
        return String::new();
    }

    let mut xml = String::from("<cols>");
    for (at, column) in described {
        // `min` and `max` are one-based and inclusive, and a `<col>` describes
        // a range. One column at a time is more XML than strictly needed and
        // exactly matches what the author declared.
        let n = at + 1;
        xml.push_str(&format!(r#"<col min="{n}" max="{n}""#));
        if let Some(width) = column.width {
            xml.push_str(&format!(r#" width="{width}" customWidth="1""#));
        }
        if let Some(style) = &column.style {
            xml.push_str(&format!(r#" style="{}""#, styles.intern(style)));
        }
        xml.push_str("/>");
    }
    xml.push_str("</cols>");
    xml
}

pub(crate) fn merges(list: &[Merge]) -> String {
    if list.is_empty() {
        return String::new();
    }
    let mut xml = format!(r#"<mergeCells count="{}">"#, list.len());
    for merge in list {
        let mut range = String::new();
        xml::cell_ref(merge.from_row, merge.from_column, &mut range);
        range.push(':');
        xml::cell_ref(merge.to_row, merge.to_column, &mut range);
        xml.push_str(&format!(r#"<mergeCell ref="{range}"/>"#));
    }
    xml.push_str("</mergeCells>");
    xml
}

/// The autofilter for a sheet, if a row asked to be its labels.
///
/// `header` is the row the labels are on, `columns` how many of them there
/// are, and `rows` how many rows the sheet ended up with — which is why this
/// is written when the sheet closes and not when the row goes past.
///
/// The range starts at the labels and ends at the last row: Excel reads the
/// first row of an autofilter's range as the header and everything under it as
/// what there is to filter, so leaving the labels out would filter by the
/// first row of data.
pub(crate) fn auto_filter(header: Option<u32>, columns: u32, rows: u32) -> String {
    let Some(header) = header else {
        return String::new();
    };
    if columns == 0 {
        return String::new();
    }

    let mut range = String::new();
    xml::cell_ref(header, 0, &mut range);
    range.push(':');
    xml::cell_ref(rows.saturating_sub(1).max(header), columns - 1, &mut range);

    format!(r#"<autoFilter ref="{range}"/>"#)
}

/// Appends one row's XML to `buf`.
pub fn write_row(row: &Row, index: u32, sheet: &Sheet, styles: &mut Styles, buf: &mut String) {
    // A row with nothing in it still occupies a row: leaving it out would
    // shift everything below it up by one, which is the sort of defect that
    // reaches a spreadsheet and not a test.
    buf.push_str("<row r=\"");
    buf.push_str(&(index + 1).to_string());
    buf.push('"');

    if let Some(height) = row.height {
        buf.push_str(&format!(r#" ht="{height}" customHeight="1""#));
    }
    if let Some(style) = &row.style {
        // On the row itself as well as on its cells, so that the format
        // reaches the cells the row does not have. A header band that stops
        // where the last cell stops is the giveaway that it was not done.
        buf.push_str(&format!(
            r#" s="{}" customFormat="1""#,
            styles.intern(style)
        ));
    }
    buf.push('>');

    for (column, cell) in row.cells.iter().enumerate() {
        let style = cell
            .style
            .as_deref()
            .or(row.style.as_ref())
            .or_else(|| sheet.columns.get(column).and_then(|c| c.style.as_ref()));
        write_cell(&cell.value, index, column as u32, style, styles, buf);
    }

    buf.push_str("</row>");
}

fn write_cell(
    value: &Value,
    row: u32,
    column: u32,
    style: Option<&Style>,
    styles: &mut Styles,
    buf: &mut String,
) {
    let format = effective(value, style, styles);

    // A blank cell carrying no format either is left out altogether. Every
    // cell writes its own reference, so an absent one shifts nothing — and
    // absent is what Excel and COUNTA both understand by empty. A blank cell
    // that *is* formatted stays, because a filled band across an empty column
    // is a thing people ask for.
    if matches!(value, Value::Blank) {
        if format != 0 {
            buf.push_str("<c r=\"");
            xml::cell_ref(row, column, buf);
            buf.push_str(&format!("\" s=\"{format}\"/>"));
        }
        return;
    }

    buf.push_str("<c r=\"");
    xml::cell_ref(row, column, buf);
    if format != 0 {
        buf.push_str(&format!("\" s=\"{format}"));
    }

    match value {
        Value::Blank => unreachable!("returned above"),

        // `inlineStr` rather than an entry in the shared string table. The
        // table is smaller for repetitive text, and it cannot be written
        // until every string in the workbook is known — which is exactly what
        // a streamed workbook never knows. One way of writing text, so that a
        // streamed file and a declared one come out the same.
        Value::Text(text) => {
            buf.push_str("\" t=\"inlineStr\"><is><t");
            // Only when it matters: XML collapses leading and trailing space
            // unless told not to, and saying so on every cell in a million-row
            // sheet is twenty megabytes of nothing.
            if text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace) {
                buf.push_str(" xml:space=\"preserve\"");
            }
            buf.push('>');
            xml::escape(text, buf);
            buf.push_str("</t></is></c>");
        }

        // No `t` attribute: a cell with no type is a number, which is both the
        // specification's default and the commonest cell in any real sheet.
        // A date is the same thing — the serial underneath — and differs only
        // in the number format it will be given.
        Value::Number(number) | Value::Date(number) => {
            buf.push_str("\"><v>");
            buf.push_str(&format_number(*number));
            buf.push_str("</v></c>");
        }

        Value::Bool(yes) => {
            buf.push_str("\" t=\"b\"><v>");
            buf.push(if *yes { '1' } else { '0' });
            buf.push_str("</v></c>");
        }

        Value::Formula(formula) => {
            buf.push_str("\"><f>");
            // The `=` is how a person types a formula, not part of one. A
            // producer supplies it about half the time; keeping it would
            // write `==SUM(...)` and Excel would refuse the file.
            xml::escape(formula.formula.trim_start_matches('='), buf);
            buf.push_str("</f>");
            if let Some(cached) = formula.cached {
                buf.push_str("<v>");
                buf.push_str(&format_number(cached));
                buf.push_str("</v>");
            }
            buf.push_str("</c>");
        }
    }
}

/// The `cellXfs` index a cell should carry.
///
/// A date is the one value that formats itself. There is no date type
/// underneath — 46237 is a number — so a date the author said nothing else
/// about has to be given a date format here or it reaches the reader as five
/// digits. Anything the author *did* ask for is left exactly alone.
fn effective(value: &Value, style: Option<&Style>, styles: &mut Styles) -> u32 {
    let Value::Date(serial) = value else {
        return style.map(|s| styles.intern(s)).unwrap_or(0);
    };

    let mut dated = style.cloned().unwrap_or_default();
    if dated.format.is_none() {
        // A serial with something after the point carries a time of day, and
        // showing 09:30 as a bare date loses it silently.
        dated.format = Some(
            if serial.fract() == 0.0 {
                DEFAULT_DATE
            } else {
                DEFAULT_DATE_TIME
            }
            .to_string(),
        );
    }
    styles.intern(&dated)
}

/// A number as Excel stores it.
///
/// Rust prints `1200_f64` as `1200`, which is what we want, and `0.1 + 0.2` as
/// `0.30000000000000004`, which is also what we want: the file must hold the
/// value the producer computed, not a rounding of it. Formatting for display
/// is a number format, and Excel applies it when the file is opened.
fn format_number(number: f64) -> String {
    // Neither infinity nor NaN has a representation in the file format. Excel
    // spells them as error values, which is a truthful thing to do and better
    // than writing `inf` and producing a file that will not open.
    if number.is_nan() || number.is_infinite() {
        return "0".to_string();
    }
    number.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Cell;

    fn row_xml(cells: Vec<Cell>) -> String {
        let mut buf = String::new();
        write_row(
            &Row::new(cells),
            0,
            &Sheet::default(),
            &mut Styles::new(),
            &mut buf,
        );
        buf
    }

    /// One cell's XML, with no style on it and nowhere for one to come from.
    fn cell_xml(value: &Value) -> String {
        let mut buf = String::new();
        write_cell(value, 0, 0, None, &mut Styles::new(), &mut buf);
        buf
    }

    #[test]
    fn a_row_of_text_names_every_cell_it_writes() {
        assert_eq!(
            row_xml(vec![Cell::text("a"), Cell::text("b")]),
            "<row r=\"1\">\
             <c r=\"A1\" t=\"inlineStr\"><is><t>a</t></is></c>\
             <c r=\"B1\" t=\"inlineStr\"><is><t>b</t></is></c>\
             </row>"
        );
    }

    #[test]
    fn a_blank_cell_is_left_out_and_shifts_nothing() {
        // The cell after the gap must still land in C, which is why every
        // cell carries its own reference rather than relying on position.
        let xml = row_xml(vec![Cell::text("a"), Cell::blank(), Cell::text("c")]);

        assert!(!xml.contains("B1"));
        assert!(xml.contains("<c r=\"C1\""));
    }

    #[test]
    fn an_empty_row_still_occupies_a_row() {
        assert_eq!(row_xml(vec![]), "<row r=\"1\"></row>");
    }

    #[test]
    fn a_number_is_written_with_no_type_attribute() {
        let buf = cell_xml(&Value::Number(1200.0));
        assert_eq!(buf, "<c r=\"A1\"><v>1200</v></c>");
    }

    #[test]
    fn a_number_keeps_every_digit_the_producer_computed() {
        // Rounding here would be the writer quietly disagreeing with the
        // total the caller already showed somewhere else.
        let buf = cell_xml(&Value::Number(0.1 + 0.2));
        assert!(buf.contains("0.30000000000000004"), "{buf}");
    }

    #[test]
    fn text_with_edge_whitespace_says_to_keep_it() {
        let buf = cell_xml(&Value::Text("  sangrado".into()));
        assert!(buf.contains("xml:space=\"preserve\""), "{buf}");
    }

    #[test]
    fn text_without_edge_whitespace_does_not_pay_for_the_attribute() {
        let buf = cell_xml(&Value::Text("normal".into()));
        assert!(!buf.contains("xml:space"), "{buf}");
    }

    #[test]
    fn a_boolean_says_so_and_is_written_as_one_or_zero() {
        assert_eq!(
            cell_xml(&Value::Bool(true)),
            "<c r=\"A1\" t=\"b\"><v>1</v></c>"
        );
        assert_eq!(
            cell_xml(&Value::Bool(false)),
            "<c r=\"A1\" t=\"b\"><v>0</v></c>"
        );
    }

    #[test]
    fn a_date_is_written_as_the_number_it_is_and_given_a_format() {
        // There is no date type underneath: the value written is the serial.
        // What makes 46237 look like a date is the number format, so a date
        // nobody formatted has to be given one here — otherwise it reaches
        // the reader as five digits, which is a date to nobody.
        let mut styles = Styles::new();
        let mut buf = String::new();
        write_cell(&Value::Date(46_237.0), 0, 0, None, &mut styles, &mut buf);

        assert!(buf.contains("<v>46237</v>"), "{buf}");
        assert!(buf.contains("s=\""), "a date must carry a format: {buf}");
        assert!(
            styles.to_xml().contains("yyyy-mm-dd"),
            "and the format is a date one"
        );
    }

    #[test]
    fn a_date_with_a_time_in_it_gets_a_format_that_shows_the_time() {
        // A serial with something after the point carries a time of day, and
        // showing 09:30 as a bare date loses it without saying so.
        let mut styles = Styles::new();
        let mut buf = String::new();
        write_cell(&Value::Date(46_237.5), 0, 0, None, &mut styles, &mut buf);

        assert!(styles.to_xml().contains("yyyy-mm-dd hh:mm:ss"), "{buf}");
    }

    #[test]
    fn a_date_the_author_formatted_is_left_alone() {
        // The default is a fallback, not a policy: asking for a month and a
        // year has to survive.
        let mut styles = Styles::new();
        let mut buf = String::new();
        let asked = Style {
            format: Some("mmm-yy".into()),
            ..Style::default()
        };
        write_cell(
            &Value::Date(46_237.0),
            0,
            0,
            Some(&asked),
            &mut styles,
            &mut buf,
        );

        assert!(!styles.to_xml().contains("yyyy-mm-dd"));
    }

    #[test]
    fn a_formula_is_written_without_the_equals_sign() {
        // The `=` is how a person types a formula and is not part of one. A
        // producer will supply it about half the time, so accept both rather
        // than writing `==SUM(...)` into the file.
        for written in ["SUM(C2:C99)", "=SUM(C2:C99)"] {
            let buf = cell_xml(&Value::formula(written));
            assert_eq!(buf, "<c r=\"A1\"><f>SUM(C2:C99)</f></c>", "{written}");
        }
    }

    #[test]
    fn a_formula_carries_its_answer_when_the_producer_knows_it() {
        // Without a cached value the cell is blank until something calculates
        // it. Excel does on open; pandas and every other plain reader does
        // not, and gets nothing. A producer that already has the total should
        // be able to say so.
        let buf = cell_xml(&Value::formula_worth("SUM(C2:C99)", 2325.25));
        assert_eq!(buf, "<c r=\"A1\"><f>SUM(C2:C99)</f><v>2325.25</v></c>");
    }

    #[test]
    fn a_formula_is_escaped_like_anything_else() {
        // `<` is a comparison operator, and it is not rare: IF(A1<0, …).
        let buf = cell_xml(&Value::formula("IF(A1<0,\"neg\",\"pos\")"));
        assert!(
            buf.contains("IF(A1&lt;0,&quot;neg&quot;,&quot;pos&quot;)"),
            "{buf}"
        );
    }

    #[test]
    fn a_number_that_is_not_a_number_does_not_produce_an_unopenable_file() {
        // `inf` and `nan` are not in the format. Writing them makes Excel
        // refuse the whole workbook, which is a bad trade for one bad cell.
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let buf = cell_xml(&Value::Number(bad));
            assert_eq!(buf, "<c r=\"A1\"><v>0</v></c>");
        }
    }
}

#[cfg(test)]
mod filter_tests {
    use super::*;
    use crate::ir::{Cell, Column, Freeze};

    fn labels() -> Row {
        Row {
            filter: true,
            ..Row::new(vec![
                Cell::text("Fecha"),
                Cell::text("Concepto"),
                Cell::text("Importe"),
            ])
        }
    }

    #[test]
    fn a_filter_covers_its_own_row_and_everything_under_it() {
        // The range is the whole table, header included: Excel reads the first
        // row of it as the labels and the rest as what there is to filter.
        // Three columns and two rows of data under a header on row 2.
        let xml = auto_filter(Some(1), 3, 4);

        assert_eq!(xml, r#"<autoFilter ref="A2:C4"/>"#);
    }

    #[test]
    fn a_filter_on_a_table_with_nothing_in_it_covers_the_labels() {
        // A filter on an empty export is not an error: the columns are there
        // and Excel opens the dropdowns on them. `A2:C2` is what Excel itself
        // writes for a header with no rows under it.
        assert_eq!(auto_filter(Some(1), 3, 2), r#"<autoFilter ref="A2:C2"/>"#);
    }

    #[test]
    fn a_sheet_nobody_asked_to_filter_gets_nothing() {
        // The whole feature has to cost nothing to everyone not using it, and
        // an empty element is not nothing: Excel shows the dropdowns for it.
        assert_eq!(auto_filter(None, 3, 40), "");
    }

    #[test]
    fn a_filter_reaches_the_last_row_that_was_written() {
        // Which row is last is only known when the sheet closes, and for a
        // streamed sheet that is the one thing the author cannot say — which
        // is the reason the range is worked out here and not declared.
        assert_eq!(
            auto_filter(Some(0), 2, 200_000),
            r#"<autoFilter ref="A1:B200000"/>"#
        );
    }

    #[test]
    fn the_filter_is_written_before_the_merges() {
        // The schema fixes the order of a worksheet's children and puts
        // `autoFilter` before `mergeCells`. Out of place it is well-formed XML,
        // invalid OOXML, and a repair dialog that names nothing.
        let sheet = Sheet {
            name: "Hoja".into(),
            columns: vec![Column::default()],
            rows: vec![labels(), Row::new(vec![Cell::text("uno")])],
            merges: vec![crate::ir::Merge {
                from_row: 0,
                from_column: 0,
                to_row: 0,
                to_column: 1,
            }],
            freeze: Some(Freeze {
                rows: 1,
                columns: 0,
            }),
            ..Sheet::default()
        };
        let bytes = crate::write(&crate::ir::Workbook::new(vec![sheet]), &[]).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(),
            &mut xml,
        )
        .unwrap();

        let filter = xml.find("<autoFilter").expect("the sheet filters");
        assert!(filter > xml.find("</sheetData>").unwrap(), "{xml}");
        assert!(filter < xml.find("<mergeCells").unwrap(), "{xml}");
    }

    #[test]
    fn two_rows_asking_to_be_the_filter_is_refused() {
        // Excel has one autofilter a sheet. Two marked rows is somebody
        // copying a header block, and the alternative to saying so is a file
        // where the second one silently wins.
        let sheet = Sheet {
            name: "Hoja".into(),
            rows: vec![labels(), labels()],
            ..Sheet::default()
        };
        let why = crate::write(&crate::ir::Workbook::new(vec![sheet]), &[])
            .expect_err("two filters on one sheet");

        assert!(matches!(why, crate::Error::TwoFilters { .. }), "{why:?}");
    }
}
