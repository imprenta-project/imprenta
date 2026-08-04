//! Node bindings for the Imprenta spreadsheet writer.
//!
//! Its own addon rather than another entry point of the PDF one. The PDF
//! engine carries a text shaper and a font subsetter, which are most of that
//! binary; a spreadsheet writer is a zip and some XML. An application that
//! only exports data has no business downloading a typography stack.
//!
//! The workbook arrives as a JSON string rather than a JS object, for the same
//! reason it does on the PDF side: `JSON.stringify` plus serde beats walking an
//! object across the boundary, and a string is also what comes back from a
//! file, a queue or an HTTP body.
//!
//! This file is glue and nothing else. What it delegates to lives in [`job`]
//! and [`stream`], where it can be tested without starting Node.

// # Why there is no `#[global_allocator]` here
//
// The PDF addon sets one — mimalloc, measured, and load-bearing for peak RSS
// on a long ledger. Copying that here looked obviously right and is a bug:
// **two Rust global allocators in one process do not coexist.** A Node service
// that prints an invoice and exports a spreadsheet loads both `.node` files,
// and with the PDF one loaded first, its next render aborted the process at
// `shape.rs` with "the font contains no usable family" — the font bytes were
// being corrupted underneath it. Load them the other way round and it worked,
// which is the signature of two static allocators fighting over the same
// symbols rather than of anything to do with fonts.
//
// So this addon uses the system allocator, and it costs nothing worth having.
// The two are not in the same position: the PDF engine makes millions of tiny
// short-lived allocations while shaping, which is exactly what the system
// allocator answers by holding on to memory, and a spreadsheet writer makes a
// row's worth at a time and drops it. Streaming already keeps a million rows
// at 48 MB.
//
// If a global allocator is ever wanted for both, it has to be one — hoisted
// somewhere they share, or with the addon symbols hidden from each other at
// link time. `packages/xlsx/test/together.test.ts` holds the line.

use napi::bindgen_prelude::*;
use napi_derive::napi;

pub mod job;
pub mod stream;

use job::{Job, Outcome, Output};

#[napi(object)]
pub struct WriteResult {
    pub xlsx: Buffer,
    /// Size of the workbook in bytes.
    pub bytes: u32,
    pub sheets: u32,
}

/// As [`WriteResult`], for a workbook that went straight to disk.
#[napi(object)]
pub struct FileResult {
    pub path: String,
    pub bytes: u32,
    pub sheets: u32,
}

pub struct WriteTask {
    job: Option<Job>,
}

impl Task for WriteTask {
    type Output = Outcome;
    type JsValue = WriteResult;

    fn compute(&mut self) -> Result<Outcome> {
        let job = self.job.take().expect("a task is computed once");
        job::run(job).map_err(fail)
    }

    fn resolve(&mut self, _env: Env, out: Outcome) -> Result<WriteResult> {
        Ok(WriteResult {
            xlsx: out.xlsx.unwrap_or_default().into(),
            bytes: out.bytes as u32,
            sheets: out.sheets as u32,
        })
    }
}

pub struct FileTask {
    job: Option<Job>,
    path: String,
}

impl Task for FileTask {
    type Output = Outcome;
    type JsValue = FileResult;

    fn compute(&mut self) -> Result<Outcome> {
        let job = self.job.take().expect("a task is computed once");
        job::run(job).map_err(fail)
    }

    fn resolve(&mut self, _env: Env, out: Outcome) -> Result<FileResult> {
        Ok(FileResult {
            path: std::mem::take(&mut self.path),
            bytes: out.bytes as u32,
            sheets: out.sheets as u32,
        })
    }
}

/// Writes a declared workbook and hands back the bytes.
///
/// A promise, and the work runs on libuv's pool. A half-million-row export
/// takes seconds, and doing that on the main thread would stop a service
/// answering anything at all.
#[napi(ts_return_type = "Promise<WriteResult>")]
pub fn write(ir: String) -> AsyncTask<WriteTask> {
    AsyncTask::new(WriteTask {
        job: Some(Job {
            ir,
            output: Output::Buffer,
        }),
    })
}

/// Writes a declared workbook straight to a file.
///
/// Preferred for anything large: the bytes are written from Rust, so a
/// hundred-megabyte export never becomes a hundred-megabyte Buffer in the JS
/// heap on its way to disk.
#[napi(ts_return_type = "Promise<FileResult>")]
pub fn write_to_file(ir: String, path: String) -> AsyncTask<FileTask> {
    AsyncTask::new(FileTask {
        job: Some(Job {
            ir,
            output: Output::File(path.clone().into()),
        }),
        path,
    })
}

fn fail(error: impl std::fmt::Display) -> Error {
    Error::from_reason(error.to_string())
}
