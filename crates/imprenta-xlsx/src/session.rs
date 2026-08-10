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
use crate::picture::{Image, PictureError, Stored, deepest_row, drawing, drawing_rels, stored};
use crate::sheet::{auto_filter, columns, merges, views, write_row};
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
    /// How many rows went into each sheet. Compared against how many of their
    /// heights were kept, so that a picture asking about a row nobody measured
    /// is refused rather than guessed at.
    written: Vec<u32>,
    /// The deepest row on the open sheet whose height a picture still needs, or
    /// `None` when no picture is placed down the page. Everything past it is
    /// written and forgotten, which is what keeps a streamed sheet flat.
    keep_to: Option<u32>,
    /// One reusable buffer. Nothing here allocates per row.
    buf: String,
    /// Which row asked to be the open sheet's autofilter, and how wide it is.
    /// Kept while the sheet is open because the range it becomes ends at the
    /// last row, which is only known when the sheet closes.
    filter: Option<(u32, u32)>,
    /// The images some sheet names, identified once and numbered.
    stored: Vec<Stored>,
    /// The bytes, kept until `finish` writes the media parts. A logo, not a
    /// dataset — this is the one thing here that is held whole on purpose.
    images: Vec<Image>,
}

impl<W: Write + Seek> Session<W> {
    /// Opens a workbook. Nothing is written for a sheet until rows arrive.
    ///
    /// Any rows already on a declared sheet are written immediately, so a
    /// header can be declared and the body streamed.
    pub fn open(out: W, sheets: Vec<Sheet>, images: Vec<Image>) -> Result<Self, Error> {
        if sheets.is_empty() {
            return Err(Error::Empty);
        }

        // Before anything is written: an image that cannot be read stops the
        // workbook here rather than producing one with a hole in it.
        let stored = stored(&sheets, &images)?;

        let options = options();
        let mut zip = ZipWriter::new(out);

        part(
            &mut zip,
            options,
            "[Content_Types].xml",
            &content_types(sheets.len(), &illustrated(&sheets), &stored),
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
            written: vec![0; sheets.len()],
            sheets,
            at: 0,
            row: 0,
            keep_to: None,
            buf: String::new(),
            stored,
            images,
            filter: None,
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
            if row.filter {
                // Excel has one autofilter to a sheet. Two marked rows is
                // somebody copying a header block, and the alternative to
                // saying so is a file where the second one silently wins.
                if let Some((first, _)) = self.filter {
                    return Err(Error::TwoFilters {
                        sheet: self.sheets[self.at].name.clone(),
                        first: first + 1,
                        second: self.row + 1,
                    });
                }
                self.filter = Some((self.row, row.cells.len() as u32));
            }

            self.buf.clear();
            let sheet = &self.sheets[self.at];
            write_row(row, self.row, sheet, &mut self.styles, &mut self.buf);
            self.zip.write_all(self.buf.as_bytes())?;
            let at = self.row;
            self.row += 1;
            self.keep(row, at);
        }
        Ok(())
    }

    /// Keeps a row's height, when a picture on this sheet is placed down it.
    ///
    /// The height and nothing else — not the cells, which are already bytes in
    /// the zip by now. A picture centred inside a block needs to know how tall
    /// the block is, and the block is the merge that swallowed its anchor: a
    /// letterhead's worth of rows, never the sheet's. Past `keep_to` a row is
    /// written and forgotten, so this cannot grow with the export.
    fn keep(&mut self, row: &Row, at: u32) {
        let Some(deepest) = self.keep_to else {
            return;
        };
        if at > deepest {
            return;
        }
        let kept = &mut self.sheets[self.at].rows;
        // A prefix, and always in step. The drawing reads these by index, so a
        // gap would shift every height below it without saying anything.
        if kept.len() as u32 == at {
            kept.push(Row {
                height: row.height,
                ..Row::default()
            });
        }
    }

