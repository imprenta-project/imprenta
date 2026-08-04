//! The OOXML package: the zip, and the several small XML parts inside it.
//!
//! An `.xlsx` is a zip of related XML documents, and Excel is strict about the
//! relationships between them in a way it is not about their contents. A part
//! that no `.rels` file points at is a part Excel does not read; a part missing
//! from `[Content_Types].xml` makes the whole workbook unopenable, behind a
//! repair dialog that names nothing.
//!
//! Everything except the worksheets is small, fixed, and written whole. The
//! worksheets stream, because they are the only part whose size depends on the
//! data.

use std::fs::File;
use std::io::{BufWriter, Cursor, Seek, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ir::{Sheet, Workbook};
use crate::session::Session;
use crate::xml::escaped;

/// Everything that can go wrong on the way to a file.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the workbook could not be written: {0}")]
    Io(#[from] std::io::Error),

    #[error("the workbook package could not be assembled: {0}")]
    Package(#[from] zip::result::ZipError),

    #[error("a workbook needs at least one sheet, and Excel will not open one without")]
    Empty,

    #[error("all {declared} declared sheets have been written")]
    NoMoreSheets { declared: usize },
}

/// Writes a workbook and hands back the bytes.
pub fn write(book: &Workbook) -> Result<Vec<u8>, Error> {
    let mut buffer = Cursor::new(Vec::new());
    write_into(book, &mut buffer)?;
    Ok(buffer.into_inner())
}

/// Writes a workbook straight to a file.
///
/// Preferred for anything large, for the same reason `renderToFile` is on the
/// PDF side: a hundred-megabyte export should never exist as a hundred
/// megabytes in memory on its way to disk.
pub fn write_to_file(book: &Workbook, path: impl AsRef<Path>) -> Result<u64, Error> {
    let file = File::create(path)?;
    let mut out = BufWriter::new(file);
    write_into(book, &mut out)?;
    let file = out.into_inner().map_err(|e| Error::Io(e.into_error()))?;
    Ok(file.metadata()?.len())
}

/// Declaring a whole workbook is feeding a session everything at once.
///
/// Written this way rather than as a second implementation so the two cannot
/// drift: a file produced in one call and the same file produced in batches
/// are the same bytes because they are the same code. The test that pins it
/// is still worth having, as a guard against somebody separating them again.
fn write_into<W: Write + Seek>(book: &Workbook, out: W) -> Result<(), Error> {
    if book.sheets.is_empty() {
        return Err(Error::Empty);
    }

    let mut session = Session::open(out, book.sheets.clone())?;
    for _ in 1..book.sheets.len() {
        session.next_sheet()?;
    }
    session.finish()?;
    Ok(())
}

/// How every part of the package is compressed and stamped.
///
/// Deflate rather than stored: spreadsheet XML is extremely repetitive and
/// compresses to a fraction of itself, and every reader supports it.
///
/// The timestamp is fixed rather than "now". Determinism is a design
/// commitment — the same input must produce the same bytes, so that a build
/// can be diffed and cached — and a zip records a modification time per entry,
/// which would otherwise make every run differ.
pub(crate) fn options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
}

/// Writes one whole part of the package.
pub(crate) fn part<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    options: SimpleFileOptions,
    name: &str,
    body: &str,
) -> Result<(), Error> {
    zip.start_file(name.to_string(), options)?;
    zip.write_all(body.as_bytes())?;
    Ok(())
}

const DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

/// Which part is what. A part missing from here is a workbook that will not open.
pub(crate) fn content_types(sheets: usize) -> String {
    let mut xml = String::from(DECLARATION);
    xml.push_str(r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#);
    xml.push_str(
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
    );
    xml.push_str(r#"<Default Extension="xml" ContentType="application/xml"/>"#);
    xml.push_str(
        r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
    );
    xml.push_str(
        r#"<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#,
    );
    for index in 1..=sheets {
        xml.push_str(&format!(
            r#"<Override PartName="/xl/worksheets/sheet{index}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
        ));
    }
    xml.push_str("</Types>");
    xml
}

pub(crate) fn root_rels() -> &'static str {
    ROOT_RELS
}

const ROOT_RELS: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>"#,
    "</Relationships>"
);

