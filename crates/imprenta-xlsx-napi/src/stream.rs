//! Feeding a workbook from Node without ever holding it whole.
//!
//! # Why there is no thread in here
//!
//! The PDF side gives its session a dedicated thread, because krilla keeps its
//! fonts behind an `Rc` and a half-written document cannot move between
//! threads at all. A spreadsheet session has no such thing in it — a zip
//! writer, a style table, a string buffer — so it is `Send`, and each batch can
//! go to libuv's pool like any other job.
//!
//! That is the whole difference, and it is worth saying rather than copying
//! the PDF design out of symmetry: a thread per open workbook would cost a
//! thread per concurrent export, for nothing.
//!
//! What does carry over is the ordering rule. A stream is read in order and two
//! promises in flight have no order at all, so a second call while one is
//! running is refused rather than quietly queued.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use imprenta_xlsx::Session;
use imprenta_xlsx::ir::{Merge, Row, Sheet};
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::job::Sink;

/// A workbook open for writing, held between one batch and the next.
type Open = Arc<std::sync::Mutex<Option<Session<Sink>>>>;

#[napi(object)]
pub struct StreamResult {
    /// Present only when no path was given.
    pub xlsx: Option<Buffer>,
    pub path: Option<String>,
    pub bytes: u32,
    pub sheets: u32,
}

/// A workbook being fed a batch of rows at a time.
#[napi]
pub struct WorkbookStream {
    session: Open,
    /// Refuses a second call while one is running. Backpressure, not a lock:
    /// the mutex does the locking, this says what went wrong.
    busy: Arc<AtomicBool>,
    path: Option<String>,
    sheets: u32,
}

#[napi]
impl WorkbookStream {
    /// Opens a workbook.
    ///
    /// `sheets` is the same JSON a whole workbook puts in its `sheets` field,
    /// minus the rows: names, columns and frozen panes have to be known before
    /// anything is written, because `[Content_Types].xml` names every sheet and
    /// the specification wants it first in the package. The rows are what
    /// streams.
    #[napi(constructor)]
    pub fn new(sheets: String, path: Option<String>) -> Result<Self> {
        let declared: Vec<Sheet> = serde_json::from_str(&sheets)
            .map_err(|e| Error::from_reason(format!("the sheets are not valid JSON: {e}")))?;
        let count = declared.len() as u32;

        let out = Sink::open(path.as_deref()).map_err(fail)?;
        let session = Session::open(out, declared).map_err(fail)?;

        Ok(Self {
            session: Arc::new(std::sync::Mutex::new(Some(session))),
            busy: Arc::new(AtomicBool::new(false)),
            path,
            sheets: count,
        })
    }

    /// Adds a batch of rows to the sheet that is open.
    #[napi(ts_return_type = "Promise<void>")]
    pub fn rows(&self, rows: String) -> AsyncTask<Feed> {
        AsyncTask::new(Feed {
            session: self.session.clone(),
            busy: self.busy.clone(),
            work: Some(Work::Rows(rows)),
        })
    }

    /// Closes the open sheet and moves to the next one that was declared.
    #[napi(ts_return_type = "Promise<void>")]
    pub fn next_sheet(&self) -> AsyncTask<Feed> {
        AsyncTask::new(Feed {
            session: self.session.clone(),
            busy: self.busy.clone(),
            work: Some(Work::NextSheet),
        })
    }

    /// Merges a block of the open sheet.
    ///
    /// Rows and columns count from the top of the sheet. Merges are written
    /// after the rows, so this can be called once the row count is known —
    /// which is when a total row's span is finally decidable.
    #[napi(ts_return_type = "Promise<void>")]
    pub fn merge(&self, merge: String) -> AsyncTask<Feed> {
        AsyncTask::new(Feed {
            session: self.session.clone(),
            busy: self.busy.clone(),
            work: Some(Work::Merge(merge)),
        })
    }

    /// Closes the workbook.
    #[napi(ts_return_type = "Promise<StreamResult>")]
    pub fn finish(&self) -> AsyncTask<Close> {
        AsyncTask::new(Close {
            session: self.session.clone(),
            busy: self.busy.clone(),
            path: self.path.clone(),
            sheets: self.sheets,
        })
    }
}

enum Work {
    Rows(String),
    NextSheet,
    Merge(String),
}

pub struct Feed {
    session: Open,
    busy: Arc<AtomicBool>,
    work: Option<Work>,
}

impl Task for Feed {
    type Output = ();
    type JsValue = ();

    fn compute(&mut self) -> Result<()> {
        let _guard = Busy::claim(&self.busy)?;
        let mut held = self.session.lock().map_err(poisoned)?;
        let session = held.as_mut().ok_or_else(closed)?;

        match self.work.take().expect("a task is computed once") {
            Work::Rows(json) => {
                let rows: Vec<Row> = serde_json::from_str(&json)
                    .map_err(|e| Error::from_reason(format!("the rows are not valid JSON: {e}")))?;
                session.rows(&rows).map_err(fail)?;
            }
            Work::NextSheet => session.next_sheet().map_err(fail)?,
            Work::Merge(json) => {
                let merge: Merge = serde_json::from_str(&json)
                    .map_err(|e| Error::from_reason(format!("the merge is not valid JSON: {e}")))?;
                session.merge(merge);
            }
        }
        Ok(())
    }

    fn resolve(&mut self, _env: Env, _out: ()) -> Result<()> {
        Ok(())
    }
}

pub struct Close {
    session: Open,
    busy: Arc<AtomicBool>,
    path: Option<String>,
    sheets: u32,
}

impl Task for Close {
    type Output = (Option<Vec<u8>>, usize);
    type JsValue = StreamResult;

    fn compute(&mut self) -> Result<(Option<Vec<u8>>, usize)> {
        let _guard = Busy::claim(&self.busy)?;
        let mut held = self.session.lock().map_err(poisoned)?;
        // Taken, not borrowed: finishing consumes the session, and a second
        // `finish()` has to say so rather than write a second central
        // directory into the same file.
        let session = held.take().ok_or_else(closed)?;

        let out = session.finish().map_err(fail)?;
        out.close().map_err(fail)
    }

    fn resolve(&mut self, _env: Env, (xlsx, bytes): Self::Output) -> Result<StreamResult> {
        Ok(StreamResult {
            xlsx: xlsx.map(Buffer::from),
            path: self.path.clone(),
            bytes: bytes as u32,
            sheets: self.sheets,
        })
    }
}

/// Held for the length of one call, so a second one is refused rather than
/// interleaved.
struct Busy<'a>(&'a AtomicBool);

impl<'a> Busy<'a> {
    fn claim(flag: &'a AtomicBool) -> Result<Self> {
        if flag.swap(true, Ordering::AcqRel) {
            return Err(Error::from_reason(
                "a call is already running on this workbook: await each one before the next",
            ));
        }
        Ok(Self(flag))
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn closed() -> Error {
    Error::from_reason("this workbook has already been finished")
}

fn poisoned<T>(_: T) -> Error {
    Error::from_reason("this workbook failed part way through and cannot be written to")
}

fn fail(error: impl std::fmt::Display) -> Error {
    Error::from_reason(error.to_string())
}