    /// Merges a block of the sheet that is open.
    ///
    /// Rows and columns are counted from the top of the sheet, not from where
    /// the current batch happens to start.
    pub fn merge(&mut self, merge: Merge) {
        // Onto the sheet rather than into a list of its own. The merges are
        // written after `</sheetData>` either way, but a picture's block *is*
        // the merge that swallowed its anchor and the drawing is not written
        // until `finish` — so a merge that lived only until the sheet closed
        // left a logo centred in its anchor cell instead of across the block.
        self.sheets[self.at].merges.push(merge);
        // And it may have just deepened the rows whose heights are needed.
        // Worked out here, because by `finish` the rows have gone past.
        self.keep_to = deepest_row(&self.sheets[self.at]);
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
        self.illustrate()?;

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
        // The relationship prefix only when there is a drawing to name with
        // it. A namespace nobody uses would change every file this engine has
        // ever written, for the benefit of the sheets that have no picture.
        let relationships = if sheet.pictures.is_empty() {
            ""
        } else {
            r#" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#
        };
        let head = format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"{}>"#,
                "{}{}<sheetData>"
            ),
            relationships,
            views(sheet),
            columns(sheet, &mut self.styles),
        );
        self.zip.write_all(head.as_bytes())?;

        self.row = 0;
        self.filter = None;
        self.keep_to = deepest_row(sheet);

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
        self.written[self.at] = self.row;
        // The order the schema fixes for a worksheet's children: the filter
        // before the merges, and the drawing after both. Out of place it is
        // well-formed XML, invalid OOXML, and a repair dialog naming nothing.
        let (header, columns) = self.filter.map_or((None, 0), |(r, c)| (Some(r), c));
        self.zip
            .write_all(auto_filter(header, columns, self.row).as_bytes())?;
        let all = merges(&self.sheets[self.at].merges);
        self.zip.write_all(all.as_bytes())?;
        // Last in the schema's order for a worksheet, and out of place it is a
        // repair dialog rather than an error anyone can read.
        if !self.sheets[self.at].pictures.is_empty() {
            self.zip.write_all(br#"<drawing r:id="rId1"/>"#)?;
        }
        self.zip.write_all(b"</worksheet>")?;
        Ok(())
    }

    /// Writes the media, the drawings and the relationships that tie them on.
    ///
    /// At the end rather than as each sheet closes, because a zip entry cannot
    /// be opened while another is: the worksheet parts stream, and everything
    /// here waits for the last of them to close.
    fn illustrate(&mut self) -> Result<(), Error> {
        if self.stored.is_empty() {
            return Ok(());
        }
        self.placeable()?;

        for image in &self.stored {
            // `stored` found these bytes at `open`, so a name with nothing
            // behind it cannot happen — and if it ever did, an empty media part
            // is a workbook with a hole in it that opens perfectly well.
            let bytes = self
                .images
                .iter()
                .find(|candidate| candidate.name == image.name)
                .ok_or_else(|| PictureError::Missing(image.name.clone()))?;
            self.zip
                .start_file(format!("xl/media/{}", image.part), self.options)?;
            self.zip.write_all(&bytes.data)?;
        }

        let rels = drawing_rels(&self.stored);
        for index in illustrated(&self.sheets) {
            let sheet = self.sheets[index - 1].clone();
            part(
                &mut self.zip,
                self.options,
                &format!("xl/drawings/drawing{index}.xml"),
                &drawing(&sheet, &self.stored),
            )?;
            part(
                &mut self.zip,
                self.options,
                &format!("xl/drawings/_rels/drawing{index}.xml.rels"),
                &rels,
            )?;
            part(
                &mut self.zip,
                self.options,
                &format!("xl/worksheets/_rels/sheet{index}.xml.rels"),
                &sheet_rels(index),
            )?;
        }
        Ok(())
    }
}

/// Refuses a picture whose block covers rows nobody kept the height of.
///
/// A row is kept only while some picture can still need it, and which rows
/// those are comes from the merges the session has been told about. A merge
/// declared *after* its own rows have been written therefore claims rows that
/// were measured and let go. Rows past the end of the sheet are a different
/// thing and perfectly fine: they are Excel's default height in a declared
/// workbook and in a streamed one alike, so both agree.
impl<W: Write + Seek> Session<W> {
    fn placeable(&self) -> Result<(), Error> {
        for (index, sheet) in self.sheets.iter().enumerate() {
            for picture in &sheet.pictures {
                if picture.valign.is_start() {
                    continue;
                }
                let bottom = sheet.block(picture.row, picture.column).2;
                let kept = sheet.rows.len() as u32;
                if bottom >= kept && bottom < self.written[index] {
                    return Err(PictureError::Unplaceable {
                        image: picture.image.clone(),
                        bottom,
                    }
                    .into());
                }
            }
        }
        Ok(())
    }
}

/// Which sheets have a picture on them, one-based, in tab order.
///
/// One-based because every part in the package is: `sheet1.xml` is the first
/// tab, and a drawing numbered from zero beside it would be a trap.
fn illustrated(sheets: &[Sheet]) -> Vec<usize> {
    sheets
        .iter()
        .enumerate()
        .filter(|(_, sheet)| !sheet.pictures.is_empty())
        .map(|(index, _)| index + 1)
        .collect()
}

