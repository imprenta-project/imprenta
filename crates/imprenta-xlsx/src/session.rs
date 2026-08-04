//! Feeding a workbook in pieces instead of declaring it whole.
//!
//! [`crate::write`] takes a workbook that already exists in memory. For an
//! export of half a million rows that declaration is the largest thing in the
//! process — larger than the file it becomes — and it exists only to be read
//! once, in order, and thrown away.
//!
//! A session reads it in order without it ever existing. Rows arrive in
//! batches, are turned into XML and released, and nothing is held but the
//! style table and the name of each sheet.
//!
//! # What has to be known up front
//!
//! The sheets: their names, their columns, whether they are frozen. Not their
//! rows. That is not a limitation grudgingly accepted but the same shape the
//! PDF side has, and for the same reason — `[Content_Types].xml` and
//! `xl/workbook.xml` name every sheet in the workbook, and the specification
//! wants the content types first in the package. A producer streaming a
//! million rows always knows which sheets it is filling; it is the rows it
//! does not have yet.
//!
//! # Why the output is identical either way
//!
//! Text is written as an inline string rather than into the shared string
//! table. The table is smaller for repetitive text — and it cannot be written
//! until every string in the workbook is known, which is exactly what a
//! streamed workbook never knows. Rather than have two ways of writing text
//! and two files that differ by how they were produced, there is one, and
//! [`crate::write`] is itself a session fed everything at once.

use std::io::{Seek, Write};

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::ir::{Merge, Row, Sheet};
use crate::package::{Error, content_types, options, part, root_rels, workbook_rels, workbook_xml};
use crate::sheet::{columns, merges, views, write_row};
use crate::style::Styles;

/// A workbook being written a batch of rows at a time.
pub struct Session<W: Write + Seek> {
    zip: ZipWriter<W>,
    options: SimpleFileOptions,
    styles: Styles,
    /// The sheets as declared: names, columns, frozen panes. Their `rows` were
    /// written when the sheet opened, and everything since has been streamed.
    sheets: Vec<Sheet>,
    /// Which sheet is open, and how far down it we are.
    at: usize,
    row: u32,
    /// Merges asked for while this sheet has been open. They are written after
    /// `</sheetData>`, so unlike the columns they need not be known in advance
    /// — which matters, because a total row's merge depends on how many rows
    /// there turned out to be.
    pending: Vec<Merge>,
    /// One reusable buffer. Nothing here allocates per row.
    buf: String,
}

impl<W: Write + Seek> Session<W> {
    /// Opens a workbook. Nothing is written for a sheet until rows arrive.
    ///
    /// Any rows already on a declared sheet are written immediately, so a
    /// header can be declared and the body streamed.
    pub fn open(out: W, sheets: Vec<Sheet>) -> Result<Self, Error> {
        if sheets.is_empty() {
            return Err(Error::Empty);
        }

        let options = options();
        let mut zip = ZipWriter::new(out);

        part(
            &mut zip,
            options,
            "[Content_Types].xml",
            &content_types(sheets.len()),
        )?;
        part(&mut zip, options, "_rels/.rels", root_rels())?;
        part(&mut zip, options, "xl/workbook.xml", &workbook_xml(&sheets))?;
        part(
            &mut zip,
            options,
            "xl/_rels/workbook.xml.rels",
            &workbook_rels(sheets.len()),
        )?;

        let mut session = Self {
            zip,
            options,
            styles: Styles::new(),
            sheets,
            at: 0,
            row: 0,
            pending: Vec::new(),
            buf: String::new(),
        };
        session.begin()?;
        Ok(session)
    }

    /// Adds a batch of rows to the sheet that is open.
    ///
    /// Batch as you would anywhere: a row at a time is a call at a time, and
    /// the overhead swamps the saving. What the caller holds is one batch.
    pub fn rows(&mut self, rows: &[Row]) -> Result<(), Error> {
        for row in rows {
            self.buf.clear();
            let sheet = &self.sheets[self.at];
            write_row(row, self.row, sheet, &mut self.styles, &mut self.buf);
            self.zip.write_all(self.buf.as_bytes())?;
            self.row += 1;
        }
        Ok(())
    }

