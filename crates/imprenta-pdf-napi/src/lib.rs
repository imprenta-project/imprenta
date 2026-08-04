//! Node bindings for the Imprenta PDF engine.
//!
//! Two calls, and both of them are promises. Rendering is arithmetic-bound and
//! a long document takes tens of seconds; doing that on the main thread would
//! stop a Node service answering anything at all, which is the trap the
//! browser-based approach falls into. Every job runs on libuv's pool instead.
//!
//! The document arrives as a JSON string rather than a JS object. Walking an
//! object property by property across the boundary would cost more than
//! `JSON.stringify` and serde together, and a string is also what comes back
//! from a file, a queue or an HTTP body — so the fast path is the plain one.
//!
//! This file is glue and nothing else. What it delegates to lives in [`job`],
//! where it can be tested without starting Node.

use napi::bindgen_prelude::*;
use napi_derive::napi;

/// A long document is millions of short allocations — one per cell, per
/// glyph run, per measured line — and the system allocator answers those by
/// keeping the memory rather than returning it. Peak resident size on a
/// hundred thousand row ledger is where that shows up.
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub mod job;
pub mod stream;

use job::{FontInput, ImageInput, Job, Outcome, Output};

/// A typeface and the file behind it.
#[napi(object)]
pub struct FontSource {
    /// `"regular"` or `"bold"`. Defaults to regular.
    pub weight: Option<String>,
    pub italic: Option<bool>,
    pub data: Buffer,
}

/// An image the document refers to by name.
///
/// Format and pixel size are read from the bytes; the caller supplies neither.
#[napi(object)]
pub struct ImageSource {
    pub name: String,
    pub data: Buffer,
}

#[napi(object)]
pub struct RenderOptions {
    pub fonts: Vec<FontSource>,
    pub images: Option<Vec<ImageSource>>,
}

#[napi(object)]
pub struct RenderResult {
    pub pdf: Buffer,
    pub pages: u32,
    /// Size of the PDF in bytes.
    pub bytes: u32,
    /// Anything the engine noticed — clipped text, a character no font covers.
    pub diagnostics: Vec<String>,
}

/// As [`RenderResult`], for a document that went straight to disk.
#[napi(object)]
pub struct WriteResult {
    pub path: String,
    pub pages: u32,
    pub bytes: u32,
    pub diagnostics: Vec<String>,
}

pub struct RenderTask {
    job: Option<Job>,
}

impl Task for RenderTask {
    type Output = Outcome;
    type JsValue = RenderResult;

    fn compute(&mut self) -> Result<Outcome> {
        let job = self.job.take().expect("a task is computed once");
        job::run(job).map_err(fail)
    }

    fn resolve(&mut self, _env: Env, out: Outcome) -> Result<RenderResult> {
        Ok(RenderResult {
            pdf: out.pdf.unwrap_or_default().into(),
            pages: out.pages as u32,
            bytes: out.bytes as u32,
            diagnostics: out.diagnostics,
        })
    }
}

pub struct WriteTask {
    job: Option<Job>,
    path: String,
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
            path: std::mem::take(&mut self.path),
            pages: out.pages as u32,
            bytes: out.bytes as u32,
            diagnostics: out.diagnostics,
        })
    }
}

/// Renders a declared document and hands back the bytes.
#[napi(ts_return_type = "Promise<RenderResult>")]
pub fn render(ir: String, options: RenderOptions) -> AsyncTask<RenderTask> {
    AsyncTask::new(RenderTask {
        job: Some(job(ir, options, Output::Buffer)),
    })
}

/// Renders a declared document straight to a file.
///
/// Preferred for anything large: the PDF is written from Rust, so a 128 MB
/// ledger never becomes a 128 MB Buffer in the JS heap on its way to disk.
#[napi(ts_return_type = "Promise<WriteResult>")]
pub fn render_to_file(ir: String, path: String, options: RenderOptions) -> AsyncTask<WriteTask> {
    AsyncTask::new(WriteTask {
        job: Some(job(ir, options, Output::File(path.clone().into()))),
        path,
    })
}

fn job(ir: String, options: RenderOptions, output: Output) -> Job {
    Job {
        ir,
        fonts: options
            .fonts
            .into_iter()
            .map(|f| FontInput {
                weight: f.weight.unwrap_or_default(),
                italic: f.italic.unwrap_or(false),
                data: f.data.to_vec(),
            })
            .collect(),
        images: options
            .images
            .unwrap_or_default()
            .into_iter()
            .map(|i| ImageInput {
                name: i.name,
                data: i.data.to_vec(),
            })
            .collect(),
        output,
    }
}

fn fail(e: job::JobError) -> Error {
    Error::new(Status::GenericFailure, e.to_string())
}