/// A worksheet's own relationships: its drawing, and nothing else yet.
fn sheet_rels(index: usize) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
            r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing{index}.xml"/>"#,
            "</Relationships>",
        ),
        index = index,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Cell, Column, Freeze, Picture, Placement, Workbook};
    use crate::picture::PictureError;
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
            pictures: vec![],
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

        let mut session =
            Session::open(Cursor::new(Vec::new()), empty, vec![]).expect("it should open");
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
        assert_eq!(streamed(&book, 100), write(&book, &[]).unwrap());
    }

    #[test]
    fn how_the_rows_are_cut_makes_no_difference() {
        // A caller batches by whatever its source hands it, and the file must
        // not record that. One row per batch is the pathological case and has
        // to agree with the rest.
        let book = Workbook::new(vec![ledger(97)]);
        let whole = write(&book, &[]).unwrap();
        for batch in [1, 2, 7, 96, 97, 1000] {
            assert_eq!(streamed(&book, batch), whole, "batch of {batch}");
        }
    }

    /// A logo, and a sheet that hangs it off `A1`.
    fn logo() -> Image {
        Image::new(
            "logo",
            include_bytes!("../tests/images/logo.png").as_slice(),
        )
    }

    /// A picture placed inside its block rather than left in the corner.
    fn placed(align: Placement, valign: Placement) -> Picture {
        Picture {
            image: "logo".into(),
            row: 0,
            column: 0,
            width: 60.0,
            align,
            valign,
            ..Picture::default()
        }
    }

    /// The same workbook fed a row at a time, with images.
    fn streamed_with(book: &Workbook, images: Vec<Image>) -> Vec<u8> {
        let empty: Vec<Sheet> = book
            .sheets
            .iter()
            .map(|sheet| Sheet {
                rows: vec![],
                ..sheet.clone()
            })
            .collect();
        let mut session =
            Session::open(Cursor::new(Vec::new()), empty, images).expect("it should open");
        for sheet in &book.sheets {
            for row in &sheet.rows {
                session.rows(std::slice::from_ref(row)).expect("rows");
            }
        }
        session.finish().expect("finish").into_inner()
    }

    #[test]
    fn a_centred_picture_lands_where_it_does_when_the_rows_are_declared() {
        // The heights of the rows a picture is centred down are the one thing
        // the drawing needs that a streaming producer supplies last. Taking
        // them from the declaration alone put a logo 37.5pt above where the
        // same workbook declared whole puts it — the file opens, the picture is
        // there, and it is in the wrong place.
        let tall = |text: &str| Row {
            height: Some(40.0),
            ..Row::new(vec![Cell::text(text)])
        };
        let book = Workbook::new(vec![Sheet {
            name: "Hoja".into(),
            rows: vec![tall("uno"), tall("dos"), tall("tres")],
            merges: vec![Merge {
                from_row: 0,
                from_column: 0,
                to_row: 2,
                to_column: 1,
            }],
            pictures: vec![placed(Placement::Start, Placement::Center)],
            ..Sheet::default()
        }]);

        assert_eq!(
            streamed_with(&book, vec![logo()]),
            write(&book, &[logo()]).unwrap()
        );
    }

    #[test]
    fn a_merge_asked_for_while_streaming_is_the_block_a_picture_hangs_from() {
        // A merge is exactly the thing this module lets a producer declare
        // late, and a picture's block is the merge that swallowed its anchor.
        // Written into a temporary the merge vanished before the drawing was
        // written, so a logo centred across `A1:B1` was centred in `A1`.
        let sheet = Sheet {
            name: "Hoja".into(),
            columns: vec![
                Column {
                    width: Some(13.0),
                    style: None,
                },
                Column {
                    width: Some(52.0),
                    style: None,
                },
            ],
            rows: vec![Row::new(vec![Cell::text("uno")])],
            merges: vec![Merge {
                from_row: 0,
                from_column: 0,
                to_row: 0,
                to_column: 1,
            }],
            pictures: vec![placed(Placement::Center, Placement::Start)],
            ..Sheet::default()
        };
        let declared = write(&Workbook::new(vec![sheet.clone()]), &[logo()]).unwrap();

        let mut session = Session::open(
            Cursor::new(Vec::new()),
            vec![Sheet {
                rows: vec![],
                merges: vec![],
                ..sheet.clone()
            }],
            vec![logo()],
        )
        .expect("it should open");
        session.rows(&sheet.rows).expect("rows");
        session.merge(sheet.merges[0]);

        assert_eq!(session.finish().expect("finish").into_inner(), declared);
    }

    #[test]
    fn a_picture_left_in_the_corner_needs_nothing_the_rows_can_tell_it() {
        // The default placement is an offset of nought whatever the rows turn
        // out to be, so a streamed sheet must not start keeping their heights
        // to answer a question nobody asked. This is the case that has to stay
        // free, because it is every picture until somebody writes `valign`.
        let book = Workbook::new(vec![Sheet {
            name: "Hoja".into(),
            rows: (0..200)
                .map(|n| Row {
                    height: Some(20.0 + f64::from(n % 3)),
                    ..Row::new(vec![Cell::text(format!("{n}"))])
                })
                .collect(),
            pictures: vec![Picture {
                image: "logo".into(),
                row: 0,
                column: 0,
                width: 60.0,
                ..Picture::default()
            }],
            ..Sheet::default()
        }]);

        assert_eq!(
            streamed_with(&book, vec![logo()]),
            write(&book, &[logo()]).unwrap()
        );
    }

    #[test]
    fn a_merge_that_arrives_after_the_rows_it_needs_is_refused() {
        // The one case the heights cannot be recovered for: the rows a centred
        // picture's block covers went past before any merge said they were the
        // block. Guessing puts the picture somewhere the same workbook
        // declared whole would not, which is the defect this module exists to
        // make impossible — so it says so instead.
        let mut session = Session::open(
            Cursor::new(Vec::new()),
            vec![Sheet {
                name: "Hoja".into(),
                pictures: vec![placed(Placement::Start, Placement::Center)],
                ..Sheet::default()
            }],
            vec![logo()],
        )
        .expect("it should open");

        session
            .rows(
                &(0..40)
                    .map(|n| Row {
                        height: Some(30.0),
                        ..Row::new(vec![Cell::text(format!("{n}"))])
                    })
                    .collect::<Vec<_>>(),
            )
            .expect("rows");
        session.merge(Merge {
            from_row: 0,
            from_column: 0,
            to_row: 30,
            to_column: 1,
        });

        assert!(matches!(
            session.finish(),
            Err(Error::Picture(PictureError::Unplaceable { .. }))
        ));
    }

    #[test]
    fn several_sheets_stream_in_the_order_they_were_declared() {
        let book = Workbook::new(vec![
            ledger(10),
            Sheet::new("Notas", vec![Row::new(vec![Cell::text("segunda")])]),
        ]);
        assert_eq!(streamed(&book, 3), write(&book, &[]).unwrap());
    }

    #[test]
    fn rows_declared_on_a_sheet_come_before_rows_that_are_fed() {
        // Declaring the header and streaming the body is the obvious way to
        // use this, so it has to produce the obvious file.
        let header = Sheet::new("Hoja", vec![Row::new(vec![Cell::text("Concepto")])]);
        let mut session =
            Session::open(Cursor::new(Vec::new()), vec![header], vec![]).expect("it should open");
        session
            .rows(&[Row::new(vec![Cell::text("Licencia")])])
            .expect("rows should go in");
        let bytes = session.finish().expect("finish").into_inner();

        let expected = write(
            &Workbook::new(vec![Sheet::new(
                "Hoja",
                vec![
                    Row::new(vec![Cell::text("Concepto")]),
                    Row::new(vec![Cell::text("Licencia")]),
                ],
            )]),
            &[],
        )
        .unwrap();

        assert_eq!(bytes, expected);
    }

    #[test]
    fn a_merge_can_be_asked_for_once_the_rows_are_known() {
        // The reason merges are not declared up front: a total row's merge
        // depends on how many rows there turned out to be, which is the one
        // thing a streaming producer learns last.
        let mut session = Session::open(
            Cursor::new(Vec::new()),
            vec![Sheet::new("Hoja", vec![])],
            vec![],
        )
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
        let refused = Session::open(Cursor::new(Vec::new()), vec![], vec![]);
        assert!(matches!(refused, Err(Error::Empty)));
    }

    #[test]
    fn asking_for_a_sheet_that_was_never_declared_says_so() {
        let mut session = Session::open(
            Cursor::new(Vec::new()),
            vec![Sheet::new("Sola", vec![])],
            vec![],
        )
        .expect("it should open");

        assert!(matches!(
            session.next_sheet(),
            Err(Error::NoMoreSheets { declared: 1 })
        ));
    }
}
