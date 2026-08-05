//! A workbook fed a batch of rows at a time.
//!
//! # Why there is no thread and no lock in here
//!
//! The Node binding wraps its `Session` in an `Arc<Mutex<_>>` and refuses a
//! second call while one is running, because every batch goes to libuv's pool
//! and the pool picks a different thread each time. A WebAssembly instance is
//! a single-threaded world by construction, so the session simply lives here
//! between calls — and one call into a module cannot interleave with another,
//! so the ordering the napi crate enforces at run time is structural.
//!
//! Keeping the host responsive is the host's problem, one level up: put the
//! instance in a worker.

use std::io::Cursor;

use imprenta_xlsx::Session;
use imprenta_xlsx::ir::{Merge, Row, Sheet};

use crate::job::{JobError, Outcome};

/// The workbook is written into memory, because a WebAssembly module has no
/// filesystem to write it to. That is the same trade the buffer-returning
/// half of the Node binding makes, and for a spreadsheet it is a smaller one
/// than for a PDF: a sheet is XML in a zip, not a hundred and twenty-eight
/// megabytes of glyphs.
type Sink = Cursor<Vec<u8>>;

pub struct Book {
    session: Session<Sink>,
    sheets: usize,
}

impl Book {
    /// Opens a workbook.
    ///
    /// `sheets` is the same JSON a whole workbook puts in its `sheets` field,
    /// minus the rows: names, columns and frozen panes have to be known before
    /// anything is written, because `[Content_Types].xml` names every sheet
    /// and the specification wants it first in the package. The rows are what
    /// streams.
    pub fn open(sheets_json: &[u8]) -> Result<Self, JobError> {
        let declared: Vec<Sheet> =
            serde_json::from_slice(sheets_json).map_err(|e| JobError::Malformed(e.to_string()))?;
        let sheets = declared.len();
        let session = Session::open(Cursor::new(Vec::new()), declared)?;
        Ok(Self { session, sheets })
    }

    pub fn rows(&mut self, json: &[u8]) -> Result<(), JobError> {
        let rows: Vec<Row> =
            serde_json::from_slice(json).map_err(|e| JobError::Malformed(e.to_string()))?;
        self.session.rows(&rows)?;
        Ok(())
    }

    pub fn merge(&mut self, json: &[u8]) -> Result<(), JobError> {
        let merge: Merge =
            serde_json::from_slice(json).map_err(|e| JobError::Malformed(e.to_string()))?;
        self.session.merge(merge);
        Ok(())
    }

    pub fn next_sheet(&mut self) -> Result<(), JobError> {
        self.session.next_sheet()?;
        Ok(())
    }

    /// How many rows have gone into the open sheet. The next one is this.
    pub fn row(&self) -> u32 {
        self.session.row()
    }

    pub fn finish(self) -> Result<Outcome, JobError> {
        let out = self.session.finish()?;
        Ok(Outcome {
            xlsx: out.into_inner(),
            sheets: self.sheets,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::run;

    const SHEETS: &[u8] = br#"[{ "name": "Ventas" }]"#;

    fn row(i: usize) -> String {
        format!(
            r#"{{"cells":[{{"value":{{"t":"text","v":"Asiento {i}"}}}},{{"value":{{"t":"number","v":{}}}}}]}}"#,
            i * 100
        )
    }

    /// The same workbook declared whole.
    fn declared(rows: usize) -> Vec<u8> {
        let body: Vec<String> = (0..rows).map(row).collect();
        format!(
            r#"{{"sheets":[{{"name":"Ventas","rows":[{}]}}]}}"#,
            body.join(",")
        )
        .into_bytes()
    }

    /// The same workbook fed in batches of `batch`.
    fn streamed(rows: usize, batch: usize) -> Outcome {
        let mut book = Book::open(SHEETS).unwrap();
        let mut sent = 0;
        while sent < rows {
            let take = batch.min(rows - sent);
            let body: Vec<String> = (sent..sent + take).map(row).collect();
            book.rows(format!("[{}]", body.join(",")).as_bytes())
                .unwrap();
            sent += take;
        }
        book.finish().unwrap()
    }

    #[test]
    fn a_streamed_workbook_is_the_one_the_same_rows_declare() {
        // The rule this crate is most likely to break silently: content fed in
        // chunks must be byte for byte what the same content declared whole
        // produces.
        let whole = run(&declared(300)).unwrap();

        let fed = streamed(300, 50);

        assert_eq!(fed.xlsx, whole.xlsx);
    }

    #[test]
    fn how_the_batches_are_cut_makes_no_difference() {
        let by_one = streamed(60, 1);
        let by_seven = streamed(60, 7);
        let in_one_go = streamed(60, 60);

        assert_eq!(by_one.xlsx, by_seven.xlsx);
        assert_eq!(by_seven.xlsx, in_one_go.xlsx);
    }

    #[test]
    fn a_second_sheet_is_opened_only_when_it_was_declared() {
        let mut book = Book::open(br#"[{ "name": "Uno" }, { "name": "Dos" }]"#).unwrap();
        book.rows(format!("[{}]", row(0)).as_bytes()).unwrap();

        book.next_sheet().unwrap();
        book.rows(format!("[{}]", row(1)).as_bytes()).unwrap();

        assert_eq!(book.finish().unwrap().sheets, 2);
    }

    #[test]
    fn asking_for_a_sheet_that_was_never_declared_is_an_error() {
        let mut book = Book::open(SHEETS).unwrap();

        assert!(book.next_sheet().is_err());
    }

    #[test]
    fn the_open_sheet_says_how_far_down_it_is() {
        // What a caller needs to merge a total row: the merge is expressed in
        // absolute rows, not in rows since the last batch.
        let mut book = Book::open(SHEETS).unwrap();

        book.rows(format!("[{},{}]", row(0), row(1)).as_bytes())
            .unwrap();

        assert_eq!(book.row(), 2);
    }

    #[test]
    fn a_malformed_batch_is_an_error_and_not_a_panic() {
        let mut book = Book::open(SHEETS).unwrap();

        let err = book.rows(b"[ not json").unwrap_err();

        assert!(matches!(err, JobError::Malformed(_)), "{err:?}");
    }

    #[test]
    fn a_workbook_with_no_sheets_says_so() {
        assert!(Book::open(b"[]").is_err());
    }
}