pub(crate) fn workbook_xml(sheets: &[Sheet]) -> String {
    let mut xml = String::from(DECLARATION);
    xml.push_str(
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
    );
    xml.push_str("<sheets>");
    for (index, sheet) in sheets.iter().enumerate() {
        // The tab order is the order the author declared, and the id is the
        // position: a workbook whose tabs shuffle between runs is one nobody
        // can diff.
        let n = index + 1;
        xml.push_str(&format!(
            r#"<sheet name="{}" sheetId="{n}" r:id="rId{n}"/>"#,
            escaped(&sheet.name)
        ));
    }
    xml.push_str("</sheets>");
    // A formula written without a cached value has no answer in the file, and
    // whether Excel works one out on open is otherwise up to Excel.
    xml.push_str(r#"<calcPr calcId="0" fullCalcOnLoad="1"/>"#);
    xml.push_str("</workbook>");
    xml
}

pub(crate) fn workbook_rels(sheets: usize) -> String {
    let mut xml = String::from(DECLARATION);
    xml.push_str(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for index in 1..=sheets {
        xml.push_str(&format!(
            r#"<Relationship Id="rId{index}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{index}.xml"/>"#
        ));
    }
    // Numbered after the sheets so the sheet ids stay 1..n and match sheetId.
    xml.push_str(&format!(
        r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
        sheets + 1
    ));
    xml.push_str("</Relationships>");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Cell, Row, Sheet};

    fn one_sheet() -> Workbook {
        Workbook::new(vec![Sheet::new(
            "Hoja",
            vec![Row::new(vec![Cell::text("Hola")])],
        )])
    }

    #[test]
    fn every_part_is_declared_in_the_content_types() {
        let xml = content_types(2);
        assert!(xml.contains("/xl/workbook.xml"));
        assert!(xml.contains("/xl/styles.xml"));
        assert!(xml.contains("/xl/worksheets/sheet1.xml"));
        assert!(xml.contains("/xl/worksheets/sheet2.xml"));
    }

    #[test]
    fn a_sheet_name_is_escaped_before_it_reaches_an_attribute() {
        // "Ventas & Compras" is an ordinary thing to call a tab, and an
        // unescaped ampersand here breaks the whole workbook rather than one
        // cell.
        let book = Workbook::new(vec![Sheet::new("Ventas & Compras", vec![])]);
        assert!(workbook_xml(&book.sheets).contains("Ventas &amp; Compras"));
    }

    #[test]
    fn the_workbook_asks_to_be_recalculated_when_it_is_opened() {
        // A formula we wrote with no cached value has no answer in the file.
        // Excel usually works one out anyway, and "usually" is how a total
        // comes to show 0 on somebody else's machine. Saying so costs one
        // attribute and removes the question.
        let book = Workbook::new(vec![Sheet::new("Hoja", vec![])]);
        assert!(workbook_xml(&book.sheets).contains(r#"fullCalcOnLoad="1""#));
    }

    #[test]
    fn the_styles_relationship_is_numbered_after_the_sheets() {
        // Sheet ids have to stay 1..n to match sheetId in workbook.xml; the
        // styles part takes the next number rather than rId1.
        let xml = workbook_rels(3);
        assert!(xml.contains(r#"Id="rId4""#));
        assert!(xml.contains("styles.xml"));
    }

    #[test]
    fn the_style_table_names_the_normal_style_it_indexes_from() {
        // Found by openpyxl, which warns "Workbook contains no default style"
        // and substitutes its own. Excel is quieter about it and then applies
        // whatever it feels like, which is worse: the file looks right in one
        // reader and wrong in another. Every xf points at xfId 0, so xfId 0
        // has to be a style the file actually declares.
        let xml = crate::style::Styles::new().to_xml();
        assert!(xml.contains(r#"<cellStyle name="Normal" xfId="0" builtinId="0"/>"#));
    }

    #[test]
    fn the_same_workbook_writes_the_same_bytes_twice() {
        // Deterministic output is a design commitment, and a zip records a
        // modification time per entry. Left at "now", two runs a second apart
        // would differ and nothing downstream could be cached or diffed.
        let book = one_sheet();
        assert_eq!(write(&book).unwrap(), write(&book).unwrap());
    }

    #[test]
    fn the_package_holds_the_parts_excel_looks_for() {
        let bytes = write(&one_sheet()).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("a zip we just wrote");
        let names: Vec<_> = zip.file_names().map(str::to_string).collect();

        for part in [
            "[Content_Types].xml",
            "_rels/.rels",
            "xl/workbook.xml",
            "xl/_rels/workbook.xml.rels",
            "xl/styles.xml",
            "xl/worksheets/sheet1.xml",
        ] {
            assert!(names.contains(&part.to_string()), "{part} is missing");
        }
        assert!(zip.by_name("xl/worksheets/sheet1.xml").is_ok());
    }
}