    /// Merges a block of the sheet that is open.
    ///
    /// Rows and columns are counted from the top of the sheet, not from where
    /// the current batch happens to start.
    pub fn merge(&mut self, merge: Merge) {
        self.pending.push(merge);
    }

    /// How many rows have gone into the open sheet. The next one is this.
    pub fn row(&self) -> u32 {
        self.row
    }

    /// Closes the open sheet and opens the next one that was declared.
    pub fn next_sheet(&mut self) -> Result<(), Error> {
        if self.at + 1 >= self.sheets.len() {
            return Err(Error::NoMoreSheets {
                declared: self.sheets.len(),
            });
        }
        self.end()?;
        self.at += 1;
        self.begin()
    }

    /// Closes the workbook and hands back whatever it was written into.
    pub fn finish(mut self) -> Result<W, Error> {
        self.end()?;

        // Last, because the style table is only complete once every cell has
        // been seen. Zip entries have no required order beyond the content
        // types coming first.
        let styles = self.styles.to_xml();
        part(&mut self.zip, self.options, "xl/styles.xml", &styles)?;

        Ok(self.zip.finish()?)
    }

    /// Starts the worksheet part for `self.at`.
    fn begin(&mut self) -> Result<(), Error> {
        let name = format!("xl/worksheets/sheet{}.xml", self.at + 1);
        self.zip.start_file(name, self.options)?;

        let sheet = &self.sheets[self.at];
        // The elements come out in the order the schema fixes — views,
        // columns, data, merges. A worksheet whose `<cols>` follows its
        // `<sheetData>` is well-formed XML, invalid OOXML, and opens as a
        // repair dialog that names nothing.
        let head = format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
                "{}{}<sheetData>"
            ),
            views(sheet),
            columns(sheet, &mut self.styles),
        );
        self.zip.write_all(head.as_bytes())?;

        self.row = 0;
        self.pending.clear();

        // Rows the sheet was declared with go in before anything streamed, so
        // that declaring a header and feeding the body is the obvious thing
        // and produces the obvious file.
        let declared = std::mem::take(&mut self.sheets[self.at].rows);
        self.rows(&declared)?;
        self.sheets[self.at].rows = declared;
        Ok(())
    }

    /// Ends the worksheet part for `self.at`.
    fn end(&mut self) -> Result<(), Error> {
        self.zip.write_all(b"</sheetData>")?;
        // Merges declared on the sheet, then any asked for while it was open.
        let mut all = self.sheets[self.at].merges.clone();
        all.append(&mut self.pending);
        self.zip.write_all(merges(&all).as_bytes())?;
        self.zip.write_all(b"</worksheet>")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Cell, Column, Freeze, Workbook};
    use crate::write;
    use std::io::Cursor;

    fn ledger(rows: usize) -> Sheet {
        Sheet {
            name: "Libro".into(),
            columns: vec![
                Column {
                    width: Some(12.0),
                    style: None,
                },
                Column {
                    width: Some(30.0),
                    style: None,
                },
            ],
            rows: (0..rows)
                .map(|n| {
                    Row::new(vec![
                        Cell::text(format!("{n:06}")),
                        Cell::number(n as f64 * 1.5),
                    ])
                })
                .collect(),
            merges: vec![],
            freeze: Some(Freeze {
                rows: 1,
                columns: 0,
            }),
        }
    }

    /// Writes a workbook by feeding its rows in batches of `batch`.
    fn streamed(book: &Workbook, batch: usize) -> Vec<u8> {
        let empty: Vec<Sheet> = book
            .sheets
            .iter()
            .map(|sheet| Sheet {
                rows: vec![],
                ..sheet.clone()
            })
            .collect();

        let mut session = Session::open(Cursor::new(Vec::new()), empty).expect("it should open");
        for (index, sheet) in book.sheets.iter().enumerate() {
            if index > 0 {
                session.next_sheet().expect("another sheet was declared");
            }
            for chunk in sheet.rows.chunks(batch) {
                session.rows(chunk).expect("rows should go in");
            }
        }
        session.finish().expect("it should finish").into_inner()
    }

    #[test]
    fn a_streamed_workbook_is_byte_for_byte_the_declared_one() {
        // The property the whole module rests on. If these ever differ, one of
        // the two ways of producing a workbook is producing a worse one, and
        // nobody would find out from the file.
        let book = Workbook::new(vec![ledger(500)]);
        assert_eq!(streamed(&book, 100), write(&book).unwrap());
    }

    #[test]
    fn how_the_rows_are_cut_makes_no_difference() {
        // A caller batches by whatever its source hands it, and the file must
        // not record that. One row per batch is the pathological case and has
        // to agree with the rest.
        let book = Workbook::new(vec![ledger(97)]);
        let whole = write(&book).unwrap();
        for batch in [1, 2, 7, 96, 97, 1000] {
            assert_eq!(streamed(&book, batch), whole, "batch of {batch}");
        }
    }

    #[test]
    fn several_sheets_stream_in_the_order_they_were_declared() {
        let book = Workbook::new(vec![
            ledger(10),
            Sheet::new("Notas", vec![Row::new(vec![Cell::text("segunda")])]),
        ]);
        assert_eq!(streamed(&book, 3), write(&book).unwrap());
    }

    #[test]
    fn rows_declared_on_a_sheet_come_before_rows_that_are_fed() {
        // Declaring the header and streaming the body is the obvious way to
        // use this, so it has to produce the obvious file.
        let header = Sheet::new("Hoja", vec![Row::new(vec![Cell::text("Concepto")])]);
        let mut session =
            Session::open(Cursor::new(Vec::new()), vec![header]).expect("it should open");
        session
            .rows(&[Row::new(vec![Cell::text("Licencia")])])
            .expect("rows should go in");
        let bytes = session.finish().expect("finish").into_inner();

        let expected = write(&Workbook::new(vec![Sheet::new(
            "Hoja",
            vec![
                Row::new(vec![Cell::text("Concepto")]),
                Row::new(vec![Cell::text("Licencia")]),
            ],
        )]))
        .unwrap();

        assert_eq!(bytes, expected);
    }

    #[test]
    fn a_merge_can_be_asked_for_once_the_rows_are_known() {
        // The reason merges are not declared up front: a total row's merge
        // depends on how many rows there turned out to be, which is the one
        // thing a streaming producer learns last.
        let mut session = Session::open(Cursor::new(Vec::new()), vec![Sheet::new("Hoja", vec![])])
            .expect("it should open");

        session
            .rows(&[
                Row::new(vec![Cell::text("a")]),
                Row::new(vec![Cell::text("b")]),
            ])
            .expect("rows");
        let last = session.row();
        session.merge(Merge {
            from_row: last,
            from_column: 0,
            to_row: last,
            to_column: 2,
        });
        session
            .rows(&[Row::new(vec![Cell::text("Total")])])
            .expect("rows");

        let bytes = session.finish().expect("finish").into_inner();
        let mut read: calamine::Xlsx<_> =
            calamine::open_workbook_from_rs(Cursor::new(bytes)).expect("calamine should open it");
        let found = read
            .worksheet_merge_cells("Hoja")
            .expect("sheet")
            .expect("merges");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start, (2, 0));
    }

    #[test]
    fn a_workbook_with_no_sheets_is_refused_rather_than_written() {
        // Excel will not open a workbook with no sheets in it, and the error
        // it gives says nothing. Better to fail where the mistake was made.
        let refused = Session::open(Cursor::new(Vec::new()), vec![]);
        assert!(matches!(refused, Err(Error::Empty)));
    }

    #[test]
    fn asking_for_a_sheet_that_was_never_declared_says_so() {
        let mut session = Session::open(Cursor::new(Vec::new()), vec![Sheet::new("Sola", vec![])])
            .expect("it should open");

        assert!(matches!(
            session.next_sheet(),
            Err(Error::NoMoreSheets { declared: 1 })
        ));
    }
}
