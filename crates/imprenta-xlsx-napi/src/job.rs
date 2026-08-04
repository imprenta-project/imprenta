//! One workbook, described in terms Node can hand over.
//!
//! Everything here is ordinary Rust. The napi layer above translates JS values
//! into a [`Job`] and the result back again, and does nothing else — so the
//! behaviour that matters can be tested without starting a Node process.

use std::io::Cursor;
use std::path::PathBuf;

use imprenta_xlsx::ir::Workbook;

/// Where the finished workbook should go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Back to the caller. Costs a copy into the JS heap.
    Buffer,
    /// Straight to disk, so the bytes never reach the JS heap.
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Job {
    pub ir: String,
    pub output: Output,
}

/// What came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Present only when the job asked for a buffer.
    pub xlsx: Option<Vec<u8>>,
    pub bytes: usize,
    pub sheets: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("the workbook is not valid JSON: {0}")]
    Malformed(String),

    #[error("{0}")]
    Write(#[from] imprenta_xlsx::Error),
}

/// Parses a declared workbook and writes it.
pub fn run(job: Job) -> Result<Outcome, JobError> {
    let book: Workbook =
        serde_json::from_str(&job.ir).map_err(|e| JobError::Malformed(e.to_string()))?;
    let sheets = book.sheets.len();

    match job.output {
        Output::Buffer => {
            let xlsx = imprenta_xlsx::write(&book)?;
            Ok(Outcome {
                bytes: xlsx.len(),
                xlsx: Some(xlsx),
                sheets,
            })
        }
        Output::File(path) => {
            let bytes = imprenta_xlsx::write_to_file(&book, path)? as usize;
            Ok(Outcome {
                xlsx: None,
                bytes,
                sheets,
            })
        }
    }
}

/// Where a streamed workbook is going.
///
/// An enum rather than a boxed trait object because the session outlives the
/// call that made it — it sits in a JS object between one batch and the next —
/// and at the end we have to get the bytes back out. A `Box<dyn Write + Seek>`
/// would need a downcast to do that, which is a runtime question standing in
/// for one the compiler can answer: there are two places a workbook can go.
pub enum Sink {
    Buffer(Cursor<Vec<u8>>),
    File(std::io::BufWriter<std::fs::File>),
}

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Sink::Buffer(out) => out.write(buf),
            Sink::File(out) => out.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Sink::Buffer(out) => out.flush(),
            Sink::File(out) => out.flush(),
        }
    }
}

impl std::io::Seek for Sink {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        match self {
            Sink::Buffer(out) => out.seek(to),
            Sink::File(out) => out.seek(to),
        }
    }
}

impl Sink {
    pub fn open(path: Option<&str>) -> Result<Self, JobError> {
        Ok(match path {
            Some(path) => Sink::File(std::io::BufWriter::new(
                std::fs::File::create(path).map_err(imprenta_xlsx::Error::Io)?,
            )),
            None => Sink::Buffer(Cursor::new(Vec::new())),
        })
    }

    /// The bytes, for a workbook that went to memory, and how many there are.
    ///
    /// A file is flushed here rather than left to `Drop`, which swallows the
    /// error — and a truncated spreadsheet that reported success is the worst
    /// of the available outcomes.
    pub fn close(self) -> Result<(Option<Vec<u8>>, usize), JobError> {
        match self {
            Sink::Buffer(out) => {
                let bytes = out.into_inner();
                Ok((Some(bytes.clone()), bytes.len()))
            }
            Sink::File(out) => {
                let file = out
                    .into_inner()
                    .map_err(|e| JobError::Write(imprenta_xlsx::Error::Io(e.into_error())))?;
                let len = file
                    .metadata()
                    .map_err(|e| JobError::Write(imprenta_xlsx::Error::Io(e)))?
                    .len();
                Ok((None, len as usize))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_CELL: &str = r#"{"sheets":[{"name":"Hoja","rows":[{"cells":[
        {"value":{"t":"text","v":"Hola"}}
    ]}]}]}"#;

    #[test]
    fn writes_a_workbook_from_the_json_a_producer_sends() {
        let out = run(Job {
            ir: ONE_CELL.into(),
            output: Output::Buffer,
        })
        .expect("valid JSON should write");

        assert_eq!(out.sheets, 1);
        assert!(out.bytes > 0);
        // A zip, whatever else it is.
        assert_eq!(&out.xlsx.expect("bytes")[..2], b"PK");
    }

    #[test]
    fn malformed_json_is_an_error_and_not_a_panic() {
        // It crosses a napi boundary. A panic there takes the process with it,
        // and the process is somebody's web server.
        let failed = run(Job {
            ir: "{ not json".into(),
            output: Output::Buffer,
        });
        assert!(matches!(failed, Err(JobError::Malformed(_))));
    }

    #[test]
    fn a_workbook_with_no_sheets_is_refused_by_name() {
        let failed = run(Job {
            ir: r#"{"sheets":[]}"#.into(),
            output: Output::Buffer,
        });
        let message = failed.expect_err("no sheets is not a workbook").to_string();
        assert!(message.contains("at least one sheet"), "{message}");
    }

    #[test]
    fn writing_to_a_file_hands_back_no_buffer() {
        // The whole point of the file path: a hundred megabytes should not
        // become a hundred megabytes in the JS heap on the way to disk.
        let path = std::env::temp_dir().join("imprenta-xlsx-job-test.xlsx");
        let out = run(Job {
            ir: ONE_CELL.into(),
            output: Output::File(path.clone()),
        })
        .expect("it should write");

        assert!(out.xlsx.is_none());
        assert_eq!(out.bytes as u64, std::fs::metadata(&path).unwrap().len());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_number_stays_a_number_across_the_wire() {
        // The one thing this format cannot get wrong. If the tag is lost in
        // transit the cell arrives as text and SUM returns zero.
        let ir = r#"{"sheets":[{"name":"H","rows":[{"cells":[
            {"value":{"t":"number","v":1200}}
        ]}]}]}"#;
        let out = run(Job {
            ir: ir.into(),
            output: Output::Buffer,
        })
        .expect("it should write");

        let bytes = out.xlsx.expect("bytes");
        let mut read: calamine::Xlsx<_> =
            calamine::open_workbook_from_rs(Cursor::new(bytes)).expect("calamine opens it");
        let range = calamine::Reader::worksheet_range(&mut read, "H").expect("sheet");
        assert_eq!(
            range.get_value((0, 0)),
            Some(&calamine::Data::Float(1200.0))
        );
    }
}
